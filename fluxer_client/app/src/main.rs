//! Fluxer desktop client (GPUI). Usage:
//!
//! ```text
//! fluxer_client [--instance <address>] [--token-env <VAR>] [--allow-insecure-loopback]
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
mod backend;
mod client_view;
mod format;
mod frecency;
mod http;
mod icons;
mod login;
mod nav;
mod notify;
mod root;
mod shortcuts;
mod theme;
mod ui;
mod view_model;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_keyboard;
#[cfg(test)]
mod tests_notify_upload;

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
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--instance" => out.instance = Some(args.next().context("--instance needs a value")?),
            "--token-env" => out.token_env = Some(args.next().context("--token-env needs a variable name")?),
            "--allow-insecure-loopback" => out.allow_insecure_loopback = true,
            other => bail!("unknown argument: {other}"),
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
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

    let open_error: Rc<RefCell<Option<String>>> = Rc::default();
    let open_error_slot = open_error.clone();
    // Without an asset source every `Icon` renders blank.
    gpui_kit::application().with_assets(icons::AppAssets).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        shortcuts::bind_keys(cx);
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
            theme::apply(theme::ThemeKind::from_env(), window, cx);
            cx.new(|cx| match (token, store) {
                (Some(token), _) => AppRoot::new_with_token(handle, insecure, instance, token, window, cx),
                (None, Some(store)) => AppRoot::new_session(handle, insecure, store, window, cx),
                (None, None) => {
                    AppRoot::new_session(handle, insecure, Err(SessionError::NoDataDir), window, cx)
                }
            })
        });
        match opened {
            Ok(_) => cx.activate(true),
            Err(err) => {
                *open_error_slot.borrow_mut() = Some(format!("{err:#}"));
                cx.quit();
            }
        }
    });
    runtime.shutdown_background();
    let open_error = open_error.borrow_mut().take();
    match open_error {
        Some(err) => Err(anyhow!("could not open the window: {err}")),
        None => Ok(()),
    }
}
