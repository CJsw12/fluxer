//! Native OS WebAuthn (passkey & FIDO2 security key) ceremony bridge.
//!
//! Uses Windows' built-in `webauthn.dll` (Windows 10 1903+ / Windows 11) via
//! dynamic loading (`LOAD_LIBRARY_SEARCH_SYSTEM32`) so Windows Hello and USB/NFC
//! FIDO2 security keys work natively with zero extra runtime dependencies.
//! All JSON parsing, `clientDataJSON` assembly, base64url encoding, and
//! response formatting stay in Safe Rust; `unsafe` is strictly confined to the
//! Win32 `webauthn.dll` FFI boundary with RAII free guards.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fluxer_client_core::rest::Endpoints;
use serde_json::{Value, json};
use url::Url;

const DEFAULT_TIMEOUT_MS: u32 = 60_000;

#[derive(Debug, Clone)]
struct CredentialDescriptor {
    id: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ParsedGetOptions {
    rp_id: String,
    challenge_b64: String,
    allow_credentials: Vec<CredentialDescriptor>,
    timeout_ms: u32,
    user_verification: u32,
    authenticator_attachment: u32,
}

#[derive(Debug, Clone)]
struct ParsedCreateOptions {
    rp_id: String,
    rp_name: String,
    challenge_b64: String,
    user_id: Vec<u8>,
    user_name: String,
    user_display_name: String,
    pub_key_algs: Vec<i32>,
    exclude_credentials: Vec<CredentialDescriptor>,
    timeout_ms: u32,
    user_verification: u32,
    authenticator_attachment: u32,
    attestation: u32,
    require_resident_key: bool,
    prefer_resident_key: bool,
}

#[derive(Debug, Clone)]
struct AssertionOutput {
    raw_id: Vec<u8>,
    authenticator_data: Vec<u8>,
    signature: Vec<u8>,
    user_handle: Option<Vec<u8>>,
    client_data_json: Vec<u8>,
    used_transport: u32,
}

#[derive(Debug, Clone)]
struct RegistrationOutput {
    raw_id: Vec<u8>,
    attestation_object: Vec<u8>,
    client_data_json: Vec<u8>,
    used_transport: u32,
}

/// Returns true when the OS native WebAuthn API is available (Windows `webauthn.dll` v1+).
pub fn is_supported() -> bool {
    #[cfg(windows)]
    {
        win::api_version() > 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Returns true when a platform authenticator (e.g. Windows Hello) is available.
pub fn is_platform_authenticator_available() -> bool {
    #[cfg(windows)]
    {
        win::is_platform_available()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn is_ip_host(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok() || (host.starts_with('[') && host.ends_with(']'))
}

fn registrable_suffix(host: &str) -> Option<&str> {
    if is_ip_host(host) || !host.contains('.') {
        return None;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let tail_start = host.len() - parts[parts.len() - 1].len() - parts[parts.len() - 2].len() - 1;
    Some(&host[tail_start..])
}

/// Validates that `rp_id` is permitted for `origin_host` under W3C WebAuthn
/// Relying Party Identifier rules: exact host match, or a non-TLD domain
/// suffix of `origin_host` (never a bare TLD or IP suffix).
pub fn validate_rp_id(rp_id: &str, origin_host: &str) -> Result<String, String> {
    let rp = normalize_host(rp_id);
    let host = normalize_host(origin_host);
    if rp.is_empty() || host.is_empty() {
        return Err("WebAuthn RP ID or origin host is empty.".to_owned());
    }
    if rp == host {
        return Ok(rp);
    }
    if is_ip_host(&host) || is_ip_host(&rp) {
        return Err(format!("WebAuthn RP ID '{rp}' does not match IP origin '{host}'."));
    }
    if !rp.contains('.') {
        return Err(format!("WebAuthn RP ID '{rp}' cannot be a bare top-level domain."));
    }
    let suffix = format!(".{rp}");
    if host.ends_with(&suffix) {
        Ok(rp)
    } else {
        Err(format!(
            "WebAuthn RP ID '{rp}' is not a valid domain suffix for instance host '{host}'."
        ))
    }
}

/// Extracts the normalized RP ID host from a validated origin URL.
pub fn rp_id_for_origin(origin: &str) -> Option<String> {
    let parsed = Url::parse(origin).ok()?;
    parsed.host_str().map(normalize_host)
}

fn parse_allowed_origin(raw: &str) -> Result<(String, String), String> {
    let parsed = Url::parse(raw).map_err(|err| format!("Invalid instance origin: {err}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Instance URL has no host for WebAuthn origin".to_owned())?;
    let norm_host = normalize_host(host);
    let scheme = parsed.scheme();
    let is_loopback = matches!(norm_host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if scheme != "https" && !(scheme == "http" && is_loopback) {
        return Err("WebAuthn requires an HTTPS instance origin.".to_owned());
    }
    let origin = match parsed.port() {
        Some(port) => format!("{scheme}://{norm_host}:{port}"),
        None => format!("{scheme}://{norm_host}"),
    };
    Ok((origin, norm_host))
}

/// Derives the trusted WebAuthn origin (`https://host[:port]`) for `endpoints`.
/// Anchors trust to the discovered `api` origin; only uses `webapp` if it
/// belongs to the same host or same registrable domain suffix as `api`.
pub fn derive_origin(endpoints: &Endpoints) -> Result<String, String> {
    let (api_origin, api_host) = parse_allowed_origin(&endpoints.api)?;
    if let Some(webapp) = endpoints.webapp.as_deref()
        && let Ok((web_origin, web_host)) = parse_allowed_origin(webapp)
    {
        let same_host = web_host == api_host;
        let same_site = match (registrable_suffix(&api_host), registrable_suffix(&web_host)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if same_host || same_site {
            return Ok(web_origin);
        }
    }
    Ok(api_origin)
}

/// Builds a standard W3C WebAuthn `clientDataJSON` payload.
pub fn build_client_data_json(ceremony_type: &str, challenge_b64: &str, origin: &str) -> String {
    json!({
        "type": ceremony_type,
        "challenge": challenge_b64,
        "origin": origin,
        "crossOrigin": false,
    })
    .to_string()
}

fn decode_b64url(s: &str) -> Result<Vec<u8>, String> {
    let trimmed = s.trim().trim_end_matches('=');
    URL_SAFE_NO_PAD
        .decode(trimmed)
        .map_err(|err| format!("Invalid base64url field in WebAuthn options: {err}"))
}

fn parse_descriptors(value: Option<&Value>) -> Vec<CredentialDescriptor> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .filter_map(|id| decode_b64url(id).ok())
        .map(|id| CredentialDescriptor { id })
        .collect()
}

fn parse_user_verification(val: Option<&str>) -> u32 {
    match val {
        Some("required") => 1,    // WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED
        Some("preferred") => 2,   // WEBAUTHN_USER_VERIFICATION_REQUIREMENT_PREFERRED
        Some("discouraged") => 3, // WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED
        _ => 2,
    }
}

fn parse_attachment(val: Option<&str>) -> u32 {
    match val {
        Some("platform") => 1,       // WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM
        Some("cross-platform") => 2, // WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM
        _ => 0,                      // WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY
    }
}

fn parse_attestation(val: Option<&str>) -> u32 {
    match val {
        Some("none") => 1,
        Some("indirect") => 2,
        Some("direct") => 3,
        _ => 1,
    }
}

fn attachment_from_transport(transport: u32) -> Option<&'static str> {
    const TRANSPORT_INTERNAL: u32 = 0x0000_0010;
    if (transport & TRANSPORT_INTERNAL) != 0 {
        Some("platform")
    } else if transport != 0 {
        Some("cross-platform")
    } else {
        None
    }
}

fn parse_get_options(options: &Value, default_origin: &str) -> Result<ParsedGetOptions, String> {
    let challenge_b64 = options
        .get("challenge")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "WebAuthn authentication options missing challenge".to_owned())?
        .to_owned();
    let origin_host = rp_id_for_origin(default_origin)
        .ok_or_else(|| "WebAuthn origin missing host".to_owned())?;
    let raw_rp_id = options
        .get("rpId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&origin_host);
    let rp_id = validate_rp_id(raw_rp_id, &origin_host)?;
    let timeout_ms = options
        .get("timeout")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    Ok(ParsedGetOptions {
        rp_id,
        challenge_b64,
        allow_credentials: parse_descriptors(options.get("allowCredentials")),
        timeout_ms,
        user_verification: parse_user_verification(options.get("userVerification").and_then(Value::as_str)),
        authenticator_attachment: parse_attachment(options.get("authenticatorAttachment").and_then(Value::as_str)),
    })
}

fn parse_create_options(options: &Value, default_origin: &str) -> Result<ParsedCreateOptions, String> {
    let challenge_b64 = options
        .get("challenge")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "WebAuthn registration options missing challenge".to_owned())?
        .to_owned();
    let origin_host = rp_id_for_origin(default_origin)
        .ok_or_else(|| "WebAuthn origin missing host".to_owned())?;
    let rp = options.get("rp");
    let raw_rp_id = rp
        .and_then(|r| r.get("id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&origin_host);
    let rp_id = validate_rp_id(raw_rp_id, &origin_host)?;
    let rp_name = rp
        .and_then(|r| r.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("Fluxer")
        .to_owned();
    let user = options
        .get("user")
        .ok_or_else(|| "WebAuthn registration options missing user".to_owned())?;
    let user_id_str = user
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "WebAuthn registration options missing user.id".to_owned())?;
    let user_id = decode_b64url(user_id_str)?;
    let user_name = user
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .to_owned();
    let user_display_name = user
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(&user_name)
        .to_owned();
    let mut pub_key_algs: Vec<i32> = options
        .get("pubKeyCredParams")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("alg").and_then(Value::as_i64))
                .filter_map(|a| i32::try_from(a).ok())
                .collect()
        })
        .unwrap_or_default();
    if pub_key_algs.is_empty() {
        pub_key_algs = vec![-7, -257]; // ES256, RS256
    }
    let timeout_ms = options
        .get("timeout")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_TIMEOUT_MS);
    let auth_sel = options.get("authenticatorSelection");
    let resident_key = auth_sel.and_then(|s| s.get("residentKey")).and_then(Value::as_str);
    let require_rk = resident_key == Some("required")
        || auth_sel
            .and_then(|s| s.get("requireResidentKey"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    // Windows treats bPreferResidentKey as downgrading bRequireResidentKey on some builds;
    // keep the two flags mutually exclusive per webauthn.h guidance.
    let prefer_rk = !require_rk && resident_key == Some("preferred");

    Ok(ParsedCreateOptions {
        rp_id,
        rp_name,
        challenge_b64,
        user_id,
        user_name,
        user_display_name,
        pub_key_algs,
        exclude_credentials: parse_descriptors(options.get("excludeCredentials")),
        timeout_ms,
        user_verification: parse_user_verification(
            auth_sel.and_then(|s| s.get("userVerification")).and_then(Value::as_str),
        ),
        authenticator_attachment: parse_attachment(
            auth_sel
                .and_then(|s| s.get("authenticatorAttachment"))
                .and_then(Value::as_str),
        ),
        attestation: parse_attestation(options.get("attestation").and_then(Value::as_str)),
        require_resident_key: require_rk,
        prefer_resident_key: prefer_rk,
    })
}

fn format_assertion_response(out: &AssertionOutput) -> Value {
    let id = URL_SAFE_NO_PAD.encode(&out.raw_id);
    let mut response = json!({
        "clientDataJSON": URL_SAFE_NO_PAD.encode(&out.client_data_json),
        "authenticatorData": URL_SAFE_NO_PAD.encode(&out.authenticator_data),
        "signature": URL_SAFE_NO_PAD.encode(&out.signature),
    });
    if let Some(handle) = &out.user_handle
        && !handle.is_empty()
        && let Some(obj) = response.as_object_mut()
    {
        obj.insert("userHandle".to_owned(), Value::String(URL_SAFE_NO_PAD.encode(handle)));
    }
    let mut payload = json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": response,
        "clientExtensionResults": {},
    });
    if let Some(att) = attachment_from_transport(out.used_transport)
        && let Some(obj) = payload.as_object_mut()
    {
        obj.insert("authenticatorAttachment".to_owned(), Value::String(att.to_owned()));
    }
    payload
}

fn format_registration_response(out: &RegistrationOutput) -> Value {
    let id = URL_SAFE_NO_PAD.encode(&out.raw_id);
    let mut payload = json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": URL_SAFE_NO_PAD.encode(&out.client_data_json),
            "attestationObject": URL_SAFE_NO_PAD.encode(&out.attestation_object),
        },
        "clientExtensionResults": {},
    });
    if let Some(att) = attachment_from_transport(out.used_transport)
        && let Some(obj) = payload.as_object_mut()
    {
        obj.insert("authenticatorAttachment".to_owned(), Value::String(att.to_owned()));
    }
    payload
}

/// Performs a native OS WebAuthn authentication ceremony (`navigator.credentials.get`).
/// Returns `(challenge, AuthenticationResponseJSON)`.
pub fn authenticate(options: &Value, origin: &str) -> Result<(String, Value), String> {
    let parsed = parse_get_options(options, origin)?;
    let client_data_json = build_client_data_json("webauthn.get", &parsed.challenge_b64, origin).into_bytes();
    #[cfg(windows)]
    {
        let out = win::get_assertion(&parsed, &client_data_json)?;
        Ok((parsed.challenge_b64, format_assertion_response(&out)))
    }
    #[cfg(not(windows))]
    {
        let _ = client_data_json;
        Err("Passkeys and hardware security keys are only supported on Windows in this build.".to_owned())
    }
}

/// Performs a native OS WebAuthn registration ceremony (`navigator.credentials.create`).
/// Returns `(challenge, RegistrationResponseJSON)`.
pub fn register(options: &Value, origin: &str) -> Result<(String, Value), String> {
    let parsed = parse_create_options(options, origin)?;
    let client_data_json = build_client_data_json("webauthn.create", &parsed.challenge_b64, origin).into_bytes();
    #[cfg(windows)]
    {
        let out = win::make_credential(&parsed, &client_data_json)?;
        Ok((parsed.challenge_b64, format_registration_response(&out)))
    }
    #[cfg(not(windows))]
    {
        let _ = client_data_json;
        Err("Passkeys and hardware security keys are only supported on Windows in this build.".to_owned())
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::mem::transmute;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{FreeLibrary, HMODULE, HWND, LPARAM};
    use windows::Win32::Networking::WindowsWebServices::{
        WEBAUTHN_ASSERTION, WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
        WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION,
        WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
        WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION, WEBAUTHN_CLIENT_DATA,
        WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
        WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
        WEBAUTHN_CREDENTIAL, WEBAUTHN_CREDENTIAL_ATTESTATION, WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
        WEBAUTHN_CREDENTIALS, WEBAUTHN_HASH_ALGORITHM_SHA_256, WEBAUTHN_LARGE_BLOB_SUPPORT_NONE,
        WEBAUTHN_RP_ENTITY_INFORMATION, WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        WEBAUTHN_USER_ENTITY_INFORMATION, WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
    };
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetDesktopWindow, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_core::{BOOL, HRESULT, PCSTR, PCWSTR, s, w};

    use super::{AssertionOutput, CredentialDescriptor, ParsedCreateOptions, ParsedGetOptions, RegistrationOutput};

    type RawExport = unsafe extern "system" fn() -> isize;
    type GetApiVersionNumberFn = unsafe extern "system" fn() -> u32;
    type IsPlatformAvailableFn = unsafe extern "system" fn(*mut BOOL) -> HRESULT;
    type MakeCredentialFn = unsafe extern "system" fn(
        HWND,
        *const WEBAUTHN_RP_ENTITY_INFORMATION,
        *const WEBAUTHN_USER_ENTITY_INFORMATION,
        *const WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
        *const WEBAUTHN_CLIENT_DATA,
        *const WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
        *mut *mut WEBAUTHN_CREDENTIAL_ATTESTATION,
    ) -> HRESULT;
    type GetAssertionFn = unsafe extern "system" fn(
        HWND,
        PCWSTR,
        *const WEBAUTHN_CLIENT_DATA,
        *const WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
        *mut *mut WEBAUTHN_ASSERTION,
    ) -> HRESULT;
    type FreeAttestationFn = unsafe extern "system" fn(*const WEBAUTHN_CREDENTIAL_ATTESTATION);
    type FreeAssertionFn = unsafe extern "system" fn(*const WEBAUTHN_ASSERTION);

    struct WebAuthnApi {
        version: u32,
        is_platform_available: IsPlatformAvailableFn,
        make_credential: MakeCredentialFn,
        get_assertion: GetAssertionFn,
        free_attestation: FreeAttestationFn,
        free_assertion: FreeAssertionFn,
    }

    static API: OnceLock<Option<WebAuthnApi>> = OnceLock::new();

    fn api() -> Option<&'static WebAuthnApi> {
        API.get_or_init(|| load_api(w!("webauthn.dll"))).as_ref()
    }

    fn export(module: HMODULE, name: PCSTR) -> Option<RawExport> {
        unsafe { GetProcAddress(module, name) }
    }

    fn load_api(library: PCWSTR) -> Option<WebAuthnApi> {
        let module = unsafe { LoadLibraryExW(library, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
        let loaded = (|| {
            let get_ver = unsafe {
                transmute::<RawExport, GetApiVersionNumberFn>(export(module, s!("WebAuthNGetApiVersionNumber"))?)
            };
            let version = unsafe { get_ver() };
            if version == 0 {
                return None;
            }
            unsafe {
                Some(WebAuthnApi {
                    version,
                    is_platform_available: transmute::<RawExport, IsPlatformAvailableFn>(export(
                        module,
                        s!("WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable"),
                    )?),
                    make_credential: transmute::<RawExport, MakeCredentialFn>(export(
                        module,
                        s!("WebAuthNAuthenticatorMakeCredential"),
                    )?),
                    get_assertion: transmute::<RawExport, GetAssertionFn>(export(
                        module,
                        s!("WebAuthNAuthenticatorGetAssertion"),
                    )?),
                    free_attestation: transmute::<RawExport, FreeAttestationFn>(export(
                        module,
                        s!("WebAuthNFreeCredentialAttestation"),
                    )?),
                    free_assertion: transmute::<RawExport, FreeAssertionFn>(export(
                        module,
                        s!("WebAuthNFreeAssertion"),
                    )?),
                })
            }
        })();
        if loaded.is_none() {
            let _ = unsafe { FreeLibrary(module) };
        }
        loaded
    }

    #[cfg(test)]
    pub(super) fn probe_library(library: PCWSTR) -> bool {
        load_api(library).is_some()
    }

    pub(super) fn api_version() -> u32 {
        api().map_or(0, |a| a.version)
    }

    pub(super) fn is_platform_available() -> bool {
        let Some(api) = api() else {
            return false;
        };
        let mut available = BOOL(0);
        let hr = unsafe { (api.is_platform_available)(&mut available) };
        hr.is_ok() && available.as_bool()
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsString::from(s).encode_wide().chain(Some(0)).collect()
    }

    const PUBLIC_KEY_WIDE: &[u16] = &[
        b'p' as u16, b'u' as u16, b'b' as u16, b'l' as u16, b'i' as u16, b'c' as u16, b'-' as u16, b'k' as u16,
        b'e' as u16, b'y' as u16, 0,
    ];

    fn window_pid(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    unsafe extern "system" fn find_own_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let slot = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
        if window_pid(hwnd) == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            *slot = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    fn interaction_window() -> HWND {
        let current_pid = unsafe { GetCurrentProcessId() };
        let fg = unsafe { GetForegroundWindow() };
        if !fg.is_invalid() && window_pid(fg) == current_pid {
            return fg;
        }
        let mut own: Option<HWND> = None;
        let _ = unsafe { EnumWindows(Some(find_own_window), LPARAM(&raw mut own as isize)) };
        if let Some(hwnd) = own {
            return hwnd;
        }
        if !fg.is_invalid() {
            return fg;
        }
        unsafe { GetDesktopWindow() }
    }

    fn build_credentials(
        descriptors: &[CredentialDescriptor],
    ) -> (Vec<WEBAUTHN_CREDENTIAL>, WEBAUTHN_CREDENTIALS) {
        let mut backing: Vec<WEBAUTHN_CREDENTIAL> = descriptors
            .iter()
            .map(|d| WEBAUTHN_CREDENTIAL {
                dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
                cbId: d.id.len() as u32,
                pbId: d.id.as_ptr() as *mut u8,
                pwszCredentialType: PCWSTR(PUBLIC_KEY_WIDE.as_ptr()),
            })
            .collect();
        let list = WEBAUTHN_CREDENTIALS {
            cCredentials: backing.len() as u32,
            pCredentials: if backing.is_empty() {
                std::ptr::null_mut()
            } else {
                backing.as_mut_ptr()
            },
        };
        (backing, list)
    }

    fn copy_buf(ptr: *const u8, len: u32) -> Result<Vec<u8>, String> {
        if len == 0 {
            return Ok(Vec::new());
        }
        if ptr.is_null() {
            return Err("Windows WebAuthn returned a null buffer.".to_owned());
        }
        Ok(unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec())
    }

    struct AssertionGuard {
        ptr: *mut WEBAUTHN_ASSERTION,
        free: FreeAssertionFn,
    }
    impl Drop for AssertionGuard {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe { (self.free)(self.ptr) };
            }
        }
    }

    struct AttestationGuard {
        ptr: *mut WEBAUTHN_CREDENTIAL_ATTESTATION,
        free: FreeAttestationFn,
    }
    impl Drop for AttestationGuard {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe { (self.free)(self.ptr) };
            }
        }
    }

    /// Maps `WebAuthNGetApiVersionNumber()` to the matching `WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS`
    /// struct version (per Microsoft `webauthn.h` version table).
    fn make_credential_options_version(api_version: u32) -> u32 {
        let mapped = match api_version {
            0 => 1,
            1 => 3,
            2 => 4,
            3 => 5,
            4 => 6,
            5..=6 => 7,
            _ => WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION,
        };
        mapped.min(WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_CURRENT_VERSION)
    }

    /// Maps `WebAuthNGetApiVersionNumber()` to the matching `WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS`
    /// struct version (per Microsoft `webauthn.h` version table).
    fn get_assertion_options_version(api_version: u32) -> u32 {
        let mapped = match api_version {
            0 => 1,
            1 => 4,
            2..=3 => 5,
            4..=5 => 6,
            6 => 7,
            _ => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION,
        };
        mapped.min(WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_CURRENT_VERSION)
    }

    pub(super) fn get_assertion(input: &ParsedGetOptions, client_data_json: &[u8]) -> Result<AssertionOutput, String> {
        let api = api().ok_or_else(|| "Windows WebAuthn API (webauthn.dll) is unavailable.".to_owned())?;
        let rp_id_w = to_wide(&input.rp_id);
        let client_data = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: client_data_json.len() as u32,
            pbClientDataJSON: client_data_json.as_ptr() as *mut u8,
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let (_keep, allow_list) = build_credentials(&input.allow_credentials);
        let opts = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
            dwVersion: get_assertion_options_version(api.version),
            dwTimeoutMilliseconds: input.timeout_ms,
            CredentialList: allow_list,
            dwAuthenticatorAttachment: input.authenticator_attachment,
            dwUserVerificationRequirement: input.user_verification,
            ..Default::default()
        };
        let mut ptr: *mut WEBAUTHN_ASSERTION = std::ptr::null_mut();
        unsafe {
            (api.get_assertion)(
                interaction_window(),
                PCWSTR(rp_id_w.as_ptr()),
                &client_data,
                &opts,
                &mut ptr,
            )
        }
        .ok()
        .map_err(|err| format!("Passkey authentication cancelled or failed: {err}"))?;
        if ptr.is_null() {
            return Err("Windows WebAuthn returned an empty assertion.".to_owned());
        }
        let _guard = AssertionGuard { ptr, free: api.free_assertion };
        // Read version-gated fields via raw pointers so we never construct a reference
        // larger than the struct allocated by an older webauthn.dll.
        let (dw_version, cred_pb, cred_cb, auth_pb, auth_cb, sig_pb, sig_cb, uid_pb, uid_cb, used_transport) = unsafe {
            let ver = std::ptr::addr_of!((*ptr).dwVersion).read();
            let cred = std::ptr::addr_of!((*ptr).Credential).read();
            let auth_cb = std::ptr::addr_of!((*ptr).cbAuthenticatorData).read();
            let auth_pb = std::ptr::addr_of!((*ptr).pbAuthenticatorData).read();
            let sig_cb = std::ptr::addr_of!((*ptr).cbSignature).read();
            let sig_pb = std::ptr::addr_of!((*ptr).pbSignature).read();
            let uid_cb = std::ptr::addr_of!((*ptr).cbUserId).read();
            let uid_pb = std::ptr::addr_of!((*ptr).pbUserId).read();
            let transport = if ver >= 4 {
                std::ptr::addr_of!((*ptr).dwUsedTransport).read()
            } else {
                0
            };
            (ver, cred.pbId, cred.cbId, auth_pb, auth_cb, sig_pb, sig_cb, uid_pb, uid_cb, transport)
        };
        let _ = dw_version;
        let raw_id = copy_buf(cred_pb, cred_cb)?;
        let authenticator_data = copy_buf(auth_pb, auth_cb)?;
        let signature = copy_buf(sig_pb, sig_cb)?;
        let user_handle = if uid_cb > 0 {
            Some(copy_buf(uid_pb, uid_cb)?)
        } else {
            None
        };
        Ok(AssertionOutput {
            raw_id,
            authenticator_data,
            signature,
            user_handle,
            client_data_json: client_data_json.to_vec(),
            used_transport,
        })
    }

    pub(super) fn make_credential(
        input: &ParsedCreateOptions,
        client_data_json: &[u8],
    ) -> Result<RegistrationOutput, String> {
        let api = api().ok_or_else(|| "Windows WebAuthn API (webauthn.dll) is unavailable.".to_owned())?;
        let rp_id_w = to_wide(&input.rp_id);
        let rp_name_w = to_wide(&input.rp_name);
        let user_name_w = to_wide(&input.user_name);
        let display_w = to_wide(&input.user_display_name);

        let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
            pwszId: PCWSTR(rp_id_w.as_ptr()),
            pwszName: PCWSTR(rp_name_w.as_ptr()),
            pwszIcon: PCWSTR::null(),
        };
        let user = WEBAUTHN_USER_ENTITY_INFORMATION {
            dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
            cbId: input.user_id.len() as u32,
            pbId: input.user_id.as_ptr() as *mut u8,
            pwszName: PCWSTR(user_name_w.as_ptr()),
            pwszIcon: PCWSTR::null(),
            pwszDisplayName: PCWSTR(display_w.as_ptr()),
        };
        let mut pub_key_params: Vec<WEBAUTHN_COSE_CREDENTIAL_PARAMETER> = input
            .pub_key_algs
            .iter()
            .map(|&alg| WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
                pwszCredentialType: PCWSTR(PUBLIC_KEY_WIDE.as_ptr()),
                lAlg: alg,
            })
            .collect();
        let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
            cCredentialParameters: pub_key_params.len() as u32,
            pCredentialParameters: pub_key_params.as_mut_ptr(),
        };
        let client_data = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: client_data_json.len() as u32,
            pbClientDataJSON: client_data_json.as_ptr() as *mut u8,
            pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
        };
        let (_keep, exclude_list) = build_credentials(&input.exclude_credentials);
        let opts = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
            dwVersion: make_credential_options_version(api.version),
            dwTimeoutMilliseconds: input.timeout_ms,
            CredentialList: exclude_list,
            dwAuthenticatorAttachment: input.authenticator_attachment,
            bRequireResidentKey: BOOL(input.require_resident_key as i32),
            dwUserVerificationRequirement: input.user_verification,
            dwAttestationConveyancePreference: input.attestation,
            dwEnterpriseAttestation: 0,
            dwLargeBlobSupport: WEBAUTHN_LARGE_BLOB_SUPPORT_NONE,
            bPreferResidentKey: BOOL(input.prefer_resident_key as i32),
            ..Default::default()
        };
        let mut ptr: *mut WEBAUTHN_CREDENTIAL_ATTESTATION = std::ptr::null_mut();
        unsafe {
            (api.make_credential)(
                interaction_window(),
                &rp,
                &user,
                &params,
                &client_data,
                &opts,
                &mut ptr,
            )
        }
        .ok()
        .map_err(|err| format!("Passkey registration cancelled or failed: {err}"))?;
        if ptr.is_null() {
            return Err("Windows WebAuthn returned an empty attestation.".to_owned());
        }
        let _guard = AttestationGuard { ptr, free: api.free_attestation };
        let (cred_pb, cred_cb, att_pb, att_cb, used_transport) = unsafe {
            let ver = std::ptr::addr_of!((*ptr).dwVersion).read();
            let cred_cb = std::ptr::addr_of!((*ptr).cbCredentialId).read();
            let cred_pb = std::ptr::addr_of!((*ptr).pbCredentialId).read();
            let att_cb = std::ptr::addr_of!((*ptr).cbAttestationObject).read();
            let att_pb = std::ptr::addr_of!((*ptr).pbAttestationObject).read();
            let transport = if ver >= 3 {
                std::ptr::addr_of!((*ptr).dwUsedTransport).read()
            } else {
                0
            };
            (cred_pb, cred_cb, att_pb, att_cb, transport)
        };
        let raw_id = copy_buf(cred_pb, cred_cb)?;
        let attestation_object = copy_buf(att_pb, att_cb)?;
        Ok(RegistrationOutput {
            raw_id,
            attestation_object,
            client_data_json: client_data_json.to_vec(),
            used_transport,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_data_and_origin_follow_w3c_webauthn() {
        let ep = Endpoints {
            api_code_version: 1,
            api: "https://api.fluxer.app/api".to_owned(),
            gateway: "wss://gateway.fluxer.app".to_owned(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: Some("https://web.fluxer.app/channels/@me".to_owned()),
            discovery_url: "https://fluxer.app/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        assert_eq!(derive_origin(&ep).unwrap(), "https://web.fluxer.app");

        let cd = build_client_data_json("webauthn.get", "abc123", "https://web.fluxer.app");
        let parsed: Value = serde_json::from_str(&cd).unwrap();
        assert_eq!(parsed["type"], "webauthn.get");
        assert_eq!(parsed["challenge"], "abc123");
        assert_eq!(parsed["origin"], "https://web.fluxer.app");
        assert_eq!(parsed["crossOrigin"], false);
    }

    #[test]
    fn options_parsing_and_response_formatting_round_trip() {
        let get_opts = json!({
            "challenge": "dGVzdC1jaGFsbGVuZ2U",
            "rpId": "fluxer.app",
            "timeout": 45000,
            "userVerification": "required",
            "allowCredentials": [{ "type": "public-key", "id": "AQIDBA" }]
        });
        let parsed_get = parse_get_options(&get_opts, "https://fluxer.app").unwrap();
        assert_eq!(parsed_get.rp_id, "fluxer.app");
        assert_eq!(parsed_get.allow_credentials.len(), 1);
        assert_eq!(parsed_get.allow_credentials[0].id, vec![1, 2, 3, 4]);
        assert_eq!(parsed_get.timeout_ms, 45_000);

        let asrt = AssertionOutput {
            raw_id: vec![1, 2, 3, 4],
            authenticator_data: vec![9, 9],
            signature: vec![7, 8],
            user_handle: Some(vec![42]),
            client_data_json: b"{}".to_vec(),
            used_transport: 0x10,
        };
        let asrt_json = format_assertion_response(&asrt);
        assert_eq!(asrt_json["id"], "AQIDBA");
        assert_eq!(asrt_json["type"], "public-key");
        assert_eq!(asrt_json["authenticatorAttachment"], "platform");
        assert!(asrt_json["response"]["userHandle"].is_string());

        let create_opts = json!({
            "challenge": "cmVnLWNoYWxsZW5nZQ",
            "rp": { "id": "fluxer.app", "name": "Fluxer" },
            "user": { "id": "NDI", "name": "alice", "displayName": "Alice" },
            "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }],
            "authenticatorSelection": { "residentKey": "required", "userVerification": "preferred" }
        });
        let parsed_create = parse_create_options(&create_opts, "https://fluxer.app").unwrap();
        assert_eq!(parsed_create.user_id, b"42");
        assert!(parsed_create.require_resident_key);
        assert!(!parsed_create.prefer_resident_key, "required must not also set prefer");
        assert_eq!(parsed_create.pub_key_algs, vec![-7]);

        // Cross-origin RP ID spoofing and bare TLD RP IDs must be rejected.
        assert!(validate_rp_id("evil.com", "web.fluxer.app").is_err());
        assert!(validate_rp_id("app", "web.fluxer.app").is_err());
        assert!(validate_rp_id("fluxer.app", "web.fluxer.app").is_ok());

        // Untrusted third-party webapp origin falls back to API origin.
        let spoofed_ep = Endpoints {
            api_code_version: 1,
            api: "https://api.fluxer.app/api".to_owned(),
            gateway: "wss://gateway.fluxer.app".to_owned(),
            media: None,
            static_cdn: None,
            upload_relay: None,
            invite: None,
            webapp: Some("https://accounts.google.com".to_owned()),
            discovery_url: "https://fluxer.app/.well-known/fluxer".to_owned(),
            document: json!({}),
        };
        assert_eq!(derive_origin(&spoofed_ep).unwrap(), "https://api.fluxer.app");

        let reg = RegistrationOutput {
            raw_id: vec![5, 6, 7],
            attestation_object: vec![10, 11],
            client_data_json: b"{}".to_vec(),
            used_transport: 0x01,
        };
        let reg_json = format_registration_response(&reg);
        assert_eq!(reg_json["type"], "public-key");
        assert_eq!(reg_json["authenticatorAttachment"], "cross-platform");
        assert!(reg_json["response"]["attestationObject"].is_string());
    }

    #[cfg(windows)]
    #[test]
    fn windows_webauthn_dll_probe_and_missing_library_safety() {
        use windows_core::w;
        assert!(!win::probe_library(w!("kernel32.dll")));
        assert!(!win::probe_library(w!("fluxer-nonexistent-webauthn.dll")));
        // On Windows 10 1903+ / Windows 11, system webauthn.dll is present.
        assert!(is_supported());
    }
}
