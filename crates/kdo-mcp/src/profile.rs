//! Agent profiles — tune kdo's MCP server behavior to the quirks of specific
//! coding agents.
//!
//! Different agents have different tolerance for tool-description length,
//! context-bundle size, and loop patterns. The `AgentProfile` enum carries
//! these per-agent tuning parameters end-to-end from the `--agent` CLI flag
//! through every tool handler.
//!
//! All parameters have sensible defaults in [`AgentProfile::Generic`] so any
//! MCP-spec-compliant client works without a dedicated profile.

use std::fmt;
use std::str::FromStr;

/// Which agent (if any) this MCP server is tuned for.
///
/// Set via `kdo serve --agent <claude|openclaw|generic>` or defaulted to
/// [`AgentProfile::Generic`] when the flag is omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AgentProfile {
    /// Claude Code — default budget, standard MCP spec, Anthropic tool defs.
    Claude,
    /// OpenClaw — tighter budget, aggressive loop detection, shorter tool
    /// descriptions (OpenClaw pays a per-turn tool-definition overhead).
    OpenClaw,
    /// Everything else — Cursor, Zed, Cline, Aider, Continue, etc.
    #[default]
    Generic,
}

impl AgentProfile {
    /// Default `budget` passed to `kdo_get_context` when the agent omits it.
    pub fn default_context_budget(self) -> usize {
        match self {
            Self::Claude => 4096,
            Self::OpenClaw => 2048,
            Self::Generic => 3072,
        }
    }

    /// Sliding-window size for [`crate::guards::LoopGuard`]. Tighter windows
    /// detect loops faster at the cost of false positives.
    pub fn loop_detection_window(self) -> usize {
        match self {
            Self::Claude => 5,
            Self::OpenClaw => 3,
            Self::Generic => 5,
        }
    }

    /// Max tokens returned from a single tool call. Enforced by truncation +
    /// a visible `[truncated]` marker so agents see the cap.
    pub fn max_tool_output_tokens(self) -> usize {
        match self {
            Self::Claude => 10_000,
            Self::OpenClaw => 4_000,
            Self::Generic => 8_000,
        }
    }

    /// Whether to prefer short, example-free tool descriptions. OpenClaw
    /// includes tool descriptions in every turn, so shorter saves real tokens.
    pub fn prefers_short_descriptions(self) -> bool {
        matches!(self, Self::OpenClaw)
    }

    /// Server-level instruction string advertised at `initialize`.
    pub fn instructions(self) -> &'static str {
        match self {
            Self::OpenClaw => concat!(
                "Workspace manager for OpenClaw. ",
                "Call kdo_list_projects once to orient, then kdo_get_context ",
                "for the project you're editing. Avoid repeated identical tool ",
                "calls — the server will error on the third duplicate."
            ),
            Self::Claude | Self::Generic => concat!(
                "Context-native workspace manager. ",
                "Use kdo_list_projects first to orient, then kdo_get_context for a ",
                "specific project within a token budget. Use kdo_read_symbol only when ",
                "you need a specific function body. Pre-generated context files are ",
                "also available as resources (kdo://context/<project>)."
            ),
        }
    }
}

impl fmt::Display for AgentProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Claude => "claude",
            Self::OpenClaw => "openclaw",
            Self::Generic => "generic",
        })
    }
}

impl FromStr for AgentProfile {
    type Err = UnknownAgent;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Ok(Self::Claude),
            "openclaw" | "open-claw" => Ok(Self::OpenClaw),
            "generic" | "default" | "" => Ok(Self::Generic),
            other => Err(UnknownAgent(other.to_string())),
        }
    }
}

/// Returned when the `--agent` flag can't be matched to a known profile.
#[derive(Debug, thiserror::Error)]
#[error("unknown agent profile: '{0}' (expected claude, openclaw, or generic)")]
pub struct UnknownAgent(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_profiles() {
        assert_eq!(
            "claude".parse::<AgentProfile>().unwrap(),
            AgentProfile::Claude
        );
        assert_eq!(
            "Claude-Code".parse::<AgentProfile>().unwrap(),
            AgentProfile::Claude
        );
        assert_eq!(
            "openclaw".parse::<AgentProfile>().unwrap(),
            AgentProfile::OpenClaw
        );
        assert_eq!(
            "Open-Claw".parse::<AgentProfile>().unwrap(),
            AgentProfile::OpenClaw
        );
        assert_eq!(
            "generic".parse::<AgentProfile>().unwrap(),
            AgentProfile::Generic
        );
        assert_eq!("".parse::<AgentProfile>().unwrap(), AgentProfile::Generic);
    }

    #[test]
    fn parse_unknown_returns_error() {
        assert!("chatgpt".parse::<AgentProfile>().is_err());
    }

    #[test]
    fn openclaw_has_tighter_params() {
        assert!(
            AgentProfile::OpenClaw.default_context_budget()
                < AgentProfile::Claude.default_context_budget()
        );
        assert!(
            AgentProfile::OpenClaw.loop_detection_window()
                < AgentProfile::Claude.loop_detection_window()
        );
        assert!(
            AgentProfile::OpenClaw.max_tool_output_tokens()
                < AgentProfile::Claude.max_tool_output_tokens()
        );
        assert!(AgentProfile::OpenClaw.prefers_short_descriptions());
        assert!(!AgentProfile::Claude.prefers_short_descriptions());
    }

    #[test]
    fn display_round_trips_through_fromstr() {
        for p in [
            AgentProfile::Claude,
            AgentProfile::OpenClaw,
            AgentProfile::Generic,
        ] {
            let s = p.to_string();
            let parsed: AgentProfile = s.parse().unwrap();
            assert_eq!(p, parsed);
        }
    }
}
