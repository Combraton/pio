use anyhow::{Context, Result, bail};
use std::path::Path;
mod client_cli;
mod ledger;
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!(
            "PIO {} ({})",
            env!("CARGO_PKG_VERSION"),
            pio_host::IMPLEMENTATION
        ),
        Some("participant") => println!("{}", pio_core::participant()),
        Some("conformance" | "serve-fake" | "serve-codex" | "serve-claude" | "serve-opencode") => {
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
                "serve-claude" => pio_protocol::serve_claude,
                "serve-opencode" => pio_protocol::serve_opencode,
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
        Some("opencode") => {
            let option = |name: &str| -> Result<&Path> {
                let index = args
                    .iter()
                    .position(|a| a == name)
                    .with_context(|| format!("missing opencode option {name}"))?;
                Ok(Path::new(
                    args.get(index + 1).context("missing option value")?,
                ))
            };
            let path = std::env::var_os("PATH");
            let env =
                |work: &Path| pio_opencode::ChildEnv::isolated(work).with_path(path.as_deref());
            match args.get(1).map(String::as_str) {
                Some("surface-identity") => println!(
                    "{}",
                    serde_json::to_string_pretty(&pio_opencode::surface_identity(
                        option("--executable")?,
                        &env(option("--work")?)
                    )?)?
                ),
                Some("qualify") => {
                    let expected: serde_json::Value = if args.iter().any(|a| a == "--expected") {
                        serde_json::from_slice(&std::fs::read(option("--expected")?)?)?
                    } else {
                        serde_json::from_str(pio_opencode::QUALIFIED_SURFACE)?
                    };
                    let record = pio_opencode::qualify(
                        option("--executable")?,
                        &expected,
                        &env(option("--work")?),
                        option("--work")?,
                    )?;
                    let qualified = record["qualified"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !qualified {
                        std::process::exit(3);
                    }
                }
                // A labeled fake ACP server, so the offline matrix runs in CI
                // with no OpenCode installed and never near the owner's service.
                Some("fake-acp") => pio_opencode::fake::run()?,
                // Launched only by the service controller.
                Some("host") => pio_host::opencode::opencode_host(
                    Path::new(args.get(2).context("missing store")?),
                    args.get(3).context("missing command id")?,
                    args.get(4).context("missing invocation identity")?,
                )?,
                // The owner's rule of 2026-09-20: refuse unless the session's
                // reported provider and model equal the requested ones. ACP
                // reports them before any prompt, so this precedes delivery.
                Some("session-guard") => {
                    let mut session = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut session)?;
                    let requested = args
                        .iter()
                        .position(|a| a == "--requested")
                        .and_then(|i| args.get(i + 1))
                        .context("missing opencode option --requested")?;
                    let guard = pio_opencode::session_configuration_guard(
                        &serde_json::from_str(&session)?,
                        requested,
                    );
                    let allowed = guard["allowed"] == true;
                    println!("{}", serde_json::to_string_pretty(&guard)?);
                    if !allowed {
                        std::process::exit(3);
                    }
                }
                Some("service-admit") => {
                    let config: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(option("--config")?)?)?;
                    let record =
                        pio_opencode::service_admission(option("--work")?, &config["opencode"])?;
                    let admitted = record["admitted"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !admitted {
                        std::process::exit(3);
                    }
                }
                _ => bail!("opencode requires surface-identity, qualify or service-admit"),
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
            let flag = |name: &str| -> Option<&Path> {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|index| args.get(index + 1))
                    .map(Path::new)
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
                    // `--fake-scenario` reaches the labeled fake only; a real
                    // Claude Code ignores it and the allowlist never carries it.
                    let scenario = args
                        .iter()
                        .position(|a| a == "--fake-scenario")
                        .and_then(|i| args.get(i + 1))
                        .map(String::as_str);
                    let env = pio_claude::ChildEnv::isolated(option("--work")?)
                        .with_path(path.as_deref())
                        .with_fake_scenario(scenario);
                    let record = pio_claude::qualify(
                        option("--executable")?,
                        &expected,
                        &env,
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
                    // Two shapes, differing in exactly one thing: which
                    // configuration the harness may see. `--home` observes the
                    // route the user actually has; `--isolated` is the negative
                    // control. Both pass USER, without which the route reads as
                    // absent whatever the configuration. ADR 004 §3.
                    let env = match flag("--isolated") {
                        Some(dir) => pio_claude::ChildEnv::isolated(dir),
                        None => pio_claude::ChildEnv::as_configured(option("--home")?),
                    }
                    .with_path(path.as_deref());
                    let record = pio_claude::auth_route(option("--executable")?, &env)?;
                    let usable = record["usable"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !usable {
                        std::process::exit(3);
                    }
                }
                // Everything a service must decide before it spawns a turn:
                // qualification, the permission mode against the user's own
                // settings, and the credential route. Exit 3 on refusal.
                Some("service-admit") => {
                    let config: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(option("--config")?)?)?;
                    let record =
                        pio_claude::service_admission(option("--work")?, &config["claude"])?;
                    let admitted = record["admitted"] == true;
                    println!("{}", serde_json::to_string_pretty(&record)?);
                    if !admitted {
                        std::process::exit(3);
                    }
                }
                // Encode a permission decision from a control request on
                // stdin. Only a single-use allow or deny is encodable, so the
                // matrix exercises the adapter's own encoder rather than a
                // decision the test wrote by hand.
                // The full surface identity, so a drift control can pin a
                // baseline without hand-writing digests.
                Some("surface-identity") => {
                    let scenario = args
                        .iter()
                        .position(|a| a == "--fake-scenario")
                        .and_then(|i| args.get(i + 1))
                        .map(String::as_str);
                    let env = pio_claude::ChildEnv::isolated(option("--work")?)
                        .with_path(path.as_deref())
                        .with_fake_scenario(scenario);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&pio_claude::surface_identity(
                            option("--executable")?,
                            &env
                        )?)?
                    );
                }
                // PIO's own decision about one tool permission request: an
                // out-of-fixture target is declined here, and everything else
                // is surfaced to the caller. Nothing is ever auto-allowed.
                Some("classify-request") => {
                    let mut request = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut request)?;
                    let classification = pio_claude::classify_permission_request(
                        &serde_json::from_str(&request)?,
                        option("--workspace")?,
                        option("--cwd")?,
                    );
                    println!("{}", serde_json::to_string_pretty(&classification)?);
                }
                // The receipt view of a transcript on stdin: every tool use
                // by digest and fixture-relative label.
                Some("tool-uses") => {
                    let mut transcript = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut transcript)?;
                    let messages: Vec<serde_json::Value> = transcript
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .map(serde_json::from_str)
                        .collect::<std::result::Result<_, _>>()?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&pio_claude::tool_use_records(
                            &messages,
                            // The transcript carries the result, and the result
                            // names what the harness refused. A transcript
                            // without one leaves undecided uses unknown.
                            messages
                                .iter()
                                .find(|m| m["type"] == "result")
                                .map(|m| &m["permission_denials"]),
                            // A transcript alone cannot say who decided a
                            // refusal, so none is attributed here.
                            &serde_json::Value::Null,
                            option("--workspace")?,
                            option("--cwd")?
                        ))?
                    );
                }
                Some("encode-decision") => {
                    let mut request = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut request)?;
                    let behavior = args
                        .iter()
                        .position(|a| a == "--behavior")
                        .and_then(|i| args.get(i + 1))
                        .context("missing claude option --behavior")?;
                    let reason = args
                        .iter()
                        .position(|a| a == "--reason")
                        .and_then(|i| args.get(i + 1))
                        .map(String::as_str)
                        .unwrap_or("refused by PIO");
                    let decision = pio_claude::permission_decision(
                        &serde_json::from_str(&request)?,
                        behavior,
                        reason,
                    )?;
                    println!("{}", serde_json::to_string_pretty(&decision)?);
                }
                // A labeled fake harness, so the offline matrix runs in CI
                // with no Claude Code installed. It is never qualified.
                Some("fake-cli") => pio_claude::fake::run()?,
                // Launched only by the service controller.
                Some("host") => pio_host::claude::claude_host(
                    Path::new(args.get(2).context("missing store")?),
                    args.get(3).context("missing command id")?,
                    args.get(4).context("missing invocation identity")?,
                )?,
                Some("settings-snapshot") => println!(
                    "{}",
                    serde_json::to_string_pretty(&pio_claude::settings_snapshot(option(
                        "--config-dir"
                    )?)?)?
                ),
                _ => bail!(
                    "claude requires qualify --executable PATH --work DIR, auth-route --executable PATH (--home DIR | --isolated DIR), service-admit --config FILE --work DIR, fake-cli or settings-snapshot --config-dir DIR"
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
        // Everything a caller does, through the public client (M4 rule 1).
        Some("client") => client_cli::run(&args[1..])?,
        // The terminal screen: its own crate, on pio-client alone (M4 rule 1).
        Some("tui") => {
            let status = pio_tui::run(&args[1..])?;
            if status != 0 {
                std::process::exit(status);
            }
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
            bail!("expected conformance, serve-fake, client, tui, codex, or diagnostic fake command")
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
