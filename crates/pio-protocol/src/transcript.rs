//! Test tooling: validate public matrix response payloads against unchanged schemas.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::BufRead, path::Path};
pub fn check(path: &Path) -> Result<usize> {
    let resources = crate::schemas::resources();
    let registry = jsonschema::Registry::new()
        .extend(
            resources
                .iter()
                .map(|v| (v["$id"].as_str().unwrap().to_owned(), v.clone())),
        )?
        .prepare()?;
    let mut validators = BTreeMap::new();
    for resource in &resources {
        let id = resource["$id"].as_str().unwrap();
        if let Some(method) = id
            .rsplit('/')
            .next()
            .and_then(|s| s.strip_suffix(".result.schema.json"))
        {
            validators.insert(
                method.to_owned(),
                jsonschema::options()
                    .with_registry(&registry)
                    .build(&json!({"$ref":id}))?,
            );
        }
    }
    let mut count = 0;
    for line in std::io::BufReader::new(std::fs::File::open(path)?).lines() {
        let record: Value = serde_json::from_str(&line?)?;
        if let Some(result) = record["response"].get("result") {
            let method = record["request"]["method"].as_str().context("method")?;
            let validator = validators
                .get(method)
                .context("unregistered result schema")?;
            ensure!(
                validator.is_valid(result),
                "{method} schema: {}",
                validator
                    .iter_errors(result)
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            count += 1;
        }
    }
    Ok(count)
}
#[cfg(test)]
mod tests {
    #[test]
    fn pinned_inspect_cannot_express_kernel_identity_without_a_protocol_change() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/proposals/process-identity.json"
        ))
        .unwrap();
        let resources = crate::schemas::resources();
        let registry = jsonschema::Registry::new()
            .extend(
                resources
                    .iter()
                    .map(|v| (v["$id"].as_str().unwrap().to_owned(), v.clone())),
            )
            .unwrap()
            .prepare()
            .unwrap();
        let validator=jsonschema::options().with_registry(&registry).build(&serde_json::json!({"$ref":"https://github.com/Combraton/protocol/schemas/execution/1/execution.inspect.result.schema.json"})).unwrap();
        assert!(validator.is_valid(&fixture["current"]));
        assert!(!validator.is_valid(&fixture["proposed"]));
    }

    #[test]
    fn pinned_errors_cannot_distinguish_capacity_from_transient_commit_failure() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../conformance/proposals/capacity-refusal.json"
        ))
        .unwrap();
        let resources = crate::schemas::resources();
        let registry = jsonschema::Registry::new()
            .extend(
                resources
                    .iter()
                    .map(|v| (v["$id"].as_str().unwrap().to_owned(), v.clone())),
            )
            .unwrap()
            .prepare()
            .unwrap();
        let validator = jsonschema::options()
            .with_registry(&registry)
            .build(&serde_json::json!({"$ref":"https://github.com/Combraton/protocol/schemas/core/1/error-data.schema.json"}))
            .unwrap();
        let capacity = &fixture["observed_capacity_refusal"];
        let transient = &fixture["observed_transient_commit_failure"];
        assert!(validator.is_valid(&capacity["data"]));
        assert!(validator.is_valid(&transient["data"]));
        assert_eq!(capacity, transient);
        // The frame PIO emits for a capacity refusal is exactly this object.
        let emitted = crate::provider::err("unavailable", serde_json::json!({}))
            .frame(serde_json::json!(1))["error"]
            .clone();
        assert_eq!(&emitted, capacity);
        assert!(!validator.is_valid(&fixture["proposed"]["data"]));
    }
}
