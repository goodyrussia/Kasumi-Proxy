# Bundled binaries

Nothing here is committed except this file and `licenses/`. The binaries are produced at build
time:

```sh
scripts/fetch-binaries.sh android   # xray (built from source) + tun2socks + hev-socks5-tunnel → bin/arm64-v8a/
scripts/build-daemon-android.sh     # kasumi-proxy daemon → the same place
```

Versions are pinned in `scripts/binary-versions.sh` and can be overridden through environment
variables (`XRAY_REPO`, `XRAY_TAG`, `XRAY_ZIP_SHA256`, `TUN2SOCKS_VERSION`, `HEV_VERSION`).

| Binary | Role | License |
| --- | --- | --- |
| [Xray-core](https://github.com/eichgee/Xray-core) | The core: VLESS, VMess, Trojan, Shadowsocks, SOCKS, HTTP, WireGuard, custom outbounds. Built from the pinned source tag (no upstream release assets) | MPL-2.0, [`licenses/xray-LICENSE`](licenses/xray-LICENSE) |
| [tun2socks](https://github.com/xjasonlyu/tun2socks) | TUN → SOCKS5 bridge (TUN engine option) | MIT, [`licenses/tun2socks-LICENSE`](licenses/tun2socks-LICENSE) |
| [hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel) | TUN engine option (smaller, Go-free) | MIT |
| `kasumi-proxy` | Our Rust daemon (core + TUN lifecycle, routing, HTTP/WS server) | GPL-3.0, the repo's own [LICENSE](../../LICENSE) |
