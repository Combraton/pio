use crate::provider::*;
use serde_json::{Value, json};
use uuid::Uuid;
type Position = (u64, u64);
fn position(v: &Value) -> Position {
    (num(&v["epoch"]), num(&v["sequence"]))
}
fn pos(p: Position) -> Value {
    json!({"epoch":p.0,"sequence":p.1})
}
impl Provider {
    pub fn head(&self) -> Position {
        (self.data.epoch, self.data.sequence)
    }
    pub fn cursor(&self, p: Position) -> String {
        format!("{}:{}:{}", self.data.stream, p.0, p.1)
    }
    fn parse_cursor(&self, s: &str) -> Result<Position, Error> {
        let mut parts = s.split(':');
        let stream = parts.next();
        let epoch = parts.next().and_then(|s| s.parse::<u64>().ok());
        let seq = parts.next().and_then(|s| s.parse::<u64>().ok());
        if stream != Some(&self.data.stream)
            || epoch.is_none()
            || seq.is_none()
            || parts.next().is_some()
        {
            return Err(err(
                "invalid_cursor",
                json!({"reason":"invalid stream position"}),
            ));
        }
        let p = (epoch.unwrap(), seq.unwrap());
        if p.0 == 0 || p.0 > self.data.epoch || (p.0 == self.data.epoch && p.1 > self.data.sequence)
        {
            return Err(err(
                "invalid_cursor",
                json!({"reason":"position beyond head"}),
            ));
        }
        Ok(p)
    }
    pub fn events_start(&mut self) {
        if self.config["events"]["new_epoch_on_start"] == true {
            let through = self
                .data
                .sequence
                .saturating_sub(num(&self.config["events"]["unvouched_last"]));
            self.data.vouches.insert(self.data.epoch, through);
            self.data
                .events
                .retain(|e| position(e) <= (self.data.epoch, through));
            self.data.epoch += 1;
            self.data.sequence = 0;
        }
        self.retain_events();
    }
    fn retain_events(&mut self) {
        if let Some(n) = self.config["events"]["retain_last"].as_u64() {
            let drop = self.data.events.len().saturating_sub(n as usize);
            if drop > 0 {
                self.data.discarded = Some(position(&self.data.events[drop - 1]));
                self.data.events.drain(..drop);
            }
        }
    }
    pub fn append_event(
        &mut self,
        subject: Value,
        revision: u64,
        kind: &str,
        payload: Value,
        command: Option<(&Value, &str)>,
    ) {
        self.data.sequence += 1;
        let mut event = json!({"stream":self.data.stream,"epoch":self.data.epoch,"sequence":self.data.sequence,"type":kind,"subject":subject,"revision":revision,"origin":"provider","caused_by":[],"recorded_at":self.now,"payload":payload});
        if let Some((p, operation_ref)) = command {
            event["origin"] = "command".into();
            event["command_id"] = p["command_id"].clone();
            event["operation_ref"] = operation_ref.into();
            event["caused_by"] = p.get("caused_by").cloned().unwrap_or(json!([]));
        }
        self.data.events.push(event);
        self.retain_events();
    }
    pub fn record_changes(&mut self, before: &Data, result: &Value, p: &Value) {
        let mut changed: Vec<_> = self
            .data
            .subjects
            .iter()
            .filter(|(k, s)| {
                before
                    .subjects
                    .get(*k)
                    .is_none_or(|old| old.revision != s.revision)
            })
            .map(|(_, s)| s.clone())
            .collect();
        changed.sort_by_key(|s| s.subject != p["subject"]);
        for s in changed {
            let kind = match text(&s.subject["kind"]) {
                "core-test.authority" => "core-test.authority.claimed",
                "core-test.subject" => "core-test.subject.changed",
                "core.grant" if s.state["grant"]["state"] == "revoked" => "core.grant.revoked",
                "core.grant" => "core.grant.issued",
                _ => continue,
            };
            let payload = if kind == "core.grant.revoked" {
                json!({"state":"revoked"})
            } else {
                s.state
            };
            self.append_event(
                s.subject,
                s.revision,
                kind,
                payload,
                Some((p, text(&result["acknowledgment"]["operation_ref"]))),
            );
        }
    }
    fn start_position(&self, p: &Value) -> Result<Position, Error> {
        if let Some(c) = p["cursor"].as_str() {
            self.parse_cursor(c)
        } else if p["from"] == "now" {
            Ok(self.head())
        } else {
            Ok((1, 0))
        }
    }
    pub fn read_events(
        &self,
        session: &Session,
        envelope: &Value,
        subscription: Option<&str>,
    ) -> Reply {
        let p = &envelope["payload"];
        let mut current = self.start_position(p)?;
        let limit = num(&p["limit"]).max(1) as usize;
        let mut filtered = false;
        let mut items = vec![];
        let allowed = |subject: &Value| {
            self.visible(session, envelope, subject)
                && (p.get("kinds").is_none() || list(&p["kinds"]).contains(&subject["kind"]))
        };
        loop {
            if items.len() >= limit {
                break;
            }
            let next;
            let item = if current.0 < self.data.epoch
                && current.1 >= *self.data.vouches.get(&current.0).unwrap_or(&0)
            {
                next = (current.0 + 1, 0);
                Some(
                    json!({"epoch_change":{"from_epoch":current.0,"to_epoch":next.0,"vouched_through":self.data.vouches.get(&current.0).copied().unwrap_or(0)}}),
                )
            } else if self.data.discarded.is_some_and(|last| current < last) {
                let mut subjects = vec![];
                for s in self.data.subjects.values() {
                    if allowed(&s.subject) {
                        subjects.push(
                            json!({"subject":s.subject,"revision":s.revision,"state":s.state}),
                        )
                    } else {
                        filtered = true
                    }
                }
                next = self.head();
                Some(
                    json!({"gap":{"kind":"retention","from":pos((current.0,current.1+1)),"to":pos(next),"snapshot":{"as_of":pos(next),"subjects":subjects}}}),
                )
            } else if let Some(event) = self
                .data
                .events
                .iter()
                .find(|e| position(e) > current && position(e).0 == current.0)
            {
                next = position(event);
                if allowed(&event["subject"]) {
                    Some(json!({"event":event}))
                } else {
                    filtered = true;
                    None
                }
            } else {
                break;
            };
            if let Some(item) = item {
                let mut tentative = items.clone();
                tentative.push(item.clone());
                let result = json!({"stream":{"id":self.data.stream,"epoch":self.data.epoch},"items":tentative,"next_cursor":self.cursor(next),"filtered":filtered});
                let frame = if let Some(id) = subscription {
                    json!({"jsonrpc":"2.0","method":"core.events.notify","params":{"subscription":id,"items":result["items"],"next_cursor":result["next_cursor"]}})
                } else {
                    json!({"jsonrpc":"2.0","id":session.request_id,"result":result})
                };
                if serde_json::to_vec(&frame).unwrap().len() > session.receive {
                    if items.is_empty() {
                        return Err(err("internal_error", json!({})));
                    }
                    break;
                }
                items.push(item);
            }
            current = next;
        }
        Ok(
            json!({"stream":{"id":self.data.stream,"epoch":self.data.epoch},"items":items,"next_cursor":self.cursor(current),"filtered":filtered}),
        )
    }
    pub fn event_query(&mut self, session: &mut Session, method: &str, p: &Value) -> Reply {
        match method {
            "core.events.read" => self.read_events(session, p, None),
            "core.events.subscribe" => {
                let at = self.start_position(&p["payload"])?;
                let id = Uuid::new_v4().to_string();
                let mut read = p.clone();
                read["payload"].as_object_mut().unwrap().remove("from");
                read["payload"]["cursor"] = self.cursor(at).into();
                read["payload"]["limit"] = 100.into();
                session.subscriptions.insert(id.clone(), read);
                Ok(
                    json!({"subscription":id,"stream":{"id":self.data.stream,"epoch":self.data.epoch}}),
                )
            }
            "core.events.unsubscribe" => {
                if session
                    .subscriptions
                    .remove(text(&p["payload"]["subscription"]))
                    .is_some()
                {
                    Ok(json!({}))
                } else {
                    Err(err("not_found", json!({})))
                }
            }
            _ => Err(err("method_not_found", json!({"operation":method}))),
        }
    }
    pub fn notifications(&mut self, session: &mut Session) -> Vec<Value> {
        let _ = self.clock(false);
        let _ = self.expire_obligations();
        let mut frames = vec![];
        for (id, mut p) in session.subscriptions.clone() {
            let mut end = None;
            let result = if self
                .authorize(session, "core.events.subscribe", &p)
                .is_err()
            {
                end = Some("authorization_lost");
                None
            } else {
                match self.read_events(session, &p, Some(&id)) {
                    Ok(r) => Some(r),
                    Err(_) => {
                        end = Some("item_too_large");
                        None
                    }
                }
            };
            let mut params =
                json!({"subscription":id,"items":[],"next_cursor":p["payload"]["cursor"]});
            if let Some(reason) = end {
                params["ended"] = json!({"reason":reason});
                session.subscriptions.remove(&id);
            } else if let Some(r) = result {
                p["payload"]["cursor"] = r["next_cursor"].clone();
                session.subscriptions.insert(id.clone(), p);
                if list(&r["items"]).is_empty() {
                    continue;
                }
                params["items"] = r["items"].clone();
                params["next_cursor"] = r["next_cursor"].clone();
            }
            frames.push(json!({"jsonrpc":"2.0","method":"core.events.notify","params":params}));
        }
        frames
    }
}
