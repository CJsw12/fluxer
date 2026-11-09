// SPDX-License-Identifier: AGPL-3.0-or-later

#[cfg(feature = "node")]
use napi::Task;
#[cfg(feature = "node")]
use napi::bindgen_prelude::{AsyncTask, Env, Error as NapiError, Result as NapiResult, Status as NapiStatus};
#[cfg(feature = "node")]
use napi_derive::napi;

#[derive(Debug, thiserror::Error)]
pub enum WinShellError {
    #[error("Win32 error: {label} (code {status})")]
    Win32 { label: &'static str, status: u32 },
    #[error("HRESULT error: {label} (hr 0x{hr:08x})")]
    HResult { label: &'static str, hr: u32 },
    #[error("Invalid argument: {property} is required and must be non-empty")]
    InvalidArg { property: &'static str },
    #[error("Invalid UTF-16 in registry value")]
    InvalidUtf16,
    #[error("win-shell not supported on this platform")]
    UnsupportedPlatform,
}

#[cfg(feature = "node")]
impl From<WinShellError> for NapiError {
    fn from(err: WinShellError) -> Self {
        match err {
            WinShellError::InvalidArg { property } => {
                NapiError::new(NapiStatus::InvalidArg, format!("{property} is required and must be non-empty"))
            }
            other => NapiError::new(NapiStatus::GenericFailure, other.to_string()),
        }
    }
}

#[cfg_attr(feature = "node", napi(object))]
#[derive(Debug, Clone, Default)]
pub struct CreateShortcutOptions {
    #[cfg_attr(feature = "node", napi(js_name = "lnkPath"))]
    pub lnk_path: Option<String>,
    pub target: Option<String>,
    pub args: Option<String>,
    #[cfg_attr(feature = "node", napi(js_name = "appUserModelId"))]
    pub app_user_model_id: Option<String>,
    #[cfg_attr(feature = "node", napi(js_name = "toastActivatorClsid"))]
    pub toast_activator_clsid: Option<String>,
    #[cfg_attr(feature = "node", napi(js_name = "iconPath"))]
    pub icon_path: Option<String>,
    #[cfg_attr(feature = "node", napi(js_name = "iconIndex"))]
    pub icon_index: Option<i32>,
    #[cfg_attr(feature = "node", napi(js_name = "workingDir"))]
    pub working_dir: Option<String>,
    pub description: Option<String>,
}

#[cfg_attr(feature = "node", napi(object))]
#[derive(Debug, Clone, Default)]
pub struct SetRunValueOptions {
    pub name: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutOptions {
    pub lnk_path: String,
    pub target: String,
    pub args: Option<String>,
    pub app_user_model_id: Option<String>,
    pub toast_activator_clsid: Option<String>,
    pub icon_path: Option<String>,
    pub icon_index: i32,
    pub working_dir: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValueOptions {
    pub name: String,
    pub command: String,
}

// =========================================================================
// 순수 Safe Rust 동기 공개 API (Electron / Node 의존성 없음)
// =========================================================================

pub fn create_shortcut_sync(opts: &ShortcutOptions) -> Result<(), WinShellError> {
    platform::create_shortcut(opts)
}

pub fn set_current_user_run_value_sync(name: &str, command: &str) -> Result<(), WinShellError> {
    if name.is_empty() {
        return Err(WinShellError::InvalidArg { property: "name" });
    }
    if command.is_empty() {
        return Err(WinShellError::InvalidArg { property: "command" });
    }
    platform::set_current_user_run_value(name, command)
}

pub fn delete_current_user_run_value_sync(name: &str) -> Result<(), WinShellError> {
    if name.is_empty() {
        return Err(WinShellError::InvalidArg { property: "name" });
    }
    platform::delete_current_user_run_value(name)
}

pub fn get_current_user_run_value_sync(name: &str) -> Result<Option<String>, WinShellError> {
    if name.is_empty() {
        return Err(WinShellError::InvalidArg { property: "name" });
    }
    platform::get_current_user_run_value(name)
}

pub fn get_user_notification_state_sync() -> Result<String, WinShellError> {
    platform::get_user_notification_state()
}

pub fn set_taskbar_overlay_icon_sync(
    hwnd: isize,
    icon_bytes: Option<&[u8]>,
    description: &str,
) -> Result<(), WinShellError> {
    platform::set_taskbar_overlay_icon(hwnd, icon_bytes, description)
}

pub fn validate_shortcut_options(opts: CreateShortcutOptions) -> Result<ShortcutOptions, WinShellError> {
    Ok(ShortcutOptions {
        lnk_path: require_non_empty(opts.lnk_path, "lnkPath")?,
        target: require_non_empty(opts.target, "target")?,
        args: opts.args,
        app_user_model_id: opts.app_user_model_id,
        toast_activator_clsid: opts.toast_activator_clsid,
        icon_path: opts.icon_path,
        icon_index: opts.icon_index.unwrap_or(0),
        working_dir: opts.working_dir,
        description: opts.description,
    })
}

pub fn validate_run_value_options(opts: SetRunValueOptions) -> Result<RunValueOptions, WinShellError> {
    Ok(RunValueOptions {
        name: require_non_empty(opts.name, "name")?,
        command: require_non_empty(opts.command, "command")?,
    })
}

fn require_non_empty(value: Option<String>, property: &'static str) -> Result<String, WinShellError> {
    match value {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(WinShellError::InvalidArg { property }),
    }
}

// =========================================================================
// Electron Node.js N-API 호환 래퍼 계층 (`feature = "node"` 전용)
// =========================================================================

#[cfg(feature = "node")]
pub struct CreateShortcutTask {
    options: ShortcutOptions,
}

#[cfg(feature = "node")]
pub struct SetRunValueTask {
    options: RunValueOptions,
}

#[cfg(feature = "node")]
pub struct DeleteRunValueTask {
    name: String,
}

#[cfg(feature = "node")]
pub struct GetRunValueTask {
    name: String,
}

#[cfg(feature = "node")]
#[napi(js_name = "createShortcut")]
pub fn create_shortcut(opts: CreateShortcutOptions) -> NapiResult<AsyncTask<CreateShortcutTask>> {
    let options = validate_shortcut_options(opts)?;
    Ok(AsyncTask::new(CreateShortcutTask { options }))
}

#[cfg(feature = "node")]
#[napi(js_name = "setCurrentUserRunValue")]
pub fn set_current_user_run_value(opts: SetRunValueOptions) -> NapiResult<AsyncTask<SetRunValueTask>> {
    let options = validate_run_value_options(opts)?;
    Ok(AsyncTask::new(SetRunValueTask { options }))
}

#[cfg(feature = "node")]
#[napi(js_name = "deleteCurrentUserRunValue")]
pub fn delete_current_user_run_value(name: String) -> NapiResult<AsyncTask<DeleteRunValueTask>> {
    let name = require_non_empty(Some(name), "name")?;
    Ok(AsyncTask::new(DeleteRunValueTask { name }))
}

#[cfg(feature = "node")]
#[napi(js_name = "getCurrentUserRunValue")]
pub fn get_current_user_run_value(name: String) -> NapiResult<AsyncTask<GetRunValueTask>> {
    let name = require_non_empty(Some(name), "name")?;
    Ok(AsyncTask::new(GetRunValueTask { name }))
}

#[cfg(all(feature = "node", target_os = "windows"))]
#[napi(js_name = "getUserNotificationState")]
pub fn get_user_notification_state() -> NapiResult<String> {
    get_user_notification_state_sync().map_err(Into::into)
}

#[cfg(feature = "node")]
impl Task for CreateShortcutTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> NapiResult<Self::Output> {
        create_shortcut_sync(&self.options).map_err(Into::into)
    }

    fn resolve(&mut self, _env: Env, _output: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(())
    }
}

#[cfg(feature = "node")]
impl Task for SetRunValueTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> NapiResult<Self::Output> {
        set_current_user_run_value_sync(&self.options.name, &self.options.command).map_err(Into::into)
    }

    fn resolve(&mut self, _env: Env, _output: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(())
    }
}

#[cfg(feature = "node")]
impl Task for DeleteRunValueTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> NapiResult<Self::Output> {
        delete_current_user_run_value_sync(&self.name).map_err(Into::into)
    }

    fn resolve(&mut self, _env: Env, _output: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(())
    }
}

#[cfg(feature = "node")]
impl Task for GetRunValueTask {
    type Output = Option<String>;
    type JsValue = Option<String>;

    fn compute(&mut self) -> NapiResult<Self::Output> {
        get_current_user_run_value_sync(&self.name).map_err(Into::into)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> NapiResult<Self::JsValue> {
        Ok(output)
    }
}

// =========================================================================
// Windows 플랫폼 실제 네이티브 구현부
// =========================================================================

#[cfg(target_os = "windows")]
mod platform {
    use super::{ShortcutOptions, WinShellError};
    use std::ffi::c_void;
    use std::mem::ManuallyDrop;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HWND, PROPERTYKEY, WIN32_ERROR,
    };
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED, CoCreateInstance,
        CoInitializeEx, CoUninitialize, IPersistFile,
    };
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    };
    use windows::Win32::System::Variant::{VT_CLSID, VT_LPWSTR};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        IShellLinkW, ITaskbarList3, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_APP, QUNS_BUSY, QUNS_NOT_PRESENT,
        QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
        SHQueryUserNotificationState, ShellLink, TaskbarList,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, DestroyIcon, HICON, LR_DEFAULTCOLOR,
    };
    use windows::core::{GUID, Interface, PCWSTR, PWSTR};

    static LAST_OVERLAY_HICON: Mutex<isize> = Mutex::new(0);

    const CURRENT_USER_RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };
    const PKEY_APP_USER_MODEL_TOAST_ACTIVATOR_CLSID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 26,
    };

    pub(super) fn get_user_notification_state() -> Result<String, WinShellError> {
        let state = unsafe { SHQueryUserNotificationState() }
            .map_err(|err| hresult_error("SHQueryUserNotificationState failed", err))?;
        let label = match state.0 {
            value if value == QUNS_NOT_PRESENT.0 => "not-present",
            value if value == QUNS_BUSY.0 => "busy",
            value if value == QUNS_RUNNING_D3D_FULL_SCREEN.0 => "running-d3d-full-screen",
            value if value == QUNS_PRESENTATION_MODE.0 => "presentation-mode",
            value if value == QUNS_ACCEPTS_NOTIFICATIONS.0 => "accepts-notifications",
            value if value == QUNS_QUIET_TIME.0 => "quiet-time",
            value if value == QUNS_APP.0 => "app",
            _ => "unknown",
        };
        Ok(label.to_string())
    }

    pub(super) fn set_taskbar_overlay_icon(
        hwnd: isize,
        icon_bytes: Option<&[u8]>,
        description: &str,
    ) -> Result<(), WinShellError> {
        if hwnd == 0 {
            return Ok(());
        }

        let _com = ComApartment::initialize()?;

        unsafe {
            let taskbar: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| hresult_error("CoCreateInstance(CLSID_TaskbarList) failed", err))?;

            taskbar
                .HrInit()
                .map_err(|err| hresult_error("ITaskbarList3::HrInit failed", err))?;

            let mut last_hicon = LAST_OVERLAY_HICON.lock().unwrap();
            if *last_hicon != 0 {
                let _ = DestroyIcon(HICON(*last_hicon as *mut c_void));
                *last_hicon = 0;
            }

            let Some(bytes) = icon_bytes else {
                let _ = taskbar.SetOverlayIcon(
                    HWND(hwnd as *mut c_void),
                    HICON(std::ptr::null_mut()),
                    PCWSTR::null(),
                );
                return Ok(());
            };

            let hicon = CreateIconFromResourceEx(
                bytes,
                true,
                0x00030000,
                16,
                16,
                LR_DEFAULTCOLOR,
            )
            .map_err(|err| hresult_error("CreateIconFromResourceEx failed", err))?;

            *last_hicon = hicon.0 as isize;
            let desc = wide_null(description);
            taskbar
                .SetOverlayIcon(HWND(hwnd as *mut c_void), hicon, PCWSTR(desc.as_ptr()))
                .map_err(|err| hresult_error("ITaskbarList3::SetOverlayIcon failed", err))?;
        }

        Ok(())
    }

    pub(super) fn create_shortcut(opts: &ShortcutOptions) -> Result<(), WinShellError> {
        let _com = ComApartment::initialize()?;

        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| hresult_error("CoCreateInstance(CLSID_ShellLink) failed", err))?;

            let target = wide_null(&opts.target);
            link.SetPath(PCWSTR(target.as_ptr()))
                .map_err(|err| hresult_error("IShellLinkW::SetPath failed", err))?;

            if let Some(args) = opts.args.as_deref() {
                let args = wide_null(args);
                link.SetArguments(PCWSTR(args.as_ptr()))
                    .map_err(|err| hresult_error("IShellLinkW::SetArguments failed", err))?;
            }

            if let Some(icon_path) = opts.icon_path.as_deref() {
                let icon_path = wide_null(icon_path);
                link.SetIconLocation(PCWSTR(icon_path.as_ptr()), opts.icon_index)
                    .map_err(|err| hresult_error("IShellLinkW::SetIconLocation failed", err))?;
            }

            if let Some(working_dir) = opts.working_dir.as_deref() {
                let working_dir = wide_null(working_dir);
                link.SetWorkingDirectory(PCWSTR(working_dir.as_ptr()))
                    .map_err(|err| hresult_error("IShellLinkW::SetWorkingDirectory failed", err))?;
            }

            if let Some(description) = opts.description.as_deref() {
                let description = wide_null(description);
                link.SetDescription(PCWSTR(description.as_ptr()))
                    .map_err(|err| hresult_error("IShellLinkW::SetDescription failed", err))?;
            }

            if opts.app_user_model_id.is_some() || opts.toast_activator_clsid.is_some() {
                let property_store: IPropertyStore = link
                    .cast()
                    .map_err(|err| hresult_error("QueryInterface(IPropertyStore) failed", err))?;
                if let Some(app_user_model_id) = opts.app_user_model_id.as_deref() {
                    set_string_property(
                        &property_store,
                        &PKEY_APP_USER_MODEL_ID,
                        app_user_model_id,
                        "IPropertyStore::SetValue(PKEY_AppUserModel_ID) failed",
                    )?;
                }
                if let Some(toast_activator_clsid) = opts.toast_activator_clsid.as_deref() {
                    set_guid_property(
                        &property_store,
                        &PKEY_APP_USER_MODEL_TOAST_ACTIVATOR_CLSID,
                        toast_activator_clsid,
                        "IPropertyStore::SetValue(PKEY_AppUserModel_ToastActivatorCLSID) failed",
                    )?;
                }
                property_store
                    .Commit()
                    .map_err(|err| hresult_error("IPropertyStore::Commit failed", err))?;
            }

            let persist: IPersistFile = link
                .cast()
                .map_err(|err| hresult_error("QueryInterface(IPersistFile) failed", err))?;
            let lnk_path = wide_null(&opts.lnk_path);
            persist
                .Save(PCWSTR(lnk_path.as_ptr()), true)
                .map_err(|err| hresult_error("IPersistFile::Save failed", err))?;
        }

        Ok(())
    }

    pub(super) fn set_current_user_run_value(name: &str, command: &str) -> Result<(), WinShellError> {
        let key = RunKey::create()?;
        let name = wide_null(name);
        let command = wide_null(command);
        let data =
            unsafe { std::slice::from_raw_parts(command.as_ptr().cast::<u8>(), command.len() * 2) };
        let status =
            unsafe { RegSetValueExW(key.raw(), PCWSTR(name.as_ptr()), None, REG_SZ, Some(data)) };
        check_win32(status, "RegSetValueExW(HKCU Run) failed")
    }

    pub(super) fn delete_current_user_run_value(name: &str) -> Result<(), WinShellError> {
        let Some(key) = RunKey::open_for_set()? else {
            return Ok(());
        };
        let name = wide_null(name);
        let status = unsafe { RegDeleteValueW(key.raw(), PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check_win32(status, "RegDeleteValueW(HKCU Run) failed")
    }

    pub(super) fn get_current_user_run_value(name: &str) -> Result<Option<String>, WinShellError> {
        let subkey = wide_null(CURRENT_USER_RUN_SUBKEY);
        let name = wide_null(name);
        let mut value_type = REG_SZ;
        let mut byte_len = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                Some(&mut value_type),
                None,
                Some(&mut byte_len),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check_win32(status, "RegGetValueW(HKCU Run size) failed")?;
        if byte_len == 0 {
            return Ok(Some(String::new()));
        }

        let mut buffer = vec![0u16; byte_len.div_ceil(2) as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                Some(&mut value_type),
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                Some(&mut byte_len),
            )
        };
        check_win32(status, "RegGetValueW(HKCU Run data) failed")?;
        let mut code_units = (byte_len / 2) as usize;
        while code_units > 0 && buffer[code_units - 1] == 0 {
            code_units -= 1;
        }
        String::from_utf16(&buffer[..code_units])
            .map(Some)
            .map_err(|_| WinShellError::InvalidUtf16)
    }

    struct RunKey(HKEY);

    impl RunKey {
        fn create() -> Result<Self, WinShellError> {
            let subkey = wide_null(CURRENT_USER_RUN_SUBKEY);
            let mut key = HKEY::default();
            let status = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(subkey.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
            };
            check_win32(status, "RegCreateKeyExW(HKCU Run) failed")?;
            Ok(Self(key))
        }

        fn open_for_set() -> Result<Option<Self>, WinShellError> {
            let subkey = wide_null(CURRENT_USER_RUN_SUBKEY);
            let mut key = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(subkey.as_ptr()),
                    None,
                    KEY_SET_VALUE,
                    &mut key,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            check_win32(status, "RegOpenKeyExW(HKCU Run) failed")?;
            Ok(Some(Self(key)))
        }

        fn raw(&self) -> HKEY {
            self.0
        }
    }

    impl Drop for RunKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    struct ComApartment;

    impl ComApartment {
        fn initialize() -> Result<Self, WinShellError> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|err| hresult_error("CoInitializeEx failed", err))?;
            }
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn propvariant_from_wide_string(wide: &mut [u16]) -> PROPVARIANT {
        PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        pwszVal: PWSTR(wide.as_mut_ptr()),
                    },
                }),
            },
        }
    }

    fn propvariant_from_guid(guid: &mut GUID) -> PROPVARIANT {
        PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_CLSID,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        puuid: guid as *mut GUID,
                    },
                }),
            },
        }
    }

    fn set_string_property(
        property_store: &IPropertyStore,
        key: &PROPERTYKEY,
        value: &str,
        label: &'static str,
    ) -> Result<(), WinShellError> {
        let mut wide = wide_null(value);
        unsafe {
            let prop = ManuallyDrop::new(propvariant_from_wide_string(&mut wide));
            property_store
                .SetValue(key as *const PROPERTYKEY, &*prop as *const PROPVARIANT)
                .map_err(|err| hresult_error(label, err))
        }
    }

    fn set_guid_property(
        property_store: &IPropertyStore,
        key: &PROPERTYKEY,
        value: &str,
        label: &'static str,
    ) -> Result<(), WinShellError> {
        let wide = wide_null(value);
        unsafe {
            let mut guid = CLSIDFromString(PCWSTR(wide.as_ptr()))
                .map_err(|err| hresult_error("CLSIDFromString failed", err))?;
            let prop = ManuallyDrop::new(propvariant_from_guid(&mut guid));
            property_store
                .SetValue(key as *const PROPERTYKEY, &*prop as *const PROPVARIANT)
                .map_err(|err| hresult_error(label, err))
        }
    }

    fn hresult_error(label: &'static str, err: windows::core::Error) -> WinShellError {
        WinShellError::HResult {
            label,
            hr: err.code().0 as u32,
        }
    }

    fn check_win32(status: WIN32_ERROR, label: &'static str) -> Result<(), WinShellError> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(WinShellError::Win32 {
                label,
                status: status.0,
            })
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{ShortcutOptions, WinShellError};

    pub(super) fn create_shortcut(_opts: &ShortcutOptions) -> Result<(), WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }

    pub(super) fn set_current_user_run_value(_name: &str, _command: &str) -> Result<(), WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }

    pub(super) fn delete_current_user_run_value(_name: &str) -> Result<(), WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }

    pub(super) fn get_current_user_run_value(_name: &str) -> Result<Option<String>, WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }

    pub(super) fn get_user_notification_state() -> Result<String, WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }

    pub(super) fn set_taskbar_overlay_icon(
        _hwnd: isize,
        _icon_bytes: Option<&[u8]>,
        _description: &str,
    ) -> Result<(), WinShellError> {
        Err(WinShellError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_empty_property() {
        let res = validate_shortcut_options(CreateShortcutOptions::default());
        assert!(res.is_err());
    }

    #[test]
    fn test_run_value_options_validation() {
        let res = validate_run_value_options(SetRunValueOptions {
            name: Some("FluxerTest".to_string()),
            command: Some("C:\\Fluxer\\fluxer.exe".to_string()),
        });
        assert!(res.is_ok());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_notification_state_query() {
        let state = get_user_notification_state_sync();
        assert!(state.is_ok());
        println!("Current Windows notification state: {}", state.unwrap());
    }
}
