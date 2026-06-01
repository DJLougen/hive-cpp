//! Label taxonomy: mirrors `honeycomb.labels.Label` and the string values
//! the Python meta-package emits on the wire.
//!
//! The Python shim uses a class with string constants:
//!
//! ```python
//! class Label:
//!     CORE = "core"
//!     DISTILL = "distill"
//!     ...
//! ```
//!
//! The Rust port uses a `u8` enum at the hot path (so dispatch is a single
//! load + mask, no string compare) and exposes the canonical string only
//! when the boundary needs it (JSON serialisation, debug logs).

use serde::{Deserialize, Serialize};

/// Same string values as `honeycomb.labels.Label`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    /// Keep verbatim (system prompts, user goal, critical context).
    Core,
    /// Extract key information (test output → summary + failures).
    Distill,
    /// Keep structure only (file → "src/foo.py (200 lines): class Foo").
    Compact,
    /// Remove entirely (stale context, duplicates).
    Drop,
    /// Mark for removal (superseded by newer information).
    Stale,
    /// Pass to LLM (ambiguous content).
    Escalate,
}

impl Label {
    /// The canonical string form. Matches the Python sibling.
    #[inline]
    pub const fn value(self) -> &'static str {
        match self {
            Label::Core => "core",
            Label::Distill => "distill",
            Label::Compact => "compact",
            Label::Drop => "drop",
            Label::Stale => "stale",
            Label::Escalate => "escalate",
        }
    }

    /// Parse from the canonical string. Inverse of [`Self::value`].
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "core" => Label::Core,
            "distill" => Label::Distill,
            "compact" => Label::Compact,
            "drop" => Label::Drop,
            "stale" => Label::Stale,
            "escalate" => Label::Escalate,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_string() {
        for s in [
            "core", "distill", "compact", "drop", "stale", "escalate",
        ] {
            let l = Label::parse(s).unwrap();
            assert_eq!(l.value(), s);
        }
    }

    #[test]
    fn round_trip_json() {
        let s = serde_json::to_string(&Label::Distill).unwrap();
        assert_eq!(s, "\"distill\"");
        let l: Label = serde_json::from_str(&s).unwrap();
        assert_eq!(l, Label::Distill);
    }
}
