#!/usr/bin/env bash
# Cross-build the Rust daemon (kasumi-proxy) for the Android module (arm64 only)
# via cargo-ndk, placing the binary where the module bundle expects it.
#
# Requires: cargo-ndk, a Rust toolchain WITH the aarch64-linux-android std target
# and NDK_ROOT pointing at an Android NDK. CI wires these (see release.yml).
set -euo pipefail

ROOT="${PROJECT_ROOT:-$PWD}"
BIN="$ROOT/module/bin"
: "${NDK_ROOT:?set NDK_ROOT to an Android NDK}"
export ANDROID_NDK_HOME="$NDK_ROOT"

mkdir -p "$BIN/arm64-v8a"

# cargo-ndk selects the right clang linker per target from the NDK.
cargo ndk -t arm64-v8a build --release -p kasumi-daemon --bin kasumi-proxy

cp -f target/aarch64-linux-android/release/kasumi-proxy "$BIN/arm64-v8a/kasumi-proxy"
chmod 755 "$BIN/arm64-v8a/kasumi-proxy"

echo "✓ kasumi-proxy → module/bin/arm64-v8a/"
