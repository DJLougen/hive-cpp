# AGENTS.md — setup and conventions for the `hive-cpp` crate

This is the setup file an AI coding agent reads first. If you are a
human, the same content is in `README.md`, `docs/cross-build.md`,
and `docs/android-build.md`; start there instead.

## What this crate is

`hive-cpp` is the **native Rust port** of the Hive agent memory stack
defined at <https://github.com/DJLougen/hive>. The Python meta-package
is the source of truth; this crate is the runtime that the Python
shim will eventually call into via PyO3 (Step 3).

The `crate::Brain`, `crate::RuleFast`, and `crate::Stack` types are
**1:1 with the Python sibling**. The wire format is the same JSON
shape, so a Hermes agent that talks to `hive.rust_brain.RustBrain`
today can switch to the Rust backend with no call-site change.

## First-time setup

```bash
# 1. Install Rust (stable) and the three targets we ship in CI.
rustup target add aarch64-unknown-linux-gnu
rustup target add aarch64-linux-android
# (The default x86_64-pc-windows-msvc target is already installed.)

# 2. Clone all three repos side by side.
git clone https://github.com/DJLougen/hive
git clone https://github.com/DJLougen/busyBee-cpu
git clone https://github.com/DJLougen/honey-comb
git clone https://github.com/DJLougen/hive-cpp
cd hive-cpp

# 3. Build and test.
cargo build --release
cargo test --all-targets           # 31 tests, < 1s
cargo clippy --all-targets -- -D warnings

# 4. Run the macro bench.
cargo run --release --bin hive-bench
# === hive-cpp bench ===
# turns:           2000
# wall:            0.008 s
# compress rate:   240880 msg/s
# brain rate:      240880 writes/s
# compression:      1.00x (206750 → 206750 tok)
# brain nodes:     2000
```

## Cross-build (Jetson Thor / Grace / RPi 5)

The `aarch64-unknown-linux-gnu` target is in `.cargo/config.toml`
with `linker = "aarch64-linux-gnu-gcc"`. On Ubuntu / WSL:

```bash
sudo apt-get install -y gcc-aarch64-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

The binary at `target/aarch64-unknown-linux-gnu/release/hive-bench`
is **fully static** under the default feature set (no C deps, no
dynamic loader). It runs on any aarch64 Linux without extra
libraries.

Expected numbers on Jetson Thor (aarch64, Cortex-A78AE):

| Component | Rate | Notes |
|---|---:|---|
| `compress::RuleFast::process` | ~80 k msg/s | 3-4× slower than x86_64 |
| `brain::Brain::remember` | ~250 k writes/s | lock-free snapshot reads scale linearly |

## Cross-build (Android phone — Pixel 8 / S24)

The `aarch64-linux-android` target needs the Android NDK because
the GNU `ld` driver does not understand `--eh-frame-hdr` for
Android ELF.

```bash
# 1. Install the NDK.
#    macOS:   brew install android-ndk
#    Linux:   download from https://developer.android.com/ndk
#    Windows: download android-ndk-r27-windows.zip
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.0.11718014

# 2. Tell Cargo which clang to use.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/clang
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar

# 3. Build.
cargo build --release --target aarch64-linux-android

# 4. Push to a phone and run.
adb push target/aarch64-linux-android/release/hive-bench /data/local/tmp/
adb shell chmod +x /data/local/tmp/hive-bench
adb shell /data/local/tmp/hive-bench
```

Expected on a Pixel 8 (Tensor G3, A78/A55 big-little): the same
`hive-bench` shape with 3-4× the wall time of the dev box.

## Coding conventions

- **Python 3.10+ / Rust 1.74+**. Stick to whatever compiles on the
  dev box without any special flags.
- **One responsibility per module.** `brain` is a graph store;
  `compress` is a regex compressor; `nvml` is a sampler. No module
  imports the others unless it has to.
- **Public surface is structs + functions.** No magic methods
  beyond what `serde`, `parking_lot`, or the stdlib gives us. No
  metaclass / trait-object magic.
- **Comments only where the code is non-obvious.** "Why" is
  welcome; "what" is usually a re-statement of the code.
- **Tests cover invariants, not defaults.** A test that breaks
  when the default value changes is a brittle test. Assert logical
  behaviour. The current test suite has 31 tests; they live in
  `#[cfg(test)] mod tests` blocks at the bottom of each module.

## Module map

| Module | What it does | Status |
|---|---|---|
| `brain` | Timestamp-protected graph memory. `Brain`, `MemoryNode`, `EdgeKind`, `BrainError`, `BrainStats`. | **done** |
| `compress` | `RuleFast`: regex classifier + deterministic compressor. `Message`, `CompressedMessage`, `ContentType`, `Label`, `Comb`. | **done** |
| `busybee` | CPU action router. `Policy`, `Prediction`. | stub (returns `escalate`); TF-IDF + ensemble port is a follow-up |
| `llm` | OpenAI-compat HTTP client. `chat()` over stdlib TCP; `ChatRequest` / `ChatResponse` / `ChatMessage`. | **done** (non-streaming) |
| `nvml` | `PowerSampler` + `estimate_cpu_energy_joules` TDP fallback. | stub (no-op); real `nvml` crate behind `nvidia-ml` feature |
| `stack` | The orchestrator. `Stack`, `RouteDecision`, `StepResult`, `Stats`. | **done** |
| `pyo3` | PyO3 binding behind the `pyo3` Cargo feature. | planned (Step 3) |
| `bin/hive_bench` | Macro benchmark binary, mirrors the Python `hive_benchmark.py` shape. | **done** |
| `bin/hive_server` | Placeholder for the future in-process agent server. | stub (returns exit 1) |

## Commit message convention

Conventional Commits:

```
<type>(<scope>): <description>

[body]

[footer]
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`.

## Pull-request workflow

1. **Open an issue first** for anything beyond a small bug fix. The
   project is small enough that an RFC-style discussion in an
   issue is faster than back-and-forth on a PR.
2. **Branch off `main`.** Use a descriptive name
   (`feat/...`, `fix/...`, `docs/...`).
3. **Keep the diff focused.** A PR that touches 9 files for one
   feature is a PR that needs to be split.
4. **Add tests.** If you change a public function, add or update a
   test in the module's `#[cfg(test)] mod tests` block.
5. **Run `cargo test` and `cargo clippy -- -D warnings` locally
   before pushing.** Both should pass cleanly.
6. **One approval from a maintainer.** The project is maintained
   by @DJLougen.

## Out of scope

- **Submitting AI-generated PRs wholesale.** AI is welcome as a
  tool, but the author must be able to defend the change in
  review. The same policy the upstream `llama.cpp` project
  follows.
- **Reorganising the crate layout.** The
  `src/{label,brain,compress,busybee,llm,nvml,stack,pyo3,util}/`
  + `src/bin/{hive_bench,hive_server}.rs` split is intentional and
  not under negotiation.

## Reporting bugs

Use the GitHub issue tracker at
<https://github.com/DJLougen/hive-cpp/issues>. Include:

- The crate version (`cargo run --bin hive-bench` will print it
  via `hive::VERSION`),
- The host (Windows / Linux / macOS, aarch64 / x86_64),
- The Rust toolchain (`rustc --version`),
- A minimal reproducer (a `cargo test` case is ideal).

For security-sensitive issues, see `SECURITY.md` in the parent
`hive/` repo.
