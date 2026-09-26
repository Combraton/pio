use crate::persistence::{EventLog, Tracked};
use crate::{encoding, schemas};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use jsonschema::Validator;
use pio_core::Store;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

pub const FEATURES: &[&str] = &[
    "core.grants",
    "core.events",
    "core.capabilities",
    "core.effects",
    "core.events.backpressure",
];
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn num(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}
pub fn list(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
pub fn is_capacity_refusal(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<pio_core::projections::CapacityExceeded>()
        .is_some()
}
pub fn key(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}
#[derive(Debug, Clone)]
pub struct Error {
    pub code: String,
    pub details: Value,
}
pub type Reply = std::result::Result<Value, Error>;
pub fn err(code: &str, details: Value) -> Error {
    Error {
        code: code.into(),
        details,
    }
}
pub fn invalid(path: &str) -> Error {
    err(
        "invalid_envelope",
        json!({"path":path,"reason":"invalid request semantics"}),
    )
}
impl Error {
    pub fn frame(&self, id: Value) -> Value {
        let rpc = match self.code.as_str() {
            "parse_error" | "invalid_utf8" => -32700,
            "frame_too_large" => -32010,
            "invalid_request" => -32600,
            "method_not_found" => -32601,
            "overloaded" => -32011,
            _ => 1,
        };
        let retry = match self.code.as_str() {
            "negotiation_required" | "profile_not_negotiated" => "after_renegotiate",
            "dedupe_history_unavailable"
            | "effect_history_unavailable"
            | "stale_authority_epoch"
            | "precondition_failed"
            | "internal_error"
            | "capability_unavailable" => "after_reconcile",
            "unavailable" | "overloaded" => "same_command",
            _ => "no",
        };
        json!({"jsonrpc":"2.0","id":id,"error":{"code":rpc,"message":self.code,"data":{"code":self.code,"retry":retry,"details":self.details}}})
    }
}
#[derive(Default)]
pub struct Session {
    pub principal: Option<String>,
    pub selected: Option<BTreeMap<String, Vec<String>>>,
    pub receive: usize,
    pub request_id: Value,
    pub subscriptions: BTreeMap<String, Value>,
}
impl Session {
    pub fn feature(&self, f: &str) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|s| s.values().any(|v| v.iter().any(|x| x == f)))
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Subject {
    pub subject: Value,
    pub revision: u64,
    pub state: Value,
    pub applied: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Bound {
    pub digest: String,
    pub generation: u64,
    pub result: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Data {
    pub subjects: Tracked<Subject>,
    pub commands: Tracked<Bound>,
    pub generation: u64,
    pub oldest: u64,
    pub stream: String,
    pub epoch: u64,
    pub sequence: u64,
    pub events: EventLog,
    pub vouches: BTreeMap<u64, u64>,
    pub discarded: Option<(u64, u64)>,
    pub cap_revision: u64,
    pub predicates: Value,
    pub effects: Tracked<Value>,
    pub executions: Tracked<Value>,
}
pub struct Provider {
    pub root: std::path::PathBuf,
    pub durable: Option<pio_host::Controller>,
    pub host_config: Value,
    pub config: Value,
    pub data: Data,
    pub store: Store,
    pub store_revision: u64,
    pub validators: BTreeMap<String, Validator>,
    pub credentials: Vec<(String, String, bool)>,
    pub limits: Value,
    pub now: String,
    pub authorities: Vec<String>,
    pub provider_id: String,
    /// Last ADR 002 capacity refusal. The frozen public error for a refused
    /// commit is `unavailable` with nothing bound, so the reason is local.
    pub capacity_refusal: Option<Value>,
    /// Test oracle for changed-key commits: after each commit, the committed
    /// projection must equal the whole in-memory state. That is what a
    /// whole-state commit wrote, so each commit journaled the same facts.
    #[cfg(test)]
    pub verify_commits: bool,
}
impl Provider {
    pub fn new(root: &Path, config: Value) -> Result<Self> {
        Self::with_host(root, config, None)
    }
    pub fn with_host(root: &Path, config: Value, host: Option<Value>) -> Result<Self> {
        let resources = schemas::resources();
        let registry = jsonschema::Registry::new()
            .extend(
                resources
                    .iter()
                    .map(|v| (text(&v["$id"]).to_owned(), v.clone())),
            )?
            .prepare()?;
        let mut validators = BTreeMap::new();
        for resource in &resources {
            let id = text(&resource["$id"]);
            if let Some(name) = id
                .rsplit('/')
                .next()
                .and_then(|s| s.strip_suffix(".params.schema.json"))
            {
                validators.insert(
                    name.to_owned(),
                    jsonschema::options()
                        .with_registry(&registry)
                        .build(&json!({"$ref":id}))?,
                );
            }
        }
        let launch=jsonschema::options().with_registry(&registry).build(&json!({"$ref":"https://github.com/Combraton/protocol/conformance/schemas/launch-config.schema.json"}))?;
        ensure!(
            launch.is_valid(&config),
            "invalid conformance launch configuration"
        );
        for k in config.as_object().context("config object")?.keys() {
            ensure!(
                [
                    "format",
                    "principal",
                    "authority_principals",
                    "provider_id",
                    "limits",
                    "credentials",
                    "dedupe",
                    "clock",
                    "faults",
                    "events",
                    "capabilities",
                    "executor",
                    "test_barriers"
                ]
                .contains(&k.as_str()),
                "unsupported launch control: {k}"
            )
        }
        for barrier in list(&config["test_barriers"]["enabled"]) {
            ensure!(
                barrier == "session.closed",
                "unsupported test barrier: {barrier}"
            );
        }
        pio_host::script::validate(&config["executor"])?;
        std::fs::create_dir_all(root)?;
        ensure!(
            !root.join("protocol.sqlite3").exists(),
            "legacy conformance blob store requires explicit migration or a fresh data directory"
        );
        let store = Store::open(root)?;
        let (store_revision, records) = store.protocol_records()?;
        let data = Data::from_records(&records)?;
        for execution in data.executions.values() {
            ensure!(
                execution.get("output").is_none(),
                "legacy inline output requires explicit migration"
            );
            let persisted = execution["source"].as_str().unwrap_or("");
            // A restarted daemon must resume its own executions and refuse
            // someone else's adapter. The source it expects comes from the
            // harness table, so a new adapter cannot be silently rejected
            // here — which a restart case caught for Claude Code.
            let expected = match host.as_ref() {
                None => persisted == pio_host::script::SOURCE,
                Some(host) => match crate::codex::profile(host["adapter"].as_str().unwrap_or("")) {
                    Some(profile) => {
                        persisted
                            == if host["labeled_fake"] == true {
                                profile.fake_source
                            } else {
                                profile.real_source
                            }
                    }
                    None => persisted == "fake-host/process",
                },
            };
            ensure!(expected, "cannot switch persisted execution adapter mode");
        }
        let mut credentials = vec![];
        for c in list(&config["credentials"]) {
            let raw = text(&c["credential"]);
            let principal = raw
                .strip_prefix("ccred1.")
                .and_then(|x| x.rsplit_once('.'))
                .map(|x| x.0)
                .context("credential format")?;
            credentials.push((
                pio_core::digest(raw.as_bytes()),
                principal.to_owned(),
                c["revoked"] == true,
            ));
        }
        let mut limits = json!({"max_frame_bytes":1048576,"max_payload_bytes":1048576,"max_string_bytes":1048576,"max_array_items":10000,"max_depth":64});
        if let Some(m) = config["limits"].as_object() {
            for (k, v) in m {
                limits[k] = v.clone();
            }
        }
        limits["max_frame_bytes"] = num(&limits["max_frame_bytes"]).max(1048576).into();
        let principal = config["principal"].as_str().unwrap_or("conformance");
        let authorities = config
            .get("authority_principals")
            .map(|a| list(a).iter().map(|v| text(v).to_owned()).collect())
            .unwrap_or(vec![principal.to_owned()]);
        let provider_id = config["provider_id"]
            .as_str()
            .unwrap_or("conformance-provider")
            .to_owned();
        let durable = if host.is_some() {
            Some(pio_host::Controller::open(root)?)
        } else {
            None
        };
        let mut p = Self {
            root: root.to_owned(),
            durable,
            host_config: host.unwrap_or(Value::Null),
            config,
            data,
            store,
            store_revision,
            validators,
            credentials,
            limits,
            now: String::new(),
            authorities,
            provider_id,
            capacity_refusal: None,
            #[cfg(test)]
            verify_commits: true,
        };
        p.clock(true)?;
        p.data.generation += num(&p.config["dedupe"]["advance_on_start"]);
        if let Some(retain) = p.config["dedupe"]["retain_generations"].as_u64() {
            p.data.oldest = p
                .data
                .oldest
                .max((p.data.generation + 1).saturating_sub(retain));
            p.data.commands.retain(|_, b| b.generation >= p.data.oldest);
        }
        p.events_start();
        p.capabilities_start();
        p.save()?;
        p.execution_recover()?;
        Ok(p)
    }
    pub fn capabilities_start(&mut self) {
        let mut statuses = BTreeMap::from([("core-test.writes".to_owned(), json!("supported"))]);
        if let Some(config) = self.config["capabilities"].as_object() {
            statuses.extend(config.clone());
        }
        let predicates:Value=statuses.into_iter().map(|(name,status)|json!({"name":name,"status":status,"evidence":{"source":"pio-conformance-launch-config"}})).collect::<Vec<_>>().into();
        if predicates != self.data.predicates {
            let changed = !list(&self.data.predicates).is_empty();
            if changed {
                self.data.cap_revision += 1;
            }
            self.data.predicates = predicates.clone();
            let subject = json!({"kind":"core.capabilities","id":self.provider_id});
            self.data.subjects.insert(
                key(&subject),
                Subject {
                    subject: subject.clone(),
                    revision: self.data.cap_revision,
                    state: json!({"predicates":predicates}),
                    applied: 0,
                },
            );
            if changed {
                self.append_event(
                    subject,
                    self.data.cap_revision,
                    "core.capabilities.changed",
                    json!({"predicates":predicates}),
                    None,
                )
            }
        }
    }
    pub fn save(&mut self) -> Result<()> {
        self.commit(false)
    }
    /// Commit that binds a new `execution.submit`; it must stay within the
    /// ADR 002 admission thresholds as well as the hard limits.
    pub fn save_admission(&mut self) -> Result<()> {
        self.commit(true)
    }
    /// Commits only the records changed since the last commit (ADR 002
    /// changed-key amendment); the store compares each with its stored value.
    fn commit(&mut self, admission: bool) -> Result<()> {
        let changes = self.data.take_changes()?;
        let result = if admission {
            self.store
                .commit_admission_changes(self.store_revision, &changes)
        } else {
            self.store.commit_changes(self.store_revision, &changes)
        };
        match result {
            Ok(revision) => {
                self.store_revision = revision;
                #[cfg(test)]
                if self.verify_commits {
                    assert_eq!(
                        self.store.protocol_records()?,
                        (revision, self.data.records()?),
                        "a changed-key commit missed a change"
                    );
                }
                Ok(())
            }
            Err(error) => {
                self.data.restore_changes(changes);
                if let Some(refusal) =
                    error.downcast_ref::<pio_core::projections::CapacityExceeded>()
                {
                    let refusal = json!({"limit":refusal.limit,"maximum":refusal.maximum,"projected":refusal.projected});
                    // Background retries repeat the same refusal every tick;
                    // log each distinct refusal once.
                    if self.capacity_refusal.as_ref() != Some(&refusal) {
                        eprintln!("PIO capacity refusal: {refusal}");
                    }
                    self.capacity_refusal = Some(refusal);
                }
                Err(error)
            }
        }
    }
    pub fn reload(&mut self) -> Result<()> {
        let (revision, records) = self.store.protocol_records()?;
        self.data = Data::from_records(&records)?;
        self.store_revision = revision;
        Ok(())
    }
    pub fn clock(&mut self, start: bool) -> Result<()> {
        let read = if let Some(file) = self.config["clock"]["file"].as_str() {
            std::fs::read_to_string(file).map(|s| s.trim().to_owned())
        } else {
            Ok(self.config["clock"]["fixed"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()))
        };
        match read {
            Ok(s) if s.len() == 20 && chrono::DateTime::parse_from_rfc3339(&s).is_ok() => {
                if s > self.now {
                    self.now = s
                }
            }
            _ => ensure!(!start, "invalid or missing controlled clock"),
        };
        Ok(())
    }
    pub fn window(&self) -> Value {
        json!({"oldest_retained":self.data.oldest,"current":self.data.generation})
    }
    pub fn execution_features(&self) -> &'static [&'static str] {
        if self.native() {
            if self.profile().steering_supported {
                crate::codex::FEATURES
            } else {
                crate::codex::FEATURES_WITHOUT_STEERING
            }
        } else if self.durable.is_some() {
            &[
                "execution.controller",
                "execution.output",
                "execution.discovery",
            ]
        } else {
            crate::execution::FEATURES
        }
    }
    pub fn manifest(&self) -> Value {
        json!({"provider":{"name":"pio-journal-fake-executor","version":"0.1.0-dev"},"profiles":[{"name":"core","majors":[1],"features":FEATURES,"depends_on":[]},{"name":"core-test","majors":[1],"features":[],"depends_on":["core"]},{"name":"execution","majors":[1],"features":self.execution_features(),"depends_on":["core"]}],"unsupported_profiles":[{"name":"coordination","reason":"not_in_release"},{"name":"remote-trust","reason":"not_in_release"}],"limits":self.limits,"dedupe_window":self.window(),"unknown_extensions":"drop"})
    }
    pub fn handle(&mut self, session: &mut Session, method: &str, p: &Value) -> Reply {
        let _ = self.clock(false);
        // Background commits reload committed state when refused. A capacity
        // refusal leaves queries and bound replays answerable from that state;
        // a new command still needs its own commit and is refused there. Any
        // other store failure makes the whole request unavailable.
        let background = |result: Result<()>| match result {
            Err(error) if !is_capacity_refusal(&error) => Err(err("unavailable", json!({}))),
            _ => Ok(()),
        };
        background(self.execution_tick())?;
        background(self.expire_obligations())?;
        if session.principal.is_none() && !matches!(method, "core.describe" | "core.authenticate") {
            return Err(err("authentication_required", json!({})));
        }
        if !self.validators.contains_key(method) {
            return Err(err("method_not_found", json!({"operation":method})));
        }
        let profile = method.split('.').next().unwrap_or("");
        if !matches!(
            method,
            "core.describe" | "core.authenticate" | "core.negotiate" | "core.feature_dependencies"
        ) {
            let Some(selected) = &session.selected else {
                return Err(err("negotiation_required", json!({})));
            };
            if !selected.contains_key(profile) {
                return Err(err("profile_not_negotiated", json!({"profile":profile})));
            }
        }
        self.validate_limits(p)?;
        if p["operation"] != method {
            return Err(invalid("/operation"));
        }
        if let Some(e) = self.validators[method].iter_errors(p).next() {
            return Err(invalid(&e.instance_path().to_string()));
        }
        if method == "core.negotiate" {
            let profiles = list(&p["payload"]["profiles"]);
            let names: std::collections::BTreeSet<_> =
                profiles.iter().map(|p| text(&p["name"])).collect();
            if names.len() != profiles.len() {
                return Err(invalid("/payload/profiles"));
            }
        }
        if p.get("grant").is_some() && !session.feature("core.grants") {
            return Err(invalid("/grant"));
        }
        self.execution_gate(session, method, p)?;
        let pre = list(&p["preconditions"]);
        let mut seen = vec![];
        for c in &pre {
            let k = key(&c["subject"]);
            if seen.contains(&k) {
                return Err(invalid("/preconditions"));
            }
            seen.push(k);
        }
        if matches!(
            method,
            "core-test.subject.put" | "core-test.authority.claim" | "core.effects.abort_obligation"
        ) && !pre.iter().any(|c| c["subject"] == p["subject"])
        {
            return Err(invalid("/preconditions"));
        }
        if matches!(method, "core.grant.issue" | "core.grant.revoke")
            && (pre.len() != 1
                || pre[0]["subject"] != p["subject"]
                || (method == "core.grant.issue" && pre[0]["revision"] != 0)
                || (method == "core.grant.revoke" && num(&pre[0]["revision"]) < 1))
        {
            return Err(invalid("/preconditions"));
        }
        let requires = list(&p["requires"]);
        for r in &requires {
            if text(r).contains('/') && p["extensions"].get(text(r)).is_none() {
                return Err(invalid("/requires"));
            }
        }
        if let Some(digest) = p["command_digest"].as_str() {
            let (a, b) = digest.split_once(':').unwrap_or(("", ""));
            if (a == "sha256" && b.len() != 64) || (a == "sha512" && b.len() != 128) {
                return Err(invalid("/command_digest"));
            }
        }
        let mut missing: Vec<Value> = requires
            .iter()
            .filter(|f| !session.feature(text(f)))
            .cloned()
            .collect();
        let feature = if method.starts_with("core.grant.") {
            Some("core.grants")
        } else if method.starts_with("core.events.") {
            Some("core.events")
        } else if method == "core.capabilities" {
            Some("core.capabilities")
        } else if method.starts_with("core.effects.") {
            Some("core.effects")
        } else if method == "execution.steer" {
            Some("execution.steering")
        } else if method == "execution.respond_action" {
            Some("execution.actions")
        } else if method == "execution.controller.claim" {
            Some("execution.controller")
        } else if method == "execution.discovery.list" {
            Some("execution.discovery")
        } else if method == "execution.output.read" {
            Some("execution.output")
        } else if method == "execution.workspace.checkpoint" {
            Some("execution.workspaces")
        } else {
            None
        };
        if let Some(f) = feature
            && !session.feature(f)
            && !missing.contains(&json!(f))
        {
            missing.push(json!(f));
        }
        if !missing.is_empty() {
            return Err(err(
                "unsupported_required_feature",
                json!({"features":missing}),
            ));
        }
        match method {
            "core.describe" => return Ok(self.manifest()),
            "core.feature_dependencies" => {
                return Ok(
                    json!({"dependencies":[{"profile":"core-test","major":1,"trigger":{"kind":"profile"},"requires":[{"profile":"core","major":1,"features":[]}]},{"profile":"execution","major":1,"trigger":{"kind":"profile"},"requires":[{"profile":"core","major":1,"features":["core.events","core.capabilities","core.effects"]}]}]}),
                );
            }
            "core.authenticate" => {
                if session.principal.is_some() {
                    return Err(err("already_authenticated", json!({})));
                }
                let digest = pio_core::digest(text(&p["payload"]["credential"]).as_bytes());
                let mut matched = None;
                for (stored, principal, revoked) in &self.credentials {
                    let different = stored
                        .bytes()
                        .zip(digest.bytes())
                        .fold(0u8, |n, (a, b)| n | (a ^ b));
                    if different == 0 && !*revoked {
                        matched = Some(principal.clone());
                    }
                }
                if let Some(principal) = matched {
                    session.principal = Some(principal.clone());
                    return Ok(json!({"principal":principal}));
                }
                return Err(err("authentication_failed", json!({})));
            }
            "core.negotiate" => return self.negotiate(session, &p["payload"]),
            _ => {}
        }
        let command = p.get("command_id").is_some();
        let scope = key(&json!([session.principal, p["command_id"]]));
        if command {
            let algorithm = text(&p["command_digest"]).split(':').next().unwrap_or("");
            if algorithm != "sha256" {
                return Err(err(
                    "unsupported_digest_algorithm",
                    json!({"algorithm":algorithm,"supported":["sha256"]}),
                ));
            }
            let mut extensions = json!({});
            for r in &requires {
                if text(r).contains('/') {
                    extensions[text(r)] = p["extensions"][text(r)].clone()
                }
            }
            let intent = json!({"operation":p["operation"],"subject":p["subject"],"preconditions":p["preconditions"],"requires":p["requires"],"payload":p["payload"],"extensions":extensions});
            let expected = pio_core::digest(&encoding::canonical(&intent));
            if p["command_digest"] != expected {
                return Err(err("digest_mismatch", json!({"expected":expected})));
            }
            if num(&p["dedupe_generation"]) > self.data.generation {
                return Err(invalid("/dedupe_generation"));
            }
            if let Some(bound) = self.data.commands.get(&scope) {
                if bound.digest != expected {
                    return Err(err(
                        "idempotency_conflict",
                        json!({"command_id":p["command_id"]}),
                    ));
                }
                let mut result = bound.result.clone();
                result["replay"] = true.into();
                return Ok(result);
            }
            if num(&p["dedupe_generation"]) < self.data.oldest {
                return Err(err(
                    "dedupe_history_unavailable",
                    json!({"oldest_retained":self.data.oldest}),
                ));
            }
        }
        self.authorize(session, method, p)?;
        self.execution_epoch(session, method, p)?;
        if !command {
            if method.starts_with("core.events.") {
                return self.event_query(session, method, p);
            }
            return self.query(session, method, p);
        }
        if method == "core-test.subject.put" {
            let status = self
                .data
                .predicates
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == "core-test.writes")
                .map(|p| text(&p["status"]))
                .unwrap_or("unknown");
            if status != "supported" {
                return Err(err(
                    "capability_unavailable",
                    json!({"capability":"core-test.writes","status":status}),
                ));
            }
            let epoch = self
                .data
                .subjects
                .get(&key(
                    &json!({"kind":"core-test.authority","id":"core-test"}),
                ))
                .map(|s| s.revision)
                .unwrap_or(0);
            if num(&p["authority_epoch"]) < epoch {
                return Err(err(
                    "stale_authority_epoch",
                    if self.visible(
                        session,
                        p,
                        &json!({"kind":"core-test.authority","id":"core-test"}),
                    ) {
                        json!({"current_epoch":epoch})
                    } else {
                        json!({})
                    },
                ));
            }
            if num(&p["authority_epoch"]) > epoch {
                return Err(err("unknown_authority_epoch", json!({})));
            }
        }
        let failed: Vec<Value> = pre
            .iter()
            .filter_map(|c| {
                let current = self.revision(&c["subject"]);
                (num(&c["revision"]) != current).then(|| {
                    let mut f = json!({"subject":c["subject"],"expected":c["revision"]});
                    if self.visible(session, p, &c["subject"]) {
                        f["current"] = current.into()
                    }
                    f
                })
            })
            .collect();
        if !failed.is_empty() {
            return Err(err("precondition_failed", json!({"failed":failed})));
        }
        let before: BTreeMap<String, u64> = self
            .data
            .subjects
            .iter()
            .map(|(k, s)| (k.clone(), s.revision))
            .collect();
        let result = self.apply(session, method, p);
        match result {
            Ok(result) => {
                self.record_changes(&before, &result, p);
                self.data.commands.insert(
                    scope,
                    Bound {
                        digest: text(&p["command_digest"]).into(),
                        generation: num(&p["dedupe_generation"]),
                        result: result.clone(),
                    },
                );
                // A launch-configured commit fault short-circuits before any commit.
                let refused = self.fault("commit_unavailable", method)
                    || if method == "execution.submit" {
                        self.save_admission()
                    } else {
                        self.save()
                    }
                    .is_err();
                if refused {
                    self.reload()
                        .map_err(|_| err("internal_error", json!({})))?;
                    return Err(err("unavailable", json!({})));
                }
                if self.fault("response_internal_error", method) {
                    return Err(err("internal_error", json!({})));
                }
                Ok(result)
            }
            Err(e) => {
                self.reload()
                    .map_err(|_| err("internal_error", json!({})))?;
                Err(e)
            }
        }
    }
    fn fault(&mut self, kind: &str, method: &str) -> bool {
        if let Some(faults) = self.config["faults"][kind].as_array_mut() {
            for f in faults {
                if f["operation"] == method && num(&f["times"]) > 0 {
                    f["times"] = (num(&f["times"]) - 1).into();
                    return true;
                }
            }
        }
        false
    }
    fn validate_limits(&self, p: &Value) -> std::result::Result<(), Error> {
        fn measure(v: &Value, depth: u64) -> (u64, u64, u64) {
            let mut m = (depth, 0, 0);
            match v {
                Value::String(s) => m.2 = s.len() as u64,
                Value::Array(a) => {
                    m.1 = a.len() as u64;
                    for x in a {
                        let t = measure(x, depth + u64::from(x.is_object() || x.is_array()));
                        m = (m.0.max(t.0), m.1.max(t.1), m.2.max(t.2));
                    }
                }
                Value::Object(o) => {
                    for (k, x) in o {
                        m.2 = m.2.max(k.len() as u64);
                        let t = measure(x, depth + u64::from(x.is_object() || x.is_array()));
                        m = (m.0.max(t.0), m.1.max(t.1), m.2.max(t.2));
                    }
                }
                _ => {}
            }
            m
        }
        let (d, a, s) = measure(p, 1);
        for (name, actual) in [
            ("max_depth", d),
            ("max_array_items", a),
            ("max_string_bytes", s),
            (
                "max_payload_bytes",
                encoding::canonical(&p["payload"]).len() as u64,
            ),
        ] {
            if actual > num(&self.limits[name]) {
                return Err(err(
                    "limit_exceeded",
                    json!({"limit":name,"maximum":self.limits[name]}),
                ));
            }
        }
        Ok(())
    }
    fn negotiate(&self, session: &mut Session, p: &Value) -> Reply {
        if session.selected.is_some() {
            return Err(err("already_negotiated", json!({})));
        }
        let mut profiles = list(&p["profiles"]);
        let mut seen = vec![];
        for profile in &profiles {
            if seen.contains(&profile["name"]) {
                return Err(invalid("/payload/profiles"));
            }
            seen.push(profile["name"].clone());
        }
        if !seen.contains(&json!("core")) {
            profiles.insert(0,json!({"name":"core","majors":[1],"required":true,"required_features":[],"optional_features":[]}));
        }
        let mut selected = BTreeMap::new();
        let mut unselected = vec![];
        let mut unsatisfied = vec![];
        let mut codes = vec![];
        for p in profiles {
            let name = text(&p["name"]);
            let required = p["required"] == true || name == "core";
            let mut items = vec![];
            let mut code = "unsupported_required_feature";
            if !["core", "core-test", "execution"].contains(&name) {
                code = "unsupported_profile";
                items.push(json!({"profile":name,"reason":if ["coordination","remote-trust"].contains(&name){"declared_unsupported"}else{"unknown_profile"}}));
            } else if !list(&p["majors"]).contains(&json!(1)) {
                code = "unsupported_version";
                items.push(json!({"profile":name,"reason":"no_common_major"}));
            } else {
                for f in list(&p["required_features"]) {
                    if !(name == "core" && FEATURES.contains(&text(&f))
                        || name == "execution" && self.execution_features().contains(&text(&f)))
                    {
                        items.push(json!({"profile":name,"feature":f,"reason":"unknown_feature"}));
                    }
                }
                if items.is_empty() {
                    let mut features = vec![];
                    for f in list(&p["required_features"])
                        .into_iter()
                        .chain(list(&p["optional_features"]))
                    {
                        if name == "core" && FEATURES.contains(&text(&f))
                            || name == "execution" && self.execution_features().contains(&text(&f))
                        {
                            if !features.contains(&text(&f).to_owned()) {
                                features.push(text(&f).to_owned());
                            }
                        } else {
                            unselected.push(
                                json!({"profile":name,"feature":f,"reason":"unknown_feature"}),
                            );
                        }
                    }
                    selected.insert(name.to_owned(), features);
                }
            }
            if required && !items.is_empty() {
                unsatisfied.extend(items);
                codes.push(code)
            } else {
                unselected.extend(items)
            }
        }
        if !unsatisfied.is_empty() {
            let code = if codes.contains(&"unsupported_profile") {
                "unsupported_profile"
            } else if codes.contains(&"unsupported_version") {
                "unsupported_version"
            } else {
                "unsupported_required_feature"
            };
            return Err(err(code, json!({"unsatisfied":unsatisfied})));
        }
        if selected.contains_key("execution") {
            let missing:Vec<Value>=["core.events","core.capabilities","core.effects"].iter().filter(|f|!selected.get("core").is_some_and(|v|v.iter().any(|s|s==**f))).map(|f|json!({"profile":"execution","feature":f,"reason":"dependency_not_selected"})).collect();
            if !missing.is_empty() {
                if list(&p["profiles"])
                    .iter()
                    .any(|p| p["name"] == "execution" && p["required"] == true)
                {
                    return Err(err("unsupported_profile", json!({"unsatisfied":missing})));
                }
                selected.remove("execution");
                unselected.extend(missing);
            }
        }
        session.receive = num(&p["receive_limits"]["max_frame_bytes"]) as usize;
        session.selected = Some(selected.clone());
        let mut limits = self.limits.clone();
        if session.feature("core.events.backpressure") {
            limits["max_pending_notification_bytes"] =
                self.config["events"]["max_pending_notification_bytes"]
                    .as_u64()
                    .unwrap_or(2097152)
                    .into();
            limits["backpressure_notice_ms"] = self.config["events"]["backpressure_notice_ms"]
                .as_u64()
                .unwrap_or(1000)
                .into();
        }
        Ok(
            json!({"selected":selected.into_iter().map(|(name,features)|json!({"name":name,"major":1,"features":features})).collect::<Vec<_>>(),"unselected":unselected,"limits":limits,"dedupe_window":self.window()}),
        )
    }
    fn query(&self, session: &Session, method: &str, p: &Value) -> Reply {
        if method.starts_with("execution.") {
            return self.execution_query(session, method, p);
        }
        if method == "core.effects.get" {
            return self.effect_get(text(&p["payload"]["effect"]));
        }
        if method == "core.capabilities" {
            return Ok(
                json!({"revision":self.data.cap_revision,"predicates":self.data.predicates}),
            );
        }
        if method == "core.grant.get" {
            return self.grant_get(session, p);
        }
        let subject = &p["payload"]["subject"];
        let record = self.data.subjects.get(&key(subject));
        match method {
            "core-test.subject.get" => record
                .map(
                    |r| json!({"subject":r.subject,"revision":r.revision,"value":r.state["value"]}),
                )
                .ok_or(err("not_found", json!({}))),
            "core-test.subject.applied_count" => {
                Ok(json!({"subject":subject,"applied_count":record.map(|r|r.applied).unwrap_or(0)}))
            }
            _ => Err(err("method_not_found", json!({"operation":method}))),
        }
    }
    fn apply(&mut self, session: &Session, method: &str, p: &Value) -> Reply {
        if method.starts_with("execution.") {
            return self.execution_apply(session, method, p);
        }
        if method == "core.effects.abort_obligation" {
            return self.effect_abort(p);
        }
        if method.starts_with("core.grant.") {
            return self.grant_apply(session, method, p);
        }
        let subject = p["subject"].clone();
        let k = key(&subject);
        let revision = self.data.subjects.get(&k).map(|r| r.revision).unwrap_or(0) + 1;
        let state = match method {
            "core-test.authority.claim" => json!({"epoch":revision}),
            "core-test.subject.put" => json!({"value":p["payload"]["value"]}),
            _ => return Err(err("method_not_found", json!({"operation":method}))),
        };
        let count = self.data.subjects.get(&k).map(|r| r.applied).unwrap_or(0) + 1;
        self.data.subjects.insert(
            k,
            Subject {
                subject: subject.clone(),
                revision,
                state: state.clone(),
                applied: count,
            },
        );
        Ok(
            json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":Uuid::new_v4().to_string(),"subject":subject,"revision":revision,"effect_refs":[]},"outcome":state,"replay":false}),
        )
    }
}
