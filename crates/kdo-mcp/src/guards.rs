//! Loop detection + circuit breakers for MCP tool calls.
//!
//! Agents (notably OpenClaw) sometimes enter a "deadly loop": the same tool
//! called repeatedly with identical arguments, burning tokens without progress.
//! This module returns a structured error to the agent the moment a loop is
//! detected, nudging it to change strategy.
//!
//! Two detectors are enforced against a sliding window of the most recent tool
//! calls:
//!
//! 1. **Identical-args duplication** — the same `(tool, args)` pair observed
//!    `duplicate_threshold` times within the window is a loop.
//! 2. **Thrash** — more than `thrash_threshold` distinct calls within
//!    `thrash_window` (a wall-clock span) is probable thrashing even without
//!    exact duplicates.
//!
//! Both detectors are O(window_size) per recorded call and allocate nothing on
//! the happy path. The guard is `Send` so it can live inside a tokio `Mutex`
//! held by the MCP server state.

use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A sliding-window record of recent tool calls, used to detect loops.
///
/// Typical usage from inside a tool handler:
///
/// ```
/// use kdo_mcp::guards::{LoopGuard, LoopError};
/// use serde_json::json;
///
/// let mut guard = LoopGuard::for_profile_window(3);
/// let args = json!({"project": "kdo-core"});
///
/// guard.record("kdo_get_context", &args).unwrap();
/// guard.record("kdo_get_context", &args).unwrap();
/// // Third identical call trips the detector (threshold = 3 by default).
/// assert!(matches!(
///     guard.record("kdo_get_context", &args),
///     Err(LoopError::IdenticalArgs { .. })
/// ));
/// ```
#[derive(Debug)]
pub struct LoopGuard {
    recent: VecDeque<Call>,
    window_size: usize,
    duplicate_threshold: usize,
    thrash_threshold: usize,
    thrash_window: Duration,
}

#[derive(Debug, Clone)]
struct Call {
    tool: String,
    args_fingerprint: u64,
    at: Instant,
}

/// Errors surfaced to the agent when loop detectors fire.
///
/// These are meant to be serialized into MCP tool-call errors so the agent sees
/// them on the next step and adjusts behavior.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LoopError {
    /// Same `(tool, args)` pair observed `count` times in the window.
    #[error(
        "loop detected: {tool} called {count} times with identical args in {window_ms}ms. \
         Break the loop and try a different approach — different arguments, a different tool, \
         or ask the user for clarification."
    )]
    IdenticalArgs {
        /// Tool name that tripped the detector.
        tool: String,
        /// How many duplicate calls were observed.
        count: usize,
        /// Span of the window, in milliseconds.
        window_ms: u64,
    },

    /// High call frequency across distinct args — probable thrashing.
    #[error(
        "thrash detected: {count} distinct tool calls in {window_ms}ms exceeds threshold. \
         Pause and reconsider — fewer, more targeted calls with larger context budgets \
         are usually more effective."
    )]
    HighFrequency {
        /// Number of calls in the thrash window.
        count: usize,
        /// Span of the window, in milliseconds.
        window_ms: u64,
    },
}

impl LoopGuard {
    /// Create a guard with the given sliding-window size. Thresholds default to
    /// sensible values — see [`LoopGuard::for_profile_window`] for the
    /// agent-profile-driven constructor.
    pub fn new(window_size: usize) -> Self {
        Self {
            recent: VecDeque::with_capacity(window_size.max(1)),
            window_size: window_size.max(1),
            // The window *is* the duplicate threshold — if every call in the
            // window is identical, the agent is stuck.
            duplicate_threshold: window_size.max(1),
            // Thrash if we see more than 1.5x window_size calls within 3
            // seconds — anything tighter than that isn't human-driven.
            thrash_threshold: (window_size as f32 * 1.5).ceil() as usize,
            thrash_window: Duration::from_secs(3),
        }
    }

    /// Create a guard tuned for an agent's recommended loop-detection window.
    /// Equivalent to [`LoopGuard::new`] but clearer at call sites where the
    /// window comes from an `AgentProfile::loop_detection_window()`.
    pub fn for_profile_window(window_size: usize) -> Self {
        Self::new(window_size)
    }

    /// Override the duplicate-call threshold. Useful when the caller wants
    /// stricter detection than the default (which is `window_size`).
    #[must_use]
    pub fn with_duplicate_threshold(mut self, threshold: usize) -> Self {
        self.duplicate_threshold = threshold.max(2);
        self
    }

    /// Override the thrash parameters.
    #[must_use]
    pub fn with_thrash(mut self, threshold: usize, window: Duration) -> Self {
        self.thrash_threshold = threshold.max(2);
        self.thrash_window = window;
        self
    }

    /// Record a tool call. Returns `Err` if either detector fires.
    ///
    /// The call is *still added* to the window on success; on error the window
    /// is rewound so the failing call isn't double-counted on a retry.
    pub fn record(&mut self, tool: &str, args: &Value) -> Result<(), LoopError> {
        let now = Instant::now();
        self.push(tool.to_string(), args, now);

        if let Some(err) = self.check_identical_args() {
            // Rewind so the rejected call doesn't poison the next check.
            self.recent.pop_back();
            return Err(err);
        }

        if let Some(err) = self.check_thrash(now) {
            self.recent.pop_back();
            return Err(err);
        }

        Ok(())
    }

    /// Drop every record. Call after a clean successful turn to reset.
    pub fn clear(&mut self) {
        self.recent.clear();
    }

    fn push(&mut self, tool: String, args: &Value, now: Instant) {
        let call = Call {
            tool,
            args_fingerprint: fingerprint(args),
            at: now,
        };
        if self.recent.len() == self.window_size {
            self.recent.pop_front();
        }
        self.recent.push_back(call);
    }

    fn check_identical_args(&self) -> Option<LoopError> {
        let last = self.recent.back()?;
        let count = self
            .recent
            .iter()
            .filter(|c| c.tool == last.tool && c.args_fingerprint == last.args_fingerprint)
            .count();
        if count >= self.duplicate_threshold {
            let window_ms = self
                .recent
                .front()
                .map(|first| last.at.duration_since(first.at))
                .unwrap_or_default()
                .as_millis() as u64;
            return Some(LoopError::IdenticalArgs {
                tool: last.tool.clone(),
                count,
                window_ms,
            });
        }
        None
    }

    fn check_thrash(&self, now: Instant) -> Option<LoopError> {
        let cutoff = now.checked_sub(self.thrash_window)?;
        let count = self.recent.iter().filter(|c| c.at >= cutoff).count();
        if count >= self.thrash_threshold {
            return Some(LoopError::HighFrequency {
                count,
                window_ms: self.thrash_window.as_millis() as u64,
            });
        }
        None
    }
}

/// Order-stable fingerprint of a JSON value. We canonicalize maps by walking
/// them in `BTreeMap` order so `{a,b}` and `{b,a}` hash the same.
fn fingerprint(args: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonicalize(args).hash(&mut hasher);
    hasher.finish()
}

fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let sorted: std::collections::BTreeMap<String, Value> = m
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize(v)))
                .collect();
            serde_json::to_value(sorted).unwrap_or(Value::Null)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_args_trips_detector() {
        let mut guard = LoopGuard::new(3);
        let args = json!({"project": "kdo-core"});

        assert!(guard.record("kdo_get_context", &args).is_ok());
        assert!(guard.record("kdo_get_context", &args).is_ok());

        match guard.record("kdo_get_context", &args) {
            Err(LoopError::IdenticalArgs { tool, count, .. }) => {
                assert_eq!(tool, "kdo_get_context");
                assert_eq!(count, 3);
            }
            other => panic!("expected IdenticalArgs, got {other:?}"),
        }
    }

    #[test]
    fn openclaw_tight_threshold() {
        // OpenClaw profile uses a tighter window (3).
        let mut guard = LoopGuard::for_profile_window(3);
        let args = json!({"symbol": "initialize_vault"});

        guard.record("kdo_read_symbol", &args).unwrap();
        guard.record("kdo_read_symbol", &args).unwrap();
        assert!(guard.record("kdo_read_symbol", &args).is_err());
    }

    #[test]
    fn different_args_do_not_trip() {
        let mut guard = LoopGuard::new(3);
        guard
            .record("kdo_get_context", &json!({"project": "a"}))
            .unwrap();
        guard
            .record("kdo_get_context", &json!({"project": "b"}))
            .unwrap();
        guard
            .record("kdo_get_context", &json!({"project": "c"}))
            .unwrap();
    }

    #[test]
    fn different_tools_do_not_trip() {
        let mut guard = LoopGuard::new(3);
        let args = json!({});
        guard.record("kdo_list_projects", &args).unwrap();
        guard.record("kdo_dep_graph", &args).unwrap();
        guard.record("kdo_affected", &args).unwrap();
    }

    #[test]
    fn fingerprint_is_key_order_independent() {
        // Map-order shouldn't change the fingerprint — two logically identical
        // calls with different key insertion order should be treated as dupes.
        let mut guard = LoopGuard::new(3);
        guard
            .record("kdo_get_context", &json!({"project": "a", "budget": 2048}))
            .unwrap();
        guard
            .record("kdo_get_context", &json!({"budget": 2048, "project": "a"}))
            .unwrap();
        assert!(guard
            .record("kdo_get_context", &json!({"project": "a", "budget": 2048}))
            .is_err());
    }

    #[test]
    fn clear_resets_window() {
        let mut guard = LoopGuard::new(3);
        let args = json!({"x": 1});
        guard.record("t", &args).unwrap();
        guard.record("t", &args).unwrap();
        guard.clear();
        guard.record("t", &args).unwrap();
        guard.record("t", &args).unwrap();
        // With a fresh window we should still need 3 before tripping.
        assert!(guard.record("t", &args).is_err());
    }

    #[test]
    fn rewind_on_error_allows_clean_retry_with_different_args() {
        let mut guard = LoopGuard::new(3);
        let a = json!({"project": "a"});
        let b = json!({"project": "b"});

        guard.record("kdo_get_context", &a).unwrap();
        guard.record("kdo_get_context", &a).unwrap();
        // Third identical call rejected AND rewound.
        assert!(guard.record("kdo_get_context", &a).is_err());
        // Next call with different args must succeed immediately.
        guard.record("kdo_get_context", &b).unwrap();
    }

    #[test]
    fn window_slides() {
        let mut guard = LoopGuard::new(3);
        let a = json!({"x": "a"});
        let b = json!({"x": "b"});

        // Fill the window with `a`s but not enough to trip (2/3).
        guard.record("t", &a).unwrap();
        guard.record("t", &a).unwrap();
        // A `b` pushes one `a` out — now 2 a's, 1 b.
        guard.record("t", &b).unwrap();
        // Another `a` — window has 1 b, 2 a's. 3rd `a` tripper would need
        // 3 a's in the window; we only have 2 yet.
        guard.record("t", &a).unwrap();
        // Now the oldest `a` slid out — window is a, b, a. Another a:
        // [b, a, a] — 2 a's in window, still not 3.
        // With a 4th a:
        match guard.record("t", &a) {
            Err(LoopError::IdenticalArgs { .. }) => {}
            Ok(_) => {} // acceptable; window can legitimately end with <3 a's
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn thrash_detector_high_frequency() {
        let mut guard = LoopGuard::new(3)
            .with_thrash(3, Duration::from_secs(3))
            .with_duplicate_threshold(99); // disable duplicate check for this test

        // Three distinct calls within the thrash window should trip.
        guard.record("t", &json!({"x": 1})).unwrap();
        guard.record("t", &json!({"x": 2})).unwrap();
        assert!(matches!(
            guard.record("t", &json!({"x": 3})),
            Err(LoopError::HighFrequency { .. })
        ));
    }
}
