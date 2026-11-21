//! Windows release integration: the process AppUserModelID, the Start menu
//! shortcut that carries it, the file log, and the fatal-error dialog.
//!
//! All Win32/COM calls are confined to this module. COM for the shortcut is
//! initialized on a dedicated thread (apartment-threaded) so it cannot disturb
//! gpui's own COM setup on the main thread.

/// AppUserModelID shared by the process, the Start menu shortcut, and the
/// `notify-rust` toast `app_id`; Windows attributes all three to one app.
pub const APP_USER_MODEL_ID: &str = "Fluxer.Client.Rust";

/// Human-readable name used by toasts, the shortcut and the error dialog.
pub const APP_DISPLAY_NAME: &str = "Fluxer";

/// File name inside `%LOCALAPPDATA%\FluxerClient\logs`.
const LOG_FILE_NAME: &str = "client.log";
/// Previous log kept by the size rotation.
const LOG_PREVIOUS_FILE_NAME: &str = "client.prev.log";
/// The log is rotated once at startup above this size; one previous file is kept.
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

#[cfg(target_os = "windows")]
const SHORTCUT_FILE_NAME: &str = "Fluxer Client.lnk";

/// Registry subkey under `HKEY_CURRENT_USER` holding per-user startup items.
#[cfg_attr(test, allow(dead_code))]
pub const RUN_REGISTRY_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Registry subkey under `HKEY_CURRENT_USER` holding the `fluxer://` URL handler.
#[cfg_attr(test, allow(dead_code))]
pub const PROTOCOL_REGISTRY_KEY: &str = r"Software\Classes\fluxer";

static OS_INTEGRATION_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Enables or disables OS integration (Start menu shortcut, protocol handler,
/// and autostart registry writes). Intended for `--no-os-integration` isolated runs.
pub fn set_os_integration_enabled(enabled: bool) {
    OS_INTEGRATION_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);
}

/// Returns whether OS integration (registry and shortcut writes) is enabled.
pub fn os_integration_enabled() -> bool {
    OS_INTEGRATION_ENABLED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Returns whether the host OS currently prefers reduced motion.
pub fn system_prefers_reduced_motion() -> bool {
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };

        let mut enabled: u32 = 1;
        // SAFETY: `enabled` is a stack-allocated 32-bit `BOOL` out-parameter for
        // `SPI_GETCLIENTAREAANIMATION`, and `SystemParametersInfoW` writes one `BOOL`.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut enabled as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        ok.is_ok() && enabled == 0
    }
    #[cfg(not(all(target_os = "windows", not(test))))]
    {
        false
    }
}

/// Resolves the user's reduced-motion preference into an effective boolean.
pub fn resolve_reduced_motion(pref: crate::prefs::ReducedMotionPref) -> bool {
    match pref {
        crate::prefs::ReducedMotionPref::System => system_prefers_reduced_motion(),
        crate::prefs::ReducedMotionPref::On => true,
        crate::prefs::ReducedMotionPref::Off => false,
    }
}

/// Formats the HKCU `Run` command for launching `exe` in autostart mode.
pub fn format_autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

/// Formats the Windows URL protocol command registered for `fluxer://` links.
pub fn format_protocol_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --url -- \"%1\"", exe.display())
}

/// Registers or removes the per-user Windows `Run` entry for Fluxer.
pub fn set_autostart_enabled(enabled: bool) -> anyhow::Result<()> {
    if !os_integration_enabled() {
        return Ok(());
    }
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use anyhow::Context as _;

        let key = windows_registry::CURRENT_USER
            .create(RUN_REGISTRY_KEY)
            .context("could not open HKCU Run registry key")?;
        if enabled {
            let exe = std::env::current_exe().context("could not locate the running executable")?;
            key.set_string(APP_USER_MODEL_ID, &format_autostart_command(&exe))
                .context("could not write the autostart Run value")?;
        } else if key.get_value(APP_USER_MODEL_ID).is_ok() {
            key.remove_value(APP_USER_MODEL_ID)
                .context("could not remove the autostart Run value")?;
        }
        Ok(())
    }
    #[cfg(not(all(target_os = "windows", not(test))))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// Refreshes the autostart registry entry on startup when enabled, keeping the
/// registered executable path up to date if the binary was moved.
pub fn sync_autostart_on_startup(enabled: bool) {
    if !enabled || !os_integration_enabled() {
        return;
    }
    if let Err(err) = set_autostart_enabled(true) {
        tracing::warn!(error = %format!("{err:#}"), "could not refresh the autostart Run value");
    }
}

/// Installs the file logger, sets the process AUMID, and ensures the Start menu
/// shortcut exists. Called once at the very start of `main`, before the window.
pub fn init() {
    logging::init();
    install_panic_hook();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "Fluxer client starting"
    );
    set_app_user_model_id();
    ensure_start_menu_shortcut();
}

/// Registers `HKCU\Software\Classes\fluxer` for the current executable if missing or stale.
pub fn ensure_protocol_handler() {
    if !os_integration_enabled() {
        return;
    }
    #[cfg(all(target_os = "windows", not(test)))]
    {
        if let Err(err) = register_protocol_handler() {
            tracing::warn!(error = %format!("{err:#}"), "could not register the fluxer:// protocol handler");
        }
    }
}

#[cfg(all(target_os = "windows", not(test)))]
fn register_protocol_handler() -> anyhow::Result<()> {
    use anyhow::Context as _;

    let exe = std::env::current_exe().context("could not locate the running executable")?;
    let expected_cmd = format_protocol_command(&exe);
    let cmd_path = format!(r"{PROTOCOL_REGISTRY_KEY}\shell\open\command");
    if let Ok(existing) = windows_registry::CURRENT_USER.open(&cmd_path)
        && existing.get_string("").ok().as_deref() == Some(expected_cmd.as_str())
    {
        return Ok(());
    }

    let base = windows_registry::CURRENT_USER
        .create(PROTOCOL_REGISTRY_KEY)
        .context("could not open HKCU fluxer protocol key")?;
    base.set_string("", "URL:Fluxer Protocol")?;
    base.set_string("URL Protocol", "")?;

    let icon = windows_registry::CURRENT_USER
        .create(format!(r"{PROTOCOL_REGISTRY_KEY}\DefaultIcon"))
        .context("could not open DefaultIcon key")?;
    icon.set_string("", &format!("\"{}\",0", exe.display()))?;

    let cmd = windows_registry::CURRENT_USER
        .create(&cmd_path)
        .context("could not open shell command key")?;
    cmd.set_string("", &expected_cmd)?;
    Ok(())
}

/// Release builds have no console, so record panics in the log file.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        tracing::error!(thread = name, %location, %message, "panic");
        previous(info);
    }));
}

/// Shows a modal error dialog when startup fails under the Windows subsystem,
/// where stderr is not visible.
pub fn show_fatal_error(message: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::HSTRING;

        let text = HSTRING::from(message);
        let caption = HSTRING::from(APP_DISPLAY_NAME);
        // SAFETY: both HSTRINGs are valid NUL-terminated wide strings for the
        // duration of this modal call, which returns before they are dropped.
        unsafe {
            MessageBoxW(None, &text, &caption, MB_ICONERROR | MB_OK);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("{APP_DISPLAY_NAME}: {message}");
    }
}

#[cfg(target_os = "windows")]
fn set_app_user_model_id() {
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::core::HSTRING;

    let id = HSTRING::from(APP_USER_MODEL_ID);
    // SAFETY: the HSTRING is a valid NUL-terminated wide string for the call.
    if let Err(err) = unsafe { SetCurrentProcessExplicitAppUserModelID(&id) } {
        tracing::warn!(%err, "could not set the application user model ID");
    }
}

#[cfg(not(target_os = "windows"))]
fn set_app_user_model_id() {}

#[cfg(target_os = "windows")]
fn ensure_start_menu_shortcut() {
    if !os_integration_enabled() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("fluxer-shortcut".to_owned())
        .spawn(|| {
            if let Err(err) = windows_shortcut::create_if_missing() {
                tracing::warn!(error = %format!("{err:#}"), "could not create the Start menu shortcut");
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "could not start the shortcut thread");
    }
}

#[cfg(not(target_os = "windows"))]
fn ensure_start_menu_shortcut() {}

#[cfg(target_os = "windows")]
mod windows_shortcut {
    use std::path::Path;

    use anyhow::{Context as _, Result, anyhow};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::IPersistFile;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{GUID, HSTRING, Interface};

    /// `PKEY_AppUserModel_ID`; not re-exported by the `windows` version in use.
    const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

    pub(super) fn create_if_missing() -> Result<()> {
        let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
        let programs = std::path::PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        std::fs::create_dir_all(&programs).context("could not create the Start menu Programs folder")?;

        let shortcut = programs.join(super::SHORTCUT_FILE_NAME);
        // Always (re)write it: Save overwrites, which repairs a link that points
        // at an old exe path or lacks the AppUserModelID.

        let exe = std::env::current_exe().context("could not locate the running executable")?;
        create(&shortcut, &exe)?;
        tracing::info!(path = %shortcut.display(), "created the Start menu shortcut");
        Ok(())
    }

    fn create(shortcut: &Path, exe: &Path) -> Result<()> {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

        // SAFETY: called on the dedicated shortcut thread. The shell link
        // property store requires a single-threaded apartment.
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        initialized
            .ok()
            .map_err(|err| anyhow!("COM initialization failed: {err}"))?;

        let result = create_link(shortcut, exe);
        // SAFETY: balances the successful CoInitializeEx above on this thread.
        unsafe { CoUninitialize() };
        result
    }

    fn create_link(shortcut: &Path, exe: &Path) -> Result<()> {
        use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

        // SAFETY: COM is initialized on this thread and both the CLSID and the
        // returned interface are used only here.
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .context("could not create the shell link object")?;

        let target = HSTRING::from(exe);
        let working_dir = exe.parent().map(HSTRING::from).unwrap_or_default();
        let destination = HSTRING::from(shortcut);
        let description = HSTRING::from(super::APP_DISPLAY_NAME);

        // SAFETY: `link` is a live COM interface and every HSTRING is a valid
        // NUL-terminated wide string that outlives the calls.
        unsafe {
            link.SetPath(&target).context("could not set the shortcut target")?;
            if !working_dir.is_empty() {
                link.SetWorkingDirectory(&working_dir)
                    .context("could not set the shortcut working directory")?;
            }
            link.SetDescription(&description).context("could not set the shortcut description")?;
            link.SetIconLocation(&target, 0).context("could not set the shortcut icon")?;
        }

        let store: IPropertyStore = link.cast().context("the shell link has no property store")?;
        // PKEY_AppUserModel_ID expects VT_LPWSTR. `From<&str>` allocates the
        // string with CoTaskMemAlloc, so PROPVARIANT's Drop (PropVariantClear)
        // frees memory it owns.
        let value = PROPVARIANT::from(super::APP_USER_MODEL_ID);
        // SAFETY: `store` is a live interface; `value` outlives the call.
        unsafe {
            store
                .SetValue(&PKEY_APP_USER_MODEL_ID, &value)
                .context("could not set the shortcut AppUserModelID")?;
            store.Commit().context("could not commit the shortcut properties")?;
        }

        let file: IPersistFile = link.cast().context("the shell link cannot be saved")?;
        // SAFETY: `file` is a live interface and the destination is a valid path.
        unsafe { file.Save(&destination, true) }.context("could not save the shortcut")?;
        Ok(())
    }
}

mod logging {
    use std::fmt::{self, Write as _};
    use std::fs::{File, OpenOptions};
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Level, Metadata, Subscriber};
    use tracing::level_filters::LevelFilter;

    /// Appends one line per event to `%LOCALAPPDATA%\FluxerClient\logs\client.log`.
    struct FileLogger {
        file: Mutex<File>,
        next_span: AtomicU64,
    }

    pub(super) fn init() {
        let path = log_path();
        if let Some(dir) = path.parent()
            && let Err(err) = std::fs::create_dir_all(dir)
        {
            // Without a log directory there is nowhere to report this; the
            // fatal-error dialog still covers startup failures.
            eprintln!("could not create the log directory {}: {err}", dir.display());
            return;
        }
        rotate(&path);

        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("could not open the log file {}: {err}", path.display());
                return;
            }
        };
        let subscriber = FileLogger { file: Mutex::new(file), next_span: AtomicU64::new(1) };
        // Another subscriber would keep logging but must not crash startup.
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

    fn log_path() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("FluxerClient")
            .join("logs")
            .join(super::LOG_FILE_NAME)
    }

    /// Keeps at most one previous log and bounds the active file. This runs
    /// once at startup because no rotation crate is in the lockfile.
    fn rotate(path: &Path) {
        let too_large = std::fs::metadata(path).map(|meta| meta.len() > super::MAX_LOG_BYTES).unwrap_or(false);
        if !too_large {
            return;
        }
        let previous = path.with_file_name(super::LOG_PREVIOUS_FILE_NAME);
        let _ = std::fs::remove_file(&previous);
        if let Err(err) = std::fs::rename(path, &previous) {
            // Keep the log bounded even if the rename is not possible.
            let _ = File::create(path);
            eprintln!("could not rotate the log file: {err}");
        }
    }

    impl Subscriber for FileLogger {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() <= &Level::INFO
        }

        fn max_level_hint(&self) -> Option<LevelFilter> {
            Some(LevelFilter::INFO)
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed))
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let metadata = event.metadata();
            if metadata.level() > &Level::INFO {
                return;
            }
            let mut line = String::with_capacity(160);
            let _ = write!(
                line,
                "{} {:<5} {}:",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                metadata.level(),
                metadata.target()
            );
            event.record(&mut FieldFormatter { line: &mut line });
            line.push('\n');

            let mut file = self.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = file.write_all(line.as_bytes());
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    struct FieldFormatter<'a> {
        line: &'a mut String,
    }

    impl Visit for FieldFormatter<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "message" {
                self.line.push_str(value);
            } else {
                let _ = write!(self.line, " {}={value:?}", field.name());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            if field.name() == "message" {
                let _ = write!(self.line, " {value:?}");
            } else {
                let _ = write!(self.line, " {}={value:?}", field.name());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn format_autostart_command_quotes_paths_with_spaces() {
        let exe = Path::new(r"C:\Program Files\Fluxer Client\fluxer_client.exe");
        assert_eq!(
            format_autostart_command(exe),
            r#""C:\Program Files\Fluxer Client\fluxer_client.exe" --autostart"#
        );
    }

    #[test]
    fn format_protocol_command_quotes_paths_with_spaces() {
        let exe = Path::new(r"C:\Program Files\Fluxer Client\fluxer_client.exe");
        assert_eq!(
            format_protocol_command(exe),
            r#""C:\Program Files\Fluxer Client\fluxer_client.exe" --url -- "%1""#
        );
    }

    #[test]
    fn disabling_os_integration_skips_writes_and_restores_flag() {
        assert!(os_integration_enabled());
        set_os_integration_enabled(false);
        assert!(!os_integration_enabled());
        assert!(set_autostart_enabled(true).is_ok());
        sync_autostart_on_startup(true);
        ensure_protocol_handler();
        ensure_start_menu_shortcut();
        set_os_integration_enabled(true);
        assert!(os_integration_enabled());
    }
}
