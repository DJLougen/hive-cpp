# Cross-build notes for hive-cpp (aarch64 + Jetson / Grace)

`hive-cpp` is a Rust crate with **zero mandatory C deps** in the
default feature set. The cross-build path is therefore
"cross-compile and copy the binary" — no emulated wheel installs, no
sdist-with-Cargo shenanigans.

## Targets we care about

| Target                          | Use case                  | Wheel / runtime |
|---------------------------------|---------------------------|------------------|
| `aarch64-unknown-linux-gnu`     | Jetson Thor / Grace / Orin | glibc 2.31+      |
| `aarch64-unknown-linux-musl`    | Raspberry Pi 5 / Alpine   | musl             |
| `x86_64-unknown-linux-gnu`      | RTX 3090 / DGX Spark      | glibc            |
| `x86_64-apple-darwin`           | Mac dev                   | system libc      |

## Native build on Jetson Thor

```bash
# 1. Install the Jetson toolchain (JetPack 7 ships Rust 1.75+ via apt).
sudo apt-get install -y rustc cargo

# 2. Build the crate with the aarch64 target.
cargo build --release --target aarch64-unknown-linux-gnu

# 3. Run the macro bench on the device.
./target/aarch64-unknown-linux-gnu/release/hive-bench
```

The release binary is **fully static** under the default feature
set (no C deps, no dynamic loader). It runs on any aarch64 Linux
without extra libraries.

## Cross-build from x86_64 with qemu + buildx

```bash
# 1. Install qemu so aarch64 binaries can run under emulation.
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes

# 2. Create a buildx builder that can target arm64.
docker buildx create --use --name hive-arm

# 3. (Optional) Add the cross C toolchain so Cargo can link for
#    aarch64-unknown-linux-gnu. The image below carries it.
docker buildx build --platform linux/arm64 \
    -f Dockerfile.cross -t hive-cpp:aarch64 --load .

# 4. Run the resulting image.
docker run --rm --platform linux/arm64 hive-cpp:aarch64 hive-bench
```

## When to add `cuda` and `nvidia-ml` features

The default `cargo build` does not link CUDA or NVML. Those are
opt-in via Cargo features:

```bash
# Jetson / CUDA 13 dev box
cargo build --release --features cuda,nvidia-ml
```

Behind the `cuda` feature: `cudarc` or `cust` for the runtime API.
Behind the `nvidia-ml` feature: the `nvml` crate on x86, or
`libnvidia-ml.so` from the JetPack `cuda-nvml` package on aarch64.

Step 2 ships the **shape** (the `nvml::PowerSampler` API and a
`Stack` integration point) but not the bindings. Step 2 follow-up
PRs wire the bindings in.
