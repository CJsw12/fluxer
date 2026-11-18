//! Persistent, encrypted login sessions ("stay logged in").
//!
//! Semantics follow the official clients: several accounts may be stored,
//! each identified by the account storage key `"{instance_key}::{user_id}"`
//! (`fluxer_app/src/features/auth/state/AccountStorageKey.ts`), where the
//! instance key is the normalized API endpoint (`runtimeInstanceKey` in
//! `InstanceSnapshotStore.ts`). One account is the active (foreground) one,
//! like `BootState.lastForeground` in `fluxer_desktop/src/main/BootState.ts`.
//! The discovered instance endpoints are cached next to the account so a
//! restart can connect before rediscovery finishes.
//!
//! Storage: one JSON file, `sessions.json`, in the store directory
//! (default `%APPDATA%\fluxer_client`). Writes go to a uniquely named
//! `sessions.json.tmp-<pid>-<random>` file, are fsynced, then renamed over
//! the old file (retrying briefly on transient Windows sharing errors, like
//! `renameWithRetry` in `fluxer_desktop/src/main/AtomicFile.ts`), so a crash
//! leaves either the old or the new file.
//!
//! Tokens are stored only as DPAPI ciphertext (see [`dpapi`]). The DPAPI
//! entropy includes the canonical JSON of the record's account key, user id
//! and API endpoint, so a blob moved to another record, or a record whose
//! endpoint was edited, no longer decrypts. Cached endpoints are checked
//! against the same transport and same-origin rules as discovery; a bad
//! cache is dropped (the caller must rediscover) but the account is kept.
//!
//! A record that cannot be parsed, fails validation or whose token cannot
//! be decrypted is not usable: it is reported through
//! [`SessionStore::issues`], its token is never returned, and it is written
//! back unchanged on the next save (so a transient DPAPI failure or a newer
//! client's record is not destroyed). Saving or removing the same account
//! key replaces it. A file that cannot be parsed at all, is too large or has
//! too many records is moved aside to `sessions.json.corrupt-<unix_ms>`
//! (only the newest few are kept) and the store starts empty.
//!
//! Version 1 files (fixed DPAPI entropy) are migrated on open: every valid
//! record is re-sealed with the bound entropy and the file is rewritten;
//! version 1 records that fail validation are dropped and reported.
//!
//! The store holds an exclusive lock (`sessions.lock`) for its lifetime, so
//! a second app instance gets [`SessionError::Locked`] instead of silently
//! overwriting the first one's changes. Windows file ACLs are inherited from
//! `%APPDATA%` (current user only by default); on Unix the files are created
//! `0600` and the directory `0700`.

mod dpapi;
#[cfg(test)]
mod tests;

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::Zeroizing;

use crate::rest::{SecretToken, TransportPolicy};

pub use dpapi::CryptoError;

const FILE_NAME: &str = "sessions.json";
const LOCK_FILE_NAME: &str = "sessions.lock";
/// Prefix of temp files; also matches the fixed name used by version 1.
const TEMP_PREFIX: &str = "sessions.json.tmp";
const CORRUPT_PREFIX: &str = "sessions.json.corrupt-";
/// How many quarantined files are kept; older ones are deleted.
const MAX_CORRUPT_BACKUPS: usize = 3;
/// Temp files older than this belong to a crashed writer and are deleted.
const STALE_TEMP_AGE: Duration = Duration::from_secs(60 * 60);
const FORMAT_VERSION: u32 = 2;
const LEGACY_FORMAT_VERSION: u32 = 1;
const TOKEN_SCHEME: &str = "dpapi-user-v2";
const LEGACY_TOKEN_SCHEME: &str = "dpapi-user-v1";
/// Entropy of version 1 tokens (not bound to the record).
const LEGACY_ENTROPY: &[u8] = b"fluxer_client/session-token/v1";
/// Prefix of the version 2 entropy; the record binding follows it.
const ENTROPY_PREFIX: &[u8] = b"fluxer_client/session-token/v2\0";
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Bounds the DPAPI calls a crafted file can force at startup.
const MAX_RECORDS: usize = 128;
const KEY_SEPARATOR: &str = "::";
const KEY_MAX_BYTES: usize = 1024;
#[cfg(windows)]
const RENAME_RETRY_DELAYS_MS: [u64; 6] = [25, 50, 100, 200, 400, 800];
#[cfg(windows)]
const ERROR_ACCESS_DENIED: i32 = 5;
#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(windows)]
const ERROR_LOCK_VIOLATION: i32 = 33;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session store i/o failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Carries only the serde error category and position, never values.
    #[error("could not serialize session store: {0}")]
    Serialize(String),
    #[error("invalid account: {0}")]
    InvalidAccount(&'static str),
    #[error("unknown account key")]
    UnknownAccount,
    #[error("stored token is not usable: {0}")]
    BadToken(&'static str),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("cannot locate the user data directory")]
    NoDataDir,
    /// Another process (or another store in this process) has the store open.
    #[error("session store at {path} is in use by another process")]
    Locked { path: PathBuf },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> SessionError + '_ {
    move |source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Describes a serde error without the offending value: serde_json's
/// messages quote input ("invalid type: string \"...\""), which may be
/// secret, so only the category and position are kept.
fn describe_serde(e: &serde_json::Error) -> String {
    let kind = match e.classify() {
        serde_json::error::Category::Io => "i/o error",
        serde_json::error::Category::Syntax => "syntax error",
        serde_json::error::Category::Data => "unexpected data",
        serde_json::error::Category::Eof => "unexpected end of input",
    };
    if e.line() == 0 {
        kind.to_owned()
    } else {
        format!("{kind} at line {} column {}", e.line(), e.column())
    }
}

/// Endpoints from instance discovery, cached per account.
/// Mirrors the string fields of `rest::Endpoints`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedEndpoints {
    pub api_code_version: u32,
    pub api: String,
    pub gateway: String,
    pub media: Option<String>,
    pub static_cdn: Option<String>,
    pub upload_relay: Option<String>,
    pub invite: Option<String>,
    pub webapp: Option<String>,
    /// When discovery produced these, unix milliseconds.
    pub discovered_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRecord {
    /// The instance origin shown in the UI (`scheme://host[:port]`). Must be
    /// the origin of `api_endpoint`.
    pub origin: String,
    /// API endpoint (`Endpoints::api`); the account key is derived from it.
    pub api_endpoint: String,
    pub endpoints: Option<CachedEndpoints>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub username: String,
    pub global_name: Option<String>,
    /// Avatar hash, as in the user object.
    pub avatar: Option<String>,
}

/// A stored account without its token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSummary {
    pub key: String,
    pub user_id: String,
    pub instance: InstanceRecord,
    pub display: DisplayInfo,
    /// Last login or activation, unix milliseconds.
    pub last_active_ms: u64,
}

/// Input for [`SessionStore::save_account`].
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub user_id: String,
    pub instance: InstanceRecord,
    pub display: DisplayInfo,
    pub token: SecretToken,
}

/// Something found while opening. Reasons never contain record values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadIssue {
    /// The whole file was unreadable; it was moved to `backup`.
    CorruptFile { backup: PathBuf, reason: String },
    /// Record at `index` could not be parsed or failed validation. It is
    /// kept verbatim in the file but not usable.
    InvalidRecord { index: usize, reason: String },
    /// Record `key` parsed but its token could not be decrypted (moved or
    /// edited record, other Windows user, DPAPI failure). Kept verbatim but
    /// not usable.
    UndecryptableToken { key: String, reason: String },
    /// Record `key` is usable but its cached endpoints were invalid and were
    /// dropped; the caller must rediscover the instance.
    InvalidCachedEndpoints { key: String, reason: &'static str },
    /// A version 1 record could not be migrated and was dropped.
    DroppedLegacyRecord(Box<LoadIssue>),
}

/// DPAPI ciphertext of a token. Never printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EncryptedToken {
    scheme: String,
    /// Base64 of the DPAPI blob.
    data: String,
}

impl std::fmt::Debug for EncryptedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncryptedToken(***)")
    }
}

/// What a version 2 token is bound to. Serialized with serde_json, whose
/// output for a struct of strings is deterministic (fixed field order,
/// fixed escaping), so it is a canonical encoding.
#[derive(Serialize)]
struct TokenBinding<'a> {
    account_key: &'a str,
    user_id: &'a str,
    api_endpoint: &'a str,
}

fn bound_entropy(summary: &AccountSummary) -> Result<Vec<u8>, SessionError> {
    let binding = serde_json::to_vec(&TokenBinding {
        account_key: &summary.key,
        user_id: &summary.user_id,
        api_endpoint: &summary.instance.api_endpoint,
    })
    .map_err(|e| SessionError::Serialize(describe_serde(&e)))?;
    let mut entropy = ENTROPY_PREFIX.to_vec();
    entropy.extend_from_slice(&binding);
    Ok(entropy)
}

fn check_plaintext(plain: &[u8]) -> Result<&str, SessionError> {
    if plain.is_empty() {
        return Err(SessionError::BadToken("empty token"));
    }
    std::str::from_utf8(plain).map_err(|_| SessionError::BadToken("token is not UTF-8"))
}

impl EncryptedToken {
    /// Encrypts `plain` bound to `summary`.
    fn seal(plain: &[u8], summary: &AccountSummary) -> Result<Self, SessionError> {
        if plain.is_empty() {
            return Err(SessionError::InvalidAccount("empty token"));
        }
        let blob = dpapi::protect(plain, &bound_entropy(summary)?)?;
        Ok(Self {
            scheme: TOKEN_SCHEME.to_owned(),
            data: BASE64.encode(blob),
        })
    }

    /// Decrypts a version 2 token sealed for `summary`. The plaintext is
    /// wiped when dropped.
    fn decrypt(&self, summary: &AccountSummary) -> Result<Zeroizing<Vec<u8>>, SessionError> {
        if self.scheme != TOKEN_SCHEME {
            return Err(SessionError::BadToken("unknown encryption scheme"));
        }
        self.decrypt_with(&bound_entropy(summary)?)
    }

    /// Decrypts a version 1 token (fixed entropy). Only used for migration.
    fn decrypt_legacy(&self) -> Result<Zeroizing<Vec<u8>>, SessionError> {
        if self.scheme != LEGACY_TOKEN_SCHEME {
            return Err(SessionError::BadToken("unknown encryption scheme"));
        }
        self.decrypt_with(LEGACY_ENTROPY)
    }

    fn decrypt_with(&self, entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, SessionError> {
        let blob = BASE64
            .decode(&self.data)
            .map_err(|_| SessionError::BadToken("ciphertext is not valid base64"))?;
        let plain = dpapi::unprotect(&blob, entropy)?;
        check_plaintext(&plain)?;
        Ok(plain)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    #[serde(flatten)]
    summary: AccountSummary,
    token: EncryptedToken,
}

/// A record as written: a usable account, or an unusable one kept verbatim.
#[derive(Serialize)]
#[serde(untagged)]
enum OutRecord<'a> {
    Account(&'a StoredAccount),
    Raw(&'a serde_json::Value),
}

#[derive(Serialize)]
struct FileOut<'a> {
    version: u32,
    active: Option<&'a str>,
    accounts: &'a [OutRecord<'a>],
}

#[derive(Deserialize)]
struct FileIn {
    version: u32,
    active: Option<String>,
    accounts: Vec<serde_json::Value>,
}

/// Normalizes an API endpoint the way `runtimeInstanceKey` does:
/// lowercase origin plus the path without trailing slashes. Returns `None`
/// for non-http(s) URLs or ones carrying credentials, query or fragment.
pub fn instance_key(api_endpoint: &str) -> Option<String> {
    let url = Url::parse(api_endpoint.trim()).ok()?;
    if !matches!(url.scheme(), "https" | "http") || has_extras(&url) {
        return None;
    }
    let origin = url.origin().ascii_serialization().to_ascii_lowercase();
    let path = url.path().trim_end_matches('/');
    Some(format!("{origin}{path}"))
}

/// `accountStorageKeyFromInstanceKey`: `"{instance_key}::{user_id}"`.
pub fn account_key(api_endpoint: &str, user_id: &str) -> Option<String> {
    let instance = instance_key(api_endpoint)?;
    if user_id.trim().is_empty() || user_id.contains('\0') || user_id.contains(KEY_SEPARATOR) {
        return None;
    }
    let key = format!("{instance}{KEY_SEPARATOR}{user_id}");
    (key.len() <= KEY_MAX_BYTES).then_some(key)
}

fn has_extras(url: &Url) -> bool {
    !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
}

/// Checks that the API endpoint is allowed by `policy` and that `origin`
/// is exactly its origin.
fn validate_instance(instance: &InstanceRecord, policy: &TransportPolicy) -> Result<(), &'static str> {
    let api = Url::parse(instance.api_endpoint.trim()).map_err(|_| "API endpoint is not a URL")?;
    if !matches!(api.scheme(), "https" | "http") || !policy.permits(&api) {
        return Err("API endpoint must be https (http only for permitted loopback)");
    }
    let origin = Url::parse(instance.origin.trim()).map_err(|_| "instance origin is not a URL")?;
    if has_extras(&origin) || !matches!(origin.path(), "" | "/") {
        return Err("instance origin must be a bare scheme://host[:port]");
    }
    if origin.origin() != api.origin() {
        return Err("instance origin does not match the API endpoint");
    }
    Ok(())
}

/// The cached-endpoint counterpart of discovery's checks: every URL must be
/// allowed by `policy` (https/wss, plain only for permitted loopback) and
/// the REST endpoint must be on the API endpoint's origin
/// (`requireDiscoveryDeclaresItsOwnOrigin`).
fn validate_endpoints(
    endpoints: &CachedEndpoints,
    api_endpoint: &str,
    policy: &TransportPolicy,
) -> Result<(), &'static str> {
    let check = |value: &str, gateway: bool| -> Result<Url, &'static str> {
        let url = Url::parse(value).map_err(|_| "cached endpoint is not a URL")?;
        let scheme_ok = if gateway {
            matches!(url.scheme(), "wss" | "ws")
        } else {
            matches!(url.scheme(), "https" | "http")
        };
        if !scheme_ok || !policy.permits(&url) {
            return Err("cached endpoint must be https/wss (plain only for permitted loopback)");
        }
        if has_extras(&url) {
            return Err("cached endpoint carries credentials, query or fragment");
        }
        Ok(url)
    };
    let api_origin = Url::parse(api_endpoint.trim())
        .map_err(|_| "API endpoint is not a URL")?
        .origin();
    if check(&endpoints.api, false)?.origin() != api_origin {
        return Err("cached API endpoint is not on the instance origin");
    }
    check(&endpoints.gateway, true)?;
    let optional = [
        &endpoints.media,
        &endpoints.static_cdn,
        &endpoints.upload_relay,
        &endpoints.invite,
        &endpoints.webapp,
    ];
    for value in optional.into_iter().flatten() {
        check(value, false)?;
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Default store directory: `%APPDATA%\fluxer_client` on Windows,
/// `$XDG_CONFIG_HOME/fluxer_client` or `~/.config/fluxer_client` elsewhere.
pub fn default_dir() -> Result<PathBuf, SessionError> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.filter(|p| p.is_absolute())
        .map(|p| p.join("fluxer_client"))
        .ok_or(SessionError::NoDataDir)
}

pub struct SessionStore {
    dir: PathBuf,
    policy: TransportPolicy,
    /// Held for the store's lifetime; dropping it releases the lock.
    _lock: fs::File,
    active: Option<String>,
    accounts: Vec<StoredAccount>,
    /// Unusable records, written back unchanged.
    preserved: Vec<serde_json::Value>,
    issues: Vec<LoadIssue>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("dir", &self.dir)
            .field("active", &self.active)
            .field("accounts", &self.accounts)
            .field("preserved", &self.preserved.len())
            .field("issues", &self.issues)
            .finish()
    }
}

impl SessionStore {
    /// Opens (creating if needed) the store in `dir`, requiring https/wss
    /// for every stored endpoint. Never fails because of file contents;
    /// fails on I/O errors or with [`SessionError::Locked`].
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, SessionError> {
        Self::open_with(dir, TransportPolicy::default())
    }

    /// Like [`open`](Self::open), with an explicit transport policy (set
    /// `allow_insecure_loopback` for local development instances).
    pub fn open_with(dir: impl Into<PathBuf>, policy: TransportPolicy) -> Result<Self, SessionError> {
        let dir = dir.into();
        create_private_dir(&dir)?;
        let lock = acquire_lock(&dir)?;
        remove_stale_temps(&dir);

        let mut store = Self {
            dir,
            policy,
            _lock: lock,
            active: None,
            accounts: Vec::new(),
            preserved: Vec::new(),
            issues: Vec::new(),
        };
        store.load()?;
        Ok(store)
    }

    fn file_path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    fn load(&mut self) -> Result<(), SessionError> {
        let path = self.file_path();
        let Some(bytes) = read_capped(&path)? else {
            return Ok(());
        };
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return self.quarantine(&path, "file is too large".to_owned());
        }
        let file: FileIn = match serde_json::from_slice::<FileIn>(&bytes) {
            Ok(file) => file,
            Err(e) => return self.quarantine(&path, format!("invalid JSON: {}", describe_serde(&e))),
        };
        if file.accounts.len() > MAX_RECORDS {
            return self.quarantine(&path, format!("more than {MAX_RECORDS} records"));
        }
        let migrate = match file.version {
            FORMAT_VERSION => false,
            LEGACY_FORMAT_VERSION => true,
            other => return self.quarantine(&path, format!("unsupported version {other}")),
        };

        for (index, raw) in file.accounts.into_iter().enumerate() {
            match self.check_record(index, &raw, migrate) {
                Ok(record) => self.accounts.push(record),
                Err(issue) if migrate => self.issues.push(LoadIssue::DroppedLegacyRecord(Box::new(issue))),
                Err(issue) => {
                    self.issues.push(issue);
                    self.preserved.push(raw);
                }
            }
        }

        self.active = file
            .active
            .filter(|key| self.accounts.iter().any(|a| &a.summary.key == key));
        if !self.issues.is_empty() {
            // Issues name accounts (user ids, endpoints); log only the count.
            tracing::warn!(count = self.issues.len(), "some stored session records are not usable");
        }
        if migrate {
            self.commit(self.accounts.clone(), Vec::new(), self.active.clone())?;
        }
        Ok(())
    }

    /// Parses and validates one record. For `legacy` records the token is
    /// decrypted with the version 1 entropy and re-sealed bound to the
    /// record. The decrypted token is wiped before returning.
    fn check_record(
        &mut self,
        index: usize,
        raw: &serde_json::Value,
        legacy: bool,
    ) -> Result<StoredAccount, LoadIssue> {
        let invalid = |reason: &str| LoadIssue::InvalidRecord {
            index,
            reason: reason.to_owned(),
        };
        let mut record = StoredAccount::deserialize(raw).map_err(|e| invalid(&describe_serde(&e)))?;
        let key = record.summary.key.clone();
        let expected = account_key(&record.summary.instance.api_endpoint, &record.summary.user_id);
        if expected.as_deref() != Some(key.as_str()) {
            return Err(invalid("account key does not match instance and user"));
        }
        if self.accounts.iter().any(|a| a.summary.key == key) {
            return Err(invalid("duplicate account key"));
        }
        validate_instance(&record.summary.instance, &self.policy).map_err(invalid)?;

        let undecryptable = |e: SessionError| LoadIssue::UndecryptableToken {
            key: key.clone(),
            reason: e.to_string(),
        };
        if legacy {
            let plain = record.token.decrypt_legacy().map_err(undecryptable)?;
            record.token = EncryptedToken::seal(&plain, &record.summary).map_err(undecryptable)?;
        } else {
            record.token.decrypt(&record.summary).map_err(undecryptable)?;
        }

        if let Some(endpoints) = &record.summary.instance.endpoints {
            if let Err(reason) =
                validate_endpoints(endpoints, &record.summary.instance.api_endpoint, &self.policy)
            {
                self.issues.push(LoadIssue::InvalidCachedEndpoints {
                    key: key.clone(),
                    reason,
                });
                record.summary.instance.endpoints = None;
            }
        }
        Ok(record)
    }

    fn quarantine(&mut self, path: &Path, reason: String) -> Result<(), SessionError> {
        let backup = self.dir.join(format!("{CORRUPT_PREFIX}{}", now_ms()));
        rename_with_retry(path, &backup)?;
        tracing::warn!(%reason, "session store unreadable; moved aside");
        prune_corrupt_backups(&self.dir);
        self.issues.push(LoadIssue::CorruptFile { backup, reason });
        Ok(())
    }

    /// Problems found while opening. Empty when everything loaded.
    pub fn issues(&self) -> &[LoadIssue] {
        &self.issues
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Usable stored accounts, most recently active first.
    pub fn list_accounts(&self) -> Vec<AccountSummary> {
        let mut out: Vec<AccountSummary> =
            self.accounts.iter().map(|a| a.summary.clone()).collect();
        out.sort_by(|a, b| b.last_active_ms.cmp(&a.last_active_ms));
        out
    }

    /// Adds or replaces an account (login / token refresh) and makes it the
    /// active one. Replaces an unusable record with the same key. Returns
    /// the account key.
    pub fn save_account(&mut self, account: NewAccount) -> Result<String, SessionError> {
        let key = account_key(&account.instance.api_endpoint, &account.user_id)
            .ok_or(SessionError::InvalidAccount("bad API endpoint or user id"))?;
        validate_instance(&account.instance, &self.policy).map_err(SessionError::InvalidAccount)?;
        if let Some(endpoints) = &account.instance.endpoints {
            validate_endpoints(endpoints, &account.instance.api_endpoint, &self.policy)
                .map_err(SessionError::InvalidAccount)?;
        }
        let summary = AccountSummary {
            key: key.clone(),
            user_id: account.user_id,
            instance: account.instance,
            display: account.display,
            last_active_ms: now_ms(),
        };
        let token = EncryptedToken::seal(account.token.expose().as_bytes(), &summary)?;
        let record = StoredAccount { summary, token };

        let mut accounts = self.accounts.clone();
        match accounts.iter_mut().find(|a| a.summary.key == key) {
            Some(existing) => *existing = record,
            None => accounts.push(record),
        }
        let preserved = self.preserved_without(&key);
        self.commit(accounts, preserved, Some(key.clone()))?;
        Ok(key)
    }

    /// Decrypts the token of `key`.
    pub fn load_token(&self, key: &str) -> Result<SecretToken, SessionError> {
        let record = self.find(key)?;
        let plain = record.token.decrypt(&record.summary)?;
        Ok(SecretToken::new(check_plaintext(&plain)?))
    }

    /// Deletes an account (usable or not) and its encrypted token. Clears
    /// `active` if it was the active account. Returns false if the key was
    /// not stored.
    pub fn remove_account(&mut self, key: &str) -> Result<bool, SessionError> {
        let preserved = self.preserved_without(key);
        if self.find(key).is_err() && preserved.len() == self.preserved.len() {
            return Ok(false);
        }
        let accounts: Vec<StoredAccount> = self
            .accounts
            .iter()
            .filter(|a| a.summary.key != key)
            .cloned()
            .collect();
        let active = self.active.clone().filter(|k| k != key);
        self.commit(accounts, preserved, active)?;
        Ok(true)
    }

    /// Makes `key` the active account and bumps its `last_active_ms`.
    pub fn set_active(&mut self, key: &str) -> Result<(), SessionError> {
        self.find(key)?;
        let mut accounts = self.accounts.clone();
        let now = now_ms();
        for a in &mut accounts {
            if a.summary.key == key {
                a.summary.last_active_ms = now;
            }
        }
        self.commit(accounts, self.preserved.clone(), Some(key.to_owned()))
    }

    /// The active account, if one is set.
    pub fn active_account(&self) -> Option<AccountSummary> {
        let key = self.active.as_deref()?;
        self.find(key).ok().map(|a| a.summary.clone())
    }

    fn find(&self, key: &str) -> Result<&StoredAccount, SessionError> {
        self.accounts
            .iter()
            .find(|a| a.summary.key == key)
            .ok_or(SessionError::UnknownAccount)
    }

    fn preserved_without(&self, key: &str) -> Vec<serde_json::Value> {
        self.preserved
            .iter()
            .filter(|raw| raw.get("key").and_then(serde_json::Value::as_str) != Some(key))
            .cloned()
            .collect()
    }

    /// Writes the new state to disk first, then adopts it in memory, so a
    /// failed write leaves memory consistent with the file.
    fn commit(
        &mut self,
        accounts: Vec<StoredAccount>,
        preserved: Vec<serde_json::Value>,
        active: Option<String>,
    ) -> Result<(), SessionError> {
        let records: Vec<OutRecord<'_>> = accounts
            .iter()
            .map(OutRecord::Account)
            .chain(preserved.iter().map(OutRecord::Raw))
            .collect();
        let bytes = serde_json::to_vec_pretty(&FileOut {
            version: FORMAT_VERSION,
            active: active.as_deref(),
            accounts: &records,
        })
        .map_err(|e| SessionError::Serialize(describe_serde(&e)))?;
        drop(records);
        let temp = write_temp(&self.dir, &bytes)?;
        if let Err(e) = rename_with_retry(&temp, &self.file_path()) {
            remove_quietly(&temp);
            return Err(e);
        }
        // The rename happened, so the file holds the new state even if the
        // directory sync below fails; memory must follow it.
        self.accounts = accounts;
        self.preserved = preserved;
        self.active = active;
        sync_dir(&self.dir);
        Ok(())
    }
}

/// Opens `sessions.lock` exclusively: share mode none on Windows, an
/// advisory `flock`-style lock elsewhere.
fn acquire_lock(dir: &Path) -> Result<fs::File, SessionError> {
    let path = dir.join(LOCK_FILE_NAME);
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
            return Err(SessionError::Locked { path });
        }
        Err(e) => return Err(io_err(&path)(e)),
    };
    #[cfg(not(windows))]
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Err(SessionError::Locked { path }),
        Err(fs::TryLockError::Error(e)) => return Err(io_err(&path)(e)),
    }
    Ok(file)
}

/// Deletes temp files left by writers that crashed more than
/// [`STALE_TEMP_AGE`] ago. Newer ones are left alone. Best effort.
fn remove_stale_temps(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "cannot list session store directory");
            return;
        }
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let is_temp = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(TEMP_PREFIX));
        let Ok(meta) = entry.metadata() else { continue };
        let stale = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_TEMP_AGE);
        if is_temp && meta.is_file() && stale {
            remove_quietly(&entry.path());
        }
    }
}

/// Keeps only the newest [`MAX_CORRUPT_BACKUPS`] quarantined files. Best
/// effort.
fn prune_corrupt_backups(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut backups: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let stamp = name.to_str()?.strip_prefix(CORRUPT_PREFIX)?.parse::<u64>().ok()?;
            Some((stamp, entry.path()))
        })
        .collect();
    backups.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in backups.into_iter().skip(MAX_CORRUPT_BACKUPS) {
        remove_quietly(&path);
    }
}

fn remove_quietly(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, "could not delete session store file");
        }
    }
}

fn read_capped(path: &Path) -> Result<Option<Vec<u8>>, SessionError> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(path)(e)),
    };
    let mut bytes = Vec::new();
    // One byte past the cap tells an oversized file apart.
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_err(path))?;
    Ok(Some(bytes))
}

fn create_private_dir(dir: &Path) -> Result<(), SessionError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(io_err(dir))
}

/// Writes `bytes` to a new, uniquely named temp file and fsyncs it.
/// Returns the temp path. The file is removed again if writing fails.
fn write_temp(dir: &Path, bytes: &[u8]) -> Result<PathBuf, SessionError> {
    let name = format!(
        "{TEMP_PREFIX}-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    );
    let temp = dir.join(name);
    let mut options = fs::OpenOptions::new();
    // create_new: never follow or reuse a planted file or link.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(io_err(&temp))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        remove_quietly(&temp);
        return Err(io_err(&temp)(e));
    }
    Ok(temp)
}

/// True for the errors `renameWithRetry` treats as transient on Windows
/// (EPERM / EACCES / EBUSY): an antivirus, indexer or backup tool briefly
/// holding the target open.
#[cfg(windows)]
fn is_transient_rename_error(e: &std::io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
    )
}

/// Atomically replaces `to` with `from` (MoveFileEx with REPLACE_EXISTING
/// on Windows, rename(2) on Unix). On Windows, transient sharing errors are
/// retried with the backoff of `WINDOWS_TRANSIENT_FS_RETRY_DELAYS_MS`.
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), SessionError> {
    #[cfg(windows)]
    for delay in RENAME_RETRY_DELAYS_MS {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if is_transient_rename_error(&e) => {
                std::thread::sleep(Duration::from_millis(delay));
            }
            Err(e) => return Err(io_err(to)(e)),
        }
    }
    fs::rename(from, to).map_err(io_err(to))
}

/// Makes a completed rename durable where the platform allows it (Unix).
/// A failure is only logged: the rename already happened, so the caller
/// adopts the new state either way.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Err(e) = fs::File::open(dir).and_then(|d| d.sync_all()) {
        tracing::warn!(error = %e, "session store directory sync failed; last write may not survive a power loss");
    }
    #[cfg(not(unix))]
    let _ = dir;
}
