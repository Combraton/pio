use crate::Store;
use serde_json::json;
fn store() -> (tempfile::TempDir, Store, u64) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let generation = store.advance_controller().unwrap().1;
    (dir, store, generation)
}
#[test]
fn admit_replay_is_read_only_and_conflict_does_not_bind() {
    let (_dir, mut s, g) = store();
    let (first, inserted) = s.admit("a", json!({"duration_ms":1}), g, false).unwrap();
    assert!(inserted);
    let journal = s.journal().unwrap();
    let (replay, inserted) = s.admit("a", json!({"duration_ms":1}), g, false).unwrap();
    assert!(!inserted);
    assert_eq!(first.invocation_id, replay.invocation_id);
    assert_eq!(s.journal().unwrap(), journal);
    assert_eq!(
        s.admit("a", json!({"duration_ms":2}), g, false)
            .unwrap_err()
            .to_string(),
        "idempotency_conflict"
    );
    assert_eq!(s.journal().unwrap(), journal);
}
#[test]
fn transitions_fence_identity_phase_and_invalid_edges() {
    let (_dir, mut s, g) = store();
    let (state, _) = s.admit("a", json!({}), g, false).unwrap();
    let journal = s.journal().unwrap();
    let mut forged = state.clone();
    forged.host_generation += 1;
    assert_eq!(
        s.transition(&forged, "host_claimed", None, None, None)
            .unwrap_err()
            .to_string(),
        "host_identity_fenced"
    );
    assert_eq!(
        s.transition(&state, "completed", None, None, None)
            .unwrap_err()
            .to_string(),
        "invalid transition"
    );
    assert_eq!(s.journal().unwrap(), journal);
    s.transition(&state, "host_claimed", None, None, None)
        .unwrap();
    assert_eq!(
        s.transition(&state, "host_claimed", None, None, None)
            .unwrap_err()
            .to_string(),
        "phase_conflict"
    );
}
#[test]
fn generation_fences_new_admission_and_release_but_allows_replay() {
    let (_dir, mut s, g) = store();
    let (state, _) = s.admit("a", json!({}), g, false).unwrap();
    let claimed = s
        .transition(&state, "host_claimed", None, None, None)
        .unwrap();
    let parked = s.transition(&claimed, "parked", None, None, None).unwrap();
    s.advance_controller().unwrap();
    let before = s.journal().unwrap();
    assert_eq!(
        s.admit("b", json!({}), g, false).unwrap_err().to_string(),
        "stale_controller_generation"
    );
    assert_eq!(
        s.transition(&parked, "released", None, None, None)
            .unwrap_err()
            .to_string(),
        "stale_controller_generation"
    );
    assert!(!s.admit("a", json!({}), g, false).unwrap().1);
    assert_eq!(s.journal().unwrap(), before);
}

#[test]
fn existing_store_open_does_not_compete_for_the_writer_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let tx = store
        .conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    // A host observation must open/read while the Protocol writer holds an
    // unrelated transaction. Re-running migrations here used to block/fail.
    let observer = Store::open(dir.path()).unwrap();
    assert!(observer.identity().is_ok());
    drop(tx);
}
