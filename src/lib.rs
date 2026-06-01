//! # hive-cpp — native Rust port of the Hive agent memory stack.
//!
//! This crate is the **Step 2** work that the Python meta-package
//! ([`DJLougen/hive`]) is reaching for. The Python `hive/` meta-package
//! stays the source of truth; this crate is the **native runtime** that
//! the Python shim will eventually call into via PyO3 or `cdylib`.
//!
//! ## What ships in Step 2
//!
//! | Component | Python surface | Rust surface (this crate) | Status |
//! |-----------|----------------|---------------------------|--------|
//! | `busybee-cpu` action router | `CpuActionPolicy` | `busybee::Policy` (planned) | skeleton |
//! | `honeycomb` context compression | `HoneyComb` | `compress::Comb` | **done** |
//! | `hive.rule_fast` fallback | `RuleFastHoneyComb` | re-exported as `compress::RuleFast` | **done** |
//! | `rust-brain` graph memory | `RustBrain` | `brain::Brain` | **done** |
//! | `hive.hardware` NVML sampler | `hive.hardware` | `nvml::*` behind `nvidia-ml` feature | planned |
//! | `hive.llm` client | `hive.llm` | `llm::*` (OpenAI-compat HTTP) | planned |
//!
//! ## Public API
//!
//! The Python meta-package's `HiveStack` is a 1:1 mirror of
//! [`crate::stack::Stack`]. Call sites written against the Python API
//! will port with no behavioural change.
//!
//! ## Backwards compatibility
//!
//! Every public type is `serde::Serialize` so the same JSON envelopes
//! that the Python `hive_benchmark.py` writes are accepted by the Rust
//! `hive-bench` binary and vice versa.
//!
//! ## Performance
//!
//! Targets (single x86_64 core, 2026 hardware, `--release`):
//!
//! | Component | Rate | Python (Step 1) | Rust (this crate) |
//! |-----------|------|-----------------|-------------------|
//! | `compress::RuleFast::process` | ≥ 200 k msg/s | 36 k msg/s | target ~5× |
//! | `brain::Brain::remember` | ≥ 1 M writes/s | 270 k writes/s | target ~4× |
//! | `busybee::Policy::predict` | ≥ 10 k r/s | 110 r/s | target ~100× |
//!
//! Numbers are reproducible with `cargo bench` and a CPU-only box.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod brain;
pub mod busybee;
pub mod compress;
pub mod label;
pub mod llm;
pub mod nvml;
pub mod stack;
pub mod util;

pub use brain::{Brain, EdgeKind, MemoryNode, TimestampRegression};
pub use compress::{Comb, CompressedMessage, ContentType, Label, Message, RuleFast, RuleFastHoneyComb};
pub use label::Label as LabelKind;
pub use stack::{RouteDecision, Stack, StepResult};

/// Crate version. Mirrors the Python meta-package's `hive.__version__`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn label_string_values_match_python_sibling() {
        // These string values are part of the wire format the Python
        // meta-package emits and consumes. Do not change them without
        // updating both sides.
        assert_eq!(label::Label::Core.value(), "core");
        assert_eq!(label::Label::Distill.value(), "distill");
        assert_eq!(label::Label::Compact.value(), "compact");
        assert_eq!(label::Label::Drop.value(), "drop");
        assert_eq!(label::Label::Stale.value(), "stale");
        assert_eq!(label::Label::Escalate.value(), "escalate");
    }
}
