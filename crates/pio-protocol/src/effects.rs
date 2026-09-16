//! Effect reconciliation storage. Public negotiation remains withheld until the
//! pinned Execution fixtures can create and reconcile real fake-host effects.
use crate::provider::*;
use serde_json::{Value, json};
use uuid::Uuid;
impl Provider {
    pub fn effect_get(&self, id: &str) -> Reply {
        match self.data.effects.get(id) {
            Some(record) if record.is_null() => Err(err("effect_history_unavailable", json!({}))),
            Some(record) => Ok(record.clone()),
            None => Err(err("not_found", json!({}))),
        }
    }
    pub fn effect_abort(&mut self, p: &Value) -> Reply {
        let id = text(&p["subject"]["id"]);
        let mut record = self.effect_get(id)?;
        let obligation = record["obligations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|o| {
                o["id"] == p["payload"]["obligation"]
                    && matches!(text(&o["state"]), "open" | "overdue")
            })
            .ok_or_else(|| err("not_found", json!({})))?;
        obligation["state"] = "aborted".into();
        record["revision"] = (num(&record["revision"]) + 1).into();
        let operation_ref = Uuid::new_v4().to_string();
        self.append_event(p["subject"].clone(),num(&record["revision"]),"core.effect.obligation.aborted",json!({"effect":id,"obligation":p["payload"]["obligation"],"target":record["effect"]["target"]}),Some((p,&operation_ref)));
        self.data.effects.insert(id.to_owned(), record.clone());
        Ok(
            json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":operation_ref,"subject":p["subject"],"revision":record["revision"],"effect_refs":[]},"outcome":{"effect":id,"obligation":{"id":p["payload"]["obligation"],"state":"aborted"},"status":record["status"]},"replay":false}),
        )
    }
    pub fn expire_obligations(&mut self) -> anyhow::Result<()> {
        let before = self.data.clone();
        let mut expired = vec![];
        for (id, record) in &mut self.data.effects {
            if record.is_null() {
                continue;
            }
            for obligation in record["obligations"].as_array_mut().unwrap() {
                if obligation["state"] == "open"
                    && obligation["deadline"]
                        .as_str()
                        .is_some_and(|deadline| deadline <= self.now.as_str())
                {
                    obligation["state"] = "overdue".into();
                    expired.push((id.clone(), obligation["id"].clone()));
                }
            }
        }
        if expired.is_empty() {
            return Ok(());
        }
        for (id, obligation) in expired {
            let record = self.data.effects.get_mut(&id).unwrap();
            record["revision"] = (num(&record["revision"]) + 1).into();
            let revision = num(&record["revision"]);
            let target = record["effect"]["target"].clone();
            self.append_event(
                json!({"kind":"core.effect","id":id}),
                revision,
                "core.effect.obligation.overdue",
                json!({"effect":id,"obligation":obligation,"target":target}),
                None,
            );
        }
        if let Err(e) = self.save() {
            self.data = before;
            return Err(e);
        }
        Ok(())
    }
    pub fn revision(&self, subject: &Value) -> u64 {
        if subject["kind"] == "core.effect" {
            self.data
                .effects
                .get(text(&subject["id"]))
                .map(|r| num(&r["revision"]))
                .unwrap_or(0)
        } else {
            self.data
                .subjects
                .get(&key(subject))
                .map(|s| s.revision)
                .unwrap_or(0)
        }
    }
}
