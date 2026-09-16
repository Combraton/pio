use crate::provider::*;
use serde_json::{Value, json};
use uuid::Uuid;

fn denied(reason: &str) -> Error {
    err("permission_denied", json!({"reason":reason}))
}
pub fn covers(resource: &Value, subject: &Value) -> bool {
    resource["kind"] == subject["kind"]
        && resource.get("id").is_none_or(|id| *id == subject["id"])
        && resource
            .get("id_prefix")
            .is_none_or(|prefix| text(&subject["id"]).starts_with(text(prefix)))
}
fn subset(child: &Value, parent: &Value) -> bool {
    child["kind"] == parent["kind"]
        && if child.get("id").is_some() {
            covers(parent, child)
        } else if parent.get("id").is_some() {
            false
        } else {
            parent.get("id_prefix").is_none_or(|prefix| {
                child
                    .get("id_prefix")
                    .is_some_and(|v| text(v).starts_with(text(prefix)))
            })
        }
}
impl Provider {
    pub fn grant(&self, id: &str) -> Option<&Value> {
        self.data
            .subjects
            .get(&key(&json!({"kind":"core.grant","id":id})))
            .map(|s| &s.state["grant"])
    }
    pub fn usable_grant(&self, session: &Session, id: &str) -> Result<&Value, Error> {
        let grant = self
            .grant(id)
            .filter(|g| g["holder"] == session.principal.as_deref().unwrap_or(""))
            .ok_or_else(|| denied("grant_not_found"))?;
        if grant["state"] == "revoked" {
            return Err(denied("revoked"));
        }
        if grant["expires_at"]
            .as_str()
            .is_some_and(|s| s <= self.now.as_str())
        {
            return Err(denied("expired"));
        }
        if let Some(binding) = grant.get("authority_binding") {
            let scope = text(&binding["scope"]);
            let subject = if let Some(host) = scope.strip_prefix("execution.controller:") {
                json!({"kind":"execution.controller","id":host})
            } else {
                json!({"kind":"core-test.authority","id":scope})
            };
            let epoch = self.revision(&subject);
            if binding["epoch"] != epoch {
                return Err(denied("authority_epoch_stale"));
            }
        }
        Ok(grant)
    }
    pub fn authorize(&self, session: &Session, method: &str, p: &Value) -> Result<(), Error> {
        let principal = session.principal.as_deref().unwrap_or("");
        let authority = self.authorities.iter().any(|a| a == principal);
        if method == "core.grant.issue" {
            let child = &p["payload"];
            if child["audience"] != self.provider_id {
                return Err(invalid("/payload/audience"));
            }
            if child["expires_at"]
                .as_str()
                .is_some_and(|s| s <= self.now.as_str())
            {
                return Err(invalid("/payload/expires_at"));
            }
            if child.get("authority_binding").is_some()
                && child["authority_binding"]["scope"] != "core-test"
                && child["authority_binding"]["scope"]
                    != format!("execution.controller:{}", self.execution_host())
            {
                return Err(invalid("/payload/authority_binding/scope"));
            }
            if !list(&child["constraints"]).is_empty() {
                return Err(invalid("/payload/constraints/0/kind"));
            }
            if let Some(parent) = child["parent"].as_str() {
                let parent = self.usable_grant(session, parent)?;
                if parent["delegation"]["allowed"] != true
                    || num(&parent["delegation"]["max_depth"]) < 1
                    || list(&child["rights"])
                        .iter()
                        .any(|r| !list(&parent["rights"]).contains(r))
                    || list(&child["resources"])
                        .iter()
                        .any(|r| !list(&parent["resources"]).iter().any(|p| subset(r, p)))
                    || parent["expires_at"]
                        .as_str()
                        .is_some_and(|p| child["expires_at"].as_str().is_none_or(|c| c > p))
                    || num(&child["delegation"]["max_depth"])
                        >= num(&parent["delegation"]["max_depth"])
                    || (parent.get("authority_binding").is_some()
                        && parent["authority_binding"] != child["authority_binding"])
                    || list(&parent["constraints"])
                        .iter()
                        .any(|c| !list(&child["constraints"]).contains(c))
                {
                    return Err(denied("delegation_exceeded"));
                }
            } else if !authority {
                return Err(denied("not_authority"));
            }
            return Ok(());
        }
        if method == "core.grant.revoke" {
            let grant = self.grant(text(&p["subject"]["id"]));
            if !authority && grant.is_none_or(|g| g["issuer"] != principal) {
                return Err(denied("not_authority"));
            }
            if grant.is_some_and(|g| g["state"] == "revoked") {
                return Err(denied("revoked"));
            }
            return Ok(());
        }
        if matches!(
            method,
            "core.grant.get" | "core.capabilities" | "core.events.unsubscribe"
        ) {
            return Ok(());
        }
        if p.get("grant").is_none() {
            return if authority {
                Ok(())
            } else {
                Err(denied("grant_required"))
            };
        }
        let grant = self.usable_grant(session, text(&p["grant"]))?;
        let mut needed = vec![];
        match method {
            "execution.submit"
            | "execution.cancel"
            | "execution.controller.claim"
            | "execution.workspace.checkpoint" => needed.push((method, p["subject"].clone())),
            "execution.inspect" | "execution.output.read" => needed.push((
                "execution.read",
                json!({"kind":"execution.execution","id":p["payload"]["execution"]}),
            )),
            "execution.discovery.list" => needed.push((
                "execution.discovery.list",
                json!({"kind":"execution.discovery","id":"installations"}),
            )),
            "execution.reconcile" => {
                if !list(&grant["rights"]).contains(&json!("execution.read")) {
                    return Err(denied("right_missing"));
                }
            }
            "core-test.authority.claim" => needed.push(("core-test.claim", p["subject"].clone())),
            "core-test.subject.put" => {
                needed.push(("core-test.write", p["subject"].clone()));
                for c in list(&p["preconditions"]) {
                    if c["subject"] != p["subject"] {
                        needed.push(("core-test.read", c["subject"].clone()))
                    }
                }
            }
            "core-test.subject.get" | "core-test.subject.applied_count" => {
                needed.push(("core-test.read", p["payload"]["subject"].clone()))
            }
            "core.effects.get" | "core.effects.abort_obligation" => {
                let id = if method == "core.effects.get" {
                    text(&p["payload"]["effect"])
                } else {
                    text(&p["subject"]["id"])
                };
                let target = self.data.effects.get(id).map(|r| &r["effect"]["target"]);
                // Neither target existence nor its kind may change the denial seen
                // by a principal that cannot read/act on it.
                let allowed = target.is_some_and(|target| {
                    let right = if method == "core.effects.abort_obligation" {
                        "core.effects.abort_obligation"
                    } else {
                        match text(&target["kind"]) {
                            "core-test.subject" | "core-test.authority" => "core-test.read",
                            "execution.execution" | "execution.controller" => "execution.read",
                            _ => return false,
                        }
                    };
                    list(&grant["rights"]).contains(&json!(right))
                        && list(&grant["resources"]).iter().any(|r| covers(r, target))
                });
                return if allowed {
                    Ok(())
                } else {
                    Err(denied("out_of_scope"))
                };
            }
            "core.events.read" | "core.events.subscribe" => {
                if !list(&grant["rights"]).contains(&json!("core.events.read")) {
                    return Err(denied("right_missing"));
                }
            }
            _ => return Err(denied("right_missing")),
        }
        if needed
            .iter()
            .any(|(r, _)| !list(&grant["rights"]).contains(&json!(r)))
        {
            return Err(denied("right_missing"));
        }
        if needed
            .iter()
            .any(|(_, s)| !list(&grant["resources"]).iter().any(|r| covers(r, s)))
        {
            return Err(denied("out_of_scope"));
        }
        Ok(())
    }
    pub fn visible(&self, session: &Session, p: &Value, subject: &Value) -> bool {
        if subject["kind"] == "core.effect" {
            return self
                .data
                .effects
                .get(text(&subject["id"]))
                .is_some_and(|r| {
                    !r.is_null()
                        && r["effect"]["target"]["kind"] != "core.effect"
                        && self.visible(session, p, &r["effect"]["target"])
                });
        }
        let principal = session.principal.as_deref().unwrap_or("");
        let scoped = p.get("grant").is_some() && !text(&p["operation"]).starts_with("core.grant.");
        if !scoped && self.authorities.iter().any(|a| a == principal) {
            return true;
        }
        if subject["kind"] == "core.grant" {
            return self
                .grant(text(&subject["id"]))
                .is_some_and(|g| g["holder"] == principal || g["issuer"] == principal);
        }
        if !scoped {
            return false;
        }
        let Some(grant) = self.grant(text(&p["grant"])) else {
            return false;
        };
        let covered = list(&grant["resources"]).iter().any(|r| covers(r, subject));
        match text(&subject["kind"]) {
            "core-test.subject" | "core-test.authority" => {
                covered && list(&grant["rights"]).contains(&json!("core-test.read"))
            }
            "core.capabilities" => covered,
            "execution.execution" | "execution.controller" => {
                covered && list(&grant["rights"]).contains(&json!("execution.read"))
            }
            _ => false,
        }
    }
    pub fn grant_get(&self, session: &Session, p: &Value) -> Reply {
        let grant = self
            .grant(text(&p["payload"]["grant"]))
            .filter(|g| {
                self.authorities
                    .contains(session.principal.as_ref().unwrap())
                    || g["holder"] == session.principal.as_deref().unwrap()
                    || g["issuer"] == session.principal.as_deref().unwrap()
            })
            .ok_or_else(|| err("not_found", json!({})))?;
        let revision =
            self.data.subjects[&key(&json!({"kind":"core.grant","id":grant["id"]}))].revision;
        Ok(json!({"grant":grant,"revision":revision}))
    }
    pub fn grant_apply(&mut self, session: &Session, method: &str, p: &Value) -> Reply {
        let subject = p["subject"].clone();
        let id = text(&subject["id"]).to_owned();
        let operation_ref = Uuid::new_v4().to_string();
        let outcome = if method == "core.grant.issue" {
            let mut record = p["payload"].clone();
            record["id"] = id.clone().into();
            record["issuer"] = session.principal.clone().unwrap().into();
            record["state"] = "active".into();
            self.data.subjects.insert(
                key(&subject),
                Subject {
                    subject: subject.clone(),
                    revision: 1,
                    state: json!({"grant":record}),
                    applied: 1,
                },
            );
            json!({"grant":record})
        } else {
            let mut descendants = vec![id.clone()];
            loop {
                let next: Vec<_> = self
                    .data
                    .subjects
                    .values()
                    .filter(|s| {
                        s.subject["kind"] == "core.grant"
                            && descendants.contains(&text(&s.state["grant"]["parent"]).to_owned())
                            && !descendants.contains(&text(&s.subject["id"]).to_owned())
                    })
                    .map(|s| text(&s.subject["id"]).to_owned())
                    .collect();
                if next.is_empty() {
                    break;
                }
                descendants.extend(next);
            }
            let mut revoked = vec![];
            for id in descendants {
                let s = self
                    .data
                    .subjects
                    .get_mut(&key(&json!({"kind":"core.grant","id":id})))
                    .ok_or_else(|| err("not_found", json!({})))?;
                if s.state["grant"]["state"] != "revoked" {
                    s.state["grant"]["state"] = "revoked".into();
                    s.revision += 1;
                    revoked.push(id)
                }
            }
            json!({"revoked":revoked})
        };
        let revision = self.data.subjects[&key(&subject)].revision;
        Ok(
            json!({"acknowledgment":{"command_id":p["command_id"],"command_digest":p["command_digest"],"operation_ref":operation_ref,"subject":subject,"revision":revision,"effect_refs":[]},"outcome":outcome,"replay":false}),
        )
    }
}
