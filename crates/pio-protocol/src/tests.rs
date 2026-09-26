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

use pio_core::projections::{
    MAX_ADMISSION_PROJECTION_RECORDS, MAX_PROJECTION_RECORDS, projection_size,
};

/// Internal store test only: pads the retained projection with inert subjects
/// so a real command lands on an exact record-count boundary.
fn fill_records(p: &mut Provider, target: u64) {
    let base = p.data.records().unwrap().len() as u64;
    assert!(base <= target);
    let existing = p
        .data
        .subjects
        .values()
        .filter(|s| s.subject["kind"] == "pio-test.filler")
        .count() as u64;
    for n in existing..existing + (target - base) {
        let subject = json!({"kind":"pio-test.filler","id":n.to_string()});
        p.data.subjects.insert(
            key(&subject),
            Subject {
                subject,
                revision: 1,
                state: json!({}),
                applied: 0,
            },
        );
    }
    p.save().unwrap();
    assert_eq!(p.data.records().unwrap().len() as u64, target);
}

fn put(id: &str) -> Value {
    let mut p = command(
        "core-test.subject.put",
        json!({"kind":"core-test.subject","id":id}),
        0,
        json!({"value":"v"}),
    );
    p["authority_epoch"] = 0.into();
    p["command_id"] = id.into();
    p
}

#[test]
fn capacity_counts_subject_event_and_dedupe_records_and_binds_nothing_on_refusal() {
    // A put adds exactly three records: subject, event and dedupe outcome.
    // Refusal one record over the limit means all three are counted.
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), config()).unwrap();
    let mut s = session();
    fill_records(&mut p, MAX_PROJECTION_RECORDS - 2);
    let journal = p.store.journal().unwrap();
    let error = p
        .handle(&mut s, "core-test.subject.put", &put("x"))
        .unwrap_err();
    assert_eq!(error.code, "unavailable");
    assert_eq!(
        error.frame(json!(1))["error"]["data"]["retry"],
        "same_command"
    );
    assert_eq!(
        p.capacity_refusal,
        Some(
            json!({"limit":"projection_records","maximum":MAX_PROJECTION_RECORDS,"projected":MAX_PROJECTION_RECORDS+1})
        )
    );
    let subject = json!({"kind":"core-test.subject","id":"x"});
    assert_eq!(p.revision(&subject), 0);
    assert!(p.data.events.is_empty() && p.data.commands.is_empty());
    assert_eq!(
        p.store.journal().unwrap(),
        journal,
        "refusal records no fact"
    );
    // Retransmitting the unbound command is safe and refused the same way.
    assert_eq!(
        p.handle(&mut s, "core-test.subject.put", &put("x"))
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(p.store.journal().unwrap(), journal);

    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), config()).unwrap();
    fill_records(&mut p, MAX_PROJECTION_RECORDS - 3);
    let ack = p
        .handle(&mut s, "core-test.subject.put", &put("x"))
        .unwrap();
    assert_eq!(ack["replay"], false);
    assert_eq!(
        p.data.records().unwrap().len() as u64,
        MAX_PROJECTION_RECORDS
    );
    assert_eq!(p.capacity_refusal, None);
    // A bound replay needs no commit and still answers at the limit.
    assert_eq!(
        p.handle(&mut s, "core-test.subject.put", &put("x"))
            .unwrap()["replay"],
        true
    );
}

#[test]
fn configured_event_retention_changes_capacity_accounting() {
    for (retain_last, second_accepted) in [(Some(1), true), (None, false)] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = config();
        if let Some(n) = retain_last {
            cfg["events"] = json!({"retain_last":n});
        }
        let mut p = Provider::new(root.path(), cfg).unwrap();
        let mut s = session();
        fill_records(&mut p, MAX_PROJECTION_RECORDS - 5);
        p.handle(&mut s, "core-test.subject.put", &put("a"))
            .unwrap();
        assert_eq!(
            p.data.records().unwrap().len() as u64,
            MAX_PROJECTION_RECORDS - 2
        );
        let second = p.handle(&mut s, "core-test.subject.put", &put("b"));
        // With one retained event the new event replaces the old one, so the
        // second put lands exactly on the limit; unretained it is one over.
        assert_eq!(
            second.is_ok(),
            second_accepted,
            "retain_last={retain_last:?}"
        );
        if second_accepted {
            assert_eq!(
                p.data.records().unwrap().len() as u64,
                MAX_PROJECTION_RECORDS
            );
            assert_eq!(p.data.events.len(), 1);
        } else {
            assert_eq!(second.unwrap_err().code, "unavailable");
            assert_eq!(
                p.capacity_refusal.as_ref().unwrap()["projected"],
                MAX_PROJECTION_RECORDS + 1
            );
            assert_eq!(p.data.events.len(), 1);
        }
    }
}

fn scripted_submit() -> (Value, Session, Value) {
    let mut cfg = config();
    cfg["executor"] = json!({"default_script":[{"deliver":"provider_ack_id"}]});
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
    (cfg, s, submit)
}

#[test]
fn hard_limit_refuses_nothing_bound_and_stalls_admitted_work_after_the_marker() {
    let (cfg, mut s, submit) = scripted_submit();
    // Measure what one admission adds: execution, delivery effect, dedupe
    // outcome and events.
    let scratch = tempfile::tempdir().unwrap();
    let mut p = Provider::new(scratch.path(), cfg.clone()).unwrap();
    let base = p.data.records().unwrap().len() as u64;
    p.handle(&mut s, "execution.submit", &submit).unwrap();
    let added = p.data.records().unwrap().len() as u64 - base;
    assert!(
        added >= 3,
        "admission adds execution, effect and dedupe records"
    );

    // Near the hard limit a new submit is refused, first by the admission
    // threshold, and creates nothing.
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), cfg.clone()).unwrap();
    fill_records(&mut p, MAX_PROJECTION_RECORDS - added + 1);
    let journal = p.store.journal().unwrap();
    assert_eq!(
        p.handle(&mut s, "execution.submit", &submit)
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(
        p.capacity_refusal.as_ref().unwrap()["limit"],
        "admission_projection_records"
    );
    assert!(p.data.executions.is_empty() && p.data.effects.is_empty());
    assert!(p.data.commands.is_empty() && p.data.events.is_empty());
    p.execution_tick().unwrap();
    assert_eq!(
        p.store.journal().unwrap(),
        journal,
        "no admission, effect or dispatch fact"
    );

    // Already-admitted work whose facts exhaust the hard limit still stalls:
    // the dispatch marker only rewrites existing records and commits; the
    // delivery observation adds an event and is refused. Delivery stays
    // pending and no further fact is recorded until capacity is freed.
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), cfg).unwrap();
    p.handle(&mut s, "execution.submit", &submit).unwrap();
    fill_records(&mut p, MAX_PROJECTION_RECORDS);
    let journal = p.store.journal().unwrap();
    let events = p.data.events.len();
    assert!(p.execution_tick().is_err());
    assert_eq!(
        p.capacity_refusal.as_ref().unwrap()["limit"],
        "projection_records"
    );
    let facts = p.store.journal().unwrap();
    assert_eq!(
        facts.len(),
        journal.len() + 1,
        "only the dispatch marker commits"
    );
    let keys: Vec<_> = list(&facts[journal.len()]["changes"])
        .iter()
        .map(|c| text(&c["key"]).to_owned())
        .collect();
    assert_eq!(keys, ["effect/e.delivery-1", "execution/e"]);
    let effect = &p.data.effects["e.delivery-1"];
    assert_eq!(
        list(&effect["observations"]).last().unwrap()["evidence"]["class"],
        "dispatch_intent"
    );
    assert_eq!(effect["status"], "pending");
    assert_eq!(p.data.executions["e"]["dispatched"], true);
    assert_eq!(p.data.executions["e"]["view"]["delivery"], "pending");
    assert_eq!(p.data.events.len(), events);
    let _ = p.execution_tick();
    assert_eq!(p.store.journal().unwrap(), facts);
    assert_eq!(p.data.executions["e"]["view"]["delivery"], "pending");
    // A bound replay needs no commit.
    assert_eq!(
        p.handle(&mut s, "execution.submit", &submit).unwrap()["replay"],
        true
    );
    assert_eq!(p.store.journal().unwrap(), facts);
}

#[test]
fn projected_size_equals_canonical_encoding_length() {
    let (cfg, mut s, submit) = scripted_submit();
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), cfg).unwrap();
    p.handle(&mut s, "execution.submit", &submit).unwrap();
    p.execution_tick().unwrap();
    let mut records = p.data.records().unwrap();
    records.insert(
        "subject/escapes".into(),
        json!({"\u{e000}":"\n\"\\","😀":[1,-2,"é"],"a":{"z":null,"b":true}}),
    );
    let canonical: u64 = records
        .iter()
        .map(|(k, v)| (k.len() + encoding::canonical(v).len()) as u64)
        .sum();
    assert_eq!(
        projection_size(&records).unwrap(),
        (canonical, records.len() as u64)
    );
}

#[test]
fn capacity_refused_background_commit_keeps_queries_and_replays_available() {
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), config()).unwrap();
    let mut s = session();
    s.selected
        .as_mut()
        .unwrap()
        .get_mut("core")
        .unwrap()
        .push("core.effects".into());
    let replayed = put("bound");
    p.handle(&mut s, "core-test.subject.put", &replayed)
        .unwrap();
    // Internal store test only: an open obligation whose expiry adds an event.
    p.data.effects.insert("effect".into(),json!({"effect":{"id":"effect","kind":"execution.prompt_submission","target":{"kind":"core-test.subject","id":"x"},"payload_digest":pio_core::digest(b"payload"),"authorization":{"principal":"owner"},"retry_class":"non_repeatable","operation_ref":"source"},"revision":1,"status":"unknown","observations":[],"attempts":[],"obligations":[{"id":"wait","expects":"outcome","deadline":"2026-01-01T00:00:01Z","state":"open"}]}));
    fill_records(&mut p, MAX_PROJECTION_RECORDS);
    let journal = p.store.journal().unwrap();
    p.now = "2026-01-01T00:00:02Z".into();
    let query = json!({"operation":"core.capabilities","message_id":"q","payload":{}});
    let answer = p.handle(&mut s, "core.capabilities", &query);
    assert!(
        answer.is_ok(),
        "query refused at capacity: {:?}",
        answer.err().map(|e| e.code)
    );
    assert_eq!(
        p.capacity_refusal.as_ref().unwrap()["limit"],
        "projection_records"
    );
    assert_eq!(
        p.effect_get("effect").unwrap()["obligations"][0]["state"],
        "open"
    );
    assert_eq!(
        p.handle(&mut s, "core-test.subject.put", &replayed)
            .unwrap()["replay"],
        true
    );
    assert_eq!(
        p.handle(&mut s, "core-test.subject.put", &put("new"))
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(p.store.journal().unwrap(), journal);
    // A non-capacity store failure keeps the conservative behavior.
    for n in 0..3 {
        let subject = json!({"kind":"pio-test.filler","id":n.to_string()});
        p.data.subjects.remove(&key(&subject));
    }
    p.save().unwrap();
    let conn = rusqlite::Connection::open(root.path().join("journal.sqlite3")).unwrap();
    conn.execute_batch("CREATE TRIGGER refuse_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT,'disk fault'); END;").unwrap();
    p.capacity_refusal = None;
    assert_eq!(
        p.handle(&mut s, "core.capabilities", &query)
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(p.capacity_refusal, None);
}

fn submit_for(id: &str) -> Value {
    let mut p = command(
        "execution.submit",
        json!({"kind":"execution.execution","id":id}),
        0,
        json!({"brief":{"digest":pio_core::digest(id.as_bytes()),"media_type":"text/plain"}}),
    );
    p["command_id"] = id.into();
    p
}

#[test]
fn admission_headroom_refuses_new_submit_while_admitted_work_completes() {
    let mut cfg = config();
    cfg["executor"] = json!({"default_script":[{"deliver":"provider_ack_id"},{"runtime":"active"},{"exit":{"code":0}}]});
    let mut s = session();
    s.selected
        .as_mut()
        .unwrap()
        .insert("execution".into(), vec![]);
    let scratch = tempfile::tempdir().unwrap();
    let mut p = Provider::new(scratch.path(), cfg.clone()).unwrap();
    let base = p.data.records().unwrap().len() as u64;
    p.handle(&mut s, "execution.submit", &submit_for("e"))
        .unwrap();
    let added = p.data.records().unwrap().len() as u64 - base;

    // One over the admission threshold: refused and unbound, although the
    // hard limit still has room.
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), cfg.clone()).unwrap();
    fill_records(&mut p, MAX_ADMISSION_PROJECTION_RECORDS - added + 1);
    let journal = p.store.journal().unwrap();
    assert_eq!(
        p.handle(&mut s, "execution.submit", &submit_for("e"))
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(
        p.capacity_refusal,
        Some(
            json!({"limit":"admission_projection_records","maximum":MAX_ADMISSION_PROJECTION_RECORDS,"projected":MAX_ADMISSION_PROJECTION_RECORDS+1})
        )
    );
    assert!(p.data.executions.is_empty() && p.data.commands.is_empty());
    assert_eq!(p.store.journal().unwrap(), journal);
    // The admission threshold applies only to new execution admission.
    p.capacity_refusal = None;
    fill_records(&mut p, MAX_ADMISSION_PROJECTION_RECORDS);
    p.handle(&mut s, "core-test.subject.put", &put("other"))
        .unwrap();
    assert!(p.data.records().unwrap().len() as u64 > MAX_ADMISSION_PROJECTION_RECORDS);

    // Exactly at the threshold: admitted. Its observations then use the
    // remaining room, it reaches exit without a stall, and a new submit is
    // refused at the admission threshold.
    let root = tempfile::tempdir().unwrap();
    let mut p = Provider::new(root.path(), cfg).unwrap();
    fill_records(&mut p, MAX_ADMISSION_PROJECTION_RECORDS - added);
    p.handle(&mut s, "execution.submit", &submit_for("e"))
        .unwrap();
    assert_eq!(
        p.data.records().unwrap().len() as u64,
        MAX_ADMISSION_PROJECTION_RECORDS
    );
    assert_eq!(
        p.handle(&mut s, "execution.submit", &submit_for("f"))
            .unwrap_err()
            .code,
        "unavailable"
    );
    assert_eq!(
        p.capacity_refusal.as_ref().unwrap()["limit"],
        "admission_projection_records"
    );
    p.capacity_refusal = None;
    for _ in 0..4 {
        p.execution_tick().unwrap();
    }
    let view = &p.data.executions["e"]["view"];
    assert_eq!(view["delivery"], "acknowledged");
    assert_eq!(view["runtime"], "exited");
    assert_eq!(view["exit"], json!({"code":0}));
    assert_eq!(
        p.capacity_refusal, None,
        "no hard-limit stall for admitted work"
    );
    let records = p.data.records().unwrap().len() as u64;
    assert!(records > MAX_ADMISSION_PROJECTION_RECORDS && records <= MAX_PROJECTION_RECORDS);
    assert!(!p.data.executions.contains_key("f"));
    assert_eq!(
        p.handle(&mut s, "execution.submit", &submit_for("e"))
            .unwrap()["replay"],
        true
    );
}

#[test]
fn codex_widening_approval_decisions_are_invalid_and_never_reach_the_host() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    let host = json!({"adapter":"codex","labeled_fake":true,"executable":"/bin/false","env":{"PATH":"/usr/bin:/bin"},"codex_home":root.path().join("home"),"fixture_root":root.path().join("fixtures"),"thread":{"sandbox":"workspace-write","approvalPolicy":"on-request"},"qualification_binding":null});
    let mut cfg = config();
    cfg["executor"] = json!({"host_id":"codex-host"});
    let mut p = Provider::with_host(&store, cfg, Some(host)).unwrap();
    let mut s = session();
    s.selected.as_mut().unwrap().insert(
        "execution".into(),
        crate::codex::FEATURES
            .iter()
            .map(|f| f.to_string())
            .collect(),
    );
    // Internal store test only: an execution with one pending native action.
    // Admission is not `admitted`, so no tick launches a host process here.
    let action = "e.action-1";
    let e = json!({"source":pio_codex::fake::SOURCE,"principal":"owner","submit":{},"view":{"execution":{"kind":"execution.execution","id":"e"},"revision":3,"admission":"refused","delivery":"pending","runtime":"requires_action","runtime_detail":{"action_id":action,"owner":"codex"},"actions":[{"action_id":action,"owner":"codex","state":"pending","requested_at":"2026-01-01T00:00:00Z"}],"effects":[],"host":{"id":"codex-host","generation":1}},"codex_actions":{action:{"seq":1,"method":"item/commandExecution/requestApproval","request_id":"r1"}}});
    p.update_execution(&e);
    p.save().unwrap();
    let journal = p.store.journal().unwrap();
    let respond = |decision: Value, id: &str| {
        let bytes = serde_json::to_vec(&json!({ "decision": decision })).unwrap();
        let mut c = command(
            "execution.respond_action",
            json!({"kind":"execution.execution","id":"e"}),
            3,
            json!({"action_id":action,"response":{"digest":pio_core::digest(&bytes),"media_type":"application/json"}}),
        );
        c["command_id"] = id.into();
        c["extensions"] = json!({(crate::codex::CONTENT_EXTENSION):{"media_type":"application/json","text":String::from_utf8(bytes).unwrap()}});
        c
    };
    for (n, decision) in [
        json!("acceptForSession"),
        json!({"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["echo","fixture"]}}),
        json!({"applyNetworkPolicyAmendment":{"network_policy_amendment":{"host":"example.com","action":"allow"}}}),
    ]
    .into_iter()
    .enumerate()
    {
        let error = p
            .handle(&mut s, "execution.respond_action", &respond(decision.clone(), &format!("widen-{n}")))
            .unwrap_err();
        assert_eq!(error.code, "invalid_envelope", "{decision}");
        assert_eq!(error.details["path"], "/payload/response", "{decision}");
        assert_eq!(p.store.journal().unwrap(), journal, "nothing committed for {decision}");
        let stored = &p.data.executions["e"];
        assert!(stored.get("codex_controls").is_none() && stored["response_count"].is_null());
        assert_eq!(stored["view"]["actions"][0]["state"], "pending");
        assert!(!p.data.effects.keys().any(|k| k.contains("response")));
    }
    assert!(
        std::fs::read_dir(&store).unwrap().all(|f| !f
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".controls.jsonl")),
        "no control ever reaches a host"
    );
    // Positive control: a plain decline is answered and queued for the host.
    let answered = p
        .handle(
            &mut s,
            "execution.respond_action",
            &respond(json!("decline"), "decline"),
        )
        .unwrap();
    assert_eq!(answered["outcome"]["state"], "answered");
    assert_eq!(
        p.data.executions["e"]["codex_controls"][0]["decision"],
        "decline"
    );
}

/// L3 (owner decision, 2026-09-25): on Codex a lead-tool spec may name the
/// lead server's own tools to pre-allow, and nothing else is widened. The
/// shape and credential refusals are OpenCode's; `pre_allowed_tools` must
/// be tool names; and OpenCode, which cannot honour it, still refuses it.
#[test]
fn a_codex_lead_tool_spec_may_pre_allow_its_own_tools_and_nothing_else() {
    let good = json!({"name":"pio-lead","command":"/usr/bin/python3",
        "args":["/opt/pio/lead_tool.py","--credential-file","/private/lead.credential"],
        "env":[{"name":"PIO_LEAD_GRANT","value":"a-grant-id"}],
        "pre_allowed_tools":["start_run","read_run"]});
    let reasons = |tool: &Value| -> Vec<String> {
        crate::codex::codex_lead_tool_refusals(tool)
            .iter()
            .map(|r| r["reason"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(reasons(&good).is_empty(), "{:?}", reasons(&good));
    for bad in [
        json!([]),
        json!("start_run"),
        json!(["start run"]),
        json!([""]),
        json!([1]),
    ] {
        let mut tool = good.clone();
        tool["pre_allowed_tools"] = bad.clone();
        assert_eq!(
            reasons(&tool),
            ["lead_tool_pre_allowed_tools_must_be_tool_names"],
            "{bad}"
        );
    }
    let mut valued = good.clone();
    valued["env"] = json!([{"name":"PIO_LEAD_NOTE","value":"ccred1.lead.abc"}]);
    assert_eq!(reasons(&valued), ["lead_tool_carries_a_credential_value"]);
    let opencode: Vec<String> = pio_opencode::lead_tool_refusals(&good)
        .iter()
        .map(|r| r["reason"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(opencode, ["lead_tool_unsupported_field"]);
}

/// L3, after its first live run (2026-09-26): on Codex a lead-tool spec may
/// carry three settings of its own server table, sent as they are:
/// `omit_tools_from` (only `code_mode` and `deferred`, each once),
/// `required` (a boolean) and `startup_timeout_sec` (1 to 120). Anything
/// else is refused before admission, and OpenCode refuses all three.
#[test]
fn a_codex_lead_tool_spec_may_keep_its_tools_in_the_model_list() {
    let good = json!({"name":"pio-lead","command":"/usr/bin/python3",
        "args":["/opt/pio/lead_tool.py"],"env":[],
        "pre_allowed_tools":["start_run","read_run"],
        "omit_tools_from":["code_mode","deferred"],"required":true,
        "startup_timeout_sec":30});
    let reasons = |tool: &Value| -> Vec<String> {
        crate::codex::codex_lead_tool_refusals(tool)
            .iter()
            .map(|r| r["reason"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(reasons(&good).is_empty(), "{:?}", reasons(&good));
    for (setting, bad) in [
        ("omit_tools_from", json!([])),
        ("omit_tools_from", json!(["direct"])),
        ("omit_tools_from", json!(["code_mode", "code_mode"])),
        ("omit_tools_from", json!("deferred")),
        ("omit_tools_from", json!(["codeMode"])),
        ("required", json!("true")),
        ("startup_timeout_sec", json!(0)),
        ("startup_timeout_sec", json!(121)),
        ("startup_timeout_sec", json!(30.5)),
    ] {
        let mut tool = good.clone();
        tool[setting] = bad.clone();
        assert_eq!(
            reasons(&tool),
            [format!("lead_tool_{setting}_not_accepted")],
            "{setting} = {bad}"
        );
    }
    let opencode: Vec<String> = pio_opencode::lead_tool_refusals(&good)
        .iter()
        .map(|r| r["reason"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(opencode, ["lead_tool_unsupported_field"; 4]);
}

/// D7. Only an identifier the harness returned earns `provider_ack_id`.
/// Claude Code's replay echo returns the message PIO sent and no identifier,
/// so its delivery carries evidence class `native_replay_echo` and no proof
/// class, exactly as OpenCode's `native_session_update` does.
#[test]
fn only_a_returned_identifier_earns_a_delivery_proof_class() {
    let codex = crate::codex::profile("codex").unwrap();
    let claude = crate::codex::profile("claude").unwrap();
    let opencode = crate::codex::profile("opencode").unwrap();
    assert_eq!(
        crate::codex::delivery_proof(
            codex,
            &json!({"kind":"turn_acknowledged","turn_id":"turn-1"})
        ),
        Some("provider_ack_id")
    );
    assert_eq!(
        crate::codex::delivery_proof(
            claude,
            &json!({"kind":"turn_acknowledged","replay_matches_sent":true})
        ),
        None
    );
    assert_eq!(claude.delivery_evidence, "native_replay_echo");
    assert_eq!(
        crate::codex::delivery_proof(
            opencode,
            &json!({"kind":"turn_acknowledged","first_session_update":true,"proof_class":null})
        ),
        None
    );
    assert_eq!(opencode.delivery_evidence, "native_session_update");
}
