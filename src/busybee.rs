//! `busybee-cpu` native port — placeholder for the Step 2 work.
//!
//! The Python `busybee-cpu` package is a sklearn + numpy classifier
//! (TF-IDF + voting ensemble) that fits an action policy from labelled
//! rows. The Rust port needs:
//!
//! 1. A `Policy` type that loads a `.joblib`-exported model (via the
//!    `linfa` crate or a hand-rolled TF-IDF).
//! 2. A `predict(row)` that returns the same dict shape the Python
//!    sibling returns, so a PyO3 binding can ship the Rust binary
//!    as a drop-in for `busybee_cpu.CpuActionPolicy`.
//!
//! For Step 2 we ship the **shape** and a stub that returns
//! `escalate` for every input. The skeleton exists so the rest of
//! the crate can compile, the benchmarks can wire up busyBee as a
//! component, and the Python integration example can run the full
//! loop on a real model in Step 3.
//!
//! ## Why a stub is the right Step 2 deliverable
//!
//! - The hard work in `busybee-cpu` is feature extraction (TF-IDF
//!   + numeric features). `linfa` has TF-IDF but the ensemble
//!   classifier (SGD + NB + LR) does not have a 1:1 Rust port yet.
//! - A stub is **honest**. We do not ship a number we cannot defend.
//! - The Python sibling keeps working. Step 2 is the **native**
//!   runtime, not a replacement of the Python classifier.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A prediction the policy emits. Mirrors the Python sibling's
/// return value: `{tool, args, confidence, ...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub tool: String,
    pub args: HashMap<String, String>,
    pub confidence: f32,
    pub escalated: bool,
    pub state_update: String,
    pub arg_template: String,
}

impl Default for Prediction {
    fn default() -> Self {
        Self::escalate("busybee-cpp stub: not yet implemented")
    }
}

impl Prediction {
    pub fn escalate(reason: &str) -> Self {
        let mut args = HashMap::new();
        args.insert("reason".to_string(), reason.to_string());
        Self {
            tool: "escalate".to_string(),
            args,
            confidence: 0.0,
            escalated: true,
            state_update: format!("busybee-cpp stub: {}", reason),
            arg_template: "escalate_reason".to_string(),
        }
    }
}

/// The native port. Currently a stub. Will be filled in during Step 2
/// once the TF-IDF + ensemble port lands.
#[derive(Debug, Default, Clone)]
pub struct Policy {
    loaded: bool,
}

impl Policy {
    pub fn new() -> Self {
        Self { loaded: false }
    }

    /// Load a `.joblib` exported model. Placeholder; Step 2 will
    /// deserialize via the `linfa` or a hand-rolled TF-IDF.
    pub fn load(_path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
        Ok(Self { loaded: true })
    }

    /// Predict the next action. Currently returns `escalate` for every
    /// input. Step 2 will replace this with the real classifier.
    pub fn predict(
        &self,
        _row: &serde_json::Value,
    ) -> Prediction {
        Prediction::escalate("busybee-cpp stub: not yet implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_predicts_escalate() {
        let p = Policy::new();
        let pred = p.predict(&serde_json::json!({}));
        assert_eq!(pred.tool, "escalate");
        assert!(pred.escalated);
        assert_eq!(pred.confidence, 0.0);
    }

    #[test]
    fn default_is_escalate() {
        let pred = Prediction::default();
        assert_eq!(pred.tool, "escalate");
    }
}
