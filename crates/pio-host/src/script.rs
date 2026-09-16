//! Deterministic fake-host input for conformance. Never executes a native harness.
//! Inputs come only from the released runner launch configuration.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
pub const SOURCE: &str = "fake-host/executor.script";
pub fn validate(config: &Value) -> Result<()> {
    if config.is_null() {
        return Ok(());
    }
    for name in config
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("executor object"))?
        .keys()
    {
        ensure!(
            [
                "adapter",
                "scripts",
                "default_script",
                "recovery_policy",
                "host_id",
                "capacity",
                "budget_pools",
                "installations",
                "output_spool_bytes"
            ]
            .contains(&name.as_str()),
            "unsupported executor control: {name}"
        );
    }
    for name in ["steering", "context_boundaries"] {
        ensure!(
            config["adapter"].get(name).is_none(),
            "unsupported executor adapter control: {name}"
        );
    }
    let mut scripts = Vec::new();
    if let Some(script) = config.get("default_script") {
        scripts.push(script);
    }
    if let Some(map) = config["scripts"].as_object() {
        scripts.extend(map.values());
    }
    for script in scripts {
        for step in script
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("script array"))?
        {
            let name = step
                .as_object()
                .and_then(|m| m.keys().next())
                .ok_or_else(|| anyhow::anyhow!("script step"))?;
            ensure!(
                [
                    "deliver",
                    "crash",
                    "reconcile_finds",
                    "runtime",
                    "host_restart",
                    "complete",
                    "exit",
                    "on_cancel",
                    "wait_until",
                    "wait_for",
                    "stall",
                    "stale_dispatch",
                    "workspace",
                    "agent_reports_commit",
                    "usage",
                    "transport_errors",
                    "probe_status",
                    "output",
                    "output_lost",
                    "runtime_burst"
                ]
                .contains(&name.as_str()),
                "unsupported script step: {name}"
            );
            ensure!(
                step["wait_for"] != "steer" && step["wait_for"] != "action",
                "unsupported wait control"
            );
        }
    }
    Ok(())
}
pub fn select(config: &Value, execution: &str) -> Value {
    config["scripts"]
        .get(execution)
        .or_else(|| config.get("default_script"))
        .cloned()
        .unwrap_or_else(|| json!([]))
}
pub fn evidence(class: &str) -> Value {
    json!({"class":class,"source":SOURCE})
}
