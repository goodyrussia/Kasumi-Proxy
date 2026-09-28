//! Platform-neutral data-path lifecycle: build and write the active config, spawn
//! the core and the TUN engine that fronts it, and verify the core stayed up.
//! A `Platform`'s `start_data_path` orchestrates these and wraps its own
//! OS-specific routing/tun/sysctl around them.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use tokio::process::Child;

use kasumi_core::core_config::CoreConfig;
use kasumi_core::enums::TunEngine;
use kasumi_core::hev_config::build_hev_config;
use kasumi_core::state::{AppState, DEFAULT_LOCAL_SOCKS_PORT, ProxyMode};
use kasumi_core::tun::TunOptions;
use kasumi_core::tun2socks_config::build_tun2socks_config;

use crate::commands::{CommandError, build_profile_config};
use crate::fsjson::{read_json, write_text_atomic};
use crate::platform::{Platform, StartDataPath};
use crate::proc::{pid_matches_bin, spawn_logged};

/// Map a hex digit to a consonant so the interface name starts with a letter
/// (kernel rejects names beginning with a digit).
fn lead_letter(c: char) -> char {
    match c {
        '0' => 'q',
        '1' => 'w',
        '2' => 'e',
        '3' => 'r',
        '4' => 't',
        '5' => 'y',
        '6' => 'u',
        '7' => 'i',
        '8' => 'o',
        '9' => 'p',
        'a' => 's',
        'b' => 'd',
        'c' => 'f',
        'd' => 'g',
        'e' => 'h',
        _ => 'j',
    }
}

/// A random tun interface name: a leading letter + 8 hex chars.
pub fn random_tun_iface() -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    let lead = lead_letter(hex.chars().next().unwrap_or('f'));
    format!("{lead}{}", &hex[1..9])
}

/// Build the config for `profile_id` (else the active profile), write it, and
/// return the resolved [`StartDataPath`] (TUN engine, engine tuning, SOCKS port,
/// proxy mode) together with the exact [`CoreConfig`] that was written — the
/// in-memory baseline a running data path is later diffed against (the on-disk
/// file may be tuned further after start).
pub async fn resolve_and_write_config(
    platform: &dyn Platform,
    profile_id: Option<&str>,
) -> Result<(StartDataPath, CoreConfig), CommandError> {
    let paths = platform.paths();
    let state = read_json::<AppState>(&paths.app_state).await;
    let id = profile_id
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| state.as_ref().and_then(|s| s.active_id.clone()))
        .unwrap_or_default();
    let built = build_profile_config(platform, &id).await?;
    let tun = built.tun;
    let cfg_text =
        serde_json::to_string_pretty(&built.config).map_err(|e| CommandError(e.to_string()))?;
    write_text_atomic(&paths.xray_config, &cfg_text)
        .await
        .map_err(|e| CommandError(e.to_string()))?;
    let settings = state.map(|s| s.settings).unwrap_or_default();
    let socks_port = settings
        .local_socks_port
        .unwrap_or(DEFAULT_LOCAL_SOCKS_PORT);
    let tun_opts = settings.tun_options();
    // Same normalization as the config build: a platform without proxy-mode
    // support always starts in tun mode.
    let mode = if platform.supports_proxy_modes() {
        settings.proxy_mode
    } else {
        ProxyMode::Tun
    };
    Ok((
        StartDataPath {
            tun,
            tun_opts,
            socks_port,
            mode,
        },
        built,
    ))
}

// ---------- core + TUN engine spawn ----------

/// The core's argv (`<bin> run -c <cfg>`) and the env it needs (xray reads its geo
/// `.dat` assets from `XRAY_LOCATION_ASSET`). Split out so a caller that supervises
/// the spawn itself can reuse the exact same command without duplicating it.
pub fn core_argv(bin: &str, cfg: &str) -> Vec<String> {
    vec![bin.to_owned(), "run".into(), "-c".into(), cfg.to_owned()]
}

pub fn core_env(dat_dir: &str) -> HashMap<String, String> {
    HashMap::from([("XRAY_LOCATION_ASSET".to_owned(), dat_dir.to_owned())])
}

/// Spawn the core (`<bin> run -c <cfg>`), logging to `log_path`. The caller
/// persists the returned pid to its pidfile. On Unix the child is tied to its
/// parent via `PR_SET_PDEATHSIG` (see [`crate::proc::spawn_logged`]).
pub async fn spawn_core(
    bin: &str,
    cfg: &str,
    log_path: &Path,
    dat_dir: &str,
    kill_on_drop: bool,
) -> std::io::Result<Child> {
    spawn_logged(
        &core_argv(bin, cfg),
        &core_env(dat_dir),
        log_path,
        kill_on_drop,
    )
    .await
}

/// Spawn tun2socks bridging the tun to the local SOCKS port: render its YAML
/// config (device/proxy/tuning — see `build_tun2socks_config`), write it next to
/// the runtime state, then run `<bin> --config <cfg>`. `fwmark`, when set, marks
/// tun2socks' own upstream socket so an `ip rule` can keep it out of the tunnel —
/// a Linux SO_MARK feature.
async fn spawn_tun2socks(s: &TunSpawn<'_>) -> std::io::Result<Child> {
    let yaml = build_tun2socks_config(s.iface, s.socks_port, s.fwmark, s.opts);
    crate::fs::write_text(s.cfg_path, &yaml).await?;
    let argv = [
        s.bin.to_owned(),
        "--config".into(),
        s.cfg_path.to_string_lossy().into_owned(),
    ];
    spawn_logged(&argv, &std::collections::HashMap::new(), s.log_path, false).await
}

/// Everything needed to bring up one external TUN engine, gathered so adding an
/// engine is a single match arm. `bin` is the engine binary (resolved per-platform);
/// `cfg_path` is where the engine's rendered config is written; `ipv4`/`ipv6` are
/// the host addresses a self-addressing engine assigns to the tun it creates
/// itself; `opts` carries the resolved tuning.
pub struct TunSpawn<'a> {
    pub bin: &'a str,
    pub iface: &'a str,
    pub ipv4: &'a str,
    pub ipv6: Option<&'a str>,
    pub socks_port: u16,
    pub log_path: &'a Path,
    pub fwmark: Option<u32>,
    pub cfg_path: &'a Path,
    pub opts: &'a TunOptions,
}

/// The single place that knows how to launch an external TUN engine. Every shell
/// routes its bring-up through here, so adding a new engine is one more arm —
/// nothing else in the orchestration learns engine specifics.
pub async fn spawn_tun_engine(tun: TunEngine, s: &TunSpawn<'_>) -> std::io::Result<Child> {
    match tun {
        TunEngine::Tun2socks => spawn_tun2socks(s).await,
        TunEngine::Hev => spawn_hev(s).await,
    }
}

/// hev creates and addresses its own tun from a YAML config: render it, write it
/// next to the runtime state, then run `<hev_bin> <cfg>`.
async fn spawn_hev(s: &TunSpawn<'_>) -> std::io::Result<Child> {
    let yaml = build_hev_config(s.iface, s.ipv4, s.ipv6, s.socks_port, s.fwmark, s.opts);
    crate::fs::write_text(s.cfg_path, &yaml).await?;
    let argv = [s.bin.to_owned(), s.cfg_path.to_string_lossy().into_owned()];
    spawn_logged(&argv, &std::collections::HashMap::new(), s.log_path, false).await
}

/// Confirm the core stayed up: a bad config makes it exit within ~1s.
pub async fn verify_core_alive(pid: i32, bin: &str, attempts: u32, delay: Duration) -> bool {
    for _ in 0..attempts {
        if !pid_matches_bin(pid, bin).await {
            return false;
        }
        tokio::time::sleep(delay).await;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::read_text;
    use crate::fsjson::write_json_atomic;
    use crate::testutil::{TestPlatform, sample_vless};
    use kasumi_core::state::default_app_state;

    #[test]
    fn random_iface_is_letter_then_hex() {
        let name = random_tun_iface();
        assert_eq!(name.len(), 9);
        let mut chars = name.chars();
        assert!(chars.next().unwrap().is_ascii_alphabetic());
        assert!(chars.all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn resolve_and_write_config_writes_the_config() {
        let (p, _d) = TestPlatform::new();
        let prof = sample_vless();
        let id = prof.meta().id.clone();
        let mut state = default_app_state();
        state.active_id = Some(id.clone());
        state.settings.local_socks_port = Some(11080);
        write_json_atomic(&p.paths().app_state, &state)
            .await
            .unwrap();
        write_json_atomic(&p.paths().profiles, &vec![prof])
            .await
            .unwrap();

        // No explicit id → uses active_id.
        let (opts, built) = resolve_and_write_config(&p, None).await.unwrap();
        assert_eq!(opts.tun, TunEngine::Tun2socks);
        assert_eq!(opts.socks_port, 11080);
        // TestPlatform doesn't support proxy modes → always normalized to tun.
        assert_eq!(opts.mode, ProxyMode::Tun);
        // The returned build mirrors what was written.
        assert!(built.config["outbounds"].is_array());
        let cfg = read_text(&p.paths().xray_config).await.unwrap();
        assert!(cfg.contains("outbounds"));
    }

    #[tokio::test]
    async fn verify_core_alive_against_self_and_wrong_bin() {
        let me = std::process::id() as i32;
        let exe = std::fs::read_link(format!("/proc/{me}/exe")).unwrap();
        let exe = exe.to_string_lossy().into_owned();
        assert!(verify_core_alive(me, &exe, 1, Duration::from_millis(1)).await);
        assert!(!verify_core_alive(me, "/bin/sh", 1, Duration::from_millis(1)).await);
    }
}
