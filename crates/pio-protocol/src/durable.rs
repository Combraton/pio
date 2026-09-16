//! Process observations from the durable, explicitly fake adapter. No script is
//! consulted here. Public command intent is committed before host admission.
use crate::provider::*;
use anyhow::Result;
use serde_json::{Value, json};

impl Provider {
    pub(crate) fn host_evidence(&self, class: &str) -> Value {
        let source = if self.durable.is_none() {
            pio_host::script::SOURCE
        } else if class.contains("timeout") {
            "fake-host/provider-clock"
        } else if matches!(
            class,
            "same_process_observed"
                | "receipt_recorded"
                | "not_released_pending"
                | "uncertain_no_respawn"
        ) {
            "fake-host/process"
        } else {
            "fake-host/journal-controller"
        };
        json!({"class":class,"source":source})
    }
    pub(crate) fn run_durable(&mut self, e: &mut Value) -> Result<()> {
        let command = pio_core::digest(text(&e["view"]["execution"]["id"]).as_bytes());
        let command = command.trim_start_matches("sha256:");
        // Re-running admission is safe: the host's inserted flag is the first
        // defense, followed by the immutable launch guard and host fences.
        self.dispatch_marker(e)?;
        let payload = json!({"duration_ms":self.host_config["duration_ms"]});
        let fault = text(&self.host_config["fault"]).to_owned();
        let host = self.durable.as_ref().unwrap();
        let current = host.inspect(command).ok();
        if current.is_none() {
            let result = host.submit(command, payload.clone(), &fault)?;
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
            "failed_before_delivery"
        } else if observed["release_observed"] == true {
            "acknowledged"
        } else if observed["recovery"] == "uncertain_no_respawn" {
            "ambiguous"
        } else {
            "pending"
        };
        if e["process_observation"] != observed {
            self.delivery_observed(
                e,
                determination,
                text(&observed["class"]),
                None,
                matches!(determination, "acknowledged" | "failed_before_delivery"),
                false,
            );
            e["view"]["deliveries"][0]["evidence"] = self.host_evidence(text(&observed["class"]));
            e["view"]["host"]["generation"] = observed["invocation"]["host_generation"].clone();
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
        let manifest = self.root.join(format!(
            "output-{}.refs.jsonl",
            text(&invocation["invocation_id"])
        ));
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
