//! Process observations from the durable, explicitly fake adapter. No script is
//! consulted here. Public command intent is committed before host admission.
use crate::provider::*;
use anyhow::Result;
use serde_json::{Value, json};

impl Provider {
    pub(crate) fn host_evidence(&self, class: &str) -> Value {
        if self.codex() {
            return json!({"class":class,"source":format!("{}/host", self.codex_source())});
        }
        let source = if self.durable.is_none() {
            pio_host::script::SOURCE
        } else if class.contains("timeout") {
            "fake-host/provider-clock"
        } else if matches!(
            class,
            "child_release_marker" | "release_not_observed" | "release_absence_confirmed"
        ) {
            "fake-host/process"
        } else {
            "fake-host/journal-controller"
        };
        json!({"class":class,"source":source})
    }
    pub(crate) fn run_durable(&mut self, e: &mut Value) -> Result<()> {
        if self.codex() {
            return self.run_codex(e);
        }
        let command = pio_core::digest(text(&e["view"]["execution"]["id"]).as_bytes());
        let command = command.trim_start_matches("sha256:");
        // A persisted marker with no host admission is conservatively ambiguous
        // after restart. Never infer permission to launch from a missing process.
        if e["recovery_no_admit"] == true
            || matches!(
                text(&e["view"]["delivery"]),
                "failed_before_delivery" | "not_delivered"
            )
        {
            return Ok(());
        }
        let payload = json!({"duration_ms":self.host_config["duration_ms"]});
        let fault = text(&self.host_config["fault"]).to_owned();
        let first = e["dispatched"] != true;
        // Deliberate negative control, available only in labeled fake-service
        // launch configuration. The journal-sequence oracle must reject it.
        if first && fault == "reorder_dispatch_intent" {
            e["host_submission"] =
                self.durable
                    .as_ref()
                    .unwrap()
                    .submit(command, payload.clone(), "")?;
        }
        self.dispatch_marker(e)?;
        if first && fault == "after_dispatch_marker" {
            std::process::exit(95);
        }
        let host_fault = if matches!(
            fault.as_str(),
            "after_dispatch_marker" | "reorder_dispatch_intent"
        ) {
            ""
        } else {
            &fault
        };
        let host = self.durable.as_ref().unwrap();
        let current = host.inspect(command).ok();
        if current.is_none() {
            let result = host.submit(command, payload.clone(), host_fault)?;
            e["host_submission"] = result;
        } else if e["mutant_attempted"] != true
            && fault.starts_with("replay_")
            && matches!(
                current
                    .as_ref()
                    .map(|v| text(&v["invocation"]["phase"]))
                    .unwrap_or(""),
                "released" | "completed"
            )
        {
            e["mutant_attempted"] = true.into();
            match host.submit(command, payload, &fault) {
                Ok(v) => e["host_mutant"] = v,
                Err(error) => e["host_mutant"] = json!({"reason":error.to_string()}),
            }
        }
        let mut observed = host.inspect(command)?;
        // Detailed kernel witnesses stay in the local journal. Frozen public
        // evidence only permits class/source; do not widen that schema.
        observed["class"] = observed["recovery"].clone();
        if let Some(attempt) = e["host_mutant"]["launch_attempt"].as_str() {
            let path = host.root.join(format!("attempt-{attempt}.json"));
            if path.exists() {
                e["host_mutant"]["outcome"] = serde_json::from_slice(&std::fs::read(path)?)?;
            }
        }
        if e.get("host_mutant").is_some() {
            observed["mutant"] = e["host_mutant"].clone();
        }
        let invocation = &observed["invocation"];
        let phase = text(&invocation["phase"]);
        let determination = if phase == "known_not_released" {
            if e["view"]["delivery"] == "ambiguous" {
                "not_delivered"
            } else {
                "failed_before_delivery"
            }
        } else if observed["release_observed"] == true {
            // A later child-written receipt establishes arrival; it is not a
            // provider_ack_id from a native harness (EXECUTION section 3.1).
            "delivered"
        } else if observed["recovery"] == "uncertain_no_respawn"
            || e["view"]["delivery"] == "ambiguous"
        {
            "ambiguous"
        } else {
            "pending"
        };
        let delivery_class = if observed["release_observed"] == true {
            "child_release_marker"
        } else if phase == "known_not_released" {
            "release_absence_confirmed"
        } else {
            "release_not_observed"
        };
        if e["process_observation"] != observed {
            self.delivery_observed(
                e,
                determination,
                delivery_class,
                None,
                matches!(
                    determination,
                    "delivered" | "not_delivered" | "failed_before_delivery"
                ),
                e["view"]["delivery"] == "ambiguous" && determination != "ambiguous",
            );
            e["view"]["deliveries"][0]["evidence"] = self.host_evidence(delivery_class);
            // Public generation fences the daemon controller, not the surviving
            // process slot. The original slot generation stays in its journal fact.
            e["view"]["host"]["generation"] = self.durable.as_ref().unwrap().generation.into();
            let runtime = if phase == "known_not_released" {
                "not_started"
            } else if phase == "completed" {
                "exited"
            } else if observed["child_alive"] == true && observed["release_observed"] == true {
                "active"
            } else if observed["recovery"] == "uncertain_no_respawn" {
                "unknown"
            } else {
                "preparing"
            };
            e["view"]["runtime"] = runtime.into();
            if phase == "completed" {
                e["view"]["exit"] = invocation["receipt"]["exit_codes"][0]
                    .as_i64()
                    .map(|code| json!({"code":code}))
                    .unwrap_or(json!("unavailable"));
                self.execution_event(
                    e,
                    "execution.exit.observed",
                    json!({"exit":e["view"]["exit"]}),
                    None,
                );
            }
            e["process_observation"] = observed.clone();
        }
        let invocation_id = text(&invocation["invocation_id"]).to_owned();
        self.drain_output(e, &invocation_id)
    }
    /// Append newly spooled host output, in offset order, to the execution.
    pub(crate) fn drain_output(&mut self, e: &mut Value, invocation_id: &str) -> Result<()> {
        let manifest = self.root.join(format!("output-{invocation_id}.refs.jsonl"));
        if manifest.exists() {
            let contents = std::fs::read_to_string(manifest)?;
            for line in contents.split_inclusive('\n').filter(|s| s.ends_with('\n')) {
                let reference: Value = serde_json::from_str(line)?;
                let offset = num(&reference["offset"]);
                if offset < num(&e["output_end"]) {
                    continue;
                }
                anyhow::ensure!(offset == num(&e["output_end"]), "host output offset gap");
                let bytes =
                    pio_core::spool::Spool::open(&self.root)?.read(text(&reference["digest"]))?;
                anyhow::ensure!(
                    bytes.len() as u64 == num(&reference["length"]),
                    "host output length mismatch"
                );
                self.append_output(e, &bytes)?;
            }
        }
        Ok(())
    }
    pub(crate) fn output_bytes(&self, e: &Value) -> Result<Vec<u8>> {
        if let Some(digest) = e["output_ref"]["digest"].as_str() {
            let bytes = pio_core::spool::Spool::open(&self.root)?.read(digest)?;
            anyhow::ensure!(
                bytes.len() as u64 == num(&e["output_ref"]["length"]),
                "output reference length mismatch"
            );
            Ok(bytes)
        } else {
            Ok(vec![])
        }
    }
    pub(crate) fn append_output(&mut self, e: &mut Value, bytes: &[u8]) -> Result<()> {
        let mut retained = self.output_bytes(e)?;
        let old_end = num(&e["output_end"]);
        let old_start = old_end - retained.len() as u64;
        retained.extend_from_slice(bytes);
        let bound = self.config["executor"]["output_spool_bytes"]
            .as_u64()
            .unwrap_or(65536) as usize;
        let dropped = retained.len().saturating_sub(bound);
        retained.drain(..dropped);
        let digest = pio_core::spool::Spool::open(&self.root)?.put(&retained)?;
        e["output_end"] = (old_end + bytes.len() as u64).into();
        e["output_ref"] =
            json!({"digest":digest,"offset":old_start+dropped as u64,"length":retained.len()});
        if dropped > 0 {
            self.lost_output(e,json!({"from":old_start,"to":old_start+dropped as u64,"bytes":dropped,"reason":"spool_limit","coverage":"incomplete"}));
        }
        Ok(())
    }
}
