use anyhow::{Context, Result, bail};
use std::path::Path;
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!(
            "PIO {} ({})",
            env!("CARGO_PKG_VERSION"),
            pio_host::IMPLEMENTATION
        ),
        Some("participant") => println!("{}", pio_core::participant()),
        Some("conformance" | "serve-fake") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .context("missing conformance option")?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            let serve = if args[0] == "serve-fake" {
                pio_protocol::serve_fake
            } else {
                pio_protocol::serve
            };
            serve(
                option("--data-dir")?,
                option("--config")?,
                option("--socket")?,
            )?;
        }
        Some("check-transcript") => {
            println!(
                "{} schema-valid public results",
                pio_protocol::transcript::check(Path::new(
                    args.get(1).context("missing transcript")?
                ))?
            );
        }
        Some("client") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .context("missing client option")?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            let action = args.get(1).context("client requires submit or reconcile")?;
            anyhow::ensure!(
                ["submit", "reconcile"].contains(&action.as_str()),
                "unknown client action"
            );
            let request = if action == "submit" {
                Some(option("--request")?)
            } else {
                None
            };
            let basis = if args.iter().any(|a| a == "--basis") {
                Some(option("--basis")?)
            } else {
                None
            };
            println!(
                "{}",
                pio_protocol::client::run(
                    option("--store")?,
                    option("--socket")?,
                    option("--credential-file")?,
                    request,
                    basis
                )?
            );
        }
        Some("fake") => {
            let action = args
                .get(1)
                .context("fake requires daemon, request, host, child or identity")?;
            let value = args.get(2).context("missing fake argument")?;
            match action.as_str() {
                "daemon" => pio_host::daemon(Path::new(value))?,
                "request" => {
                    let request =
                        serde_json::from_str(args.get(3).context("missing JSON request")?)?;
                    println!(
                        "{}",
                        pio_host::request(&Path::new(value).join("daemon.sock"), &request)?
                    );
                }
                "host" => pio_host::host(
                    Path::new(value),
                    args.get(3).context("missing command id")?,
                    args.get(4).context("missing invocation identity")?,
                    args.get(5).map(String::as_str).unwrap_or(""),
                )?,
                "child" => pio_host::child(
                    Path::new(value),
                    args.get(3).context("missing command id")?,
                    args.get(4).context("missing invocation identity")?,
                )?,
                "identity" => println!(
                    "{}",
                    serde_json::to_string(&pio_host::identity(value.parse()?)?)?
                ),
                _ => bail!("unknown fake action"),
            }
        }
        _ => {
            bail!("expected conformance, serve-fake, client, or diagnostic fake command")
        }
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PIO: {error:#}");
            std::process::ExitCode::from(2)
        }
    }
}
