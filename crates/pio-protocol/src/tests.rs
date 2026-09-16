use crate::{encoding, provider::*};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn config() -> Value {
    json!({"format":"combraton-conformance-config/1","principal":"owner","clock":{"fixed":"2026-01-01T00:00:00Z"}})
}
fn session() -> Session {
    Session {
        principal: Some("owner".into()),
        selected: Some(BTreeMap::from([
            (
                "core".into(),
                FEATURES.iter().map(|s| s.to_string()).collect(),
            ),
            ("core-test".into(), vec![]),
        ])),
        receive: 1048576,
        ..Session::default()
    }
}
fn command(method: &str, subject: Value, revision: u64, payload: Value) -> Value {
    let preconditions = json!([{"subject":subject,"revision":revision}]);
    let intent = json!({"operation":method,"subject":subject,"preconditions":preconditions,"requires":[],"payload":payload,"extensions":{}});
    let mut p = intent.clone();
    p.as_object_mut().unwrap().remove("extensions");
    p["message_id"] = "message".into();
    p["command_id"] = "command".into();
    p["dedupe_generation"] = 1.into();
    p["command_digest"] = pio_core::digest(&encoding::canonical(&intent)).into();
    p
}

#[test]
fn strict_encoding_and_utf16_canonical_order() {
    for bytes in [
        r#"{"a":1,"\u0061":2}"#,
        r#"1e0"#,
        r#"1.0"#,
        r#"-0"#,
        r#"9007199254740992"#,
        r#""\ud800""#,
        r#""\uffff""#,
    ] {
        assert!(encoding::parse(bytes.as_bytes()).is_err(), "{bytes}");
    }
    let v = encoding::parse("{\"\u{e000}\":1,\"😀\":2,\"a\":\"\\n\"}".as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(encoding::canonical(&v)).unwrap(),
        "{\"a\":\"\\n\",\"😀\":2,\"\u{e000}\":1}"
    );
}

#[test]
fn failed_commit_rolls_back_subject_event_and_dedupe_together() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg["faults"] = json!({"commit_unavailable":[{"operation":"core-test.subject.put","times":1}]});
    let mut store = Provider::new(root.path(), cfg).unwrap();
    let mut s = session();
    let subject = json!({"kind":"core-test.subject","id":"x"});
    let mut p = command(
        "core-test.subject.put",
        subject.clone(),
        0,
        json!({"value":"once"}),
    );
    p["authority_epoch"] = 0.into();
    assert_eq!(
        store
            .handle(&mut s, "core-test.subject.put", &p)
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(store.revision(&subject), 0);
    assert!(store.data.events.is_empty());
    assert!(store.data.commands.is_empty());
    drop(store);
    let mut store = Provider::new(root.path(), config()).unwrap();
    let applied = store.handle(&mut s, "core-test.subject.put", &p).unwrap();
    assert_eq!(applied["replay"], false);
    assert_eq!(
        store.handle(&mut s, "core-test.subject.put", &p).unwrap()["replay"],
        true
    );
    assert_eq!(store.data.events.len(), 1);
    assert_eq!(store.data.subjects[&key(&subject)].applied, 1);
}

#[test]
fn response_loss_keeps_committed_state_and_exact_ack() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg["faults"] =
        json!({"response_internal_error":[{"operation":"core-test.subject.put","times":1}]});
    let mut store = Provider::new(root.path(), cfg).unwrap();
    let mut s = session();
    let mut p = command(
        "core-test.subject.put",
        json!({"kind":"core-test.subject","id":"x"}),
        0,
        json!({"value":"once"}),
    );
    p["authority_epoch"] = 0.into();
    assert_eq!(
        store
            .handle(&mut s, "core-test.subject.put", &p)
            .unwrap_err()
            .code,
        "internal_error"
    );
    let ack = store.data.commands.values().next().unwrap().result["acknowledgment"].clone();
    drop(store);
    let mut store = Provider::new(root.path(), config()).unwrap();
    let replay = store.handle(&mut s, "core-test.subject.put", &p).unwrap();
    assert_eq!(replay["acknowledgment"], ack);
    assert_eq!(replay["replay"], true);
    assert_eq!(store.data.events.len(), 1);
}

#[test]
fn effects_abort_closes_wait_without_claiming_an_outcome() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Provider::new(root.path(), config()).unwrap();
    let mut s = session();

    // Internal store test only. Launch configuration and public core-test cannot seed effects.
    store.data.effects.insert("effect".into(),json!({"effect":{"id":"effect","kind":"execution.prompt_submission","target":{"kind":"core-test.subject","id":"x"},"payload_digest":pio_core::digest(b"payload"),"authorization":{"principal":"owner"},"retry_class":"non_repeatable","operation_ref":"source"},"revision":1,"status":"unknown","observations":[{"status":"unknown","evidence":{"class":"lost_response","source":"test"},"recorded_at":"2026-01-01T00:00:00Z"}],"attempts":[{"attempt":1,"outcome":"unknown","recorded_at":"2026-01-01T00:00:00Z"}],"obligations":[{"id":"wait","expects":"outcome","deadline":"2026-01-01T00:00:01Z","state":"open"}]}));
    store.save().unwrap();
    store.now = "2026-01-01T00:00:02Z".into();
    store.expire_obligations().unwrap();
    assert_eq!(
        store.effect_get("effect").unwrap()["obligations"][0]["state"],
        "overdue"
    );
    assert_eq!(store.data.events.len(), 1);
    store.expire_obligations().unwrap();
    assert_eq!(store.data.events.len(), 1);
    s.selected
        .as_mut()
        .unwrap()
        .get_mut("core")
        .unwrap()
        .push("core.effects".into());
    let p = command(
        "core.effects.abort_obligation",
        json!({"kind":"core.effect","id":"effect"}),
        2,
        json!({"obligation":"wait"}),
    );
    let result = store
        .handle(&mut s, "core.effects.abort_obligation", &p)
        .unwrap();
    assert_eq!(result["outcome"]["status"], "unknown");
    assert_eq!(
        store.effect_get("effect").unwrap()["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(store);
    let mut store = Provider::new(root.path(), config()).unwrap();
    assert_eq!(
        store
            .handle(&mut s, "core.effects.abort_obligation", &p)
            .unwrap()["replay"],
        true
    );
    assert_eq!(
        store.effect_get("effect").unwrap()["obligations"][0]["state"],
        "aborted"
    );
    assert_eq!(store.data.events.len(), 2);
    assert_eq!(store.effect_get("absent").unwrap_err().code, "not_found");
    store.data.effects.insert("forgotten".into(), Value::Null);
    assert_eq!(
        store.effect_get("forgotten").unwrap_err().code,
        "effect_history_unavailable"
    );
}

#[test]
fn negotiation_treats_features_as_sets_and_validates_before_session_refusal() {
    let root = tempfile::tempdir().unwrap();
    let mut store = Provider::new(root.path(), config()).unwrap();
    let mut s = Session {
        principal: Some("owner".into()),
        receive: 1048576,
        ..Session::default()
    };
    let profile = json!({"name":"core","majors":[1],"required":true,"required_features":["core.grants"],"optional_features":["core.grants"]});
    let mut p = json!({"operation":"core.negotiate","message_id":"m","payload":{"caller":{"name":"test","version":"1"},"receive_limits":{"max_frame_bytes":1048576},"profiles":[profile]}});
    let result = store.handle(&mut s, "core.negotiate", &p).unwrap();
    assert_eq!(result["selected"][0]["features"], json!(["core.grants"]));
    p["payload"]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(profile);
    p["requires"] = json!(["unknown.feature"]);
    assert_eq!(
        store.handle(&mut s, "core.negotiate", &p).unwrap_err().code,
        "invalid_envelope"
    );
}

#[test]
fn execution_admission_is_atomic_and_replay_never_dispatches_twice() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg["executor"] = json!({"default_script":[{"deliver":"provider_ack_id"}]});
    cfg["faults"] = json!({"commit_unavailable":[{"operation":"execution.submit","times":1}]});
    let mut p = Provider::new(root.path(), cfg.clone()).unwrap();
    let mut s = session();
    s.selected
        .as_mut()
        .unwrap()
        .insert("execution".into(), vec![]);
    let command = command(
        "execution.submit",
        json!({"kind":"execution.execution","id":"e"}),
        0,
        json!({"brief":{"digest":pio_core::digest(b"brief"),"media_type":"text/plain"}}),
    );
    assert_eq!(
        p.handle(&mut s, "execution.submit", &command)
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert!(p.data.effects.is_empty());
    assert!(p.data.executions.is_empty());
    let ack = p.handle(&mut s, "execution.submit", &command).unwrap();
    let facts = p.store.journal().unwrap();
    let admission = facts
        .iter()
        .find(|f| {
            f["changes"]
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c["key"] == "execution/e"))
        })
        .unwrap();
    let changes = admission["changes"].as_array().unwrap();
    for prefix in ["execution/", "effect/", "command/", "event/"] {
        assert!(
            changes.iter().any(|c| text(&c["key"]).starts_with(prefix)),
            "{prefix} must commit with admission"
        );
    }
    assert_eq!(p.data.effects["e.delivery-1"]["status"], "pending");
    p.execution_tick().unwrap();
    assert_eq!(
        p.data.effects["e.delivery-1"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(p);
    cfg.as_object_mut().unwrap().remove("faults");
    let mut p = Provider::new(root.path(), cfg).unwrap();
    let replay = p.handle(&mut s, "execution.submit", &command).unwrap();
    assert_eq!(replay["acknowledgment"], ack["acknowledgment"]);
    assert_eq!(replay["replay"], true);
    assert_eq!(
        p.data.effects["e.delivery-1"]["attempts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let before = p.data.records().unwrap();
    p.store.rebuild_protocol().unwrap();
    p.reload().unwrap();
    assert_eq!(p.data.records().unwrap(), before);
}

#[test]
fn legacy_blob_store_is_refused_without_modifying_it() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("protocol.sqlite3");
    std::fs::write(&file, b"legacy").unwrap();
    let result = Provider::new(root.path(), config());
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("explicit migration")
    );
    assert_eq!(std::fs::read(file).unwrap(), b"legacy");
    assert!(!root.path().join("journal.sqlite3").exists());
}

#[test]
fn delivery_timeout_does_not_postpone_inactivity() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg["executor"] = json!({"default_script":[{"deliver":"bytes_written"}]});
    let mut p = Provider::new(root.path(), cfg).unwrap();
    let mut s = session();
    s.selected
        .as_mut()
        .unwrap()
        .insert("execution".into(), vec![]);
    let submit = command(
        "execution.submit",
        json!({"kind":"execution.execution","id":"e"}),
        0,
        json!({"brief":{"digest":pio_core::digest(b"brief"),"media_type":"text/plain"},"timeouts":{"delivery":30,"inactivity":60}}),
    );
    p.handle(&mut s, "execution.submit", &submit).unwrap();
    p.execution_tick().unwrap();
    p.now = "2026-01-01T00:00:30Z".into();
    p.execution_tick().unwrap();
    assert_eq!(p.data.executions["e"]["view"]["delivery"], "ambiguous");
    p.now = "2026-01-01T00:01:00Z".into();
    p.execution_tick().unwrap();
    assert!(list(&p.data.executions["e"]["timeouts_passed"]).contains(&json!("inactivity")));
    assert_eq!(p.data.executions["e"]["view"]["runtime"], "preparing");
}

#[test]
fn forwarding_intent_pins_its_own_payload_and_key_before_attempt() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config();
    cfg["executor"] = json!({"default_script":[{"stale_dispatch":{"generation":1}},{"deliver":"provider_ack_id"}]});
    let mut p = Provider::new(root.path(), cfg).unwrap();
    let mut s = session();
    s.selected
        .as_mut()
        .unwrap()
        .insert("execution".into(), vec![]);
    let subject = json!({"kind":"execution.execution","id":"e"});
    let submit = command(
        "execution.submit",
        subject.clone(),
        0,
        json!({"brief":{"digest":pio_core::digest(b"brief"),"media_type":"text/plain"}}),
    );
    p.handle(&mut s, "execution.submit", &submit).unwrap();
    p.execution_tick().unwrap();
    assert_eq!(list(&p.data.effects["e.delivery-1"]["attempts"]).len(), 1);
    let mut cancel = command(
        "execution.cancel",
        subject.clone(),
        p.revision(&subject),
        json!({}),
    );
    cancel["command_id"] = "cancel".into();
    p.handle(&mut s, "execution.cancel", &cancel).unwrap();
    let effect = &p.data.effects["e.cancel-1"];
    assert!(list(&effect["attempts"]).is_empty());
    assert_eq!(effect["effect"]["idempotency_key"], "e.cancel-1");
    assert_eq!(effect["effect"]["payload_digest"], pio_core::digest(b"{}"));
}

#[test]
fn reconciliation_events_use_frozen_outcomes_and_survive_journal_rebuild() {
    for (determination, outcome) in [
        ("acknowledged", "delivered"),
        ("failed_before_delivery", "not_delivered"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg["executor"] = json!({"default_script":[{"stall":true}]});
        let mut p = Provider::new(root.path(), cfg).unwrap();
        let mut s = session();
        s.selected
            .as_mut()
            .unwrap()
            .insert("execution".into(), vec![]);
        let submit = command(
            "execution.submit",
            json!({"kind":"execution.execution","id":"e"}),
            0,
            json!({"brief":{"digest":pio_core::digest(b"brief"),"media_type":"text/plain"}}),
        );
        p.handle(&mut s, "execution.submit", &submit).unwrap();
        p.execution_tick().unwrap();
        let mut e = p.data.executions["e"].clone();
        p.delivery_observed(&mut e, "ambiguous", "recovery", None, false, false);
        p.delivery_observed(&mut e, determination, "later_evidence", None, true, true);
        p.commit_execution(&e).unwrap();
        p.store.rebuild_protocol().unwrap();
        p.reload().unwrap();
        let event = p
            .data
            .events
            .iter()
            .rev()
            .find(|e| e["type"] == "execution.delivery.reconciled")
            .unwrap();
        assert_eq!(event["payload"]["outcome"], outcome);
        assert_eq!(event["payload"]["delivery"], determination);
        assert!(
            list(&p.data.executions["e"]["view"]["deliveries"][0]["history"])
                .iter()
                .any(|h| h["delivery"] == "ambiguous")
        );
    }
}
