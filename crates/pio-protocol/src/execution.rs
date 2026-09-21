//! Execution state machine for the explicitly labeled, launch-controlled fake
//! host. Every transition uses Core's journal transaction, never the old blob.
use crate::provider::*;
use pio_host::script::{self, evidence};
use serde_json::{Value, json};
use uuid::Uuid;

pub const FEATURES: &[&str] = &[
    "execution.controller",
    "execution.discovery",
    "execution.output",
    "execution.workspaces",
    "execution.usage",
];
fn subject(id: &str) -> Value {
    json!({"kind":"execution.execution","id":id})
}
fn push(v: &mut Value, x: Value) {
    v.as_array_mut().unwrap().push(x);
}
fn after(start: &str, seconds: u64) -> String {
    (chrono::DateTime::parse_from_rfc3339(start).unwrap()
        + chrono::Duration::seconds(seconds as i64))
    .format("%Y-%m-%dT%H:%M:%SZ")
    .to_string()
}
fn open_obligations(data: &Data, e: &Value) -> Vec<Value> {
    list(&e["view"]["effects"])
        .iter()
        .flat_map(|id| {
            data.effects
                .get(text(id))
                .map(|r| list(&r["obligations"]))
                .unwrap_or_default()
        })
        .filter(|o| matches!(text(&o["state"]), "open" | "overdue"))
        .collect()
}
impl Provider {
    pub fn execution_host(&self) -> &str {
        self.config["executor"]["host_id"]
            .as_str()
            .unwrap_or("scripted-host")
    }
    pub fn execution_gate(&self, session: &Session, method: &str, p: &Value) -> Result<(), Error> {
        if self.native() {
            self.codex_content(method, p)?;
        }
        if method == "execution.submit" {
            for (field, feature) in [
                ("workspace", "execution.workspaces"),
                ("budget", "execution.usage"),
                ("continuation", "execution.continuation"),
                ("context_bindings", "execution.context"),
                ("origin", "execution.context_revalidation"),
            ] {
                if p["payload"].get(field).is_some() && !session.feature(feature) {
                    return Err(invalid(&format!("/payload/{field}")));
                }
            }
        }
        if p.get("command_id").is_some()
            && method.starts_with("execution.")
            && (list(&p["preconditions"]).len() != 1
                || p["preconditions"][0]["subject"] != p["subject"]
                || (method == "execution.submit" && p["preconditions"][0]["revision"] != 0))
        {
            return Err(invalid("/preconditions"));
        }
        Ok(())
    }
    pub fn execution_epoch(&self, session: &Session, method: &str, p: &Value) -> Result<(), Error> {
        if !method.starts_with("execution.")
            || method == "execution.controller.claim"
            || p.get("command_id").is_none()
        {
            return Ok(());
        }
        let controller = json!({"kind":"execution.controller","id":self.execution_host()});
        let current = self.revision(&controller);
        let supplied = num(&p["authority_epoch"]);
        if supplied < current {
            return Err(err(
                "stale_authority_epoch",
                if self.visible(session, p, &controller) {
                    json!({"current_epoch":current})
                } else {
                    json!({})
                },
            ));
        }
        if supplied > current {
            return Err(err("unknown_authority_epoch", json!({})));
        }
        Ok(())
    }
    fn capacity_used(&self) -> usize {
        self.data
            .executions
            .values()
            .filter(|e| {
                e["view"]["admission"] == "admitted"
                    && e["view"]["runtime"] != "exited"
                    && !matches!(
                        text(&e["view"]["delivery"]),
                        "failed_before_delivery" | "not_delivered"
                    )
                    && e["view"]["cancellation"]["outcome"] != "cancelled"
            })
            .count()
    }
    fn capacity_available(&self) -> bool {
        self.capacity_used()
            < self.config["executor"]["capacity"]
                .as_u64()
                .unwrap_or(u64::MAX) as usize
    }
    pub(crate) fn update_execution(&mut self, e: &Value) {
        let v = &e["view"];
        self.data.subjects.insert(
            key(&v["execution"]),
            Subject {
                subject: v["execution"].clone(),
                revision: num(&v["revision"]),
                state: v.clone(),
                applied: 1,
            },
        );
        self.data
            .executions
            .insert(text(&v["execution"]["id"]).to_owned(), e.clone());
    }
    pub(crate) fn execution_event(
        &mut self,
        e: &mut Value,
        kind: &str,
        payload: Value,
        command: Option<(&Value, &str)>,
    ) {
        e["view"]["revision"] = (num(&e["view"]["revision"]) + 1).into();
        self.append_event(
            e["view"]["execution"].clone(),
            num(&e["view"]["revision"]),
            kind,
            payload.clone(),
            command,
        );
        // Administrative requests and passed waits are not fresh host activity.
        let observed = matches!(
            kind,
            "execution.runtime.changed"
                | "execution.exit.observed"
                | "execution.completion.recorded"
                | "execution.usage.observed"
                | "execution.delivery.reconciled"
                | "execution.cancel.observed"
                | "execution.admission.changed"
        ) || (kind == "execution.delivery.observed"
            && !text(&payload["evidence"]["class"]).starts_with("delivery_timeout")
            && payload["evidence"]["class"] != "recovery");
        if observed {
            e["last_observed"] = self.now.clone().into();
        }
    }
    #[allow(clippy::too_many_arguments)] // mirrors the released effect and obligation fields
    fn record_effect(
        &mut self,
        e: &mut Value,
        id: &str,
        kind: &str,
        class: &str,
        operation: &str,
        deadline: Value,
        expects: &str,
    ) {
        let mut auth = json!({"principal":e["principal"]});
        if let Some(grant) = e["submit"].get("grant") {
            auth["grant"] = grant.clone();
        }
        let mut descriptor = json!({"id":id,"kind":kind,"target":e["view"]["execution"],"payload_digest":e["submit"]["payload"]["brief"]["digest"],"authorization":auth,"retry_class":class,"operation_ref":operation});
        if class == "idempotent_key" {
            descriptor["idempotency_key"] = id.into();
        }
        self.data.effects.insert(id.into(),json!({"effect":descriptor,"revision":1,"status":"pending","observations":[{"status":"pending","evidence":self.host_evidence("recorded_before_dispatch"),"recorded_at":self.now}],"attempts":[],"obligations":[{"id":format!("{id}.{expects}"),"expects":expects,"deadline":deadline,"state":"open"}]}));
        push(&mut e["view"]["effects"], json!(id));
    }
    fn observe_effect(&mut self, id: &str, status: &str, class: &str, close: bool) {
        let proof = self.host_evidence(class);
        let r = self.data.effects.get_mut(id).unwrap();
        r["revision"] = (num(&r["revision"]) + 1).into();
        r["status"] = status.into();
        push(
            &mut r["observations"],
            json!({"status":status,"evidence":proof,"recorded_at":self.now}),
        );
        if close {
            for o in r["obligations"].as_array_mut().unwrap() {
                if matches!(text(&o["state"]), "open" | "overdue") {
                    o["state"] = "satisfied".into();
                }
            }
        }
    }
    fn admit_execution(&mut self, e: &mut Value, command: Option<(&Value, &str)>) {
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        let delivery = format!("{id}.delivery-1");
        e["view"]["admission"] = "admitted".into();
        e["view"]["runtime"] = "preparing".into();
        e["view"].as_object_mut().unwrap().remove("queue_reason");
        e["admitted_at"] = self.now.clone().into();
        push(
            &mut e["view"]["deliveries"],
            json!({"delivery_id":delivery,"delivery":"pending","history":[]}),
        );
        let deadline = e["submit"]["payload"]["timeouts"]["delivery"]
            .as_u64()
            .map(|n| json!(after(&self.now, n)))
            .unwrap_or(Value::Null);
        let op = text(&e["operation_ref"]).to_owned();
        self.record_effect(
            e,
            &delivery,
            "execution.prompt_submission",
            "non_repeatable",
            &op,
            deadline,
            "evidence",
        );
        self.execution_event(
            e,
            "execution.admission.changed",
            json!({"admission":"admitted","runtime":"preparing","delivery_id":delivery}),
            command,
        );
    }
    pub fn execution_apply(&mut self, session: &Session, method: &str, p: &Value) -> Reply {
        let id = text(&p["subject"]["id"]);
        let op = Uuid::new_v4().to_string();
        if method == "execution.controller.claim" {
            if id != self.execution_host() {
                return Err(err("not_found", json!({})));
            }
            let revision = self.revision(&p["subject"]) + 1;
            let outcome = json!({"epoch":revision});
            self.data.subjects.insert(
                key(&p["subject"]),
                Subject {
                    subject: p["subject"].clone(),
                    revision,
                    state: outcome.clone(),
                    applied: 1,
                },
            );
            self.append_event(
                p["subject"].clone(),
                revision,
                "execution.controller.claimed",
                json!({"epoch":revision,"controller":session.principal}),
                Some((p, &op)),
            );
            return Ok(
                json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":op,"subject":p["subject"],"revision":revision,"effect_refs":[]},"outcome":outcome,"replay":false}),
            );
        }
        let mut e = if method == "execution.submit" {
            json!({"source":script::SOURCE,"view":{"execution":subject(id),"revision":0,"admission":"queued","delivery":"pending","runtime":"not_started","result":"absent","exit":"unavailable","evaluation":"not_requested","deliveries":[],"completions":[],"effects":[],"host":{"id":self.execution_host(),"generation":1},"recovery":[],"usage":{"observations":[],"liability":"none"}},"submit":p,"principal":session.principal,"operation_ref":op,"script":script::select(&self.config["executor"],id),"step":0,"dispatched":false,"created_at":self.now,"order":self.data.sequence,"last_observed":self.now,"timeouts_passed":[],"output_ref":null,"output_end":0,"output_lost":[],"annotations":[]})
        } else {
            self.data
                .executions
                .get(id)
                .cloned()
                .ok_or_else(|| err("not_found", json!({})))?
        };
        if method == "execution.submit" {
            let script = e.as_object_mut().unwrap().remove("script").unwrap();
            let digest = pio_core::spool::Spool::open(&self.root)
                .and_then(|spool| spool.put(&serde_json::to_vec(&script)?))
                .map_err(|_| err("unavailable", json!({"reason":"script spool unavailable"})))?;
            e["script_digest"] = digest.into();
        }
        if self.native() {
            e["source"] = self.codex_source().into();
            e["view"]["host"]["generation"] = self.durable.as_ref().unwrap().generation.into();
            if let Some(extensions) = e["submit"]["extensions"].as_object_mut() {
                // Content bytes live in the spool by digest, not in the journal.
                extensions.remove(crate::codex::CONTENT_EXTENSION);
            }
            if matches!(method, "execution.steer" | "execution.respond_action") {
                let mut effects = vec![];
                let outcome = self.codex_apply(&mut e, method, p, &op, &mut effects)?;
                self.update_execution(&e);
                return Ok(
                    json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":op,"subject":p["subject"],"revision":e["view"]["revision"],"effect_refs":effects},"outcome":outcome,"replay":false}),
                );
            }
            if !matches!(method, "execution.submit" | "execution.cancel") {
                return Err(err(
                    "capability_unavailable",
                    json!({"reason":"not implemented by the Codex adapter"}),
                ));
            }
        } else if let Some(host) = &self.durable {
            e["source"] = "fake-host/process".into();
            e["view"]["host"]["generation"] = host.generation.into();
            if method == "execution.submit"
                && (p["payload"].get("workspace").is_some() || p["payload"].get("budget").is_some())
            {
                return Err(err(
                    "capability_unavailable",
                    json!({"reason":"durable fake host has no workspace or usage adapter"}),
                ));
            }
            if method != "execution.submit" {
                return Err(err(
                    "capability_unavailable",
                    json!({"reason":"durable fake host supports submit and observation only"}),
                ));
            }
        }
        let mut effects = vec![];
        let outcome = match method {
            "execution.submit" => {
                for field in ["predecessor", "correlation"] {
                    if let Some(value) = p["payload"].get(field) {
                        e["view"][field] = value.clone();
                    }
                }
                let rank = |s: &str| match s {
                    "enforced" => 3,
                    "mediated" => 2,
                    _ => 1,
                };
                let enforcement = self.config["executor"]["adapter"]["enforcement"]
                    .as_str()
                    .unwrap_or("cooperative");
                let mut refusal = None;
                if list(&p["payload"]["restrictions"])
                    .iter()
                    .any(|r| rank(text(&r["enforcement"])) > rank(enforcement))
                {
                    refusal = Some("enforcement_unavailable");
                }
                if refusal.is_none() {
                    for required in list(&p["payload"]["adapter"]["requires"]) {
                        if !list(&self.config["executor"]["adapter"]["predicates"])
                            .iter()
                            .any(|r| r["name"] == required && r["status"] == "supported")
                        {
                            refusal = Some("capability_unavailable");
                            break;
                        }
                    }
                }
                self.execution_budget(&mut e, p, &mut refusal);
                if refusal.is_none() && self.native() {
                    refusal = self.codex_admission(&mut e, p);
                }
                if let Some(reason) = refusal {
                    e["view"]["admission"] = "refused".into();
                    e["view"]["delivery"] = "failed_before_delivery".into();
                    e["view"]["reason"] = reason.into();
                    if reason == "enforcement_unavailable" {
                        e["view"]["alternative"] =
                            "Use a fake adapter that supports the required enforcement".into();
                    }
                    self.execution_event(
                        &mut e,
                        "execution.admission.changed",
                        json!({"admission":"refused","runtime":"not_started","reason":reason}),
                        Some((p, &op)),
                    );
                } else if self.capacity_available() {
                    self.admit_execution(&mut e, Some((p, &op)));
                    effects.push(json!(format!("{id}.delivery-1")));
                } else {
                    e["view"]["queue_reason"] = "capacity".into();
                    self.execution_event(&mut e,"execution.admission.changed",json!({"admission":"queued","runtime":"not_started","queue_reason":"capacity"}),Some((p,&op)));
                }
                if let Some(request) = p["payload"].get("workspace") {
                    let mut lease = request.clone();
                    lease["lease_id"] = format!("{id}.workspace").into();
                    lease["writer"] = session.principal.clone().unwrap().into();
                    lease["lease_epoch"] = 1.into();
                    e["view"]["workspace"] = json!({"lease":lease,"checkpoints":[]});
                }
                let mut outcome =
                    json!({"execution":subject(id),"admission":e["view"]["admission"]});
                for field in ["queue_reason", "reason", "alternative"] {
                    if let Some(v) = e["view"].get(field) {
                        outcome[field] = v.clone();
                    }
                }
                if e["view"]["admission"] == "admitted" {
                    outcome["delivery_id"] = format!("{id}.delivery-1").into();
                }
                outcome
            }
            "execution.cancel" => {
                let n = num(&e["cancel_count"]) + 1;
                e["cancel_count"] = n.into();
                let effect = format!("{id}.cancel-{n}");
                self.record_effect(
                    &mut e,
                    &effect,
                    "execution.cancel_forwarding",
                    "idempotent_key",
                    &op,
                    Value::Null,
                    "outcome",
                );
                self.data.effects.get_mut(&effect).unwrap()["effect"]["payload_digest"] =
                    pio_core::digest(&crate::encoding::canonical(&p["payload"])).into();
                effects.push(json!(effect));
                e["pending_cancel"] = effect.into();
                let receipt = json!({"state":"cancel_requested","operation_ref":op});
                e["view"]["cancellation"] = json!({"receipt":receipt});
                self.execution_event(
                    &mut e,
                    "execution.cancel.requested",
                    json!({"receipt":receipt}),
                    Some((p, &op)),
                );
                json!({"receipt":receipt})
            }
            "execution.workspace.checkpoint" => self.execution_checkpoint(&mut e, p, &op)?,
            _ => return Err(err("method_not_found", json!({"operation":method}))),
        };
        self.update_execution(&e);
        Ok(
            json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":op,"subject":p["subject"],"revision":e["view"]["revision"],"effect_refs":effects},"outcome":outcome,"replay":false}),
        )
    }
    pub fn execution_query(&self, session: &Session, method: &str, p: &Value) -> Reply {
        if method == "execution.discovery.list" {
            return Ok(self.execution_discovery());
        }
        if method == "execution.reconcile" {
            let mut executions = vec![];
            let mut deliveries = vec![];
            let mut obligations = vec![];
            for e in self.data.executions.values() {
                let command = p["payload"].get("command_id").is_some_and(|c| {
                    *c == e["submit"]["command_id"]
                        && e["principal"] == session.principal.clone().unwrap()
                });
                let delivery = p["payload"].get("delivery_id").is_some_and(|d| {
                    list(&e["view"]["deliveries"])
                        .iter()
                        .any(|r| r["delivery_id"] == *d)
                });
                if (command || delivery) && self.visible(session, p, &e["view"]["execution"]) {
                    executions.push(e["view"]["execution"].clone());
                    deliveries.extend(list(&e["view"]["deliveries"]));
                    obligations.extend(open_obligations(&self.data, e));
                }
            }
            return Ok(
                json!({"executions":executions,"deliveries":deliveries,"obligations":obligations}),
            );
        }
        let e = self
            .data
            .executions
            .get(text(&p["payload"]["execution"]))
            .ok_or_else(|| err("not_found", json!({})))?;
        if method == "execution.output.read" {
            return self.execution_output(e, p);
        }
        let mut view = e["view"].clone();
        view["next_cursor"] = self.cursor(self.head()).into();
        view["obligations"] = open_obligations(&self.data, e).into();
        Ok(view)
    }
    fn execution_budget(&self, e: &mut Value, p: &Value, refusal: &mut Option<&str>) {
        let Some(request) = p["payload"].get("budget") else {
            return;
        };
        let mut budget = request.clone();
        budget["amount"] = request["amount"].as_u64().unwrap_or(1).into();
        let pool = &self.config["executor"]["budget_pools"][text(&request["pool"])];
        if let Some(measure) = pool.get("measure") {
            budget["measure"] = measure.clone();
        }
        if refusal.is_none() {
            if pool.is_null() {
                *refusal = Some("budget_unavailable");
            } else if request["ceiling"] == "hard"
                && !list(&self.config["executor"]["adapter"]["enforced_bounds"])
                    .contains(&pool["measure"])
            {
                *refusal = Some("enforcement_unavailable");
            } else {
                let used: u64 = self
                    .data
                    .executions
                    .values()
                    .filter(|e| {
                        e["view"]["usage"]["budget"]["pool"] == request["pool"]
                            && matches!(
                                text(&e["view"]["usage"]["budget"]["reservation"]),
                                "reserved" | "settled"
                            )
                    })
                    .map(|e| num(&e["view"]["usage"]["budget"]["amount"]))
                    .sum();
                if used.saturating_add(num(&budget["amount"])) > num(&pool["limit"]) {
                    *refusal = Some("budget_exhausted");
                }
            }
        }
        budget["reservation"] = if refusal.is_some() {
            "not_reserved"
        } else {
            "reserved"
        }
        .into();
        e["view"]["usage"]["budget"] = budget;
    }
    fn execution_checkpoint(&mut self, e: &mut Value, p: &Value, op: &str) -> Reply {
        if e["view"].get("workspace").is_none() {
            return Err(err("not_found", json!({})));
        }
        let n = list(&e["view"]["workspace"]["checkpoints"]).len() + 1;
        let id = text(&e["view"]["execution"]["id"]);
        let probes = list(&e["workspace_probe"]["probes"]);
        let mut checkpoint = json!({"checkpoint_id":format!("{id}.checkpoint-{n}"),"lease_epoch":e["view"]["workspace"]["lease"]["lease_epoch"],"recorded_at":self.now,"coverage":{},"complete":probes.len()==3,"annotations":e["annotations"]});
        for (area, field) in [
            ("tracked", "head"),
            ("dirty", "dirty_paths"),
            ("untracked", "untracked_paths"),
        ] {
            let probed = probes.contains(&json!(area));
            checkpoint["coverage"][area] = if probed { "probed" } else { "not_probed" }.into();
            if probed {
                checkpoint["probed_at"] = self.now.clone().into();
                if let Some(value) = e["workspace_probe"].get(field) {
                    checkpoint[field] = value.clone();
                }
            }
        }
        push(
            &mut e["view"]["workspace"]["checkpoints"],
            checkpoint.clone(),
        );
        self.execution_event(
            e,
            "execution.workspace.checkpointed",
            json!({"checkpoint_id":checkpoint["checkpoint_id"],"complete":checkpoint["complete"]}),
            Some((p, op)),
        );
        Ok(checkpoint)
    }
    fn execution_discovery(&self) -> Value {
        if self.native() {
            return self.codex_discovery();
        }
        if self.durable.is_some() {
            // The adapter is built into this executable and this live controller
            // is reachable. It has no native authentication probe: unknown is
            // not a positive fact, even though the Protocol caller authenticated.
            return json!({"installations":[{"installation_id":"pio-builtin-fake-process","harness":"PIO labeled fake process (no native authentication)","version":env!("CARGO_PKG_VERSION"),"detected":true,"adapter_recognized":"yes","version_supported":"yes","authentication":"unknown","reachable":"yes","last_verified":self.now,"usable":false}]});
        }
        let installations: Vec<_> = list(&self.config["executor"]["installations"])
            .into_iter()
            .map(|mut i| {
                for name in [
                    "adapter_recognized",
                    "version_supported",
                    "authentication",
                    "reachable",
                ] {
                    if i.get(name).is_none() {
                        i[name] = "unknown".into();
                    }
                }
                i["usable"] = (i["detected"] == true
                    && i["adapter_recognized"] == "yes"
                    && i["version_supported"] == "yes"
                    && i["authentication"] == "authenticated"
                    && i["reachable"] == "yes"
                    && i["last_verified"].is_string())
                .into();
                i
            })
            .collect();
        json!({"installations":installations})
    }
    fn execution_output(&self, e: &Value, p: &Value) -> Reply {
        use base64::Engine;
        let bytes = self
            .output_bytes(e)
            .map_err(|error| err("unavailable", json!({"reason":error.to_string()})))?;
        let end = num(&e["output_end"]);
        let oldest = end - bytes.len() as u64;
        let offset = num(&p["payload"]["offset"]).max(oldest).min(end);
        let limit = p["payload"]["max_bytes"].as_u64().unwrap_or(65536) as usize;
        let data = &bytes[(offset - oldest) as usize..];
        let data = &data[..data.len().min(limit)];
        Ok(
            json!({"execution":e["view"]["execution"],"offset":offset,"data_base64":base64::engine::general_purpose::STANDARD.encode(data),"next_offset":offset+data.len() as u64,"end_offset":end,"lost_ranges":e["output_lost"],"coverage":if list(&e["output_lost"]).is_empty(){"complete"}else{"incomplete"},"policy":{"spool_bytes":self.config["executor"]["output_spool_bytes"].as_u64().unwrap_or(65536),"overflow":"discard_oldest"}}),
        )
    }
    pub(crate) fn lost_output(&mut self, e: &mut Value, loss: Value) {
        self.execution_event(e, "execution.output.lost", loss.clone(), None);
        let previous = e["output_lost"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .rev()
            .find(|r| r["reason"] == "spool_limit");
        if let Some(previous) = previous
            && previous["reason"] == "spool_limit"
            && loss["reason"] == "spool_limit"
            && previous["to"] == loss["from"]
        {
            previous["to"] = loss["to"].clone();
            previous["bytes"] = (num(&previous["bytes"]) + num(&loss["bytes"])).into();
        } else {
            push(&mut e["output_lost"], loss);
        }
    }
    pub(crate) fn delivery_observed(
        &mut self,
        e: &mut Value,
        determination: &str,
        class: &str,
        proof: Option<&str>,
        close: bool,
        reconcile: bool,
    ) {
        let id = format!("{}.delivery-1", text(&e["view"]["execution"]["id"]));
        let record = &mut e["view"]["deliveries"][0];
        let old = record["delivery"].clone();
        if old != determination {
            let mut history = json!({"delivery":old,"recorded_at":record.get("determined_at").cloned().unwrap_or_else(||json!(self.now))});
            if let Some(v) = record.get("evidence") {
                history["evidence"] = v.clone();
            }
            push(&mut record["history"], history);
        }
        record["delivery"] = determination.into();
        record["evidence"] = self.host_evidence(class);
        record["determined_at"] = self.now.clone().into();
        record.as_object_mut().unwrap().remove("proof_class");
        if let Some(proof) = proof {
            record["proof_class"] = proof.into();
        }
        e["view"]["delivery"] = determination.into();
        if determination == "ambiguous" {
            e["ambiguous_at"] = self.now.clone().into();
        }
        let status = match determination {
            "acknowledged" | "delivered" => "succeeded",
            "not_delivered" | "failed_before_delivery" => "failed",
            "ambiguous" => "unknown",
            _ => "pending",
        };
        self.observe_effect(&id, status, class, close);
        let mut payload =
            json!({"delivery_id":id,"delivery":determination,"evidence":self.host_evidence(class)});
        if let Some(proof) = proof {
            payload["proof_class"] = proof.into();
        }
        let kind = if reconcile {
            payload["outcome"] = match determination {
                "acknowledged" | "delivered" => "delivered",
                "failed_before_delivery" | "not_delivered" => "not_delivered",
                _ => "unknown",
            }
            .into();
            "execution.delivery.reconciled"
        } else {
            "execution.delivery.observed"
        };
        self.execution_event(e, kind, payload, None);
        if determination == "failed_before_delivery" && e["view"]["usage"].get("budget").is_some() {
            e["view"]["usage"]["budget"]["reservation"] = "released".into();
        }
    }
    fn overdue_execution(&mut self, e: &mut Value) {
        let ids = list(&e["view"]["effects"]);
        for id in ids {
            let mut changed = vec![];
            let r = self.data.effects.get_mut(text(&id)).unwrap();
            for o in r["obligations"].as_array_mut().unwrap() {
                if o["state"] == "open" {
                    o["state"] = "overdue".into();
                    changed.push(o["id"].clone());
                }
            }
            if !changed.is_empty() {
                r["revision"] = (num(&r["revision"]) + 1).into();
            }
            for obligation in changed {
                self.execution_event(
                    e,
                    "core.effect.obligation.overdue",
                    json!({"effect":id,"obligation":obligation}),
                    None,
                );
            }
        }
    }
    fn execution_timeouts(&mut self, e: &mut Value) {
        for name in [
            "queue",
            "delivery",
            "execution_deadline",
            "inactivity",
            "reconciliation",
        ] {
            let Some(seconds) = e["submit"]["payload"]["timeouts"][name].as_u64() else {
                continue;
            };
            if list(&e["timeouts_passed"]).contains(&json!(name)) {
                continue;
            }
            let live = match name {
                "queue" => e["view"]["admission"] == "queued",
                "delivery" => {
                    e["view"]["admission"] == "admitted" && e["view"]["delivery"] == "pending"
                }
                "reconciliation" => e["view"]["delivery"] == "ambiguous",
                _ => e["view"]["admission"] == "admitted" && e["view"]["runtime"] != "exited",
            };
            let start = match name {
                "queue" => text(&e["created_at"]),
                "inactivity" => text(&e["last_observed"]),
                "reconciliation" => text(&e["ambiguous_at"]),
                _ => text(&e["admitted_at"]),
            };
            if !live || start.is_empty() || self.now < after(start, seconds) {
                continue;
            }
            push(&mut e["timeouts_passed"], json!(name));
            self.execution_event(e, "execution.timeout.passed", json!({"timeout":name}), None);
            match name {
                "queue" => {
                    e["view"]["admission"] = "refused".into();
                    e["view"]["delivery"] = "failed_before_delivery".into();
                    e["view"]["reason"] = "queue_timeout".into();
                    e["view"].as_object_mut().unwrap().remove("queue_reason");
                    if e["view"]["usage"].get("budget").is_some() {
                        e["view"]["usage"]["budget"]["reservation"] = "released".into();
                    }
                    self.execution_event(e,"execution.admission.changed",json!({"admission":"refused","runtime":"not_started","reason":"queue_timeout"}),None);
                }
                "delivery" => {
                    self.overdue_execution(e);
                    let dispatched = e["dispatched"] == true;
                    self.delivery_observed(
                        e,
                        if dispatched {
                            "ambiguous"
                        } else {
                            "failed_before_delivery"
                        },
                        if dispatched {
                            "delivery_timeout"
                        } else {
                            "delivery_timeout_before_dispatch"
                        },
                        None,
                        false,
                        false,
                    );
                }
                "reconciliation" => self.overdue_execution(e),
                _ => {}
            }
        }
    }
    pub fn execution_recover(&mut self) -> anyhow::Result<()> {
        for id in self.data.executions.keys().cloned().collect::<Vec<_>>() {
            let mut e = self.data.executions[&id].clone();
            let pending = e["view"]["delivery"] == "pending";
            let live_durable = self.durable.is_some()
                && e["view"]["runtime"] != "exited"
                && !matches!(
                    text(&e["view"]["delivery"]),
                    "failed_before_delivery" | "not_delivered"
                );
            if e["view"]["admission"] != "admitted" || (!pending && !live_durable) {
                continue;
            }
            let session = Session {
                principal: Some(text(&e["principal"]).to_owned()),
                ..Session::default()
            };
            let deadline = ["delivery", "execution_deadline"].iter().any(|n| {
                e["submit"]["payload"]["timeouts"][*n]
                    .as_u64()
                    .is_some_and(|seconds| self.now >= after(text(&e["admitted_at"]), seconds))
            });
            let (decision, reason) = if self.config["events"]["new_epoch_on_start"] == true {
                ("ambiguous", "journal_not_intact")
            } else if e["dispatched"] == true {
                ("ambiguous", "dispatch_may_have_begun")
            } else if e["view"].get("cancellation").is_some() {
                ("failed_before_delivery", "cancelled")
            } else if self
                .authorize(&session, "execution.submit", &e["submit"])
                .is_err()
            {
                ("failed_before_delivery", "authorization_lost")
            } else if deadline {
                ("failed_before_delivery", "deadline_passed")
            } else if self.config["executor"]["recovery_policy"] == "terminate" {
                ("failed_before_delivery", "recovery_policy")
            } else {
                ("dispatch_resumed", "provably_not_dispatched")
            };
            e["view"]["host"]["generation"] = self
                .durable
                .as_ref()
                .map(|host| host.generation)
                .unwrap_or(num(&e["view"]["host"]["generation"]) + 1)
                .into();
            if let Some(host) = &self.durable {
                let command = pio_core::digest(id.as_bytes());
                if e["dispatched"] == true
                    && host.inspect(command.trim_start_matches("sha256:")).is_err()
                {
                    e["recovery_no_admit"] = true.into();
                    e["view"]["runtime"] = "unknown".into();
                }
            }
            let recovery = json!({"delivery_id":format!("{id}.delivery-1"),"decision":decision,"reason":reason,"recorded_at":self.now});
            push(&mut e["view"]["recovery"], recovery.clone());
            let mut event = recovery;
            event.as_object_mut().unwrap().remove("recorded_at");
            event["host"] = e["view"]["host"].clone();
            self.execution_event(&mut e, "execution.recovery.decided", event, None);
            // Reconciliation does not erase an already observed child release.
            // A pending delivery is made ambiguous before observing the host.
            if pending && decision != "dispatch_resumed" {
                if reason == "deadline_passed" {
                    self.overdue_execution(&mut e);
                }
                self.delivery_observed(
                    &mut e,
                    decision,
                    "recovery",
                    None,
                    decision == "failed_before_delivery" && reason != "deadline_passed",
                    false,
                );
                let effect = self
                    .data
                    .effects
                    .get_mut(&format!("{id}.delivery-1"))
                    .unwrap();
                let obs = effect["observations"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap();
                obs["evidence"] = evidence(if decision == "ambiguous" {
                    "dispatch_uncertain"
                } else {
                    "never_dispatched"
                });
            }
            let host = e["view"]["host"].clone();
            self.execution_event(&mut e, "execution.host.changed", json!({"host":host}), None);
            self.update_execution(&e);
        }
        self.save()
    }
    pub(crate) fn commit_execution(&mut self, e: &Value) -> anyhow::Result<()> {
        self.update_execution(e);
        if let Err(error) = self.save() {
            self.reload()?;
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn dispatch_marker(&mut self, e: &mut Value) -> anyhow::Result<()> {
        if e["dispatched"] != true {
            e["dispatched"] = true.into();
            e["dispatch_generation"] = e["view"]["host"]["generation"].clone();
            let id = format!("{}.delivery-1", text(&e["view"]["execution"]["id"]));
            self.observe_effect(&id, "pending", "dispatch_intent", false);
            if self.durable.is_some() {
                push(
                    &mut self.data.effects.get_mut(&id).unwrap()["attempts"],
                    json!({"attempt":1,"outcome":"unknown","recorded_at":self.now}),
                );
            }
            // This synchronous FULL journal commit is the authority to deliver.
            // A failed commit never reaches the scripted host observation below.
            self.commit_execution(e)?;
        }
        Ok(())
    }
    fn attempt(&mut self, e: &mut Value, effect: &str, retry: bool) -> bool {
        let max = if retry { 3 } else { 1 };
        let mut completed = false;
        for _ in 0..max {
            let fail = num(&e["transport_errors"]) > 0;
            if fail {
                e["transport_errors"] = (num(&e["transport_errors"]) - 1).into();
            }
            let r = self.data.effects.get_mut(effect).unwrap();
            let n = list(&r["attempts"]).len() + 1;
            let mut attempt = json!({"attempt":n,"outcome":if fail{"unknown"}else{"completed"},"recorded_at":self.now});
            if r["effect"]["retry_class"] == "idempotent_key" {
                attempt["idempotency_key"] = effect.into();
                r["effect"]["idempotency_key"] = effect.into();
            }
            push(&mut r["attempts"], attempt);
            if fail {
                self.observe_effect(effect, "unknown", "transport_error", false);
            } else {
                completed = true;
                break;
            }
        }
        completed
    }
    fn run_script(&mut self, e: &mut Value) -> anyhow::Result<()> {
        let script: Value = serde_json::from_slice(
            &pio_core::spool::Spool::open(&self.root)?.read(text(&e["script_digest"]))?,
        )?;
        let id = text(&e["view"]["execution"]["id"]).to_owned();
        let delivery = format!("{id}.delivery-1");
        loop {
            let i = num(&e["step"]) as usize;
            let Some(step) = script.as_array().and_then(|a| a.get(i)).cloned() else {
                break;
            };
            if step["stall"] == true {
                break;
            }
            if step["wait_until"]
                .as_str()
                .is_some_and(|at| at > self.now.as_str())
            {
                break;
            }
            if step["wait_for"] == "cancel" && e["view"].get("cancellation").is_none() {
                break;
            }
            e["step"] = (i + 1).into();
            if let Some(proof) = step["deliver"].as_str() {
                if e["view"]["delivery"] == "pending" {
                    let first = e["dispatched"] != true;
                    self.dispatch_marker(e)?;
                    if first && !self.attempt(e, &delivery, false) {
                        self.delivery_observed(
                            e,
                            "ambiguous",
                            "transport_error",
                            None,
                            false,
                            false,
                        );
                    } else {
                        let ack = proof == "provider_ack_id"
                            || (proof == "echo"
                                && self.config["executor"]["adapter"]["echo_proves_delivery"]
                                    == true);
                        self.delivery_observed(
                            e,
                            if ack { "acknowledged" } else { "pending" },
                            proof,
                            Some(proof),
                            ack,
                            false,
                        );
                    }
                }
            } else if let Some(crash) = step["crash"].as_str() {
                if e["view"]["delivery"] == "pending" {
                    if crash == "after_write" {
                        self.dispatch_marker(e)?;
                        let r = self.data.effects.get_mut(&delivery).unwrap();
                        if list(&r["attempts"]).is_empty() {
                            push(
                                &mut r["attempts"],
                                json!({"attempt":1,"outcome":"unknown","recorded_at":self.now}),
                            );
                        }
                    }
                    self.commit_execution(e)?;
                    std::process::exit(86);
                }
            } else if let Some(outcome) = step["reconcile_finds"].as_str() {
                if e["view"]["delivery"] == "ambiguous" {
                    self.delivery_observed(
                        e,
                        if outcome == "unknown" {
                            "ambiguous"
                        } else {
                            outcome
                        },
                        "reconciliation",
                        None,
                        outcome != "unknown",
                        true,
                    );
                }
            } else if let Some(runtime) = step["runtime"].as_str() {
                e["view"]["runtime"] = runtime.into();
                self.execution_event(
                    e,
                    "execution.runtime.changed",
                    json!({"runtime":runtime}),
                    None,
                );
            } else if step["host_restart"] == true {
                e["view"]["host"]["generation"] =
                    (num(&e["view"]["host"]["generation"]) + 1).into();
                let host = e["view"]["host"].clone();
                self.execution_event(e, "execution.host.changed", json!({"host":host}), None);
            } else if let Some(generation) = step["stale_dispatch"]["generation"].as_u64() {
                let current = num(&e["view"]["host"]["generation"]);
                if generation != current {
                    self.execution_event(e,"execution.dispatch.fenced",json!({"delivery_id":delivery,"generation":generation,"current_generation":current}),None);
                } else if e["view"]["delivery"] == "pending" && e["dispatched"] != true {
                    self.dispatch_marker(e)?;
                    if !self.attempt(e, &delivery, false) {
                        self.delivery_observed(
                            e,
                            "ambiguous",
                            "transport_error",
                            None,
                            false,
                            false,
                        );
                    }
                }
            } else if let Some(completion) = step.get("complete") {
                let digest = pio_core::digest(text(&completion["content"]).as_bytes());
                let generation = completion["generation"]
                    .as_u64()
                    .unwrap_or(num(&e["view"]["host"]["generation"]));
                let existing = list(&e["view"]["completions"])
                    .into_iter()
                    .find(|c| c["completion_id"] == completion["completion_id"]);
                let status = if generation != num(&e["view"]["host"]["generation"]) {
                    "superseded_attempt"
                } else if let Some(old) = existing {
                    if old["digest"] == digest {
                        "duplicate"
                    } else {
                        "conflict"
                    }
                } else {
                    "recorded"
                };
                push(
                    &mut e["view"]["completions"],
                    json!({"completion_id":completion["completion_id"],"digest":digest,"generation":generation,"status":status}),
                );
                self.execution_event(e,"execution.completion.recorded",json!({"completion_id":completion["completion_id"],"digest":digest,"invocation_id":format!("{id}.invocation-1"),"host":{"id":e["view"]["host"]["id"],"generation":generation},"status":status}),None);
                if status == "recorded" {
                    e["view"]["result"] = "returned".into();
                    e["view"]["finalized_by"] = completion["completion_id"].clone();
                    self.execution_event(
                        e,
                        "execution.result.changed",
                        json!({"result":"returned"}),
                        None,
                    );
                }
            } else if let Some(exit) = step.get("exit") {
                e["view"]["exit"] = exit.clone();
                e["view"]["runtime"] = "exited".into();
                self.execution_event(e, "execution.exit.observed", json!({"exit":exit}), None);
            } else if let Some(outcome) = step.get("on_cancel") {
                e["on_cancel"] = outcome.clone();
            } else if let Some(errors) = step.get("transport_errors") {
                e["transport_errors"] = errors.clone();
            } else if let Some(probe) = step.get("workspace") {
                e["workspace_probe"] = probe.clone();
            } else if let Some(commit) = step.get("agent_reports_commit") {
                push(
                    &mut e["annotations"],
                    json!({"kind":"agent_reported_commit","value":commit,"basis":"agent_report","recorded_at":self.now}),
                );
            } else if let Some(usage) = step.get("usage") {
                let mut observation = usage.clone();
                observation["recorded_at"] = self.now.clone().into();
                let observations = e["view"]["usage"]["observations"].as_array_mut().unwrap();
                if let Some(old) = observations
                    .iter_mut()
                    .find(|o| o["invocation_id"] == usage["invocation_id"])
                {
                    *old = observation.clone();
                } else {
                    observations.push(observation.clone());
                }
                let resolved = observations
                    .iter()
                    .all(|o| matches!(text(&o["basis"]), "observed" | "enforced_bound"));
                e["view"]["usage"]["liability"] =
                    if resolved { "resolved" } else { "unresolved" }.into();
                if resolved && e["view"]["usage"].get("budget").is_some() {
                    e["view"]["usage"]["budget"]["reservation"] = "settled".into();
                }
                self.execution_event(e, "execution.usage.observed", observation, None);
            } else if step["probe_status"] == true {
                let n = num(&e["probe_count"]) + 1;
                e["probe_count"] = n.into();
                let effect = format!("{id}.probe-{n}");
                let op = text(&e["operation_ref"]).to_owned();
                self.record_effect(
                    e,
                    &effect,
                    "execution.status_probe",
                    "read",
                    &op,
                    Value::Null,
                    "result",
                );
                self.data.effects.get_mut(&effect).unwrap()["effect"]["payload_digest"] =
                    pio_core::digest(&crate::encoding::canonical(&json!({"execution":id}))).into();
                self.commit_execution(e)?;
                let success = self.attempt(e, &effect, true);
                self.observe_effect(
                    &effect,
                    if success { "succeeded" } else { "unknown" },
                    if success {
                        "harness_status"
                    } else {
                        "transport_error"
                    },
                    success,
                );
            } else if let Some(output) = step.get("output") {
                e["last_observed"] = self.now.clone().into();
                let bytes = if let Some(s) = output["text"].as_str() {
                    s.as_bytes().to_vec()
                } else {
                    let repeat = text(&output["repeat"]).as_bytes();
                    repeat
                        .iter()
                        .copied()
                        .cycle()
                        .take(num(&output["bytes"]) as usize)
                        .collect()
                };
                self.append_output(e, &bytes)?;
            } else if let Some(loss) = step.get("output_lost") {
                let mut loss = loss.clone();
                loss["from"] = e["output_end"].clone();
                loss["to"] = e["output_end"].clone();
                loss["coverage"] = "incomplete".into();
                self.lost_output(e, loss);
            } else if let Some(count) = step["runtime_burst"].as_u64() {
                for n in 0..count {
                    let runtime = if n % 2 == 0 { "active" } else { "quiescent" };
                    e["view"]["runtime"] = runtime.into();
                    self.execution_event(
                        e,
                        "execution.runtime.changed",
                        json!({"runtime":runtime}),
                        None,
                    );
                }
            }
        }
        if let Some(effect) = e["pending_cancel"].as_str().map(str::to_owned)
            && e.get("on_cancel").is_some()
        {
            let outcome = e["on_cancel"].as_str().unwrap_or("unknown").to_owned();
            let success = self.attempt(e, &effect, true);
            let outcome = if success {
                outcome
            } else {
                "unknown".to_owned()
            };
            self.observe_effect(
                &effect,
                if outcome == "unknown" {
                    "unknown"
                } else {
                    "succeeded"
                },
                "harness_response",
                outcome != "unknown",
            );
            e["view"]["cancellation"]["outcome"] = outcome.clone().into();
            e.as_object_mut().unwrap().remove("pending_cancel");
            self.execution_event(
                e,
                "execution.cancel.observed",
                json!({"outcome":outcome}),
                None,
            );
        }
        Ok(())
    }
    pub fn execution_tick(&mut self) -> anyhow::Result<()> {
        if let Err(error) = self.execution_tick_inner() {
            self.reload()?;
            return Err(error);
        }
        Ok(())
    }
    fn execution_tick_inner(&mut self) -> anyhow::Result<()> {
        let mut ids: Vec<_> = self
            .data
            .executions
            .iter()
            .map(|(id, e)| (num(&e["order"]), id.clone()))
            .collect();
        ids.sort();
        for (_, id) in &ids {
            let mut e = self.data.executions[id].clone();
            self.execution_timeouts(&mut e);
            if e["view"]["admission"] == "admitted" {
                if self.durable.is_some() {
                    self.run_durable(&mut e)?;
                } else {
                    self.run_script(&mut e)?;
                }
            }
            self.commit_execution(&e)?;
        }
        for (_, id) in ids {
            let mut e = self.data.executions[&id].clone();
            if e["view"]["admission"] == "queued" && self.capacity_available() {
                self.admit_execution(&mut e, None);
                self.commit_execution(&e)?;
                if self.durable.is_some() {
                    self.run_durable(&mut e)?;
                } else {
                    self.run_script(&mut e)?;
                }
                self.commit_execution(&e)?;
            }
        }
        Ok(())
    }
}
