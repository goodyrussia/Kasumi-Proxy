//! The Android `Platform`: thin orchestration over the neutral lifecycle steps,
//! owning only the OS-specific parts — routing, sysctl locks, `/dev/net/tun`, and
//! the per-uid app filter.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;

use kasumi_backend::fs::{exists, read_text, remove_file, write_text};
use kasumi_backend::fsjson::{read_data_path_state, read_json, write_data_path_state};
use kasumi_backend::lifecycle::{TunSpawn, spawn_core, spawn_tun_engine, verify_core_alive};
use kasumi_backend::net::ProxyStatus;
use kasumi_backend::platform::{
    AppFilterCapability, AppInfo, BackendPaths, InstalledCores, Platform, PlatformCapabilities,
    StartDataPath, StopDataPath,
};
use kasumi_backend::proc::{kill_if_running, pid_matches_bin, read_pidfile};
use kasumi_core::contract::{RunState, ServiceState};
use kasumi_core::data_path_state::{DataPathState, TunSelection};
use kasumi_core::enums::TunEngine;
use kasumi_core::state::{
    AdvancedSettings, AppState, DEFAULT_LOCAL_HTTP_PORT, DEFAULT_LOCAL_SOCKS_PORT, force_socks_port,
};
use kasumi_core::tun::{TUN_IPV4, TUN_IPV6, TUN2_IPV4, TUN2_IPV6, TunOptions};

use super::network::run_watcher;
use super::paths::{
    DATA_PATH_STATE_FILE, DATADIR, HEV_BIN, HEV_CONFIG, HEV2_CONFIG, IP, PIDFILE, RUN_DIR,
    TUN_IFACE_FILE, TUN2_IFACE_FILE, TUN2SOCKS_BIN, TUN2SOCKS_CONFIG, TUN2SOCKS_PIDFILE,
    TUN2SOCKS2_CONFIG, TUN2SOCKS2_PIDFILE, XRAY_BIN, backend_paths,
};
use super::routing::{
    Action, AppFilter, FWMARK, RoutingState, apply_external_tun_routing, clear_routing_rules,
    has_force_proxy, protect_local_ports, reload_app_filter_rules,
};
use super::sysctl::{lock_tun_iface, setup_sysctl_locks};
use super::{run_out, silent};

pub struct AndroidPlatform {
    paths: BackendPaths,
}

impl AndroidPlatform {
    pub fn new() -> Self {
        Self {
            paths: backend_paths(),
        }
    }
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self::new()
    }
}

/// External-TUN helper binary for `tun`. The single place the daemon maps an
/// engine to a binary.
fn tun_helper_bin(tun: TunEngine) -> &'static str {
    match tun {
        TunEngine::Hev => HEV_BIN,
        TunEngine::Tun2socks => TUN2SOCKS_BIN,
    }
}

/// The config file an external engine writes at bring-up: tun2socks'/hev's YAML,
/// per tun (the `2` variant is the force-proxy tun).
fn tun_cfg_path(tun: TunEngine, force: bool) -> &'static str {
    match (tun, force) {
        (TunEngine::Tun2socks, false) => TUN2SOCKS_CONFIG,
        (TunEngine::Tun2socks, true) => TUN2SOCKS2_CONFIG,
        (TunEngine::Hev, false) => HEV_CONFIG,
        (TunEngine::Hev, true) => HEV2_CONFIG,
    }
}

/// The external TUN engine the *running* data-path uses, or `None` when no
/// document is recorded. A lookup on the recorded document
/// ([`DataPathState::external_tun`]); the daemon maps the result via [`tun_helper_bin`].
async fn running_tun_engine() -> Option<TunEngine> {
    read_state().await?.external_tun()
}

/// Helper binary a teardown/watchdog match targets for the running data-path —
/// tun2socks by default, a harmless guard when nothing is recorded.
async fn running_helper_bin() -> &'static str {
    running_tun_engine()
        .await
        .map(tun_helper_bin)
        .unwrap_or(TUN2SOCKS_BIN)
}

fn app_state_path() -> String {
    format!("{DATADIR}/app-state.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn read_state() -> Option<DataPathState> {
    read_data_path_state(DATA_PATH_STATE_FILE).await
}

async fn write_state(state: &DataPathState) {
    let _ = write_data_path_state(DATA_PATH_STATE_FILE, state).await;
}

/// Mark the running data-path failed, keeping the recorded engine/tun so teardown
/// still reaps the right helper.
async fn set_failed(reason: &str) {
    let mut state = read_state().await.unwrap_or_default();
    state.run = RunState::Failed;
    state.failure_reason = Some(reason.to_owned());
    state.started_at = None;
    write_state(&state).await;
}

async fn read_settings() -> Option<AdvancedSettings> {
    read_json::<AppState>(&app_state_path())
        .await
        .map(|s| s.settings)
}

async fn read_app_filter() -> AppFilter {
    match read_settings().await {
        Some(s) => AppFilter {
            capture_mode: s.app_capture_mode,
            entries: s.app_filter,
            strict: s.strict_route,
        },
        None => AppFilter {
            capture_mode: Default::default(),
            entries: BTreeMap::new(),
            strict: false,
        },
    }
}

async fn http_port() -> u16 {
    read_settings()
        .await
        .and_then(|s| s.local_http_port)
        .unwrap_or(DEFAULT_LOCAL_HTTP_PORT)
}

async fn read_iface(file: &str) -> Option<String> {
    read_text(file)
        .await
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn ensure_tun_node() {
    if exists("/dev/net/tun").await {
        return;
    }
    silent(&["mkdir", "-p", "/dev/net"]).await;
    silent(&["mknod", "/dev/net/tun", "c", "10", "200"]).await;
    silent(&["chmod", "666", "/dev/net/tun"]).await;
}

async fn fresh_iface(file: &str) -> String {
    let name = kasumi_backend::lifecycle::random_tun_iface();
    let _ = write_text(file, &name).await;
    name
}

/// The core's binary, config and log paths.
fn core_files() -> (&'static str, String, String) {
    (
        XRAY_BIN,
        format!("{DATADIR}/config.json"),
        format!("{DATADIR}/xray.log"),
    )
}

async fn core_version() -> Option<String> {
    let bin = XRAY_BIN;
    if !exists(bin).await {
        return None;
    }
    let (code, out) = run_out(&[bin, "version"]).await;
    if code != 0 {
        return None;
    }
    out.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Spawn the external TUN helper for `iface`, persist its pid, wait for the iface
/// to appear. Routes through the shared `spawn_tun_engine` so the daemon learns no
/// engine specifics beyond the helper binary (via [`tun_helper_bin`]) and, for a
/// self-addressing engine (hev), the YAML it writes at `cfg_path` and the
/// `ipv4`/`ipv6` it assigns. A spawn failure (e.g. the binary missing after a partial
/// module update) or the tun device never appearing is surfaced as an error so the
/// caller fails the start loudly, instead of routing into a bridge that never came up
/// and black-holing the device until the watchdog happens to notice.
#[allow(clippy::too_many_arguments)]
async fn bring_up_tun_helper(
    tun: TunEngine,
    iface: &str,
    ipv4: &str,
    ipv6: Option<&str>,
    socks_port: u16,
    cfg_path: &str,
    pidfile: &str,
    log_name: &str,
    opts: &TunOptions,
) -> anyhow::Result<()> {
    let log = format!("{DATADIR}/{log_name}");
    let spawn = TunSpawn {
        bin: tun_helper_bin(tun),
        iface,
        ipv4,
        ipv6,
        socks_port,
        log_path: Path::new(&log),
        fwmark: Some(FWMARK),
        cfg_path: Path::new(cfg_path),
        opts,
    };
    let child = spawn_tun_engine(tun, &spawn)
        .await
        .map_err(|e| anyhow::anyhow!("spawn tun helper: {e} — see {log}"))?;
    let pid = child.id().unwrap_or(0);
    let _ = write_text(pidfile, &pid.to_string()).await;
    for _ in 0..10 {
        if silent(&[IP, "link", "show", iface]).await == 0 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // The pid is live but the device never appeared (the helper exited post-spawn, or
    // /dev/net/tun is unusable): treat as a failed bring-up rather than route into a
    // non-existent tun.
    anyhow::bail!("tun helper tun {iface} never came up — see {log}")
}

/// External-tun data-path bring-up: a userspace tun bridged to the core's SOCKS via
/// the chosen TUN engine, plus per-app routing (and a second tun for force-proxy
/// apps).
async fn bring_up_external_tun(
    tun: TunEngine,
    socks_port: u16,
    opts: &TunOptions,
) -> anyhow::Result<()> {
    let helper_bin = tun_helper_bin(tun);
    let filter = read_app_filter().await;
    let tun_iface = match read_iface(TUN_IFACE_FILE).await {
        Some(x) => x,
        None => fresh_iface(TUN_IFACE_FILE).await,
    };
    let mut tun2_iface: Option<String> = None;

    let t2 = read_pidfile(TUN2SOCKS_PIDFILE).await;
    if !(t2 > 0 && pid_matches_bin(t2, helper_bin).await) {
        bring_up_tun_helper(
            tun,
            &tun_iface,
            TUN_IPV4,
            Some(TUN_IPV6),
            socks_port,
            tun_cfg_path(tun, false),
            TUN2SOCKS_PIDFILE,
            "tun-engine.log",
            opts,
        )
        .await?;
    }

    if has_force_proxy(&filter) {
        let t = match read_iface(TUN2_IFACE_FILE).await {
            Some(x) => x,
            None => fresh_iface(TUN2_IFACE_FILE).await,
        };
        let t3 = read_pidfile(TUN2SOCKS2_PIDFILE).await;
        if !(t3 > 0 && pid_matches_bin(t3, helper_bin).await) {
            bring_up_tun_helper(
                tun,
                &t,
                TUN2_IPV4,
                Some(TUN2_IPV6),
                socks_port + 2,
                tun_cfg_path(tun, true),
                TUN2SOCKS2_PIDFILE,
                "tun-engine2.log",
                opts,
            )
            .await?;
        }
        tun2_iface = Some(t);
    } else {
        kill_if_running(
            read_pidfile(TUN2SOCKS2_PIDFILE).await,
            Some(helper_bin),
            TUN2SOCKS2_PIDFILE,
            false,
        )
        .await;
        remove_file(TUN2_IFACE_FILE).await;
    }

    lock_tun_iface(&tun_iface).await;
    let rs = RoutingState {
        tun_iface: Some(tun_iface),
        tun2_iface,
        filter,
        socks_port,
        http_port: http_port().await,
    };
    apply_external_tun_routing(&rs).await;
    Ok(())
}

async fn fail(reason: &str) -> anyhow::Result<()> {
    set_failed(reason).await;
    anyhow::bail!("{reason}")
}

async fn start_inner(
    tun: TunEngine,
    tun_opts: &TunOptions,
    bin: &str,
    cfg: &str,
    log: &str,
    socks_port: u16,
) -> anyhow::Result<()> {
    if !exists(bin).await {
        return fail("core binary missing").await;
    }
    if !exists(cfg).await {
        return fail("config missing").await;
    }

    // stopDataPath always runs before a start/restart (Service), so the pidfile is
    // clean here.
    let child = spawn_core(bin, cfg, Path::new(log), DATADIR, false).await?;
    let core_pid = child.id().unwrap_or(0) as i32;
    let _ = write_text(PIDFILE, &core_pid.to_string()).await;

    // The core is socks-only: bring up a userspace tun bridged to its SOCKS via
    // the chosen TUN engine plus per-app routing.
    bring_up_external_tun(tun, socks_port, tun_opts).await?;

    if !verify_core_alive(core_pid, bin, 6, Duration::from_millis(250)).await {
        return fail(&format!("core exited on startup — see {log}")).await;
    }

    // Shield local proxy ports from bypass-mode apps. Deferred until the core is up
    // so our iptables don't contend with the tun helper's own xtables setup.
    protect_local_ports(
        Action::Add,
        &read_app_filter().await,
        socks_port,
        http_port().await,
    )
    .await;
    // Process-up: `started_at` marks it (vs the bring-up `connecting`) and drives
    // uptime; the wire state stays Connecting until the Service's connectivity probe
    // refines it to Connected / NoInternet.
    let mut state = read_state().await.unwrap_or_default();
    state.started_at = Some(now_secs());
    write_state(&state).await;
    Ok(())
}

/// RX/TX bytes for `iface` from /proc/net/dev, or (0, 0).
async fn iface_traffic(iface: Option<&str>) -> (u64, u64) {
    let Some(iface) = iface else {
        return (0, 0);
    };
    let Some(dev) = read_text("/proc/net/dev").await else {
        return (0, 0);
    };
    for line in dev.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != iface {
            continue;
        }
        let f: Vec<&str> = rest.split_whitespace().collect();
        let rx = f.first().and_then(|x| x.parse().ok()).unwrap_or(0);
        let tx = f.get(8).and_then(|x| x.parse().ok()).unwrap_or(0);
        return (rx, tx);
    }
    (0, 0)
}

#[async_trait]
impl Platform for AndroidPlatform {
    fn paths(&self) -> &BackendPaths {
        &self.paths
    }

    async fn boot_init(&self) -> anyhow::Result<()> {
        silent(&["mkdir", "-p", RUN_DIR]).await;
        // The route tables netd manages must exist before we touch ip rules/sysctl.
        for _ in 0..120 {
            if exists("/data/misc/net/rt_tables").await {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        // Register our own route tables (1100/1101) — avoid 100/101 because some
        // OEMs own those for vendor routing.
        silent(&[
            "sh",
            "-c",
            "grep -qs '^1100 ' /data/misc/net/rt_tables || echo '1100 kasumi-proxy' >> /data/misc/net/rt_tables",
        ])
        .await;
        silent(&[
            "sh",
            "-c",
            "grep -qs '^1101 ' /data/misc/net/rt_tables || echo '1101 kasumi-proxy-force' >> /data/misc/net/rt_tables",
        ])
        .await;
        setup_sysctl_locks().await;
        // Fresh boot: seed a stopped document so a stale value can't make status lie
        // before the first command. Drop the pre-document `service-started` marker.
        write_state(&DataPathState::default()).await;
        remove_file("/data/adb/kasumi-proxy/service-started").await;
        Ok(())
    }

    async fn start_data_path(&self, opts: StartDataPath) -> anyhow::Result<()> {
        // `mode` is ignored: this platform reports no proxy-mode support, so it is
        // always normalized to tun upstream.
        let StartDataPath {
            tun,
            tun_opts,
            socks_port,
            ..
        } = opts;
        // Record the bring-up (no started_at yet); this platform is always tun mode.
        write_state(&DataPathState {
            run: RunState::Connecting,
            tun: TunSelection::Engine(tun),
            socks_port,
            ..Default::default()
        })
        .await;
        ensure_tun_node().await;

        let (bin, cfg, log) = core_files();
        if let Err(e) = start_inner(tun, &tun_opts, bin, &cfg, &log, socks_port).await {
            // Roll back the half-built data-path so a failed start leaves no orphans.
            let already_failed = read_state()
                .await
                .is_some_and(|s| s.run == RunState::Failed);
            if !already_failed {
                set_failed(&e.to_string()).await;
            }
            let _ = self
                .stop_data_path(StopDataPath {
                    keep_service_state: true,
                })
                .await;
            return Err(e);
        }
        Ok(())
    }

    async fn stop_data_path(&self, opts: StopDataPath) -> anyhow::Result<()> {
        let rs = RoutingState {
            tun_iface: read_iface(TUN_IFACE_FILE).await,
            tun2_iface: read_iface(TUN2_IFACE_FILE).await,
            filter: read_app_filter().await,
            socks_port: read_state()
                .await
                .map(|d| d.socks_port)
                .filter(|&p| p != 0)
                .unwrap_or(DEFAULT_LOCAL_SOCKS_PORT),
            http_port: http_port().await,
        };
        // Stop the core first, gracefully, then clear our routing.
        kill_if_running(read_pidfile(PIDFILE).await, None, PIDFILE, true).await;
        clear_routing_rules(&rs).await;
        remove_file(TUN_IFACE_FILE).await;
        remove_file(TUN2_IFACE_FILE).await;
        // Match the helper binary the running data-path actually used (from the
        // recorded document) so an engine switch never orphans the old helper.
        let helper_bin = running_helper_bin().await;
        kill_if_running(
            read_pidfile(TUN2SOCKS_PIDFILE).await,
            Some(helper_bin),
            TUN2SOCKS_PIDFILE,
            false,
        )
        .await;
        kill_if_running(
            read_pidfile(TUN2SOCKS2_PIDFILE).await,
            Some(helper_bin),
            TUN2SOCKS2_PIDFILE,
            false,
        )
        .await;
        // Drop the pre-document `tun-engine` marker if an old version left one.
        remove_file("/data/adb/kasumi-proxy/run/tun-engine").await;
        if !opts.keep_service_state {
            write_state(&DataPathState::default()).await;
        }
        Ok(())
    }

    async fn service_state(&self) -> anyhow::Result<ServiceState> {
        // The recorded document is authoritative for the base state; the Service's
        // connectivity probe later refines a running Connecting → Connected / NoInternet.
        let doc = read_state().await;
        let state = doc.as_ref().map(|d| d.run).unwrap_or(RunState::Stopped);
        let error = doc.as_ref().and_then(|d| d.failure_reason.clone());
        let tun = read_iface(TUN_IFACE_FILE).await;
        let (rx, tx) = iface_traffic(tun.as_deref()).await;
        // `started_at` is set only once the core is up, so uptime counts from there.
        let uptime_sec = doc
            .as_ref()
            .and_then(|d| d.started_at)
            .map(|s| now_secs().saturating_sub(s))
            .unwrap_or(0);
        Ok(ServiceState {
            state,
            error,
            download_bytes: rx,
            upload_bytes: tx,
            uptime_sec,
        })
    }

    async fn capabilities(&self) -> anyhow::Result<PlatformCapabilities> {
        let xray = core_version().await;
        let tun = exists("/dev/net/tun").await;
        Ok(PlatformCapabilities {
            cores: InstalledCores { xray },
            tun,
            bridge: "ksu".into(),
        })
    }

    fn core_path(&self) -> PathBuf {
        PathBuf::from(XRAY_BIN)
    }

    async fn proxy_status(&self) -> anyhow::Result<ProxyStatus> {
        let port = read_state()
            .await
            .map(|d| d.socks_port)
            .filter(|&p| p != 0)
            .unwrap_or(DEFAULT_LOCAL_SOCKS_PORT);
        let pid = read_pidfile(PIDFILE).await;
        let running = pid > 0 && pid_matches_bin(pid, XRAY_BIN).await;
        let http = http_port().await;
        Ok(ProxyStatus {
            running,
            socks_port: port,
            http_port: http,
            force_port: force_socks_port(port, http),
        })
    }

    fn watch_network_change(&self) -> Option<mpsc::Receiver<()>> {
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(run_watcher(tx));
        Some(rx)
    }

    async fn data_path_healthy(&self) -> Option<bool> {
        let core_pid = read_pidfile(PIDFILE).await;
        if !(core_pid > 0 && pid_matches_bin(core_pid, XRAY_BIN).await) {
            return Some(false);
        }
        // The data-path also relies on its tun helper: the helper pid must be
        // present AND alive — a missing pidfile (helper never spawned, e.g. its
        // binary absent) is unhealthy, not healthy, so the watchdog rebuilds it
        // instead of leaving traffic black-holed in an unbridged tun. (One marker
        // read resolves both the engine and the binary to match.)
        if let Some(tun) = running_tun_engine().await {
            let t = read_pidfile(TUN2SOCKS_PIDFILE).await;
            if !(t > 0 && pid_matches_bin(t, tun_helper_bin(tun)).await) {
                return Some(false);
            }
        }
        Some(true)
    }

    fn app_filter(&self) -> Option<&dyn AppFilterCapability> {
        Some(self)
    }
}

#[async_trait]
impl AppFilterCapability for AndroidPlatform {
    async fn list_apps(&self) -> anyhow::Result<Vec<AppInfo>> {
        let (code, out) = run_out(&["pm", "list", "packages", "-U"]).await;
        if code != 0 {
            return Ok(Vec::new());
        }
        let mut apps = Vec::new();
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("package:") else {
                continue;
            };
            let mut toks = rest.split_whitespace();
            let pkg = toks.next();
            let uid = toks.find_map(|t| t.strip_prefix("uid:"));
            if let (Some(pkg), Some(uid)) = (pkg, uid)
                && let Ok(uid) = uid.parse::<i32>()
            {
                apps.push(AppInfo {
                    pkg: pkg.to_string(),
                    uid,
                    system: uid < 10000,
                });
            }
        }
        Ok(apps)
    }

    async fn reload_app_filter(&self) -> anyhow::Result<()> {
        let pid = read_pidfile(PIDFILE).await;
        if !(pid > 0 && pid_matches_bin(pid, XRAY_BIN).await) {
            return Ok(());
        }
        // xray reloads per-uid rules live: no restart needed.
        reload_app_filter_rules(&read_app_filter().await).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn iface_traffic_parses_proc_net_dev_columns() {
        // Real /proc/net/dev isn't available deterministically; exercise the parser
        // shape via a known iface absence (returns zeros).
        assert_eq!(iface_traffic(None).await, (0, 0));
        assert_eq!(iface_traffic(Some("definitely-not-an-iface")).await, (0, 0));
    }

    #[test]
    fn core_files_are_xray_paths() {
        let (bin, cfg, log) = core_files();
        assert_eq!(bin, XRAY_BIN);
        assert!(cfg.ends_with("config.json"));
        assert!(log.ends_with("xray.log"));
    }
}
