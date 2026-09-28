#!/usr/bin/env bash
# ============================================================
# scripts/build-xray.sh
# Build the pinned Xray core (eichgee/Xray-core) from source for one
# GOOS/GOARCH pair. The fork publishes no release assets, so this is the only
# way to produce the core; CI uses it for the Android module payload and for
# the config-compat harness (a linux/amd64 build).
#
# The source archive is downloaded once, verified against the pinned sha256
# (scripts/binary-versions.sh), extracted into a cache dir and reused.
#
# Usage: scripts/build-xray.sh <goos> <goarch> <output-path>
#   scripts/build-xray.sh android arm64 module/bin/arm64-v8a/xray
#   scripts/build-xray.sh linux amd64 /tmp/xray-linux-amd64   # compat harness
#
# Honours PROJECT_ROOT (else the repo root); the cache lives in
# $XRAY_SRC_CACHE (default <root>/.cache/xray-src, git-ignored).
# ============================================================
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/binary-versions.sh
. "$HERE/binary-versions.sh"
ROOT="${PROJECT_ROOT:-$(cd "$HERE/.." && pwd)}"

GOOS_ARG="${1:?usage: build-xray.sh <goos> <goarch> <output-path>}"
GOARCH_ARG="${2:?usage: build-xray.sh <goos> <goarch> <output-path>}"
OUT="${3:?usage: build-xray.sh <goos> <goarch> <output-path>}"

need() { command -v "$1" >/dev/null 2>&1 || {
	echo "❌ missing dependency: $1" >&2
	exit 1
}; }
need go
need curl
need sha256sum
need unzip

CACHE="${XRAY_SRC_CACHE:-$ROOT/.cache/xray-src}"
SRC="$CACHE/${XRAY_REPO#*/}-$XRAY_TAG"
ZIP="$CACHE/xray-$XRAY_TAG.zip"
URL="https://github.com/$XRAY_REPO/archive/refs/tags/$XRAY_TAG.zip"

mkdir -p "$CACHE"

if [ ! -d "$SRC" ]; then
	echo "→ fetching pinned Xray source: $XRAY_REPO $XRAY_TAG"
	if [ ! -f "$ZIP" ]; then
		curl -fL --retry 3 -o "$ZIP.part" "$URL"
		mv "$ZIP.part" "$ZIP"
	fi
	echo "$XRAY_ZIP_SHA256  $ZIP" | sha256sum -c --quiet - || {
		echo "❌ sha256 mismatch for $ZIP — expected $XRAY_ZIP_SHA256" >&2
		echo "   delete the file and retry; if it persists the upstream tag moved." >&2
		exit 1
	}
	unzip -q -o "$ZIP" -d "$CACHE/extract"
	inner="$(find "$CACHE/extract" -mindepth 1 -maxdepth 1 -type d | head -n1)"
	[ -n "$inner" ] || {
		echo "❌ no source dir inside $ZIP" >&2
		exit 1
	}
	mv "$inner" "$SRC"
	rm -rf "$CACHE/extract"
fi

echo "→ building xray $XRAY_TAG ($GOOS_ARG/$GOARCH_ARG) → $OUT"

# ── Toolchain selection ──────────────────────────────────────────────
# Android builds MUST use CGO. Go's pure resolver cannot reach Android's
# system DNS (netd): with CGO_ENABLED=0 the core reads resolv.conf, tries
# 127.0.0.1 / [::1]:53 and every server-name bootstrap lookup dies with
# "connection refused" (nothing listens there on Android). Upstream Xray
# builds its Android assets with CGO_ENABLED=1 + NDK clang (API level 24);
# mirror that exactly.
CC_TARGET=""
LDFLAGS="-s -w -buildid="
if [ "$GOOS_ARG" = "android" ]; then
	NDK="${NDK_ROOT:-${ANDROID_NDK_HOME:-${ANDROID_NDK:-}}}"
	if [ -z "$NDK" ] || [ ! -d "$NDK" ]; then
		echo "❌ android builds need the NDK: set NDK_ROOT (or ANDROID_NDK_HOME)" >&2
		exit 1
	fi
	case "$GOARCH_ARG" in
	arm64) TRIPLE=aarch64-linux-android ;;
	amd64) TRIPLE=x86_64-linux-android ;;
	*)
		echo "❌ unsupported android arch: $GOARCH_ARG" >&2
		exit 1
		;;
	esac
	CC_TARGET="$(find "$NDK/toolchains/llvm/prebuilt" -path "*/bin/${TRIPLE}24-clang" 2>/dev/null | head -n1)"
	if [ -z "$CC_TARGET" ]; then
		echo "❌ no ${TRIPLE}24-clang under $NDK/toolchains/llvm/prebuilt" >&2
		exit 1
	fi
	LDFLAGS="-s -w -buildid= -checklinkname=0"
	echo "→ NDK clang: $CC_TARGET"
fi

mkdir -p "$(dirname "$OUT")"
(
	cd "$SRC"
	if [ "$GOOS_ARG" = "android" ]; then
		CGO_ENABLED=1 CC="$CC_TARGET" GOOS="$GOOS_ARG" GOARCH="$GOARCH_ARG" \
			go build -mod=readonly -trimpath -buildvcs=false \
			-ldflags="$LDFLAGS" -o "$OUT" ./main
	else
		CGO_ENABLED=0 GOOS="$GOOS_ARG" GOARCH="$GOARCH_ARG" \
			go build -mod=readonly -trimpath -buildvcs=false \
			-ldflags="$LDFLAGS" -o "$OUT" ./main
	fi
)
chmod 755 "$OUT"
printf '   %s\n' "$(du -h "$OUT" | cut -f1) $OUT"
if [ "$GOOS_ARG" = "linux" ] && [ "$GOARCH_ARG" = "$(go env GOARCH)" ]; then
	"$OUT" version | head -n1
fi
