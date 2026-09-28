#!/usr/bin/env bash
# ============================================================
# scripts/fetch-binaries.sh
# Stage the module's native binaries for Android arm64 into module/bin/arm64-v8a/:
#
#   xray                — built from the pinned eichgee/Xray-core source
#                         (scripts/build-xray.sh; there are no release assets)
#   tun2socks           — downloaded release asset (catalogued in scripts/binaries.json)
#   hev-socks5-tunnel   — downloaded release asset (catalogued in scripts/binaries.json)
#
# The kasumi-proxy daemon is built separately (scripts/build-daemon-android.sh).
# These binaries are NOT committed (.gitignore).
#
# Usage: scripts/fetch-binaries.sh android
# ============================================================
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/binary-versions.sh
. "$HERE/binary-versions.sh"
CATALOG="$HERE/binaries.json"
ROOT="${PROJECT_ROOT:-$(cd "$HERE/.." && pwd)}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

need() { command -v "$1" >/dev/null 2>&1 || {
	echo "❌ missing dependency: $1" >&2
	exit 1
}; }
need curl
need jq
need unzip

dl() {
	echo "  ↓ $1" >&2
	curl -fL --retry 3 -o "$2" "$1"
}

# Download + extract the catalogued binary for <arch> into $TMP/<core>-<arch>/,
# then copy it to <dest>. A 'raw' asset IS the binary.
stage_core() { # <core> <arch> <dest>
	local core="$1" arch="$2" dest="$3"
	local repo vvar member file archive tag ver url dir
	repo=$(jq -r --arg c "$core" '.[$c].repo' "$CATALOG")
	vvar=$(jq -r --arg c "$core" '.[$c].version_var' "$CATALOG")
	member=$(jq -r --arg c "$core" '.[$c].member' "$CATALOG")
	file=$(jq -r --arg c "$core" --arg a "$arch" '.[$c].assets[$a].file' "$CATALOG")
	archive=$(jq -r --arg c "$core" --arg a "$arch" '.[$c].assets[$a].archive' "$CATALOG")
	tag="${!vvar}"
	ver="${tag#v}"
	file="${file//\{ver\}/$ver}"
	url="https://github.com/$repo/releases/download/$tag/$file"
	dir="$TMP/$core-$arch"
	if [ "$archive" = "raw" ]; then
		dl "$url" "$dest"
	else
		dl "$url" "$dir.ar"
		mkdir -p "$dir"
		unzip -o -q "$dir.ar" -d "$dir"
		local src
		src=$(find "$dir" -type f -name "$member*" | head -n1)
		[ -n "$src" ] || {
			echo "❌ no match for '$member' under $dir" >&2
			exit 1
		}
		cp "$src" "$dest"
	fi
}

fetch_android() {
	local out="$ROOT/module/bin/arm64-v8a"
	local arch="android-arm64"
	echo "→ android binaries → module/bin/arm64-v8a/"
	mkdir -p "$out"

	# Xray core: no release assets upstream — build from the pinned source.
	bash "$HERE/build-xray.sh" android arm64 "$out/xray"

	for core in tun2socks hev-socks5-tunnel; do
		echo "→ $core ($arch)"
		stage_core "$core" "$arch" "$out/$core"
	done

	chmod 755 "$out/xray" "$out/tun2socks" "$out/hev-socks5-tunnel"

	echo "✅ module/bin/arm64-v8a/ populated:"
	for f in xray tun2socks hev-socks5-tunnel; do
		printf '   %-22s %s\n' "$f" "$(du -h "$out/$f" | cut -f1)"
	done
}

case "${1:-}" in
android) fetch_android ;;
*)
	echo "usage: fetch-binaries.sh android" >&2
	exit 2
	;;
esac
