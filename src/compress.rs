//! Context compression: regex-based content-type inference and
//! deterministic compression. Mirrors the Python `hive/rule_fast` shim
//! so the in-repo fallback path is identical.
//!
//! ## What ships
//!
//! - [`ContentType`]: same string values as the Python sibling.
//! - [`Label`]: re-export of [`crate::label::Label`].
//! - [`Message`] / [`CompressedMessage`]: wire-compatible with the
//!   Python `Message` / `CompressedMessage` dataclasses.
//! - [`RuleFast`]: the regex classifier + compressor. Drop-in
//!   replacement for `hive.rule_fast.RuleFastHoneyComb`.
//! - [`RuleFastHoneyComb`]: legacy alias kept for Python-side
//!   `from hive.rule_fast import RuleFastHoneyComb` callers — not used
//!   inside the Rust crate but exported so a future PyO3 binding can
//!   expose the same name.

use ahash::AHashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub use crate::label::Label;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Same string values as the Python `hive.rule_fast.ContentType` and
/// `honeycomb.labels.ContentType`. Used in JSON wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    System,
    UserGoal,
    AgentReasoning,
    AgentPatch,
    ToolCall,
    ToolResultFile,
    ToolResultTest,
    ToolResultError,
    ToolResultSearch,
    ToolResultCommand,
    Unknown,
}

impl ContentType {
    pub const fn as_str(self) -> &'static str {
        match self {
            ContentType::System => "system",
            ContentType::UserGoal => "user_goal",
            ContentType::AgentReasoning => "agent_reasoning",
            ContentType::AgentPatch => "agent_patch",
            ContentType::ToolCall => "tool_call",
            ContentType::ToolResultFile => "tool_result_file",
            ContentType::ToolResultTest => "tool_result_test",
            ContentType::ToolResultError => "tool_result_error",
            ContentType::ToolResultSearch => "tool_result_search",
            ContentType::ToolResultCommand => "tool_result_command",
            ContentType::Unknown => "unknown",
        }
    }
}

/// Pre-compiled regex set. Compiled once on first use; the cost is
/// amortised over the hot loop. On aarch64 the underlying `regex`
/// engine uses NEON-accelerated memchr for the literal-prefix fast
/// path, which is the dominant case for the test-output and
/// source-file compressors.
struct CompiledRules {
    patch: Regex,
    tool_call: Regex,
    test_line: Regex,
    code: Regex,
    traceback: Regex,
    file_line: Regex,
    exit: Regex,
    test_summary: Regex,
}

static RULES: OnceLock<CompiledRules> = OnceLock::new();

fn rules() -> &'static CompiledRules {
    RULES.get_or_init(|| CompiledRules {
        patch: Regex::new(r"^[-+]{3} |^diff --git|^@@ ").unwrap(),
        tool_call: Regex::new(r#""name"\s*:\s*"(?:read_file|run_tests|apply_patch|search|run_command)""#).unwrap(),
        test_line: Regex::new(r"\b\d+\s*(?:passed|failed|errors?)\b").unwrap(),
        code: Regex::new(r"^(class|def|import|from|export|function|pub |fn |struct |enum |impl )").unwrap(),
        traceback: Regex::new(r"Traceback \(most recent call last\)|^\w+(?:Error|Exception): ").unwrap(),
        file_line: Regex::new(r"^[^\s:]+:\d+:").unwrap(),
        exit: Regex::new(r"exit[= ]+\d+").unwrap(),
        test_summary: Regex::new(r"(?P<ok>\d+ passed|\d+ ok)|(?P<bad>\d+ failed|\d+ error)").unwrap(),
    })
}

/// Heuristic content-type inference. The Python sibling does the same
/// check as a cascade of `re.search` calls; the Rust port keeps the
/// order but does them in one pass.
pub fn infer_content_type(role: &str, content: &str) -> ContentType {
    let r = rules();
    let role = role.to_ascii_lowercase();
    match role.as_str() {
        "system" => return ContentType::System,
        "user" => return ContentType::UserGoal,
        "assistant" => {
            if r.patch.is_match(content) {
                return ContentType::AgentPatch;
            }
            if r.tool_call.is_match(content) {
                return ContentType::ToolCall;
            }
            return ContentType::AgentReasoning;
        }
        _ => {} // "tool" or anything else: fall through
    }
    if r.patch.is_match(content) {
        return ContentType::AgentPatch;
    }
    if r.test_line.is_match(content) {
        return ContentType::ToolResultTest;
    }
    if r.traceback.is_match(content) {
        return ContentType::ToolResultError;
    }
    if r.code.is_match(content) {
        return ContentType::ToolResultFile;
    }
    if r.file_line.is_match(content) {
        return ContentType::ToolResultSearch;
    }
    if r.exit.is_match(content) {
        return ContentType::ToolResultCommand;
    }
    ContentType::Unknown
}

fn classify(role: &str, content_type: ContentType, content: &str) -> Label {
    match content_type {
        ContentType::System | ContentType::UserGoal => Label::Core,
        ContentType::ToolCall => Label::Drop,
        ContentType::ToolResultError => Label::Core, // recent errors stay verbatim; cool loop handles staleness
        ContentType::ToolResultTest
        | ContentType::ToolResultSearch
        | ContentType::ToolResultCommand
        | ContentType::AgentPatch => Label::Distill,
        ContentType::ToolResultFile => {
            if content.len() > 2000 {
                Label::Compact
            } else {
                Label::Distill
            }
        }
        ContentType::AgentReasoning => {
            if content.len() < 300 {
                Label::Core
            } else {
                Label::Distill
            }
        }
        ContentType::Unknown => {
            // Python sibling returns DISTILL for the catch-all.
            let _ = role;
            Label::Distill
        }
    }
}

// ---------------------------------------------------------------------------
// Compressors. Each is `fn(&str) -> String` and is kept tiny so the
// optimiser can inline.
// ---------------------------------------------------------------------------

fn compress_test_output(content: &str) -> String {
    let r = &rules().test_summary;
    let summary = r
        .find(content)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| format!("{} lines", content.matches('\n').count()));
    let failed: Vec<&str> = content
        .lines()
        .filter(|l| l.contains("FAIL") || l.contains("ERROR"))
        .take(5)
        .collect();
    if failed.is_empty() {
        format!("[test] {}", summary)
    } else {
        format!("[test] {}. failures: {}", summary, failed.join(" | "))
    }
}

fn compress_search(content: &str) -> String {
    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(8)
        .collect();
    format!(
        "[search] {} hits; sample: {}",
        content.lines().count(),
        lines.join(" | ")
    )
}

fn compress_command(content: &str) -> String {
    let head: String = content.lines().take(6).collect::<Vec<_>>().join("\n");
    format!("[cmd] {}\n... ({} chars)", head, content.len())
}

fn compact_file(content: &str) -> String {
    let lines: Vec<&str> = content.lines().take(3).collect();
    format!(
        "[file] {} lines, {} chars\n{}\n...",
        content.lines().count(),
        content.len(),
        lines.join("\n")
    )
}

fn distill(content: &str) -> String {
    // Multi-line content: head + tail, drop the middle.
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 8 {
        let mut out = String::new();
        for line in lines.iter().take(3) {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("...\n");
        for line in lines.iter().rev().take(3).rev() {
            out.push_str(line);
            out.push('\n');
        }
        return out.trim_end().to_string();
    }
    // Single-line but very long content: word-based head/tail.
    if content.len() > 800 {
        let words: Vec<&str> = content.split_whitespace().collect();
        if words.len() > 80 {
            let head = words.iter().take(60).copied().collect::<Vec<_>>().join(" ");
            let tail = words
                .iter()
                .rev()
                .take(60)
                .rev()
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            return format!("{}\n...\n{}", head, tail);
        }
    }
    content.to_string()
}

fn compress(content: &str, content_type: ContentType, label: Label) -> String {
    if label == Label::Core {
        return content.to_string();
    }
    match content_type {
        ContentType::ToolResultTest => compress_test_output(content),
        ContentType::ToolResultSearch => compress_search(content),
        ContentType::ToolResultCommand => compress_command(content),
        ContentType::ToolResultFile => compact_file(content),
        _ => match label {
            Label::Compact => compact_file(content),
            Label::Distill => distill(content),
            _ => content.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Data classes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<ContentType>,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            content_type: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedMessage {
    pub role: String,
    pub content: String,
    pub label: Label,
    pub content_type: ContentType,
    pub original_tokens: u32,
    pub compressed_tokens: u32,
}

impl CompressedMessage {
    pub fn ratio(&self) -> f64 {
        if self.compressed_tokens == 0 {
            0.0
        } else {
            self.original_tokens as f64 / self.compressed_tokens as f64
        }
    }
}

fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        0
    } else {
        ((text.len() / 4) as u32).max(1)
    }
}

// ---------------------------------------------------------------------------
// The compressor itself
// ---------------------------------------------------------------------------

/// Regex-based high-perf compressor. Drop-in replacement for the
/// Python `hive.rule_fast.RuleFastHoneyComb`.
#[derive(Debug, Default, Clone)]
pub struct RuleFast {
    turn: u64,
    total_in: u64,
    total_out: u64,
    entries: u64,
    label_hist: AHashMap<Label, u64>,
    type_hist: AHashMap<ContentType, u64>,
}

impl RuleFast {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run a single message through the hot loop.
    pub fn process(&mut self, msg: &Message) -> CompressedMessage {
        self.turn += 1;
        let content_type = msg
            .content_type
            .unwrap_or_else(|| infer_content_type(&msg.role, &msg.content));
        let label = classify(&msg.role, content_type, &msg.content);
        let compressed = compress(&msg.content, content_type, label);
        let in_tok = estimate_tokens(&msg.content);
        let out_tok = estimate_tokens(&compressed);
        self.total_in += in_tok as u64;
        self.total_out += out_tok as u64;
        self.entries += 1;
        *self.label_hist.entry(label).or_insert(0) += 1;
        *self.type_hist.entry(content_type).or_insert(0) += 1;
        CompressedMessage {
            role: msg.role.clone(),
            content: compressed,
            label,
            content_type,
            original_tokens: in_tok,
            compressed_tokens: out_tok,
        }
    }

    pub fn stats(&self) -> RuleFastStats {
        RuleFastStats {
            turn_count: self.turn,
            total_entries: self.entries,
            active_entries: self.entries,
            total_tokens: self.total_out,
            original_tokens: self.total_in,
            compression_ratio: self.total_in as f64 / self.total_out.max(1) as f64,
            labels: self.label_hist.iter().map(|(k, v)| (*k, *v)).collect(),
            types: self.type_hist.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }
}

/// Wire-compatible stats struct. Matches the Python `get_stats()`
/// output. Field names are snake_case to match the JSON the Python
/// benchmark reads.
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleFastStats {
    pub turn_count: u64,
    pub total_entries: u64,
    pub active_entries: u64,
    pub total_tokens: u64,
    pub original_tokens: u64,
    pub compression_ratio: f64,
    pub labels: Vec<(Label, u64)>,
    pub types: Vec<(ContentType, u64)>,
}

/// Legacy alias for the Python shim. See the module docs.
pub type RuleFastHoneyComb = RuleFast;

/// Drop-in replacement for the Python `honeycomb.HoneyComb`.
///
/// When the `honeycomb-compat` feature is enabled, this struct gains
/// the same public surface (`process`, `get_stats`, `Message`,
/// `CompressedMessage`) so a Python caller can swap it in via PyO3
/// without call-site changes. The class is currently a thin
/// wrapper over [`RuleFast`] — the real honey-comb Python shim is
/// richer (ML classifier, cool loop, budget enforcement); this is
/// the **fast** path only.
pub type Comb = RuleFast;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_string_values_match_python() {
        // Pin the wire format. If any of these change, update both
        // the Python `hive/rule_fast/__init__.py` ContentType class
        // and the README.
        assert_eq!(ContentType::System.as_str(), "system");
        assert_eq!(ContentType::ToolResultTest.as_str(), "tool_result_test");
        assert_eq!(ContentType::ToolResultError.as_str(), "tool_result_error");
    }

    #[test]
    fn infer_test_output() {
        assert_eq!(
            infer_content_type("tool", "tests: 12 passed, 0 failed"),
            ContentType::ToolResultTest
        );
    }

    #[test]
    fn infer_search_results() {
        let c = "src/auth.py:42: def login():\nsrc/auth.py:43:     return user";
        assert_eq!(infer_content_type("tool", c), ContentType::ToolResultSearch);
    }

    #[test]
    fn infer_file() {
        let c = "import os\nclass Foo:\n    def bar(self):\n        return 1\n".to_string() + &"x".repeat(1000);
        assert_eq!(infer_content_type("tool", &c), ContentType::ToolResultFile);
    }

    #[test]
    fn infer_error() {
        let c = "Traceback (most recent call last):\nValueError: bad";
        assert_eq!(infer_content_type("tool", c), ContentType::ToolResultError);
    }

    #[test]
    fn test_compressor_keeps_first_failure() {
        let out = compress_test_output("test_a ... ok\ntest_b ... FAIL\ntest_c ... ok");
        assert!(out.contains("FAIL"));
        assert!(!out.contains("12 passed"));
    }

    #[test]
    fn long_reasoning_gets_distilled() {
        let text = "Let me think step by step. ".repeat(200); // 5 KB single line
        let mut rf = RuleFast::new();
        let m = rf.process(&Message::new("assistant", &text));
        assert_eq!(m.label, Label::Distill);
        assert!(m.content.contains("..."));
        assert!(m.compressed_tokens < m.original_tokens);
    }

    #[test]
    fn classify_user_goal_is_core() {
        assert_eq!(
            classify("user", ContentType::UserGoal, "fix auth"),
            Label::Core
        );
    }

    #[test]
    fn classify_empty_falls_to_distill() {
        assert_eq!(
            classify("tool", ContentType::Unknown, ""),
            Label::Distill
        );
    }

}
