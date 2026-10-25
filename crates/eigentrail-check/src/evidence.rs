use eigentrail_core::{serialization::digest, *};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub source: String,
    pub specification: String,
    pub fixtures: String,
    pub checker: String,
    pub dependencies: String,
    pub platform: String,
    pub execution: String,
    pub input_hash: String,
    pub report_hash: String,
    pub commands: Vec<String>,
    pub log_hash: String,
    pub completed: bool,
    pub outcome: String,
}
impl Receipt {
    pub fn matches(&self, expected: &Self) -> Result<()> {
        if !self.completed || self.outcome != "passed" || digest(self)? != digest(expected)? {
            return Err(Error::Invalid(
                "receipt does not match independently replayed execution".into(),
            ));
        }
        Ok(())
    }
}
/// Edges point dependent -> prerequisite. Return affected implementation records only.
pub fn invalidate(
    changed: &BTreeSet<String>,
    requires: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<String> {
    let mut stale = changed.clone();
    loop {
        let old = stale.len();
        for (id, deps) in requires {
            if id.starts_with("ET:") && deps.iter().any(|d| stale.contains(d)) {
                stale.insert(id.clone());
            }
        }
        if stale.len() == old {
            return stale;
        }
    }
}
