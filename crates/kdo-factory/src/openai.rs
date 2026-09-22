//! OpenAI-compatible chat completions.
//!
//! `base_url` is the prefix before `/chat/completions`
//! (`https://api.openai.com/v1`, `https://api.deepseek.com`).

use crate::connector::{estimate_cost, Completion, CompletionRequest, Connector};
use crate::error::{FactoryError, FactoryResult};
use crate::keys::redact;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct OpenAiConnector {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAiConnector {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn chat_url(&self) -> String {
        chat_url(&self.base_url)
    }
}

pub fn chat_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

pub fn parse_openai_body(body: &str, model: &str) -> FactoryResult<Completion> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    let text = message_text(&value);
    let tokens_in = value["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
    let tokens_out = value["usage"]["completion_tokens"]
        .as_u64()
        .unwrap_or((text.len() as u64) / 4);
    Ok(Completion {
        text,
        tokens_in,
        tokens_out,
        cost_usd: estimate_cost(model, tokens_in, tokens_out),
    })
}

fn message_text(value: &serde_json::Value) -> String {
    let content = &value["choices"][0]["message"]["content"];
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
}

#[async_trait]
impl Connector for OpenAiConnector {
    async fn complete(&self, req: CompletionRequest<'_>) -> FactoryResult<Completion> {
        let body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user},
            ],
        });
        let response = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        let secrets = vec![self.api_key.clone()];
        if !status.is_success() {
            return Err(FactoryError::Connector(redact(
                &format!("openai {status}: {text}"),
                &secrets,
            )));
        }
        let mut completion = parse_openai_body(&text, req.model)
            .map_err(|err| FactoryError::Connector(redact(&err.to_string(), &secrets)))?;
        if completion.tokens_in == 0 {
            completion.tokens_in = (req.system.len() + req.user.len()) as u64 / 4;
        }
        completion.text = redact(&completion.text, &secrets);
        Ok(completion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_chat_url_and_parses_string_content() {
        assert_eq!(
            chat_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        let completion = parse_openai_body(
            r#"{"choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
            "gpt-4o",
        )
        .unwrap();
        assert_eq!(completion.text, "hello");
        assert_eq!(completion.tokens_in, 3);
        assert_eq!(completion.tokens_out, 1);
    }
}
