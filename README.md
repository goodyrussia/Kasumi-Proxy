<p align="center">
  <img src=".github/logo.png" width="160" alt="Kasumi Proxy" />
</p>

<h1 align="center">Kasumi Proxy</h1>

<p align="center">
  System-wide transparent proxy on <b>Xray-core</b> for <b>rooted Android</b> (Magisk / KernelSU / APatch).
</p>

<p align="center">
  <a href="https://github.com/goodyrussia/Kasumi-Proxy/actions/workflows/ci.yml"><img src="https://github.com/goodyrussia/Kasumi-Proxy/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/goodyrussia/Kasumi-Proxy/releases/latest"><img src="https://img.shields.io/github/v/release/goodyrussia/Kasumi-Proxy?sort=semver" alt="Latest release" /></a>
  <a href="https://github.com/goodyrussia/Kasumi-Proxy/issues"><img src="https://img.shields.io/github/issues/goodyrussia/Kasumi-Proxy" alt="Open issues" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/goodyrussia/Kasumi-Proxy" alt="License: GPL v3" /></a>
</p>

- A Magisk / KernelSU / APatch **module** for arm64 devices. It runs the Xray core as a root
  daemon and routes traffic with `iptables` / `ip rule`, not through `VpnService`.

> [!NOTE]
> This is a fork of [loss-and-quick/Kasumi-Proxy](https://github.com/loss-and-quick/Kasumi-Proxy)
> (itself a fork of [vincentng295/Magic_V2Ray](https://github.com/vincentng295/Magic_V2Ray)),
> trimmed to the Android module and the Xray core only: sing-box, the desktop (Tauri) app,
> subscriptions and the non-Xray protocols were removed. Most of the code was written by AI,
> so review it before you trust it.

## Features

- **One core: Xray-core** (the [eichgee fork](https://github.com/eichgee/Xray-core), pinned and
  built from source). Protocols: VLESS, VMess, Trojan, Shadowsocks, SOCKS, HTTP, WireGuard and
  custom JSON outbounds — each with its transports (TCP, WS, gRPC, HTTPUpgrade, XHTTP, mKCP),
  TLS / REALITY / XTLS.
- **Import.** Share links (`vless://`, `vmess://`, `trojan://`, …) and QR codes.
- **Honest status.** An end-to-end probe tells *connected* apart from *no internet* and *failed*.
- **Diagnostics.** TCP ping, real ping and a speed test for each profile.
- **Per-app routing** on Android, plus a choice of TUN engine: tun2socks or hev-socks5-tunnel.

### Why a root module rather than a VPN app?

- The root daemon is not killed by the low-memory killer, so the tunnel does not drop and leak
  your IP.
- Traffic is intercepted in the kernel (Netfilter) and never passes through a user-space
  `tun0`.
- Switching between Wi-Fi and mobile data reapplies the routing rules on the fly.

> [!TIP]
> No root? Use an ordinary client from the
> [Xray-core GUI list](https://github.com/XTLS/Xray-core#gui-clients).

## Install

Download the latest `kasumi-proxy-module-vX.Y.Z.zip` from the
[releases page](https://github.com/goodyrussia/Kasumi-Proxy/releases/latest), flash it in
Magisk / KernelSU / APatch, then reboot. Open the UI with the module's **Action** button or its
WebUI entry.

State and logs live in `/data/adb/kasumi-proxy/`.

## Development

Build steps, checks and the repository layout are in [CONTRIBUTING.md](CONTRIBUTING.md). Plain
`cargo` and `bun` are enough; CI runs the same steps.

## Acknowledgments

Kasumi Proxy ships prebuilt binaries from
[eichgee/Xray-core](https://github.com/eichgee/Xray-core) (built from source at the pinned tag),
[tun2socks](https://github.com/xjasonlyu/tun2socks) and
[hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel). See
[module/bin/README.md](module/bin/README.md) for their licenses.

## License

[GPL-3.0](LICENSE)
