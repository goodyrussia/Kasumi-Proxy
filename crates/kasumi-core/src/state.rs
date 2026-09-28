//! Groups, routing rules, asset files, the global advanced settings and the
//! top-level app state. Field names/defaults are fixed by the persisted
//! `app-state.json` shape so old data round-trips on read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::contract::FetchMode;
use crate::enums::TunEngine;
use crate::profile::Profile;

// Default local inbound ports used when settings leave them unset.
pub const DEFAULT_LOCAL_SOCKS_PORT: u16 = 10808;
pub const DEFAULT_LOCAL_HTTP_PORT: u16 = 10809;
pub const DEFAULT_LOCAL_PAC_PORT: u16 = 10811;

/// Port of the `force-in` socks inbound, derived from the user-facing ports. This
/// inbound routes straight to the `proxy` outbound, bypassing the geo rules — used
/// for the app's own fetches (asset downloads) when the proxy is wanted
/// regardless of routing.
///
/// Sits at `socks + 2` (the default layout is socks, http = socks + 1, force =
/// socks + 2). A custom `http_port` could land on `socks + 2`, so step past it when
/// it does — `force-in` and `http-in` must never claim the same port or the core
/// won't bind. Deterministic in `(socks, http)`, so the config builders and the
/// platform `proxy_status` compute the identical port without coordinating.
pub const fn force_socks_port(socks: u16, http: u16) -> u16 {
    let candidate = socks.saturating_add(2);
    if candidate == http {
        socks.saturating_add(3)
    } else {
        candidate
    }
}

// Default probe URLs used when delayTestUrl/speedTestUrl are unset.
pub const DEFAULT_DELAY_TEST_URL: &str = "https://www.gstatic.com/generate_204";
pub const DEFAULT_SPEED_TEST_URL: &str = "http://speed.cloudflare.com/__down?bytes=10000000";

// Default upstream resolvers when remoteDns is unset. DNS-over-TCP queried
// through the proxy — UDP DNS to a public resolver is what carriers drop/hijack,
// while TCP rides the tunnel fine.
pub const DEFAULT_REMOTE_DNS: [&str; 1] = ["tcp://1.1.1.1"];
// Default resolver for direct/off-tunnel lookups (the proxy-server hostname,
// domains of bypass rules) when domesticDns is unset.
pub const DEFAULT_DOMESTIC_DNS: [&str; 1] = ["tcp://1.1.1.1"];
// fake-IP v4 range for the fakeDns feature.
pub const FAKEIP_INET4_RANGE: &str = "198.18.0.0/15";
// Default log-rotation cap (KB).
pub const DEFAULT_LOG_ROTATE_KB: i64 = 512;
// Default interval (minutes) for the headless geosite/geoip auto-update (24h). One
// global cadence covers every asset file; the UI offers a few coarse presets.
pub const DEFAULT_ASSET_UPDATE_INTERVAL: i64 = 1440;
// Floor (minutes) for that interval, matching the smallest preset the UI offers.
// Geo data changes slowly and each fetch is multi-megabyte, so the updater clamps
// to this rather than honouring a smaller value from a hand-edited state file.
pub const MIN_ASSET_UPDATE_INTERVAL: i64 = 360;

/// The base group that must always exist (default `groupId`, can't be deleted).
pub const BASE_GROUP_ID: &str = "g-main";
pub const BASE_GROUP_NAME: &str = "Main";

/// A profile group (`GroupSchema`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
}

/// Transport scope of a routing rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum RuleNetwork {
    Tcp,
    Udp,
    #[serde(rename = "tcp,udp")]
    TcpUdp,
}

/// A custom routing rule (`RoutingRuleSchema`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RoutingRule {
    pub id: String,
    pub remarks: String,
    pub enabled: bool,
    pub outbound_tag: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub domain: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ip: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub network: Option<RuleNetwork>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub protocol: Option<Vec<String>>,
    /// Local processes that opened the connection: a bare name (`curl`), an
    /// absolute path (`/usr/bin/curl`), or a directory ending in `/`. Only
    /// connections made on this machine carry a process, and only a core that
    /// sees the app's own socket can tell (any core addressed directly through
    /// its local proxy port).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub process: Option<Vec<String>>,
    /// Android package names of the app that opened the connection.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub package_name: Option<Vec<String>>,
    /// Source addresses/CIDRs, e.g. LAN clients using the shared proxy port.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_ip: Option<Vec<String>>,
}

/// A downloadable asset (geoip/geosite) the daemon keeps current (`AssetFileSchema`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AssetFile {
    pub id: String,
    pub remarks: String,
    pub url: String,
    /// Epoch-ms of last refresh, or `null` if never fetched (required + nullable).
    pub last_updated: Option<i64>,
    pub locked: bool,
}

// ---- AdvancedSettings enums ----

/// How traffic is routed. `bypass-lan` is a legacy alias mapped to `global`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    strum::EnumIter,
    specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    #[default]
    #[serde(alias = "bypass-lan")]
    Global,
    Custom,
    Rules,
}

/// How the data path captures traffic. `tun` (default) brings up a system-wide
/// tun device and rewrites OS routing — it needs the privileged data-path. The
/// other modes run the core with only its local socks/http inbound: `proxy-only`
/// leaves the OS untouched (the user points apps at the port), `system` sets the
/// OS proxy to that inbound, `pac` serves a PAC the OS is pointed at. These are
/// mutually exclusive — there is no "tun + system proxy" combination, since the
/// tun already captures everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProxyMode {
    #[default]
    Tun,
    ProxyOnly,
    System,
    Pac,
}

/// Xray domain resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
pub enum DomainStrategy {
    #[serde(rename = "AsIs")]
    AsIs,
    #[default]
    #[serde(rename = "IPIfNonMatch")]
    IpIfNonMatch,
    #[serde(rename = "IPOnDemand")]
    IpOnDemand,
}

/// Mux xudp-over-443 handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MuxXudp443 {
    Reject,
    Proxy,
}

/// Core log verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    None,
}

/// Which apps the tun captures by default (per-app filtering base).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum AppCaptureMode {
    #[default]
    All,
    None,
}

/// Per-app override against the capture mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AppFilterMode {
    ForceProxy,
    Bypass,
}

/// Global advanced settings (`AdvancedSettingsSchema`). Optional fields omit when
/// unset; the rest always serialize with their Zod defaults (see [`Default`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedSettings {
    pub routing_mode: RoutingMode,
    /// How the data path captures traffic (tun vs proxy-only/system/pac). Desktop
    /// only; platforms without proxy-mode support (Android) always run tun.
    pub proxy_mode: ProxyMode,
    pub domain_sniffing: bool,
    pub route_only: bool,
    pub domain_strategy: DomainStrategy,
    pub strict_route: bool,
    pub dns_via_proxy: bool,
    pub fake_dns: bool,
    pub prefer_ipv6: bool,
    pub mux: bool,
    pub mux_concurrency: i64,
    pub ping_concurrency: i64,
    pub speed_concurrency: i64,
    pub auto_start: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mux_xudp_concurrency: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mux_xudp443: Option<MuxXudp443>,
    pub fragment: bool,
    pub fragment_packets: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_length: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment_delay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<LogLevel>,
    pub log_rotate_max_kb: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_socks_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_http_port: Option<u16>,
    /// The loopback port the desktop PAC server binds in `pac` mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_pac_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_dns: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domestic_dns: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_hosts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socks_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socks_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_test_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_test_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_routing: Option<String>,
    pub app_capture_mode: AppCaptureMode,
    pub app_filter: BTreeMap<String, AppFilterMode>,
    pub allow_non_localhost: bool,
    // ---- Geo asset auto-update ----
    /// Headless geosite/geoip auto-update: refresh the asset files on an interval.
    pub asset_auto_update: bool,
    /// Asset auto-update interval in minutes (shared by all asset files), floored
    /// at [`MIN_ASSET_UPDATE_INTERVAL`] by the updater.
    pub asset_update_interval: i64,
    /// Fetch mode for both manual and headless asset downloads.
    pub asset_update_mode: FetchMode,
    // ---- TUN engine ----
    /// Which TUN engine fronts the core: the core is built socks-only and this
    /// userspace tun→socks process bridges the tun device to it.
    pub tun_engine: TunEngine,
    /// MTU of the TUN interface (external engines in front of the core).
    pub tun_mtu: i64,
    /// Connect timeout (ms) for external TUN engines (hev `misc.connect-timeout`).
    pub tun_connect_timeout_ms: i64,
    /// TCP read/write timeout (ms) (hev `misc.tcp-read-write-timeout`).
    pub tun_tcp_rw_timeout_ms: i64,
    /// UDP read/write timeout (ms) (hev `misc.udp-read-write-timeout`, tun2socks
    /// `udp-timeout`).
    pub tun_udp_rw_timeout_ms: i64,
    /// Per-session TCP buffer size in bytes (hev `misc.tcp-buffer-size`, tun2socks
    /// `tcp-send/receive-buffer-size`).
    pub tun_tcp_buffer_size: i64,
    /// UDP receive buffer (SO_RCVBUF) size in bytes (hev `misc.udp-recv-buffer-size`).
    pub tun_udp_recv_buffer_size: i64,
    /// Comma- or newline-separated CIDRs the tun must not capture (e.g. docker
    /// bridge networks like `172.17.0.0/16`). Empty/`None` = nothing extra excluded.
    /// Parsed into a `Vec<String>` and merged with the proxy-server bypass wherever
    /// that set is computed, so the same setting works on every engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tun_exclude_addresses: Option<String>,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            routing_mode: RoutingMode::Global,
            proxy_mode: ProxyMode::Tun,
            domain_sniffing: true,
            route_only: false,
            domain_strategy: DomainStrategy::IpIfNonMatch,
            strict_route: false,
            dns_via_proxy: true,
            fake_dns: false,
            prefer_ipv6: false,
            mux: false,
            mux_concurrency: 8,
            ping_concurrency: 3,
            speed_concurrency: 1,
            auto_start: true,
            mux_xudp_concurrency: None,
            mux_xudp443: None,
            fragment: false,
            fragment_packets: "tlshello".into(),
            fragment_length: None,
            fragment_delay: None,
            log_level: None,
            log_rotate_max_kb: DEFAULT_LOG_ROTATE_KB,
            local_socks_port: None,
            local_http_port: None,
            local_pac_port: None,
            remote_dns: None,
            domestic_dns: None,
            dns_hosts: None,
            ipv6_enabled: None,
            socks_username: None,
            socks_password: None,
            delay_test_url: None,
            speed_test_url: None,
            custom_routing: None,
            app_capture_mode: AppCaptureMode::All,
            app_filter: BTreeMap::new(),
            allow_non_localhost: false,
            asset_auto_update: false,
            asset_update_interval: DEFAULT_ASSET_UPDATE_INTERVAL,
            asset_update_mode: FetchMode::default(),
            tun_engine: TunEngine::Tun2socks,
            tun_mtu: 9000,
            // hev upstream defaults (mirror its built-in values, so an unedited
            // config behaves exactly like stock hev).
            tun_connect_timeout_ms: 10_000,
            tun_tcp_rw_timeout_ms: 300_000,
            tun_udp_rw_timeout_ms: 60_000,
            tun_tcp_buffer_size: 65_536,
            tun_udp_recv_buffer_size: 524_288,
            tun_exclude_addresses: None,
        }
    }
}

/// The persisted top-level state (`AppStateSchema`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub routing_rules: Vec<RoutingRule>,
    #[serde(default)]
    pub asset_files: Vec<AssetFile>,
    pub settings: AdvancedSettings,
    /// Active profile id, or `null` (required + nullable).
    pub active_id: Option<String>,
    /// Module version that last wrote this state; absent on legacy state.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub version: Option<String>,
    /// On-disk schema version; absent (→ 0) on pre-versioning data. The read path
    /// runs [`crate::migrate`] up to the current version before deserializing.
    #[serde(default)]
    pub schema_version: u32,
}

/// The canonical fresh state: empty everything but the mandatory base group
/// `g-main`, default settings, nothing active. Fixes the first-run "{}" →
/// ZodError / "import into nowhere" bugs (findings #12/#12b).
pub fn default_app_state() -> AppState {
    AppState {
        profiles: Vec::new(),
        groups: vec![Group {
            id: BASE_GROUP_ID.into(),
            name: BASE_GROUP_NAME.into(),
        }],
        routing_rules: Vec::new(),
        asset_files: Vec::new(),
        settings: AdvancedSettings::default(),
        active_id: None,
        version: None,
        schema_version: crate::migrate::SCHEMA_VERSION,
    }
}

/// Null a dangling `active_id`: a required invariant for [`crate::core_config`],
/// which looks the active profile up by id and fails when it's missing. After any
/// edit that may have removed the active profile (a removal, a group deletion,
/// a backup restore), clear `active_id` when it no longer points at a live profile.
/// Pure; idempotent.
pub fn fixup_active_id(state: &mut AppState) {
    if let Some(id) = &state.active_id
        && !state.profiles.iter().any(|p| p.meta().id == *id)
    {
        state.active_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_round_trips() {
        let g: Group = serde_json::from_str(r#"{"id":"g-main","name":"Main"}"#).unwrap();
        assert_eq!(g.id, "g-main");
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(v["name"], "Main");
    }

    #[test]
    fn force_socks_port_steps_past_a_colliding_http_port() {
        // Default layout: socks, http = socks + 1 → force = socks + 2, no collision.
        assert_eq!(force_socks_port(10808, 10809), 10810);
        // A custom http_port on socks + 2 would clash with force-in → step to socks + 3.
        assert_eq!(force_socks_port(10808, 10810), 10811);
        // socks + 3 itself never collides (only reached when http == socks + 2).
        assert_eq!(force_socks_port(10808, 10811), 10810);
    }

    #[test]
    fn rule_network_and_asset_null() {
        assert_eq!(
            serde_json::to_string(&RuleNetwork::TcpUdp).unwrap(),
            "\"tcp,udp\""
        );
        let a: AssetFile = serde_json::from_str(
            r#"{"id":"geoip","remarks":"GeoIP","url":"u","lastUpdated":null,"locked":true}"#,
        )
        .unwrap();
        assert!(serde_json::to_value(&a).unwrap()["lastUpdated"].is_null());
    }

    #[test]
    fn advanced_settings_defaults_from_empty() {
        let s: AdvancedSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, AdvancedSettings::default());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["routingMode"], "global");
        assert_eq!(v["domainSniffing"], true);
        assert_eq!(v["domainStrategy"], "IPIfNonMatch");
        assert_eq!(v["muxConcurrency"], 8);
        assert_eq!(v["fragmentPackets"], "tlshello");
        assert_eq!(v["logRotateMaxKb"], 512);
        assert_eq!(v["appCaptureMode"], "all");
        // TUN engine: single setting, defaulted to tun2socks; MTU present.
        assert_eq!(v["tunEngine"], "tun2socks");
        assert_eq!(v["tunMtu"], 9000);
        // Optional fields omitted, not null.
        assert!(v.get("localSocksPort").is_none());
        assert!(v.get("localPacPort").is_none());
        assert!(v.get("logLevel").is_none());
        assert!(v.get("remoteDns").is_none());
    }

    #[test]
    fn local_ports_fall_back_to_defaults_when_unset() {
        let s: AdvancedSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            s.local_socks_port.unwrap_or(DEFAULT_LOCAL_SOCKS_PORT),
            DEFAULT_LOCAL_SOCKS_PORT
        );
        assert_eq!(
            s.local_http_port.unwrap_or(DEFAULT_LOCAL_HTTP_PORT),
            DEFAULT_LOCAL_HTTP_PORT
        );
        assert_eq!(
            s.local_pac_port.unwrap_or(DEFAULT_LOCAL_PAC_PORT),
            DEFAULT_LOCAL_PAC_PORT
        );
    }

    #[test]
    fn routing_mode_legacy_alias() {
        // Legacy "bypass-lan" deserializes to Global and re-serializes as "global".
        let m: RoutingMode = serde_json::from_str("\"bypass-lan\"").unwrap();
        assert_eq!(m, RoutingMode::Global);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"global\"");
    }

    #[test]
    fn tun_engine_and_app_filter_round_trip() {
        let s: AdvancedSettings = serde_json::from_str(
            r#"{"tunEngine":"hev","appFilter":{"com.x":"force-proxy"},"tunMtu":1500}"#,
        )
        .unwrap();
        assert_eq!(s.tun_engine, TunEngine::Hev);
        assert_eq!(s.app_filter["com.x"], AppFilterMode::ForceProxy);
        assert_eq!(s.tun_mtu, 1500);
        // Unspecified tun tunables fall back to the stock-hev defaults.
        assert_eq!(s.tun_connect_timeout_ms, 10_000);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["tunEngine"], "hev");
        assert_eq!(v["appFilter"]["com.x"], "force-proxy");
    }

    #[test]
    fn default_state_has_base_group() {
        let st = default_app_state();
        assert_eq!(st.groups.len(), 1);
        assert_eq!(st.groups[0].id, "g-main");
        assert_eq!(st.groups[0].name, "Main");
        assert_eq!(st.active_id, None);
        // active_id is required+nullable → serializes as null; version omitted.
        let v = serde_json::to_value(&st).unwrap();
        assert!(v["activeId"].is_null());
        assert!(v.get("version").is_none());
        assert!(v["profiles"].as_array().unwrap().is_empty());
    }
}
