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
        Some("conformance" | "serve-fake" | "serve-codex") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .context("missing conformance option")?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            let serve = match args[0].as_str() {
                "serve-fake" => pio_protocol::serve_fake,
                "serve-codex" => pio_protocol::serve_codex,
                _ => pio_protocol::serve,
            };
            serve(
                option("--data-dir")?,
                option("--config")?,
                option("--socket")?,
            )?;
        }
        Some("codex") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .with_context(|| format!("missing codex option {name}"))?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            match args.get(1).map(String::as_str) {
                Some("schema-identity") => println!(
                    "{}",
                    serde_json::to_string_pretty(&pio_codex::schema_identity(Path::new(
                        args.get(2).context("missing schema directory")?
                    ))?)?
                ),
                Some("qualify") => {
                    // `--expected FILE` exists for drift controls; the
                    // checked-in qualified identity is the default.
                    let expected: serde_json::Value = if args.iter().any(|a| a == "--expected") {
                        serde_json::from_slice(&std::fs::read(option("--expected")?)?)?
                    } else {
                        serde_json::from_str(pio_codex::QUALIFIED_SCHEMA_IDENTITY)?
                    };
                    let record = pio_codex::qualify(
                        option("--executable")?,
                        &expected,
                        pio_codex::inherited_path().as_deref(),
                        option("--work")?,
                    )?;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if record["qualified"] != true {
                        std::process::exit(3);
                    }
                }
                // Labeled offline test double; accepts the `app-server` argument
                // a real Codex executable receives.
                Some("fake-app-server") => pio_codex::fake::run()?,
                // Launched only by the service controller.
                Some("host") => pio_host::codex::codex_host(
                    Path::new(args.get(2).context("missing store")?),
                    args.get(3).context("missing command id")?,
                    args.get(4).context("missing invocation identity")?,
                )?,
                Some("config-snapshot") => println!(
                    "{}",
                    serde_json::to_string_pretty(&pio_codex::config_snapshot(option(
                        "--codex-home"
                    )?)?)?
                ),
                Some("config-diff") => {
                    let read = |index: usize| -> Result<serde_json::Value> {
                        Ok(serde_json::from_slice(&std::fs::read(
                            args.get(index).context("config-diff BEFORE AFTER")?,
                        )?)?)
                    };
                    let fixture_root = if args.iter().any(|a| a == "--fixture-root") {
                        Some(option("--fixture-root")?)
                    } else {
                        None
                    };
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&pio_codex::config_diff(
                            &read(2)?,
                            &read(3)?,
                            fixture_root
                        ))?
                    );
                }
                _ => bail!(
                    "codex requires schema-identity DIR, qualify --executable PATH --work DIR, config-snapshot --codex-home DIR or config-diff BEFORE AFTER"
                ),
            }
        }
        Some("claude") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .with_context(|| format!("missing claude option {name}"))?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            let path = std::env::var_os("PATH");
            match args.get(1).map(String::as_str) {
                Some("qualify") => {
                    // `--expected FILE` exists for drift controls; the
                    // checked-in qualified surface is the default.
                    let expected: serde_json::Value = if args.iter().any(|a| a == "--expected") {
                        serde_json::from_slice(&std::fs::read(option("--expected")?)?)?
                    } else {
                        serde_json::from_str(pio_claude::QUALIFIED_SURFACE)?
                    };
                    let record = pio_claude::qualify(
                        option("--executable")?,
                        &expected,
                        path.as_deref(),
                        option("--work")?,
                    )?;
                    let qualified = record["qualified"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !qualified {
                        std::process::exit(3);
                    }
                }
                Some("auth-route") => {
                    // Observes the route only. Never reads a credential file.
                    let record = pio_claude::auth_route(
                        option("--executable")?,
                        option("--config-dir")?,
                        path.as_deref(),
                    )?;
                    let usable = record["usable"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !usable {
                        std::process::exit(3);
                    }
                }
                Some("settings-snapshot") => println!(
                    "{}",
                    serde_json::to_string_pretty(&pio_claude::settings_snapshot(option(
                        "--config-dir"
                    )?)?)?
                ),
                _ => bail!(
                    "claude requires qualify --executable PATH --work DIR, auth-route --executable PATH --config-dir DIR or settings-snapshot --config-dir DIR"
                ),
            }
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
            let action = args.get(1).context(
                "fake requires daemon, request, host, child, identity or fill-projection",
            )?;
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
                // Test tooling only: pads a stopped store up to a record count.
                "fill-projection" => println!(
                    "{}",
                    pio_protocol::capacity::fill_projection_records(
                        Path::new(value),
                        args.get(3)
                            .context("missing target record count")?
                            .parse()?,
                    )?
                ),
                "identity" => println!(
                    "{}",
                    serde_json::to_string(&pio_host::identity(value.parse()?)?)?
                ),
                _ => bail!("unknown fake action"),
            }
        }
        _ => {
            bail!("expected conformance, serve-fake, client, codex, or diagnostic fake command")
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
