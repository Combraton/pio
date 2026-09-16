//! PIO execution authority. M1 skeleton: no runtime capabilities yet.
pub fn participant() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../conformance/participant.json"))
        .expect("checked-in participant descriptor")
}
