//! Provider caches are reconstructed from journal-backed per-record projections.
use crate::provider::Data;
use anyhow::Result;
use pio_core::projections::Records;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;
impl Data {
    pub fn records(&self) -> Result<Records> {
        let mut r = Records::new();
        r.insert("meta".into(),json!({"generation":self.generation,"oldest":self.oldest,"stream":self.stream,"epoch":self.epoch,"sequence":self.sequence,"vouches":self.vouches,"discarded":self.discarded,"cap_revision":self.cap_revision,"predicates":self.predicates}));
        for (key, value) in &self.subjects {
            r.insert(format!("subject/{key}"), serde_json::to_value(value)?);
        }
        for (key, value) in &self.commands {
            r.insert(format!("command/{key}"), serde_json::to_value(value)?);
        }
        for (key, value) in &self.effects {
            r.insert(format!("effect/{key}"), value.clone());
        }
        for event in &self.events {
            r.insert(
                format!(
                    "event/{:020}/{:020}",
                    event["epoch"].as_u64().unwrap(),
                    event["sequence"].as_u64().unwrap()
                ),
                event.clone(),
            );
        }
        Ok(r)
    }
    pub fn from_records(records: &Records) -> Result<Self> {
        let Some(meta) = records.get("meta") else {
            return Ok(Self {
                subjects: BTreeMap::new(),
                commands: BTreeMap::new(),
                generation: 1,
                oldest: 1,
                stream: Uuid::new_v4().to_string(),
                epoch: 1,
                sequence: 0,
                events: vec![],
                vouches: BTreeMap::new(),
                discarded: None,
                cap_revision: 1,
                predicates: json!([]),
                effects: BTreeMap::new(),
            });
        };
        let mut v = meta.clone();
        v["subjects"] = json!({});
        v["commands"] = json!({});
        v["effects"] = json!({});
        v["events"] = json!([]);
        for (key, value) in records {
            if let Some(key) = key.strip_prefix("subject/") {
                v["subjects"][key] = value.clone();
            }
            if let Some(key) = key.strip_prefix("command/") {
                v["commands"][key] = value.clone();
            }
            if let Some(key) = key.strip_prefix("effect/") {
                v["effects"][key] = value.clone();
            }
            if key.starts_with("event/") {
                v["events"].as_array_mut().unwrap().push(value.clone());
            }
        }
        Ok(serde_json::from_value::<Self>(v)?)
    }
}
