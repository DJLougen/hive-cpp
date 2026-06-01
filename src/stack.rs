//! `Stack` — the native orchestrator that mirrors the Python
//! `hive.stack.HiveStack` API.
//!
//! Step 2 ships a thin orchestration: the Python sibling's fallback
//! chain (honey-comb → `rule_fast`) becomes a single [`RuleFast`]
//! compressor in Rust. busyBee is a [`busybee::Policy`] stub. rust-brain
//! is real. The interface is identical so a future PyO3 binding can
//! drop in.

use serde::{Deserialize, Serialize};

use crate::brain::{Brain, MemoryNode};
use crate::busybee::{Policy, Prediction};
use crate::compress::{Comb, CompressedMessage, Message};

/// `RouteDecision` — output of [`Stack::route`]. Same field names as
/// the Python `hive.stack.RouteDecision` so the wire format lines up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    pub tool: String,
    pub args: serde_json::Value,
    pub confidence: f32,
    pub escalated: bool,
    pub source: String, // "busybee" | "honeycomb-escalate" | "fallback"
}

impl From<Prediction> for RouteDecision {
    fn from(p: Prediction) -> Self {
        Self {
            tool: p.tool,
            args: serde_json::to_value(p.args).unwrap_or(serde_json::Value::Null),
            confidence: p.confidence,
            escalated: p.escalated,
            source: "busybee".to_string(),
        }
    }
}

/// `StepResult` — output of [`Stack::step`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub decision: RouteDecision,
    pub compressed: Option<CompressedMessage>,
    pub stats: Stats,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub brain: crate::brain::BrainStats,
}

/// The native stack. Drop-in replacement for `hive.stack.HiveStack`.
///
/// The Python integration example calls into `HiveStack` for the
/// `route` / `compress` / `remember` triad. The Rust port preserves
/// the same triple so a PyO3 binding does not need a translation
/// layer.
#[derive(Debug, Clone)]
pub struct Stack {
    pub busybee: Option<Policy>,
    pub comb: Comb,
    pub brain: Brain,
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            busybee: None,
            comb: Comb::new(),
            brain: Brain::new(),
        }
    }
}

impl Stack {
    pub fn new() -> Self {
        Self::default()
    }

    /// CPU-only action router. Returns [`RouteDecision`].
    pub fn route(&self, state: &serde_json::Value) -> RouteDecision {
        match &self.busybee {
            Some(p) => p.predict(state).into(),
            None => RouteDecision {
                tool: "escalate".to_string(),
                args: serde_json::json!({"reason": "no busybee policy loaded"}),
                confidence: 0.0,
                escalated: true,
                source: "fallback".to_string(),
            },
        }
    }

    /// Run a single message through the inline compression hot loop.
    pub fn compress(&mut self, role: &str, content: &str) -> CompressedMessage {
        self.comb.process(&Message::new(role, content))
    }

    /// Write a memory node, optionally causal-linked to earlier nodes.
    pub fn remember(
        &self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
        trust: f32,
    ) -> Result<MemoryNode, crate::brain::BrainError> {
        self.brain.remember(key, value, trust)
    }

    pub fn recall(&self, key: &str) -> Option<serde_json::Value> {
        self.brain.recall(key)
    }

    /// End-to-end Hive step: route → compress → write the decision.
    /// Mirrors the Python `HiveStack.step`.
    pub fn step(&mut self, state: &serde_json::Value, transcript: &[(String, String)]) -> StepResult {
        let decision = self.route(state);
        let last = transcript.last().cloned().unwrap_or_default();
        let compressed = if !last.1.is_empty() {
            Some(self.compress(&last.0, &last.1))
        } else {
            None
        };
        let key = format!("decision:{}", state.get("step").and_then(|v| v.as_u64()).unwrap_or(0));
        let _ = self.brain.remember(
            key,
            serde_json::json!({
                "tool": decision.tool,
                "args": decision.args,
                "confidence": decision.confidence,
                "escalated": decision.escalated,
            }),
            0.8,
        );
        StepResult {
            decision,
            compressed,
            stats: Stats { brain: self.brain.stats() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_route_falls_back_to_escalate() {
        let s = Stack::new();
        let d = s.route(&serde_json::json!({}));
        assert_eq!(d.tool, "escalate");
        assert_eq!(d.source, "fallback");
        assert!(d.escalated);
    }

    #[test]
    fn remember_then_recall() {
        let s = Stack::new();
        s.remember("ep", serde_json::json!("/v1/chat"), 0.9).unwrap();
        assert_eq!(s.recall("ep").unwrap(), serde_json::json!("/v1/chat"));
    }

    #[test]
    fn step_writes_decision_to_brain() {
        let mut s = Stack::new();
        let result = s.step(
            &serde_json::json!({"step": 0}),
            &[("user".into(), "do thing".into())],
        );
        assert_eq!(result.decision.tool, "escalate");
        // The decision was written under the "decision:0" key.
        let stored = s.brain.recall("decision:0");
        assert!(stored.is_some());
    }

    #[test]
    fn step_with_empty_transcript_yields_no_compression() {
        let mut s = Stack::new();
        let result = s.step(&serde_json::json!({"step": 1}), &[]);
        assert!(result.compressed.is_none());
    }
}

// Re-export the `Label` enum from `crate::label` so the test path can
// import it via `crate::stack::LabelKind` (matches the lib.rs re-export).
pub use crate::label::Label as LabelKind;
