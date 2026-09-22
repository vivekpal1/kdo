//! Spec language — the YAML the developer commits.
//!
//! Minimal v1: `Feature` kind only. Future kinds (Bugfix, Refactor,
//! Release, Memory) slot in via the `kind` enum.

use crate::error::{FactoryError, FactoryResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecDocument {
    pub kind: String,
    pub metadata: SpecMetadata,
    pub spec: SpecBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecMetadata {
    pub name: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecBody {
    pub description: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub agents: AgentAssignments,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u64,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_max_cost")]
    pub max_cost_usd: f64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_tokens: default_max_tokens(),
            max_iterations: default_max_iterations(),
            max_cost_usd: default_max_cost(),
        }
    }
}

fn default_max_tokens() -> u64 {
    200_000
}
fn default_max_iterations() -> u32 {
    8
}
fn default_max_cost() -> f64 {
    15.0
}

/// Per-role model assignment. Empty role = role is skipped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentAssignments {
    #[serde(default)]
    pub planner: Option<String>,
    #[serde(default)]
    pub implementer: Option<String>,
    #[serde(default)]
    pub reviewer: Option<String>,
    #[serde(default)]
    pub tester: Option<String>,
}

impl AgentAssignments {
    /// Default assignment when the spec leaves `agents:` empty.
    pub fn or_defaults(self) -> Self {
        Self {
            planner: self.planner.or_else(|| Some("claude-opus-4-7".into())),
            implementer: self
                .implementer
                .or_else(|| Some("claude-sonnet-4-6".into())),
            reviewer: self.reviewer.or_else(|| Some("claude-sonnet-4-6".into())),
            tester: self.tester.or_else(|| Some("claude-haiku-4-5".into())),
        }
    }
}

pub fn parse_spec(yaml: &str) -> FactoryResult<SpecDocument> {
    let doc: SpecDocument = serde_yml::from_str(yaml)?;
    if doc.kind.is_empty() {
        return Err(FactoryError::InvalidSpec("missing `kind`".into()));
    }
    if doc.metadata.name.is_empty() {
        return Err(FactoryError::InvalidSpec("missing `metadata.name`".into()));
    }
    if doc.spec.description.is_empty() {
        return Err(FactoryError::InvalidSpec(
            "missing `spec.description`".into(),
        ));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_spec() {
        let yaml = r#"
kind: Feature
metadata:
  name: hello
spec:
  description: |
    Print hello to stdout.
"#;
        let doc = parse_spec(yaml).unwrap();
        assert_eq!(doc.kind, "Feature");
        assert_eq!(doc.metadata.name, "hello");
        assert!(doc.spec.acceptance.is_empty());
    }

    #[test]
    fn parses_full_spec() {
        let yaml = r#"
kind: Feature
metadata:
  name: vault-pause
  project: vault-program
spec:
  description: Add emergency pause.
  acceptance:
    - pause() callable by authorities
    - unpause() admin-only
  budget:
    max_tokens: 100000
    max_iterations: 5
    max_cost_usd: 10.0
  agents:
    planner: claude-opus-4-7
    implementer: claude-sonnet-4-6
"#;
        let doc = parse_spec(yaml).unwrap();
        assert_eq!(doc.metadata.project.as_deref(), Some("vault-program"));
        assert_eq!(doc.spec.acceptance.len(), 2);
        assert_eq!(doc.spec.budget.max_iterations, 5);
        assert_eq!(
            doc.spec.agents.implementer.as_deref(),
            Some("claude-sonnet-4-6")
        );
    }

    #[test]
    fn rejects_missing_description() {
        let yaml = r#"
kind: Feature
metadata:
  name: bad
spec:
  description: ""
"#;
        assert!(parse_spec(yaml).is_err());
    }
}
