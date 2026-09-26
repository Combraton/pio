//! Provider caches are reconstructed from journal-backed per-record projections.
//!
//! Changed-key commits (ADR 002 amendment, 2026-09-26): every retained
//! collection records which of its keys changed since the last commit, so a
//! commit names only those records instead of serializing and diffing the
//! whole provider state. The collections expose no general mutable access:
//! each mutation goes through a method that records its key, so a change the
//! commit could miss does not compile.
use crate::provider::Data;
use anyhow::Result;
use pio_core::projections::{Changes, Records};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, RangeTo};
use uuid::Uuid;

/// A retained keyed collection that records its changed keys.
#[derive(Clone, Debug)]
pub struct Tracked<V> {
    map: BTreeMap<String, V>,
    changed: BTreeSet<String>,
}

impl<V> Default for Tracked<V> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
            changed: BTreeSet::new(),
        }
    }
}

impl<V> Deref for Tracked<V> {
    type Target = BTreeMap<String, V>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a, V> IntoIterator for &'a Tracked<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = std::collections::btree_map::Iter<'a, String, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

impl<V: PartialEq> Tracked<V> {
    /// Rewriting a record with an equal value is not a change.
    pub fn insert(&mut self, key: String, value: V) -> Option<V> {
        if self.map.get(&key) != Some(&value) {
            self.changed.insert(key.clone());
        }
        self.map.insert(key, value)
    }
}

impl<V> Tracked<V> {
    /// Mutable access counts as a change; the commit compares the result
    /// with the stored record, so an unmodified value is still not journaled.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        let value = self.map.get_mut(key)?;
        self.changed.insert(key.to_owned());
        Some(value)
    }
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let value = self.map.remove(key)?;
        self.changed.insert(key.to_owned());
        Some(value)
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&String, &V) -> bool) {
        let changed = &mut self.changed;
        self.map.retain(|key, value| {
            let kept = keep(key, value);
            if !kept {
                changed.insert(key.clone());
            }
            kept
        });
    }
}

impl<V: Serialize> Serialize for Tracked<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.map.serialize(serializer)
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Tracked<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            map: BTreeMap::deserialize(deserializer)?,
            changed: BTreeSet::new(),
        })
    }
}

/// The retained event list, ordered by stream position, which records the
/// events appended or dropped since the last commit.
#[derive(Clone, Debug, Default)]
pub struct EventLog {
    events: Vec<Value>,
    changed: BTreeMap<String, Option<Value>>,
}

fn event_key(event: &Value) -> String {
    format!(
        "event/{:020}/{:020}",
        event["epoch"].as_u64().unwrap(),
        event["sequence"].as_u64().unwrap()
    )
}

impl Deref for EventLog {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl<'a> IntoIterator for &'a EventLog {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

impl EventLog {
    pub fn push(&mut self, event: Value) {
        self.changed.insert(event_key(&event), Some(event.clone()));
        self.events.push(event);
    }
    pub fn drain(&mut self, range: RangeTo<usize>) {
        for event in self.events.drain(range) {
            self.changed.insert(event_key(&event), None);
        }
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&Value) -> bool) {
        let changed = &mut self.changed;
        self.events.retain(|event| {
            let kept = keep(event);
            if !kept {
                changed.insert(event_key(event), None);
            }
            kept
        });
    }
}

impl Serialize for EventLog {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.events.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EventLog {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            events: Vec::deserialize(deserializer)?,
            changed: BTreeMap::new(),
        })
    }
}

/// Adds the changed keys of one collection, with their current values.
fn collect<V: Serialize>(
    changes: &mut Changes,
    prefix: &str,
    collection: &Tracked<V>,
) -> Result<()> {
    for key in &collection.changed {
        let value = collection
            .map
            .get(key)
            .map(serde_json::to_value)
            .transpose()?;
        changes.insert(format!("{prefix}{key}"), value);
    }
    Ok(())
}

impl Data {
    fn meta(&self) -> Value {
        json!({"generation":self.generation,"oldest":self.oldest,"stream":self.stream,"epoch":self.epoch,"sequence":self.sequence,"vouches":self.vouches,"discarded":self.discarded,"cap_revision":self.cap_revision,"predicates":self.predicates})
    }
    /// The complete projection. Used to load, measure and verify; the commit
    /// path uses [`Data::take_changes`].
    pub fn records(&self) -> Result<Records> {
        let mut r = Records::new();
        r.insert("meta".into(), self.meta());
        for (key, value) in &self.subjects {
            r.insert(format!("subject/{key}"), serde_json::to_value(value)?);
        }
        for (key, value) in &self.commands {
            r.insert(format!("command/{key}"), serde_json::to_value(value)?);
        }
        for (key, value) in &self.executions {
            r.insert(format!("execution/{key}"), value.clone());
        }
        for (key, value) in &self.effects {
            r.insert(format!("effect/{key}"), value.clone());
        }
        for event in &self.events {
            r.insert(event_key(event), event.clone());
        }
        Ok(r)
    }
    /// The records changed since the last commit, plus `meta`, whose scalars
    /// are rewritten freely and which costs one record to compare. Clears the
    /// change sets; a failed commit hands them back with
    /// [`Data::restore_changes`].
    pub fn take_changes(&mut self) -> Result<Changes> {
        let mut changes = Changes::from([("meta".to_owned(), Some(self.meta()))]);
        collect(&mut changes, "subject/", &self.subjects)?;
        collect(&mut changes, "command/", &self.commands)?;
        collect(&mut changes, "execution/", &self.executions)?;
        collect(&mut changes, "effect/", &self.effects)?;
        changes.extend(std::mem::take(&mut self.events.changed));
        self.subjects.changed.clear();
        self.commands.changed.clear();
        self.executions.changed.clear();
        self.effects.changed.clear();
        Ok(changes)
    }
    /// Marks the records of an uncommitted change set as changed again, so
    /// the next commit still carries them.
    pub fn restore_changes(&mut self, changes: Changes) {
        for (key, value) in changes {
            if key.starts_with("event/") {
                self.events.changed.entry(key).or_insert(value);
                continue;
            }
            let Some((prefix, rest)) = key.split_once('/') else {
                continue;
            };
            let changed = match prefix {
                "subject" => &mut self.subjects.changed,
                "command" => &mut self.commands.changed,
                "execution" => &mut self.executions.changed,
                "effect" => &mut self.effects.changed,
                _ => continue,
            };
            changed.insert(rest.to_owned());
        }
    }
    pub fn from_records(records: &Records) -> Result<Self> {
        let Some(meta) = records.get("meta") else {
            return Ok(Self {
                subjects: Tracked::default(),
                commands: Tracked::default(),
                generation: 1,
                oldest: 1,
                stream: Uuid::new_v4().to_string(),
                epoch: 1,
                sequence: 0,
                events: EventLog::default(),
                vouches: BTreeMap::new(),
                discarded: None,
                cap_revision: 1,
                predicates: json!([]),
                effects: Tracked::default(),
                executions: Tracked::default(),
            });
        };
        let mut v = meta.clone();
        v["subjects"] = json!({});
        v["commands"] = json!({});
        v["effects"] = json!({});
        v["executions"] = json!({});
        v["events"] = json!([]);
        for (key, value) in records {
            if let Some(key) = key.strip_prefix("execution/") {
                v["executions"][key] = value.clone();
            }
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
