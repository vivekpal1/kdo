//! LLM connectors. The reconciler talks to one `Connector` per task —
//! `MockConnector` for tests / no-API-key dev, `AnthropicConnector` when
//! `ANTHROPIC_API_KEY` is set.

use crate::error::{FactoryError, FactoryResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct CompletionRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    /// Hard token cap for this single call — connectors should pass through.
    pub max_tokens: u32,
}

#[async_trait]
pub trait Connector: Send + Sync {
    async fn complete(&self, req: CompletionRequest<'_>) -> FactoryResult<Completion>;
}

// --- Mock -----------------------------------------------------------------

/// Deterministic stub for tests and offline runs. Returns a fake completion
/// keyed by the role embedded in the system prompt.
#[derive(Debug, Clone, Default)]
pub struct MockConnector;

#[async_trait]
impl Connector for MockConnector {
    async fn complete(&self, req: CompletionRequest<'_>) -> FactoryResult<Completion> {
        // Reviewer prompts are the only ones that mention a `"pass"` field.
        // Everyone else finishes in one turn so tests don't need a script.
        let text = if req.system.contains("\"pass\"") {
            "{\"pass\":true,\"notes\":\"ok\"}".to_string()
        } else {
            "{\"done\":true,\"summary\":\"mock\"}".to_string()
        };
        let tokens_in = (req.system.len() + req.user.len() + req.model.len()) as u64 / 4;
        let tokens_out = text.len() as u64 / 4;
        Ok(Completion {
            text,
            tokens_in,
            tokens_out,
            cost_usd: 0.0,
        })
    }
}

// --- Anthropic ------------------------------------------------------------

/// Talks to the Anthropic Messages API. Costs are computed from public
/// per-token rates — keep in sync with `arch/06-BRING-YOUR-OWN-LLM.md`.
#[derive(Debug, Clone)]
pub struct AnthropicConnector {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicConnector {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: "https://api.anthropic.com/v1".into(),
        }
    }

    /// Override the API root. `base_url` is joined with `/messages`.
    #[must_use]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Pull the API key from `ANTHROPIC_API_KEY` if present.
    pub fn from_env() -> Option<Self> {
        std::env::var("ANTHROPIC_API_KEY").ok().map(Self::new)
    }
}

#[async_trait]
impl Connector for AnthropicConnector {
    async fn complete(&self, req: CompletionRequest<'_>) -> FactoryResult<Completion> {
        let body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": [{ "role": "user", "content": req.user }],
        });
        let url = format!("{}/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(FactoryError::Connector(format!(
                "anthropic {status}: {text}"
            )));
        }

        let v: serde_json::Value = serde_json::from_str(&text)?;
        let out_text = v["content"]
            .as_array()
            .and_then(|arr| arr.iter().find(|c| c["type"] == "text"))
            .and_then(|c| c["text"].as_str())
            .unwrap_or_default()
            .to_string();
        let tokens_in = v["usage"]["input_tokens"].as_u64().unwrap_or(0);
        let tokens_out = v["usage"]["output_tokens"].as_u64().unwrap_or(0);
        let cost = estimate_cost(req.model, tokens_in, tokens_out);
        Ok(Completion {
            text: out_text,
            tokens_in,
            tokens_out,
            cost_usd: cost,
        })
    }
}

/// Public per-million-token rates as of 2026-05. Models we don't know
/// fall back to a conservative Sonnet-tier estimate.
pub(crate) fn estimate_cost(model: &str, tokens_in: u64, tokens_out: u64) -> f64 {
    let (in_rate, out_rate) = match model {
        m if m.contains("haiku") => (0.80, 4.0),
        m if m.contains("sonnet") => (3.0, 15.0),
        m if m.contains("opus") => (15.0, 75.0),
        _ => (3.0, 15.0),
    };
    (tokens_in as f64 / 1_000_000.0) * in_rate + (tokens_out as f64 / 1_000_000.0) * out_rate
}
