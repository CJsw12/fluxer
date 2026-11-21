#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Fluxer desktop client (GPUI). Usage:
//!
//! ```text
//! fluxer_client [--instance <address>] [--token-env <VAR>] [--allow-insecure-loopback] [--autostart]
//! ```
//!
//! Without `--token-env`, logins are saved (encrypted) in the session store
//! and the active account is resumed on the next start; a second instance
//! finds the store locked and only shows "Fluxer is already running".
//!
//! `--token-env VAR` reads a session token from the environment variable
//! `VAR` and skips the login form (development only; the token is never
//! printed or stored, and the session store is not opened).
//! `--allow-insecure-loopback` permits http/ws for localhost instances.

mod accounts;
mod ack;
mod audio_devices;
mod backend;
mod client_view;
mod deep_links;
mod format;
mod frecency;
mod http;
mod icons;
mod login;
mod nav;
mod notify;
mod platform;
mod prefs;
mod profile;
mod root;
mod settings_model;
mod shortcuts;
mod single_instance;
mod theme;
mod tray;
mod trusted_domains;
mod ui;
mod view_model;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_accounts;
#[cfg(test)]
mod tests_channel_mgmt;
#[cfg(test)]
mod tests_deep_links;
#[cfg(test)]
mod tests_emoji_picker;
#[cfg(test)]
mod tests_group_dm;
#[cfg(test)]
mod tests_keyboard;
#[cfg(test)]
mod tests_media_display;
#[cfg(test)]
mod tests_moderation;
#[cfg(test)]
mod tests_notify_upload;
#[cfg(test)]
mod tests_profile;
#[cfg(test)]
mod tests_search;
#[cfg(test)]
mod tests_settings;
#[cfg(test)]
mod tests_social;
#[cfg(test)]
mod tests_voice;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow, bail};
use fluxer_client_core::rest::SecretToken;
use fluxer_client_core::session::SessionError;
use gpui_kit::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

use crate::root::AppRoot;

#[derive(Debug, Default, PartialEq)]
struct Args {
    instance: Option<String>,
    token_env: Option<String>,
    allow_insecure_loopback: bool,
    autostart: bool,
    no_os_integration: bool,
    deep_link: Option<String>,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut out = Args::default();
    let mut args = args.into_iter();
    let mut flags_ended = false;
    while let Some(arg) = args.next() {
        if flags_ended {
            if !deep_links::is_app_protocol_url(&arg) {
                bail!("unexpected argument: {arg}");
            }
            if out.deep_link.is_some() {
                bail!("multiple deep link URLs provided");
            }
            out.deep_link = Some(arg);
            continue;
        }
        match arg.as_str() {
            "--instance" => out.instance = Some(args.next().context("--instance needs a value")?),
            "--token-env" => out.token_env = Some(args.next().context("--token-env needs a variable name")?),
            "--url" => {
                let mut raw = args.next().context("--url needs a value")?;
                if raw == "--" {
                    raw = args.next().context("--url needs a value")?;
                    flags_ended = true;
                }
                if !deep_links::is_app_protocol_url(&raw) {
                    bail!("--url requires a fluxer:// URL");
                }
                if out.deep_link.is_some() {
                    bail!("multiple deep link URLs provided");
                }
                out.deep_link = Some(raw);
            }
            "--allow-insecure-loopback" => out.allow_insecure_loopback = true,
            "--autostart" => out.autostart = true,
            "--no-os-integration" => out.no_os_integration = true,
            "--" => flags_ended = true,
            _ if deep_links::is_app_protocol_url(&arg) => {
                if out.deep_link.is_some() {
                    bail!("multiple deep link URLs provided");
                }
                out.deep_link = Some(arg);
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    if out.deep_link.is_some() && out.instance.is_some() {
        bail!("--instance cannot be combined with a deep link URL");
    }
    Ok(out)
}

fn main() {
    if std::env::args_os()
        .skip(1)
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--no-os-integration")
    {
        platform::set_os_integration_enabled(false);
    }
    platform::init();
    if let Err(err) = run() {
        let message = format!("{err:#}");
        tracing::error!(error = %message, "fatal error");
        platform::show_fatal_error(&format!("{} could not start.\n\n{message}", platform::APP_DISPLAY_NAME));
        std::process::exit(1);
    }
    tracing::info!("Fluxer client stopped");
}

fn run() -> Result<()> {
    let args = parse_args(std::env::args().skip(1))?;
    let token = match &args.token_env {
        Some(var) => {
            let value = std::env::var(var).map_err(|_| anyhow!("environment variable {var} is not set"))?;
            let value = value.trim();
            if value.is_empty() {
                bail!("environment variable {var} is empty");
            }
            Some(SecretToken::new(value))
        }
        None => None,
    };
    let instance = args.instance.clone().unwrap_or_else(|| login::DEFAULT_INSTANCE.to_owned());
    let insecure = args.allow_insecure_loopback;
    let autostart_launch = args.autostart;

    // Network work (discovery, REST, gateway) runs here; the UI awaits it.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("fluxer-net")
        .enable_all()
        .build()
        .context("could not start the tokio runtime")?;
    let handle = runtime.handle().clone();
    let images = http::ImageHttpClient::new(handle.clone(), insecure)?;
    // Opened before the window so the store lock is held for the app's
    // lifetime; `--token-env` sessions never touch it.
    let store = match token {
        Some(_) => None,
        None => Some(accounts::open_store(insecure)),
    };
    if let Some(Err(SessionError::Locked { ref path })) = store
        && let Some(dir) = path.parent()
        && single_instance::try_forward_with_retry(
            dir,
            args.deep_link.as_deref(),
            std::time::Duration::from_millis(1500),
        )
    {
        tracing::info!(deep_link = args.deep_link.is_some(), "handed off launch to running instance");
        runtime.shutdown_background();
        return Ok(());
    }
    let (ipc_server, ipc_rx) = match &store {
        Some(Ok(s)) => {
            platform::ensure_protocol_handler();
            match single_instance::SingleInstanceServer::bind(s.dir()) {
                Ok((server, rx)) => (Some(server), Some(rx)),
                Err(err) => {
                    tracing::warn!(error = %format!("{err:#}"), "could not start single-instance IPC server");
                    (None, None)
                }
            }
        }
        _ => (None, None),
    };
    let initial_deep_link = args.deep_link;
    let initial_prefs = match &store {
        Some(Ok(s)) => Some(prefs::PrefsStore::open(s.dir().join(prefs::FILE_NAME))),
        _ => None,
    };
    if let Some(prefs) = &initial_prefs {
        tray::set_close_to_tray(prefs.data.close_to_tray);
        platform::sync_autostart_on_startup(prefs.data.autostart);
        if std::env::var_os("FLUXER_LOCALE").is_none() {
            ui::l10n::set_locale(&prefs.data.locale);
        }
    }
    let initial_theme = if std::env::var_os("FLUXER_THEME").is_some() {
        theme::ThemeKind::from_env()
    } else {
        initial_prefs
            .as_ref()
            .map(|p| p.data.theme)
            .unwrap_or_else(theme::ThemeKind::from_env)
    };

    let open_error: Rc<RefCell<Option<String>>> = Rc::default();
    let open_error_slot = open_error.clone();
    // Without an asset source every `Icon` renders blank.
    gpui_kit::application().with_assets(icons::AppAssets).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        shortcuts::bind_keys(cx);
        theme::set_kind(initial_theme, cx);
        // `img()` loads attachment previews and avatars through this client.
        cx.set_http_client(Arc::new(images));
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Fluxer".into()), ..Default::default() }),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, |window, cx| {
            // Make the gpui-kit widgets (inputs, buttons, scrollbars) follow the app theme.
            theme::apply(initial_theme, window, cx);
            window.on_window_should_close(cx, |window, _cx| {
                if tray::should_intercept_close() {
                    window.minimize_window();
                    false
                } else {
                    true
                }
            });
            let root = cx.new(|cx| match (token, store) {
                (Some(token), _) => AppRoot::new_with_token(handle, insecure, instance, token, window, cx),
                (None, Some(store)) => AppRoot::new_session(handle, insecure, store, window, cx),
                (None, None) => {
                    AppRoot::new_session(handle, insecure, Err(SessionError::NoDataDir), window, cx)
                }
            });
            tray::install(window.window_handle(), root.downgrade(), cx);
            if let Some(rx) = ipc_rx {
                single_instance::install(rx, window.window_handle(), root.downgrade(), cx);
            }
            if let Some(url) = initial_deep_link {
                root.update(cx, |r, cx| r.handle_deep_link(&url, window, cx));
            }
            if autostart_launch && tray::close_to_tray() && tray::is_active() {
                window.minimize_window();
            }
            root
        });
        match opened {
            Ok(_) => {
                if !autostart_launch {
                    cx.activate(true);
                }
            }
            Err(err) => {
                *open_error_slot.borrow_mut() = Some(format!("{err:#}"));
                cx.quit();
            }
        }
    });
    drop(ipc_server);
    runtime.shutdown_background();
    let open_error = open_error.borrow_mut().take();
    match open_error {
        Some(err) => Err(anyhow!("could not open the window: {err}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parse_args_supports_no_os_integration() {
        let default_args = parse_args(["--instance".to_owned(), "http://127.0.0.1:8080".to_owned()]).unwrap();
        assert!(!default_args.no_os_integration);

        let isolated = parse_args([
            "--instance".to_owned(),
            "http://127.0.0.1:8080".to_owned(),
            "--allow-insecure-loopback".to_owned(),
            "--no-os-integration".to_owned(),
        ])
        .unwrap();
        assert!(isolated.no_os_integration);
        assert!(isolated.allow_insecure_loopback);
        assert_eq!(isolated.instance.as_deref(), Some("http://127.0.0.1:8080"));
    }
}
