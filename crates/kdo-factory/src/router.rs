//! Pick a provider for a model id.
//!
//! `KDO_FACTORY_MOCK=1` forces the mock connector. With no credentials
//! configured at all, the mock is also the fallback so `kdo factory tick`
//! works offline. If any key is set and the chosen model's key is missing,
//! the call fails.

use crate::connector::{
    AnthropicConnector, Completion, CompletionRequest, Connector, MockConnector,
};
use crate::error::{FactoryError, FactoryResult};
use crate::keys::KeyStore;
use crate::openai::OpenAiConnector;
use crate::plugin::{PluginSet, Protocol, ProviderPlugin};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

struct LiveRoute {
    plugin: ProviderPlugin,
    backend: Option<Arc<dyn Connector>>,
}

struct Router {
    force_mock: bool,
    allow_mock_fallback: bool,
    routes: Vec<LiveRoute>,
    mock: MockConnector,
}

pub fn build_connector(workspace: &Path) -> FactoryResult<Arc<dyn Connector>> {
    let force_mock = std::env::var("KDO_FACTORY_MOCK")
        .ok()
        .is_some_and(|value| value == "1");
    let plugins = PluginSet::load(workspace)?;
    let keys = KeyStore::load_default()?;
    Ok(build_connector_from(force_mock, plugins, &keys))
}

pub fn build_connector_from(
    force_mock: bool,
    plugins: PluginSet,
    keys: &KeyStore,
) -> Arc<dyn Connector> {
    let allow_mock_fallback = !plugins
        .providers()
        .iter()
        .any(|provider| keys.resolve(&provider.name, &provider.api_key_env).is_ok());
    let routes = plugins
        .providers()
        .iter()
        .cloned()
        .map(|plugin| {
            let backend = keys.resolve(&plugin.name, &plugin.api_key_env).ok().map(
                |secret| -> Arc<dyn Connector> {
                    match plugin.protocol {
                        Protocol::Anthropic => Arc::new(
                            AnthropicConnector::new(secret).with_base_url(plugin.base_url.clone()),
                        ),
                        Protocol::Openai => {
                            Arc::new(OpenAiConnector::new(secret, plugin.base_url.clone()))
                        }
                    }
                },
            );
            LiveRoute { plugin, backend }
        })
        .collect();
    Arc::new(Router {
        force_mock,
        allow_mock_fallback,
        routes,
        mock: MockConnector,
    })
}

fn route_matches(plugin: &ProviderPlugin, model: &str) -> bool {
    if plugin.models.iter().any(|candidate| candidate == model) {
        return true;
    }
    if !plugin.models.is_empty() {
        return false;
    }
    match plugin.name.as_str() {
        "anthropic" => model.starts_with("claude"),
        "openai" => {
            model.starts_with("gpt-")
                || model.starts_with("o1")
                || model.starts_with("o3")
                || model.starts_with("o4")
                || model.starts_with("chatgpt")
        }
        _ => false,
    }
}

#[async_trait]
impl Connector for Router {
    async fn complete(&self, req: CompletionRequest<'_>) -> FactoryResult<Completion> {
        if self.force_mock {
            return self.mock.complete(req).await;
        }
        let Some(route) = self
            .routes
            .iter()
            .rev()
            .find(|route| route_matches(&route.plugin, req.model))
        else {
            if self.allow_mock_fallback {
                return self.mock.complete(req).await;
            }
            return Err(FactoryError::UnknownProvider(req.model.to_string()));
        };
        match &route.backend {
            Some(backend) => backend.complete(req).await,
            None if self.allow_mock_fallback => self.mock.complete(req).await,
            None => Err(FactoryError::MissingCredential(route.plugin.name.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Plugin, Protocol};

    fn demo_set() -> PluginSet {
        let mut set = PluginSet::builtin();
        set.apply(Plugin::Provider(ProviderPlugin {
            name: "demo".into(),
            protocol: Protocol::Openai,
            base_url: "https://example.invalid".into(),
            api_key_env: "KDO_FACTORY_TEST_KEY_UNSET".into(),
            models: vec!["demo-model".into()],
        }));
        set
    }

    #[tokio::test]
    async fn missing_key_errors_when_another_key_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        std::fs::write(&path, "[keys]\nanthropic = \"sk-ant-test-value\"\n").unwrap();
        let keys = KeyStore::load(&path).unwrap();
        let connector = build_connector_from(false, demo_set(), &keys);
        let err = connector
            .complete(CompletionRequest {
                model: "demo-model",
                system: "sys",
                user: "user",
                max_tokens: 16,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, FactoryError::MissingCredential(_)));
        assert!(!err.to_string().contains("sk-ant"));
    }

    #[tokio::test]
    async fn no_keys_falls_back_to_mock() {
        let keys = KeyStore::load(Path::new("/this/file/does/not/exist-kdo-credentials")).unwrap();
        let connector = build_connector_from(false, PluginSet::builtin(), &keys);
        let completion = connector
            .complete(CompletionRequest {
                model: "claude-sonnet-4-6",
                system: "plan",
                user: "go",
                max_tokens: 16,
            })
            .await
            .unwrap();
        assert!(completion.text.contains("done"));
    }
}
