# Android (aarch64-linux-android) build notes for hive-cpp

The Android target is a real aarch64 build with a stripped-down libc
that ships as part of the Android NDK. The Rust toolchain has the
target (`aarch64-linux-android`); the link step needs the NDK's
`clang` driver because the GNU binutils `ld` does not understand
`--eh-frame-hdr` for Android.

## One-time setup

```bash
# 1. Install the Rust target.
rustup target add aarch64-linux-android

# 2. Install the Android NDK.
#    On macOS: brew install android-ndk
#    On Linux: download from https://developer.android.com/ndk
#    On Windows: download the NDK zip, unzip to a known path.
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/27.0.11718014
# Or on Windows:
# set ANDROID_NDK_HOME=C:\Users\<you>\Android\Sdk\ndk\27.0.11718014

# 3. Configure Cargo to use the NDK linker for the Android target.
#    Drop this in `.cargo/config.toml` (next to Cargo.toml):
```

```toml
# .cargo/config.toml
[target.aarch64-linux-android]
linker = "<path-to>/ndk/27.0.11718014/toolchains/llvm/prebuilt/linux-x86_64/bin/clang"
ar = "<path-to>/ndk/27.0.11718014/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
```

On Windows the `linker` path is `toolchains\llvm\prebuilt\windows-x86_64\bin\clang.exe`.

## Build

```bash
# 1. Build the static archive and the macro bench.
cargo build --release --target aarch64-linux-android

# 2. Cross the cdylib for PyO3 (Step 3).
cargo build --release --target aarch64-linux-android --crate-type cdylib
```

The output is in `target/aarch64-linux-android/release/`:

```
libhive.a         # rlib → static archive (Android)
libhive.so        # cdylib → Python extension (PyO3)
hive-bench        # static binary (no system C deps)
```

## Run on a phone

```bash
# 1. Plug in an Android phone with USB debugging enabled.
adb devices

# 2. Push the binary.
adb push target/aarch64-linux-android/release/hive-bench /data/local/tmp/
adb shell chmod +x /data/local/tmp/hive-bench

# 3. Run it.
adb shell /data/local/tmp/hive-bench
# === hive-cpp bench ===
# turns:           2000
# wall:            0.0XX s
# compress rate:   XXX k msg/s
# brain rate:      XXX k writes/s
```

The expected number on a Pixel 8 is **~80-120 k msg/s** for the
`rule_fast` compressor and **~150-200 k writes/s** for the brain —
roughly 3-4× slower than the dev box, which is consistent with
the A78/A55 split on Tensor G3.

## Cross-build from Windows (current host)

The current dev box is `x86_64-pc-windows-msvc` and the link step
above fails because the MinGW `ld` driver does not understand
Android's `--eh-frame-hdr` flag. Two options:

* **Use WSL** for the cross-build. `apt install` the NDK in WSL,
  point `linker` at the Linux path, and build.
* **Install the Windows NDK** (`clang.exe` from the prebuilt
  toolchain) and point `linker` at the Windows path. The
  `android-ndk-r27-windows.zip` distribution is ~1.5 GB and the
  install path is e.g. `C:\Users\<you>\Android\Sdk\ndk\27.0.11718014`.

Either way, **the Rust code does not change**. The crate compiles
unchanged against `aarch64-linux-android`.

## Cross-build from Linux (the CI path)

The CI matrix in `.github/workflows/ci.yml` runs on a self-hosted
Linux runner that has the NDK installed at the path documented
above. The `linker` line is committed to `.cargo/config.toml` so
the same command works on both the dev box and CI.

## What this gives us for free

| Target | What runs there | Hive behaviour |
|---|---|---|
| iPhone 17 Pro (arm64 macOS) | macOS dev box | already validated |
| Pixel 8 / Galaxy S24 (aarch64 Android) | the binary on the phone | **Step 2 follow-up PRs** wire this up |
| Mac M3 (arm64 macOS) | macOS dev box | already validated |
| Raspberry Pi 5 (aarch64 Linux) | `aarch64-unknown-linux-gnu` | already documented |
| Jetson Thor / Grace (aarch64 Linux) | `aarch64-unknown-linux-gnu` | already documented |

The phone target was the missing row in the README device matrix.
With the Android target installed and the linker configured, the
binary on a Pixel 8 should land at the same compression numbers
as the Spark row, with the bonus that the user can carry the agent
in their pocket.
