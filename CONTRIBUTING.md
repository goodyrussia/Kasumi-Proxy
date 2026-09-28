# Contributing

## Repository layout

One Rust workspace and a React UI. The Android module is the only shell.

```
crates/
  kasumi-core/      domain types and logic with no IO: profiles, share links, xray config builders, migrations
  kasumi-backend/   orchestration: the Platform trait, typed Command/Response, lifecycle, jobs, the Service
  kasumi-daemon/    Android binary: axum HTTP (webroot) + token-gated WebSocket → Service
  kasumi-codegen/   generates frontend/src/generated/{bindings,schemas,defaults}.ts from the Rust types
frontend/           React + TypeScript UI (Vite, Zustand, Biome)
module/             root of the Android module zip (module.prop, *.sh, META-INF/)
scripts/            fetching/building the cores and daemon, packaging the release
```

Everything OS-specific lives behind the `Platform` trait (implemented by `kasumi-daemon` for
Android). The frontend types, Zod schemas and defaults in `frontend/src/generated/` are
**generated from Rust**. Don't edit them by hand.

## Requirements

| Tool | What it's for |
| --- | --- |
| [rustup](https://rustup.rs) | Rust. The version is pinned in `rust-toolchain.toml` and installs on first use. |
| [bun](https://bun.sh) | The UI. |
| `shellcheck` | Linting `module/*.sh`. |
| [Go](https://go.dev) | Building the pinned Xray core from source. |
| Android NDK (`NDK_ROOT`) + `cargo-ndk` + the `aarch64-linux-android` rustup target | Cross-building the daemon for the module. |
| `curl`, `jq`, `unzip`, `zip` | Binary fetching + packaging. |

## First build

```sh
bun install
scripts/package-release.sh            # cores → daemon → webroot → module zip
```

`scripts/package-release.sh` does everything: it builds the Xray core from the pinned source,
downloads tun2socks + hev-socks5-tunnel, cross-builds the daemon (needs `NDK_ROOT`, `cargo-ndk`
and the `aarch64-linux-android` target), builds the UI, and zips the module.

## Everyday commands

```sh
# UI with a mock backend, no device needed
cd frontend && bun run dev

# Validate every generated config against the real pinned core
scripts/check-binary-compat.sh
```

## Checks before a PR

CI runs the same checks, and they must stay green.

```sh
# Rust
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Codegen: after changing Rust types, regenerate and commit the result
cargo run -q -p kasumi-codegen -- --check

# Frontend
cd frontend
bun run build && bun run test
bun run check && bun run check:i18n

# Module scripts (Android mksh)
shellcheck -s sh module/*.sh
```

## Commits and PRs

- Use [Conventional Commits](https://www.conventionalcommits.org/) with a scope, e.g.
  `fix(backend): …` or `feat(frontend): …`. `CHANGELOG.md` is generated from them by
  `scripts/gen-changelog.sh`.
- Fill in the PR description using [the template](.github/PULL_REQUEST_TEMPLATE.md).
- Don't commit build artifacts: `module/bin/arm64-v8a/` and `module/webroot/`.
