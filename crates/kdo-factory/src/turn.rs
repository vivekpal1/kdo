//! One model turn is a single JSON object.
//!
//! Planner and implementer may also return plain text, which is treated as
//! `{"done": true}`. A reviewer must return a verdict.

use crate::error::{FactoryError, FactoryResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTurn {
    Done {
        summary: String,
    },
    Tool {
        name: String,
        input: serde_json::Value,
    },
    Verdict {
        pass: bool,
        notes: String,
    },
}

pub fn parse_model_turn(text: &str) -> FactoryResult<ModelTurn> {
    let Some(json) = extract_json(text) else {
        return Ok(ModelTurn::Done {
            summary: text.trim().to_string(),
        });
    };
    let value: serde_json::Value = serde_json::from_str(&json)?;
    if value.get("pass").is_some() {
        let pass = value
            .get("pass")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| FactoryError::InvalidVerdict("`pass` must be a bool".into()))?;
        let notes = value
            .get("notes")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        return Ok(ModelTurn::Verdict { pass, notes });
    }
    if let Some(name) = value.get("tool").and_then(serde_json::Value::as_str) {
        let input = value
            .get("input")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        return Ok(ModelTurn::Tool {
            name: name.to_string(),
            input,
        });
    }
    if value.get("done").and_then(serde_json::Value::as_bool) == Some(true) {
        let summary = value
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        return Ok(ModelTurn::Done { summary });
    }
    Ok(ModelTurn::Done {
        summary: text.trim().to_string(),
    })
}

pub fn require_verdict(text: &str) -> FactoryResult<(bool, String)> {
    match parse_model_turn(text)? {
        ModelTurn::Verdict { pass, notes } => Ok((pass, notes)),
        _ => Err(FactoryError::InvalidVerdict(clip(text, 200))),
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// Pull the first balanced `{...}` object out of a reply, ignoring fences.
fn extract_json(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim_start())
        .unwrap_or(trimmed);
    let start = stripped.find('{')?;
    let bytes = &stripped.as_bytes()[start..];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(stripped[start..start + index + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_done_tool_and_verdict() {
        assert_eq!(
            parse_model_turn("{\"done\": true, \"summary\": \"ship it\"}").unwrap(),
            ModelTurn::Done {
                summary: "ship it".into()
            }
        );
        match parse_model_turn("```json\n{\"tool\":\"read\",\"input\":{\"path\":\"a.rs\"}}\n```")
            .unwrap()
        {
            ModelTurn::Tool { name, input } => {
                assert_eq!(name, "read");
                assert_eq!(input["path"], "a.rs");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            require_verdict("notes first {\"pass\": false, \"notes\": \"no test\"}").unwrap(),
            (false, "no test".into())
        );
    }

    #[test]
    fn plain_text_is_done_but_not_a_verdict() {
        assert!(matches!(
            parse_model_turn("just a plan").unwrap(),
            ModelTurn::Done { .. }
        ));
        assert!(require_verdict("looks fine").is_err());
    }
}
