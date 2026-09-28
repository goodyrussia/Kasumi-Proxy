#!/usr/bin/env bash
# ============================================================
# scripts/check-binary-compat.sh
# Run the config-validation harness against the REAL pinned Xray core: every
# config our generators emit is fed to `xray run -test`, so a core whose config
# schema drifted from the generators (a config the core now rejects) fails
# loudly instead of shipping in a module.
#
# Used two ways:
#   - CI (ci.yml, release.yml) gates on it, so a bad core pin never ships.
#   - Locally: run it after bumping XRAY_TAG/XRAY_ZIP_SHA256 in
#     scripts/binary-versions.sh.
#
# The core under test is compiled for the host (linux) from the pinned source —
# the harness runs it locally, not on-device. Override the pin via the usual env
# vars (XRAY_TAG, XRAY_ZIP_SHA256, …) to test a candidate core.
#
# Usage: scripts/check-binary-compat.sh
# ============================================================
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

XRAY_BIN="${XRAY_BIN:-$ROOT/.cache/xray-bin/xray-linux-$([ "$(uname -m)" = "aarch64" ] && echo arm64 || echo amd64)}"

# Build the pinned core for the host unless a prebuilt one is supplied.
if [ ! -x "$XRAY_BIN" ]; then
	case "$(uname -m)" in
	aarch64) GOARCH_ARG=arm64 ;;
	*) GOARCH_ARG=amd64 ;;
	esac
	bash "$ROOT/scripts/build-xray.sh" linux "$GOARCH_ARG" "$XRAY_BIN"
fi

# The harness (crates/kasumi-core/tests/core_validation.rs) validates every
# generated config against the core at $KASUMI_XRAY_BIN and fails on the first
# rejection.
export KASUMI_XRAY_BIN="$XRAY_BIN"
echo "→ validating generated configs against $("$XRAY_BIN" version | head -n1)"
cargo test --manifest-path "$ROOT/Cargo.toml" \
	-p kasumi-core --test core_validation -- --nocapture
