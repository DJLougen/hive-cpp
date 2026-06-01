//! PyO3 binding for the hive-cpp crate.
//!
//! Step 3 wires the Rust runtime behind the Python `hive` meta-package's
//! `rule_fast`, `rust_brain`, and `Stack` paths. The Python class
//! shapes are preserved byte-for-byte; calls land in Rust, which
//! closes the perf gap on the hot path.
//!
//! This file is the **plan** — it does not yet compile because
//! PyO3 is not in the default feature set. Step 3 enables the
//! `pyo3` feature and the `maturin` build step; everything else
//! stays the same.

#![cfg_attr(feature = "pyo3", allow(unsafe_op_in_unsafe_fn))]

#[cfg(feature = "pyo3")]
mod inner {
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    use crate::brain::{Brain, EdgeKind, MemoryNode};
    use crate::compress::{Message, RuleFast};
    use crate::stack::Stack;

    /// Python wrapper for [`Brain`]. The class shape mirrors
    /// `hive.rust_brain.RustBrain` so the Python integration example
    /// can swap in the Rust backend with no call-site change.
    #[pyclass(name = "RustBrain")]
    pub struct PyBrain {
        inner: Brain,
    }

    #[pymethods]
    impl PyBrain {
        #[new]
        fn new() -> Self {
            Self { inner: Brain::new() }
        }

        #[staticmethod]
        fn with_monotonic(enforce: bool) -> Self {
            Self { inner: Brain::new().with_monotonic(enforce) }
        }

        fn remember(&self, key: &str, value: &PyAny, trust: f32) -> PyResult<PyObject> {
            let v: serde_json::Value = pythonize::from_python(value)?;
            let node = self
                .inner
                .remember(key, v, trust)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            Ok(node_to_dict(&node, value.py()))
        }

        fn supersede(&self, key: &str, value: &PyAny, trust: f32) -> PyResult<PyObject> {
            let v: serde_json::Value = pythonize::from_python(value)?;
            let node = self
                .inner
                .supersede(key, v, trust)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            Ok(node_to_dict(&node, value.py()))
        }

        fn recall(&self, key: &str) -> PyResult<PyObject> {
            Python::with_gil(|py| {
                let v = self.inner.recall(key);
                Ok(pythonize::to_python(py, &v)?)
            })
        }

        fn get(&self, key: &str) -> PyResult<PyObject> {
            Python::with_gil(|py| {
                let node = self.inner.get(key);
                let v = node.map(|n| n.to_dict());
                Ok(pythonize::to_python(py, &v)?)
            })
        }

        fn neighbours(&self, key: &str, kind: Option<&str>) -> Vec<String> {
            let k = kind.and_then(|s| match s {
                "related_to" => Some(EdgeKind::RelatedTo),
                "caused_by" => Some(EdgeKind::CausedBy),
                "supersedes" => Some(EdgeKind::Supersedes),
                "attached_to" => Some(EdgeKind::AttachedTo),
                _ => None,
            });
            self.inner.neighbours(key, k)
        }

        fn search(&self, tag: Option<&str>, min_trust: f32) -> PyResult<PyObject> {
            Python::with_gil(|py| {
                let nodes = self.inner.search(tag, min_trust);
                let dicts: Vec<_> = nodes.iter().map(|n| n.to_dict()).collect();
                Ok(pythonize::to_python(py, &dicts)?)
            })
        }

        fn forget(&self, key: &str) {
            self.inner.forget(key);
        }

        fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
            let stats = self.inner.stats();
            let d = PyDict::new_bound(py);
            d.set_item("node_count", stats.node_count)?;
            d.set_item("oldest_ts_ns", stats.oldest_ts_ns)?;
            d.set_item("newest_ts_ns", stats.newest_ts_ns)?;
            d.set_item(
                "edge_kinds",
                stats
                    .edge_kinds
                    .iter()
                    .map(|k| k.as_str().to_string())
                    .collect::<Vec<_>>(),
            )?;
            Ok(d)
        }

        fn __len__(&self) -> usize {
            self.inner.len()
        }

        fn __contains__(&self, key: &str) -> bool {
            self.inner.get(key).is_some()
        }
    }

    /// Python wrapper for [`RuleFast`]. Mirrors
    /// `hive.rule_fast.RuleFastHoneyComb`.
    #[pyclass(name = "RuleFastHoneyComb")]
    pub struct PyRuleFast {
        inner: RuleFast,
    }

    #[pymethods]
    impl PyRuleFast {
        #[new]
        fn new() -> Self {
            Self { inner: RuleFast::new() }
        }

        fn process(&mut self, role: &str, content: &str) -> PyResult<PyObject> {
            let m = Message::new(role, content);
            let out = self.inner.process(&m);
            Python::with_gil(|py| {
                let d = PyDict::new_bound(py);
                d.set_item("role", out.role.clone())?;
                d.set_item("content", out.content.clone())?;
                d.set_item("label", out.label.value())?;
                d.set_item("content_type", out.content_type.as_str())?;
                d.set_item("original_tokens", out.original_tokens)?;
                d.set_item("compressed_tokens", out.compressed_tokens)?;
                Ok(d.unbind())
            })
        }

        fn get_stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
            let stats = self.inner.stats();
            let d = PyDict::new_bound(py);
            d.set_item("turn_count", stats.turn_count)?;
            d.set_item("total_entries", stats.total_entries)?;
            d.set_item("active_entries", stats.active_entries)?;
            d.set_item("total_tokens", stats.total_tokens)?;
            d.set_item("original_tokens", stats.original_tokens)?;
            d.set_item("compression_ratio", stats.compression_ratio)?;
            d.set_item(
                "labels",
                stats
                    .labels
                    .iter()
                    .map(|(k, v)| (k.value().to_string(), *v))
                    .collect::<std::collections::HashMap<_, _>>(),
            )?;
            Ok(d)
        }
    }

    /// Python wrapper for [`Stack`]. Mirrors `hive.stack.HiveStack`.
    #[pyclass(name = "HiveStack")]
    pub struct PyStack {
        inner: Stack,
    }

    #[pymethods]
    impl PyStack {
        #[new]
        fn new() -> Self {
            Self { inner: Stack::new() }
        }

        fn route<'py>(&self, py: Python<'py>, state: &PyAny) -> PyResult<Bound<'py, PyDict>> {
            let state: serde_json::Value = pythonize::from_python(state)?;
            let d = self.inner.route(&state);
            let out = PyDict::new_bound(py);
            out.set_item("tool", d.tool.clone())?;
            out.set_item("args", d.args.clone())?;
            out.set_item("confidence", d.confidence)?;
            out.set_item("escalated", d.escalated)?;
            out.set_item("source", d.source.clone())?;
            Ok(out)
        }

        fn compress(&mut self, role: &str, content: &str) -> PyResult<PyObject> {
            let out = self.inner.compress(role, content);
            Python::with_gil(|py| {
                let d = PyDict::new_bound(py);
                d.set_item("role", out.role.clone())?;
                d.set_item("content", out.content.clone())?;
                d.set_item("label", out.label.value())?;
                d.set_item("content_type", out.content_type.as_str())?;
                d.set_item("original_tokens", out.original_tokens)?;
                d.set_item("compressed_tokens", out.compressed_tokens)?;
                Ok(d.unbind())
            })
        }

        fn remember(&self, key: &str, value: &PyAny, trust: f32) -> PyResult<()> {
            let v: serde_json::Value = pythonize::from_python(value)?;
            self.inner
                .remember(key, v, trust)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            Ok(())
        }

        fn recall<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Bound<'py, PyAny>> {
            let v = self.inner.recall(key);
            Ok(pythonize::to_python(py, &v)?.into_bound(py))
        }

        fn step<'py>(
            &mut self,
            py: Python<'py>,
            state: &PyAny,
            transcript: &PyAny,
        ) -> PyResult<Bound<'py, PyDict>> {
            let state: serde_json::Value = pythonize::from_python(state)?;
            let transcript: Vec<(String, String)> = pythonize::from_python(transcript)?;
            let result = self.inner.step(&state, &transcript);
            let d = PyDict::new_bound(py);
            let dec = PyDict::new_bound(py);
            dec.set_item("tool", result.decision.tool.clone())?;
            dec.set_item("args", result.decision.args.clone())?;
            dec.set_item("confidence", result.decision.confidence)?;
            dec.set_item("escalated", result.decision.escalated)?;
            dec.set_item("source", result.decision.source.clone())?;
            d.set_item("decision", dec)?;
            if let Some(c) = result.compressed {
                let cd = PyDict::new_bound(py);
                cd.set_item("role", c.role.clone())?;
                cd.set_item("content", c.content.clone())?;
                cd.set_item("label", c.label.value())?;
                cd.set_item("content_type", c.content_type.as_str())?;
                cd.set_item("original_tokens", c.original_tokens)?;
                cd.set_item("compressed_tokens", c.compressed_tokens)?;
                d.set_item("compressed", cd)?;
            } else {
                d.set_item("compressed", py.None())?;
            }
            d.set_item("stats", pythonize::to_python(py, &result.stats)?.into_bound(py))?;
            Ok(d)
        }
    }

    /// The Python module entry point. The module is named `hive._native`
    /// so the Python side can `from hive import _native` and call into
    /// the Rust runtime with zero overhead.
    #[pymodule]
    pub fn _native(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<PyBrain>()?;
        m.add_class::<PyRuleFast>()?;
        m.add_class::<PyStack>()?;
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        Ok(())
    }

    fn node_to_dict(node: &MemoryNode, py: Python<'_>) -> PyObject {
        // The `to_dict()` output is already a JSON-compatible value; we
        // hand it through `pythonize` so the Python side gets the
        // right type (dict, list, str, int, float, bool, None).
        let d = node.to_dict();
        pythonize::to_python(py, &d).expect("pythonize")
    }
}
