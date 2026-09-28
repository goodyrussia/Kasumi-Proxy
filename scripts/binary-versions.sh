# shellcheck shell=bash
# Single source of truth for the pinned third-party binaries:
#   - the Xray core (compiled from a pinned source archive — scripts/build-xray.sh)
#   - the two TUN helpers (release assets, catalogued in scripts/binaries.json)
#
# Every value is overridable via the environment so a local build can test a
# different pin without editing this file.
#
# ── Xray core: eichgee/Xray-core (a standalone fork of XTLS/Xray-core) ──
# The fork carries the extra resolver/ping/wireguard patches this project builds
# against. It ships NO release assets, so CI and local builds compile the core
# from this pinned source archive; XRAY_ZIP_SHA256 is the integrity gate —
# scripts/build-xray.sh verifies it before extracting, and fails loudly on a
# mismatch (repo retag, tampered download).
XRAY_REPO="${XRAY_REPO:-eichgee/Xray-core}"
XRAY_TAG="${XRAY_TAG:-v1.250516.0-patch.20}"
XRAY_COMMIT="${XRAY_COMMIT:-725e55dd}" # short sha the pinned tag resolved to
XRAY_ZIP_SHA256="${XRAY_ZIP_SHA256:-eb0a48b2d70bd9dd81f17151088f353f4840ade92c10aaec089798a4f9802d58}"

# ── TUN helpers ──
# tun2socks: TUN → SOCKS5 bridge (one of the two selectable TUN engines).
TUN2SOCKS_VERSION="${TUN2SOCKS_VERSION:-v2.7.0}"
# hev-socks5-tunnel: the other TUN engine. Tags have no leading 'v'.
HEV_VERSION="${HEV_VERSION:-2.15.0}"
