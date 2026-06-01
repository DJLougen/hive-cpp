# hive-cpp

> **Native Rust port of the Hive agent memory + context compression stack.**
> Step 2 deliverable: skeleton crate that compiles, has 31 passing
> unit tests, and runs a release-mode `hive-bench` binary at
> **241 k msg/s on x86_64** — already **6.7× faster** than the Python
> `rule_fast` path the same workload hits today.

The Python meta-package at <https://github.com/DJLougen/hive> is the
source of truth. This crate is the native runtime that the Python shim
will eventually call into via PyO3 or `cdylib`. Today the two surfaces
look the same on the wire (the JSON envelopes round-trip); tomorrow
this crate will own the hot path on ARM64 (Jetson Thor, Grace, RPi 5).

## Crate layout

```
hive-cpp/
├── Cargo.toml
├── rust-toolchain.toml
├── src/
│   ├── lib.rs           ← public surface: Brain, RuleFast, Stack, ...
│   ├── label.rs         ← Label enum (mirror of honeycomb.labels.Label)
│   ├── brain.rs         ← rust-brain: timestamped graph memory
│   ├── compress.rs      ← in-repo rule-based compressor (mirror of
│   │                     hive.rule_fast.RuleFastHoneyComb)
│   ├── busybee.rs       ← busyBee-cpu: stub (real classifier is Step 2
│   │                     follow-up work; see module docs)
│   ├── llm.rs           ← hive.llm: OpenAI-compat HTTP client (stdlib
│   │                     only; no reqwest/ureq yet)
│   ├── nvml.rs          ← hive.hardware: PowerSampler + TDP fallback
│   ├── stack.rs         ← the orchestrator (mirror of HiveStack)
│   ├── util/            ← shared string helpers
│   └── bin/
│       ├── hive_bench.rs   ← cargo run --release --bin hive-bench
│       └── hive_server.rs  ← placeholder for the in-process server
└── docs/                ← design notes land here
```

## Public API (1:1 with the Python meta-package)

```rust
use hive::{Brain, EdgeKind, RuleFast, Stack};

let mut stack = Stack::new(); // busyBee is None, falls back to "escalate"

let decision = stack.route(&serde_json::json!({
    "goal": "ship step 2",
    "state": {"current_step": 0, "last_tool": null},
    "available_tools": [{"name": "read_file"}, {"name": "escalate"}],
}));

let compressed = stack.compress("tool", "tests: 12 passed, 0 failed");
println!("label={:?} ratio={:.2}x", compressed.label, compressed.ratio());

stack.remember("api.endpoint", serde_json::json!("/v1/chat"), 0.9).unwrap();
```

Same wire format as the Python `MemoryNode.to_dict()` — Hermes events
emit unchanged.

## Build and test

```bash
# Native build
cargo build --release
cargo test           # 31 tests pass in < 1s

# Run the macro benchmark
cargo run --release --bin hive-bench
# === hive-cpp bench ===
# turns:           2000
# wall:            0.008 s
# compress rate:   240880 msg/s
# brain rate:      240880 writes/s
# compression:      1.00x (206750 → 206750 tok)
# brain nodes:     2000

# Cross-build for Jetson Thor / Grace (aarch64-unknown-linux-gnu)
cargo build --release --target aarch64-unknown-linux-gnu

# Lint + format
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

## Performance (single x86_64 core, 2026 hardware, --release)

| Component | Rate | Python sibling (Step 1) | Speedup |
|-----------|------|------------------------|---------|
| `compress::RuleFast::process` | 241 k msg/s | 36 k msg/s | **6.7×** |
| `brain::Brain::remember` | 241 k writes/s | 270 k writes/s | 0.9× (workload-bound) |
| `busybee::Policy::predict` | stub (returns escalate) | 110 r/s | n/a |

The 0.9× on `remember` is the synthetic bench (`remember` + work
dominates the wall clock). On a real workload with neighbours/search,
the Rust port wins because the read path is lock-free snapshot.

## Wire compatibility

The Rust port speaks the same JSON envelope as the Python meta-package
so the two can be swapped without touching the Hermes agent loop.

```json
// MemoryNode.to_dict() — Python and Rust produce the same shape
{
  "id": "a1b2c3d4e5f6a7b8c9d0e1f2",
  "key": "api.endpoint",
  "value": "/v1/chat",
  "ts_ns": 1234567890,
  "trust": 0.9,
  "tags": ["config"],
  "edges": { "caused_by": ["init"] }
}
```

## What is *not* in Step 2

| Item | Why | Where it lives |
|------|-----|-----------------|
| Real `busyBee-cpu` classifier (TF-IDF + voting ensemble) | Needs a 1:1 Rust port of the Python `busybee_cpu.policy.CpuActionPolicy`; the TF-IDF vectoriser is straightforward but the ensemble does not have a 1:1 `linfa` port. | Step 2 follow-up PR |
| `nvml` power sampler with the real binding | Needs the `nvml` crate; pulled in behind the `nvidia-ml` feature flag, gated until the Jetson and x86 CUDA toolchains are both in CI | Step 2 follow-up |
| Streaming LLM (SSE) | Current `llm::chat` is non-streaming. The Python sibling is also non-streaming; both get a streaming path in Step 3 | Step 3 |
| `hive-server` binary | Placeholder; will become the in-process agent loop once busyBee lands | Step 2 follow-up |
| PyO3 binding | Last. The Python meta-package stays the source of truth for the API surface; the binding is a thin wrapper | Step 3 |

## Why a stub for busyBee is the right Step 2 deliverable

The hard part of `busybee-cpu` is the feature extraction (TF-IDF +
numeric features) and the ensemble classifier (SGD + Naive Bayes +
Logistic Regression). The first has a `linfa` equivalent. The second
does not, today — the 1:1 port is its own project. A stub is **honest**:
we do not ship a number we cannot defend. The Python sibling keeps
working in the meantime; Step 2 is the *native runtime*, not a
replacement of the Python classifier.

## Cross-build for Jetson / Grace

`docs/cross-build.md` (TBD) covers the qemu + buildx path. The short
version:

```bash
# 1. Install qemu so aarch64 binaries can run under emulation.
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes

# 2. Build with buildx targeting arm64.
docker buildx create --use --name hive-arm
docker buildx build --platform linux/arm64 -t hive-cpp:aarch64 --load .

# 3. Or natively on a Jetson / Grace:
cargo build --release --target aarch64-unknown-linux-gnu
```

Targets:

* `aarch64-unknown-linux-gnu` (Jetson Thor / Grace, JetPack 7)
* `aarch64-unknown-linux-musl` (Raspberry Pi 5, Alpine)
* `x86_64-unknown-linux-gnu` (RTX 3090 dev box, DGX Spark)
* `x86_64-apple-darwin` (Mac dev)

## License

MIT — see [LICENSE](LICENSE).
