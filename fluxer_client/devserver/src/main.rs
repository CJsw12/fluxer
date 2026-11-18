//! `fluxer_devserver [--port <n>] [--chatter <secs>]`
//!
//! Local fixture instance for the Rust client. Binds 127.0.0.1 only.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use fluxer_devserver::{Config, banner, start};

const DEFAULT_PORT: u16 = 8787;

fn parse_args() -> Result<Config> {
    let mut config = Config { port: DEFAULT_PORT, chatter: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                config.port = args.next().context("--port needs a value")?.parse().context("--port must be 0..=65535")?;
            }
            "--chatter" => {
                let secs: f64 = args.next().context("--chatter needs seconds")?.parse().context("--chatter must be a number")?;
                if !secs.is_finite() || secs <= 0.0 || secs > 86_400.0 {
                    bail!("--chatter must be in (0, 86400] seconds");
                }
                config.chatter = Some(Duration::from_secs_f64(secs));
            }
            "-h" | "--help" => {
                println!("fluxer_devserver [--port <n> (default {DEFAULT_PORT}, 0 = any)] [--chatter <secs>]");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?} (try --help)"),
        }
    }
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;
    let server = start(config.clone()).await?;
    println!("{}", banner(&server));
    if let Some(every) = config.chatter {
        println!("chatter: a message from another user every {:.1}s", every.as_secs_f64());
    }
    server.wait().await
}
