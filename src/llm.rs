//! `rust-brain` — timestamp-protected, graph-structured agent memory.
//!
//! The reference implementation is the Python shim at
//! `hive/rust_brain/__init__.py`. This Rust module is the **Step 2
//! port**: same data model, same invariants, same wire format.
//!
//! ## Invariants (must match the Python shim)
//!
//! 1. **Monotonic timestamps.** Replays of older writes raise
//!    [`TimestampRegression`] when `enforce_monotonic` is on (the default).
//! 2. **Graph relations are typed.** Edges are
//!    [`EdgeKind`]: `related_to`, `caused_by`, `supersedes`, `attached_to`.
//! 3. **Wire format is `serde_json`.** [`MemoryNode::to_dict`] produces
//!    a JSON object the Python `rust_brain.HermesBackend.publish` will
//!    accept on the other side.

use ahash::AHashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Typed edges. Same names as the Python sibling; the JSON wire format
/// uses the snake_case string values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    RelatedTo,
    CausedBy,
    Supersedes,
    AttachedTo,
}

impl EdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            EdgeKind::RelatedTo => "related_to",
            EdgeKind::CausedBy => "caused_by",
            EdgeKind::Supersedes => "supersedes",
            EdgeKind::AttachedTo => "attached_to",
        }
    }
}

pub type HiveTimestamp = u64;

pub const HIVE_EPOCH_NS: u64 = 1_704_067_200_000_000_000;

pub fn now_ns() -> HiveTimestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        .saturating_sub(HIVE_EPOCH_NS)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryNode {
    pub key: String,
    pub value: serde_json::Value,
    pub ts_ns: HiveTimestamp,
    pub trust: f32,
    pub tags: Vec<String>,
    pub edges: BTreeMap<EdgeKind, Vec<String>>,
    pub node_id: String,
}

impl MemoryNode {
    pub fn new(key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ts_ns: now_ns(),
            trust: 1.0,
            tags: Vec::new(),
            edges: BTreeMap::new(),
            node_id: uuid_v4_hex(),
        }
    }

    pub fn to_dict(&self) -> serde_json::Value {
        let edges: BTreeMap<String, Vec<String>> = self
            .edges
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
            .collect();
        serde_json::json!({
            "id": self.node_id,
            "key": self.key,
            "value": self.value,
            "ts_ns": self.ts_ns,
            "trust": self.trust,
            "tags": self.tags.iter().map(String::as_str).collect::<Vec<_>>(),
            "edges": edges,
        })
    }

    pub fn attach(&mut self, kind: EdgeKind, other_key: impl Into<String>) {
        let other = other_key.into();
        if other == self.key {
            return;
        }
        self.edges.entry(kind).or_default().push(other);
        if let Some(v) = self.edges.get_mut(&kind) {
            v.sort();
            v.dedup();
        }
    }
}

#[derive(Debug, Error)]
pub enum BrainError {
    #[error("refusing to write {key:?}: ts={ts} < stored={stored} (monotonic)")]
    TimestampRegression {
        key: String,
        ts: HiveTimestamp,
        stored: HiveTimestamp,
    },
    #[error("key not found: {0:?}")]
    NotFound(String),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BrainStats {
    pub node_count: usize,
    pub oldest_ts_ns: Option<HiveTimestamp>,
    pub newest_ts_ns: Option<HiveTimestamp>,
    pub edge_kinds: Vec<EdgeKind>,
}

#[derive(Debug, Clone)]
pub struct Brain {
    inner: Arc<RwLock<BrainInner>>,
    enforce_monotonic: bool,
}

#[derive(Debug)]
struct BrainInner {
    nodes: AHashMap<String, MemoryNode>,
    order: Vec<String>,
}

impl Default for Brain {
    fn default() -> Self {
        Self::new()
    }
}

impl Brain {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(BrainInner {
                nodes: AHashMap::new(),
                order: Vec::new(),
            })),
            enforce_monotonic: true,
        }
    }

    pub fn with_monotonic(mut self, enforce: bool) -> Self {
        self.enforce_monotonic = enforce;
        self
    }

    pub fn remember(
        &self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
        trust: f32,
    ) -> Result<MemoryNode, BrainError> {
        self.remember_at(key, value, trust, now_ns(), &[], None)
    }

    pub fn remember_at(
        &self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
        trust: f32,
        ts_ns: HiveTimestamp,
        tags: &[String],
        edges: Option<&[(EdgeKind, String)]>,
    ) -> Result<MemoryNode, BrainError> {
        let key = key.into();
        let mut inner = self.inner.write();
        if let Some(existing) = inner.nodes.get(&key) {
            if self.enforce_monotonic && ts_ns < existing.ts_ns {
                return Err(BrainError::TimestampRegression {
                    key,
                    ts: ts_ns,
                    stored: existing.ts_ns,
                });
            }
        }
        let mut node = MemoryNode {
            key: key.clone(),
            value: value.into(),
            ts_ns,
            trust: trust.clamp(0.0, 1.0),
            tags: tags.to_vec(),
            edges: BTreeMap::new(),
            node_id: uuid_v4_hex(),
        };
        if let Some(edges) = edges {
            for (kind, other) in edges {
                node.attach(*kind, other.clone());
            }
        }
        // Insert and unwrap. Insert always returns the new value.
        inner.nodes.insert(key.clone(), node);
        if !inner.order.contains(&key) {
            inner.order.push(key.clone());
        }
        let result = inner.nodes.get(&key).unwrap().clone();
        drop(inner);
        Ok(result)
    }

    pub fn supersede(
        &self,
        key: impl Into<String>,
        new_value: impl Into<serde_json::Value>,
        trust: f32,
    ) -> Result<MemoryNode, BrainError> {
        let key = key.into();
        let previous_ts = self.inner.read().nodes.get(&key).map(|n| n.ts_ns);
        let ts = previous_ts.map(|t| t + 1).unwrap_or_else(now_ns);
        let mut inner = self.inner.write();
        if let Some(prev_ts) = previous_ts {
            if let Some(prev) = inner.nodes.get_mut(&key) {
                // The previous node's ts is now older than the new write.
                // Stamp a supersedes edge on the previous reference.
                if prev.ts_ns < ts {
                    prev.attach(EdgeKind::Supersedes, key.clone());
                }
            }
        }
        let mut node = MemoryNode {
            key: key.clone(),
            value: new_value.into(),
            ts_ns: ts,
            trust: trust.clamp(0.0, 1.0),
            tags: Vec::new(),
            edges: BTreeMap::new(),
            node_id: uuid_v4_hex(),
        };
        if let Some(prev) = inner.nodes.get(&key) {
            node.attach(EdgeKind::Supersedes, prev.key.clone());
        }
        inner.nodes.insert(key.clone(), node);
        if !inner.order.contains(&key) {
            inner.order.push(key.clone());
        }
        Ok(inner.nodes.get(&key).unwrap().clone())
    }

    pub fn recall(&self, key: &str) -> Option<serde_json::Value> {
        self.inner.read().nodes.get(key).map(|n| n.value.clone())
    }

    pub fn get(&self, key: &str) -> Option<MemoryNode> {
        self.inner.read().nodes.get(key).cloned()
    }

    pub fn neighbours(&self, key: &str, kind: Option<EdgeKind>) -> Vec<String> {
        let inner = self.inner.read();
        let Some(node) = inner.nodes.get(key) else {
            return Vec::new();
        };
        match kind {
            Some(k) => node.edges.get(&k).cloned().unwrap_or_default(),
            None => node.edges.values().flatten().cloned().collect(),
        }
    }

    pub fn snapshot(&self) -> Vec<MemoryNode> {
        let inner = self.inner.read();
        inner
            .order
            .iter()
            .filter_map(|k| inner.nodes.get(k).cloned())
            .collect()
    }

    pub fn search(&self, tag: Option<&str>, min_trust: f32) -> Vec<MemoryNode> {
        let inner = self.inner.read();
        let mut out: Vec<MemoryNode> = inner
            .nodes
            .values()
            .filter(|n| n.trust >= min_trust)
            .filter(|n| tag.map_or(true, |t| n.tags.iter().any(|x| x == t)))
            .cloned()
            .collect();
        out.sort_by(|a, b| b.ts_ns.cmp(&a.ts_ns));
        out
    }

    pub fn forget(&self, key: &str) {
        let mut inner = self.inner.write();
        inner.nodes.remove(key);
        if let Some(idx) = inner.order.iter().position(|k| k == key) {
            inner.order[idx] = String::new();
        }
    }

    pub fn stats(&self) -> BrainStats {
        let inner = self.inner.read();
        let mut ts: Vec<HiveTimestamp> = inner.nodes.values().map(|n| n.ts_ns).collect();
        ts.sort_unstable();
        let kinds: std::collections::BTreeSet<EdgeKind> =
            inner.nodes.values().flat_map(|n| n.edges.keys()).copied().collect();
        BrainStats {
            node_count: inner.nodes.len(),
            oldest_ts_ns: ts.first().copied(),
            newest_ts_ns: ts.last().copied(),
            edge_kinds: kinds.into_iter().collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.read().nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub use BrainError as TimestampRegression;

fn uuid_v4_hex() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = ahash::RandomState::with_seeds(1, 2, 3, 4).build_hasher();
    h.write_u128(now_ns() as u128);
    let a = h.finish();
    h.write_u64(a);
    let b = h.finish();
    format!("{:012x}{:012x}", a & 0xFFFFFFFFFFFF, b & 0xFFFFFFFFFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn remember_then_recall() {
        let b = Brain::new();
        b.remember("endpoint", json!("/v1/chat"), 0.9).unwrap();
        assert_eq!(b.recall("endpoint").unwrap(), json!("/v1/chat"));
    }

    #[test]
    fn monotonic_regression_raises() {
        let b = Brain::new();
        b.remember_at("a", json!(1), 1.0, 1000, &[], None).unwrap();
        let err = b.remember_at("a", json!(2), 1.0, 999, &[], None).unwrap_err();
        assert!(matches!(err, BrainError::TimestampRegression { .. }));
    }

    #[test]
    fn snapshot_is_jsonable() {
        let b = Brain::new();
        b.remember("a", json!(1), 0.5).unwrap();
        b.remember("b", json!(2), 0.8).unwrap();
        let dump: Vec<serde_json::Value> =
            b.snapshot().iter().map(|n| n.to_dict()).collect();
        let s = serde_json::to_string(&dump).unwrap();
        let back: Vec<serde_json::Value> = serde_json::from_str(&s).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn search_orders_newest_first() {
        let b = Brain::new();
        b.remember_at("a", json!(1), 1.0, 1000, &[], None).unwrap();
        b.remember_at("b", json!(2), 1.0, 3000, &[], None).unwrap();
        b.remember_at("c", json!(3), 1.0, 2000, &[], None).unwrap();
        let keys: Vec<String> = b.search(None, 0.0).into_iter().map(|n| n.key).collect();
        assert_eq!(keys, vec!["b".to_string(), "c".to_string(), "a".to_string()]);
    }

    #[test]
    fn neighbours_walks_edge_kind() {
        let b = Brain::new();
        b.remember_at("a", json!(1), 1.0, 1000, &[], None).unwrap();
        b.remember_at("b", json!(2), 1.0, 1000, &[], None).unwrap();
        b.remember_at("c", json!(3), 1.0, 1000, &[], None).unwrap();
        b.remember_at(
            "a",
            json!(1),
            1.0,
            2000,
            &[],
            Some(&[(EdgeKind::RelatedTo, "b".into()), (EdgeKind::RelatedTo, "c".into())]),
        )
        .unwrap();
        let mut got = b.neighbours("a", Some(EdgeKind::RelatedTo));
        got.sort();
        assert_eq!(got, vec!["b".to_string(), "c".to_string()]);
        assert!(b.neighbours("a", Some(EdgeKind::CausedBy)).is_empty());
    }

    #[test]
    fn attach_dedupes() {
        let mut n = MemoryNode::new("a", json!(1));
        n.attach(EdgeKind::RelatedTo, "b");
        n.attach(EdgeKind::RelatedTo, "b");
        n.attach(EdgeKind::RelatedTo, "c");
        let v = n.edges.get(&EdgeKind::RelatedTo).unwrap();
        assert_eq!(v, &vec!["b".to_string(), "c".to_string()]);
    }
}
