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
    assert!(
        !FEATURES.contains(&"core.effects"),
        "public claim waits for pinned Execution fixtures"
    );
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
