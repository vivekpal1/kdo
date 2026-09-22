//! TOML plugins. Two kinds: providers and micro-agents.
//!
//! Built-ins load first. `~/.kdo/plugins/*.toml` overrides them, then
//! `<workspace>/.kdo/plugins/*.toml`. A provider override matches by name.
//! An agent override matches by role. Native code is not loaded.

use crate::error::{FactoryError, FactoryResult};
use crate::keys::home_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Anthropic,
    Openai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolName {
    Graph,
    Read,
    Write,
    Diff,
    Run,
}

impl ToolName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Read => "read",
            Self::Write => "write",
            Self::Diff => "diff",
            Self::Run => "run",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "graph" => Self::Graph,
            "read" => Self::Read,
            "write" => Self::Write,
            "diff" => Self::Diff,
            "run" => Self::Run,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPlugin {
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub api_key_env: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPlugin {
    pub name: String,
    pub role: String,
    pub model: String,
    pub tools: Vec<ToolName>,
    pub system: String,
    /// When true, the reconciler runs the workspace `test` task and does
    /// not call a model. Plugin agents default this to false.
    pub command_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plugin {
    Provider(ProviderPlugin),
    Agent(AgentPlugin),
}

#[derive(Debug, Clone)]
pub struct PluginSet {
    providers: Vec<ProviderPlugin>,
    agents: Vec<AgentPlugin>,
}

impl PluginSet {
    pub fn builtin() -> Self {
        Self {
            providers: vec![
                ProviderPlugin {
                    name: "anthropic".into(),
                    protocol: Protocol::Anthropic,
                    base_url: "https://api.anthropic.com/v1".into(),
                    api_key_env: "ANTHROPIC_API_KEY".into(),
                    models: Vec::new(),
                },
                ProviderPlugin {
                    name: "openai".into(),
                    protocol: Protocol::Openai,
                    base_url: "https://api.openai.com/v1".into(),
                    api_key_env: "OPENAI_API_KEY".into(),
                    models: Vec::new(),
                },
            ],
            agents: vec![
                AgentPlugin {
                    name: "planner".into(),
                    role: "planner".into(),
                    model: "claude-opus-4-7".into(),
                    tools: vec![ToolName::Graph, ToolName::Read],
                    system: "You are the Planner micro-agent. Tools: graph, read. \
                        Do not write files. When finished return only \
                        {\"done\": true, \"summary\": \"...\"}. \
                        A tool call is {\"tool\": \"read\", \"input\": {\"path\": \"rel\"}}."
                        .into(),
                    command_only: false,
                },
                AgentPlugin {
                    name: "implementer".into(),
                    role: "implementer".into(),
                    model: "claude-sonnet-4-6".into(),
                    tools: vec![
                        ToolName::Graph,
                        ToolName::Read,
                        ToolName::Write,
                        ToolName::Diff,
                    ],
                    system:
                        "You are the Implementer micro-agent. Tools: graph, read, write, diff. \
                        write input is {\"path\": \"rel\", \"contents\": \"full file\"}. \
                        Paths stay inside the worktree. When finished return only \
                        {\"done\": true, \"summary\": \"...\"}."
                            .into(),
                    command_only: false,
                },
                AgentPlugin {
                    name: "tester".into(),
                    role: "tester".into(),
                    model: String::new(),
                    tools: vec![ToolName::Run],
                    system: String::new(),
                    command_only: true,
                },
                AgentPlugin {
                    name: "reviewer".into(),
                    role: "reviewer".into(),
                    model: "claude-sonnet-4-6".into(),
                    tools: vec![ToolName::Graph, ToolName::Read, ToolName::Diff],
                    system: "You are the Reviewer micro-agent. Tools: graph, read, diff. \
                        Return only {\"pass\": true, \"notes\": \"...\"} or \
                        {\"pass\": false, \"notes\": \"...\"}. \
                        pass is true only when every acceptance criterion is met."
                        .into(),
                    command_only: false,
                },
            ],
        }
    }

    pub fn load(workspace: &Path) -> FactoryResult<Self> {
        let home = home_dir().map(|dir| dir.join(".kdo").join("plugins"));
        Self::load_dirs(home.as_deref(), &workspace.join(".kdo").join("plugins"))
    }

    pub fn load_dirs(home_plugins: Option<&Path>, workspace_plugins: &Path) -> FactoryResult<Self> {
        let mut set = Self::builtin();
        if let Some(dir) = home_plugins {
            load_dir(dir, &mut set)?;
        }
        load_dir(workspace_plugins, &mut set)?;
        Ok(set)
    }

    pub fn apply(&mut self, plugin: Plugin) {
        match plugin {
            Plugin::Provider(provider) => {
                self.providers
                    .retain(|existing| existing.name != provider.name);
                self.providers.push(provider);
            }
            Plugin::Agent(agent) => {
                self.agents.retain(|existing| existing.role != agent.role);
                self.agents.push(agent);
            }
        }
    }

    pub fn providers(&self) -> &[ProviderPlugin] {
        &self.providers
    }

    pub fn agents(&self) -> &[AgentPlugin] {
        &self.agents
    }

    pub fn len(&self) -> usize {
        self.providers.len() + self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty() && self.agents.is_empty()
    }

    pub fn agent_for_role(&self, role: &str) -> Option<&AgentPlugin> {
        self.agents.iter().rev().find(|agent| agent.role == role)
    }

    /// Exact model id wins. Empty `models` means the built-in prefix.
    pub fn provider_for_model(&self, model: &str) -> Option<&ProviderPlugin> {
        if let Some(exact) = self
            .providers
            .iter()
            .rev()
            .find(|provider| provider.models.iter().any(|candidate| candidate == model))
        {
            return Some(exact);
        }
        self.providers
            .iter()
            .rev()
            .find(|provider| provider.models.is_empty() && prefix_match(provider, model))
    }
}

fn prefix_match(provider: &ProviderPlugin, model: &str) -> bool {
    match provider.name.as_str() {
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

pub fn parse_plugin(text: &str) -> FactoryResult<Plugin> {
    let value: toml::Value =
        toml::from_str(text).map_err(|err| FactoryError::Plugin(err.to_string()))?;
    let meta = value
        .get("plugin")
        .ok_or_else(|| FactoryError::Plugin("missing [plugin]".into()))?;
    let name = required_str(meta, "plugin.name")?;
    let kind = required_str(meta, "plugin.kind")?;
    match kind {
        "provider" => {
            let body = value
                .get("provider")
                .ok_or_else(|| FactoryError::Plugin("missing [provider]".into()))?;
            let protocol = match required_str(body, "provider.protocol")? {
                "anthropic" => Protocol::Anthropic,
                "openai" => Protocol::Openai,
                other => return Err(FactoryError::Plugin(format!("unknown protocol `{other}`"))),
            };
            let models = match body.get("models") {
                None => Vec::new(),
                Some(toml::Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| FactoryError::Plugin("models must be strings".into()))
                    })
                    .collect::<FactoryResult<Vec<_>>>()?,
                Some(_) => return Err(FactoryError::Plugin("models must be an array".into())),
            };
            Ok(Plugin::Provider(ProviderPlugin {
                name: name.to_string(),
                protocol,
                base_url: required_str(body, "provider.base_url")?
                    .trim_end_matches('/')
                    .to_string(),
                api_key_env: required_str(body, "provider.api_key_env")?.to_string(),
                models,
            }))
        }
        "agent" => {
            let body = value
                .get("agent")
                .ok_or_else(|| FactoryError::Plugin("missing [agent]".into()))?;
            let tools = match body.get("tools") {
                Some(toml::Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        let name = item
                            .as_str()
                            .ok_or_else(|| FactoryError::Plugin("tools must be strings".into()))?;
                        ToolName::parse(name)
                            .ok_or_else(|| FactoryError::Plugin(format!("unknown tool `{name}`")))
                    })
                    .collect::<FactoryResult<Vec<_>>>()?,
                _ => return Err(FactoryError::Plugin("agent.tools must be an array".into())),
            };
            let command_only = body
                .get("command_only")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            Ok(Plugin::Agent(AgentPlugin {
                name: name.to_string(),
                role: required_str(body, "agent.role")?.to_string(),
                model: body
                    .get("model")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                tools,
                system: body
                    .get("system")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                command_only,
            }))
        }
        other => Err(FactoryError::Plugin(format!(
            "unknown plugin kind `{other}`"
        ))),
    }
}

fn required_str<'a>(value: &'a toml::Value, label: &str) -> FactoryResult<&'a str> {
    let key = label.rsplit('.').next().unwrap_or(label);
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| FactoryError::Plugin(format!("missing {label}")))
}

fn load_dir(dir: &Path, into: &mut PluginSet) -> FactoryResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    for path in paths {
        let text = std::fs::read_to_string(&path)?;
        let plugin = parse_plugin(&text)
            .map_err(|err| FactoryError::Plugin(format!("{}: {err}", path.display())))?;
        into.apply(plugin);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_and_agent() {
        let provider = parse_plugin(
            r#"
            [plugin]
            name = "deepseek"
            kind = "provider"
            [provider]
            protocol = "openai"
            base_url = "https://api.deepseek.com/"
            api_key_env = "DEEPSEEK_API_KEY"
            models = ["deepseek-chat"]
            "#,
        )
        .unwrap();
        match provider {
            Plugin::Provider(provider) => {
                assert_eq!(provider.base_url, "https://api.deepseek.com");
                assert_eq!(provider.models, vec!["deepseek-chat".to_string()]);
            }
            Plugin::Agent(_) => panic!("expected provider"),
        }

        let agent = parse_plugin(
            r#"
            [plugin]
            name = "careful"
            kind = "agent"
            [agent]
            role = "reviewer"
            model = "deepseek-chat"
            tools = ["read", "diff"]
            system = "Be careful."
            "#,
        )
        .unwrap();
        match agent {
            Plugin::Agent(agent) => {
                assert_eq!(agent.role, "reviewer");
                assert!(!agent.command_only);
                assert_eq!(agent.tools, vec![ToolName::Read, ToolName::Diff]);
            }
            Plugin::Provider(_) => panic!("expected agent"),
        }
    }

    #[test]
    fn workspace_plugin_overrides_builtin_role() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(
            plugins.join("reviewer.toml"),
            r#"
            [plugin]
            name = "careful"
            kind = "agent"
            [agent]
            role = "reviewer"
            model = "demo-reviewer"
            tools = ["read"]
            system = "custom"
            "#,
        )
        .unwrap();
        let set = PluginSet::load_dirs(None, &plugins).unwrap();
        let reviewer = set.agent_for_role("reviewer").unwrap();
        assert_eq!(reviewer.model, "demo-reviewer");
        assert_eq!(reviewer.system, "custom");
        assert!(set.agent_for_role("planner").is_some());
    }

    #[test]
    fn model_routing_prefers_exact_id() {
        let mut set = PluginSet::builtin();
        set.apply(Plugin::Provider(ProviderPlugin {
            name: "deepseek".into(),
            protocol: Protocol::Openai,
            base_url: "https://api.deepseek.com".into(),
            api_key_env: "DEEPSEEK_API_KEY".into(),
            models: vec!["deepseek-chat".into()],
        }));
        assert_eq!(
            set.provider_for_model("deepseek-chat").unwrap().name,
            "deepseek"
        );
        assert_eq!(
            set.provider_for_model("claude-sonnet-4-6").unwrap().name,
            "anthropic"
        );
        assert_eq!(set.provider_for_model("gpt-4o").unwrap().name, "openai");
    }

    #[test]
    fn bad_plugin_file_fails_the_load() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("bad.toml"), "not toml").unwrap();
        let err = PluginSet::load_dirs(None, &plugins).unwrap_err();
        assert!(matches!(err, FactoryError::Plugin(_)));
        assert!(err.to_string().contains("bad.toml"));
    }
}
