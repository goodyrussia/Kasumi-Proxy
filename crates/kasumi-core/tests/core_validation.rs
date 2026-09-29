//! Validates that the configs our builder emits are actually accepted by the real
//! Xray core (`xray run -test`), catching schema drift — e.g. a field our builder
//! emits that the pinned core version rejects. This is the config-output safety
//! net: `core-compat.yml` runs it with the staged core on every PR touching
//! `crates/kasumi-core/**`.
//!
//! The case matrix is GENERATED from our own enums (`Protocol`, `Network`,
//! `Security`, `SsMethod`, `VmessEnc` via `strum::IntoEnumIterator`), not
//! hand-written, so a new protocol/transport/enum variant is swept automatically.
//! On top of the enum sweeps, explicit per-field cases exercise the builder
//! branches that defaults alone don't trigger (SS plugin options, TLS cipher
//! suites, gRPC advanced sub-fields, fragment, fake-DNS, routing).
//!
//! The binary is NOT committed; stage it with `scripts/fetch-binaries.sh` (the
//! pinned eichgee source lives in `scripts/binary-versions.sh`) or point at one via
//! `KASUMI_XRAY_BIN`. When it's absent — as in a plain CI checkout — every case is
//! skipped and the test passes, so this never blocks the normal
//! `cargo test --workspace` path.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use strum::IntoEnumIterator;

use kasumi_core::core_config::{build_core_config, build_core_config_pinned};
use kasumi_core::enums::{
    Fingerprint, Flow, HeaderType, Network, PacketEncoding, Security, SsMethod, VmessEnc,
};
use kasumi_core::mixins::{TcpTransport, Transport};
use kasumi_core::profile::{Profile, Protocol};
use kasumi_core::state::{
    AdvancedSettings, DomainStrategy, LogLevel, MuxXudp443, RoutingMode, RoutingRule,
};
use kasumi_core::xray_config::DialPin;

// ── valid credential / crypto material (cores validate these) ──
const UUID: &str = "11111111-1111-1111-1111-111111111111";
const PW: &str = "password123";
const WG_PRIV: &str = "sOs7Qk6VSmoowjvnQw37LUnV39bIG2rOjmmVItntGUw=";
const WG_PUB: &str = "xK8Tw4nv6TBWHl3WlVqoMLVrNsejQdC7/7jiTlR2rg8=";
// A valid base64 32-byte WireGuard pre-shared key (reuses a valid key shape).
const WG_PSK: &str = "YAnz7T2bQ4uR7Mm3y3Hzx2Ysj5PE2lqZqDY8Y8QpZHM=";
const REALITY_PBK: &str = "c7twR4u_IvJsLGDqYsx2yb1nr2Kg74vsRlA_ou8c4QQ";
const SS_KEY_16: &str = "MTIzNDU2Nzg5MGFiY2RlZg==";
const SS_KEY_32: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI=";
// A valid base64 ECHConfigList (generated with a standard ECH keypair tool),
// the raw form share links carry in the `ech` parameter.
const ECH_CONFIG_LIST_B64: &str = "AEb+DQBCAAAgACAYjkLlzMEK3J2Dcv8wBSVwYDz4j8o9tRSTBPSr+m52FwAMAAEAAQABAAIAAQADAAtleGFtcGxlLmNvbQAA";
// A 64-char hex SHA-256 (xray `pinnedPeerCertSha256`) — the core syntax-checks it.
const PCS_HEX: &str = "aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd";

/// The wire string of a serde enum (e.g. `Network::Ws` → `"ws"`).
fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn tls_carrying(p: Protocol) -> bool {
    matches!(
        p,
        Protocol::Vless
            | Protocol::Vmess
            | Protocol::Trojan
            | Protocol::Shadowsocks
            | Protocol::Http
    )
}

/// Protocol-specific credentials a config needs to be buildable + core-valid.
fn creds(seed: &mut Value, proto: Protocol) {
    let put = |seed: &mut Value, k: &str, v: &str| seed[k] = json!(v);
    match proto {
        Protocol::Vless | Protocol::Vmess => put(seed, "uuid", UUID),
        Protocol::Trojan => put(seed, "password", PW),
        Protocol::Socks | Protocol::Http => {
            put(seed, "username", "user");
            put(seed, "password", PW);
        }
        Protocol::Shadowsocks => {
            put(seed, "password", PW);
            put(seed, "method", "aes-256-gcm");
        }
        Protocol::Wireguard => {
            put(seed, "secretKey", WG_PRIV);
            put(seed, "peerPublicKey", WG_PUB);
        }
        Protocol::Custom => {}
    }
}

/// Build a profile seed for a protocol with an optional transport + TLS mode.
fn make(
    proto: Protocol,
    network: Option<Network>,
    security: Security,
    name: &str,
) -> Option<Profile> {
    if proto == Protocol::Custom {
        return None; // carries only a raw blob; no builder path
    }
    let mut seed = json!({
        "protocol": wire(&proto),
        "meta": { "id": "", "remarks": name, "groupId": "g-main" },
        "endpoint": { "address": "e.example", "port": 443 },
    });
    creds(&mut seed, proto);

    if let Some(net) = network {
        let mut t = json!({ "kind": wire(&net) });
        if net == Network::Grpc {
            t["serviceName"] = json!("GunService");
        }
        if net == Network::Ws {
            t["host"] = json!("cdn.example");
            t["path"] = json!("/ws");
        }
        if net == Network::Xhttp {
            t["host"] = json!("cdn.example");
            t["path"] = json!("/x");
            t["mode"] = json!("auto");
        }
        seed["transport"] = t;
    }

    if tls_carrying(proto) {
        let mut tls = json!({ "security": wire(&security), "sni": "s.example" });
        if security == Security::Reality {
            tls["publicKey"] = json!(REALITY_PBK);
            tls["shortId"] = json!("ab");
            tls["fingerprint"] = json!("chrome");
        }
        seed["tls"] = tls;
        if security == Security::Reality && proto == Protocol::Vless {
            seed["flow"] = json!("xtls-rprx-vision");
        }
    }

    serde_json::from_value(seed).ok()
}

/// The base64 key a shadowsocks method needs (2022 AEAD methods are length-checked).
fn ss_key(method: SsMethod) -> &'static str {
    match method {
        SsMethod::Blake3Aes128Gcm => SS_KEY_16,
        SsMethod::Blake3Aes256Gcm | SsMethod::Blake3Chacha20Poly1305 => SS_KEY_32,
        _ => PW,
    }
}

/// Every profile case to validate, generated by sweeping our enums.
fn generate() -> Vec<(String, Profile)> {
    let mut cases: Vec<(String, Profile)> = Vec::new();

    // 1. Every protocol once (default transport, TLS).
    for proto in Protocol::iter() {
        let name = format!("proto/{}", wire(&proto));
        if let Some(p) = make(proto, None, Security::Tls, &name) {
            cases.push((name, p));
        }
    }

    // 2. Transport-carrying protocols × every transport (TLS).
    for proto in [
        Protocol::Vless,
        Protocol::Vmess,
        Protocol::Trojan,
        Protocol::Shadowsocks,
    ] {
        for net in Network::iter() {
            let name = format!("xport/{}-{}", wire(&proto), wire(&net));
            if let Some(p) = make(proto, Some(net), Security::Tls, &name) {
                cases.push((name, p));
            }
        }
    }

    // 3. Shadowsocks × every cipher (TCP).
    for method in SsMethod::iter() {
        let name = format!("ss/{}", wire(&method));
        if let Some(Profile::Shadowsocks(mut s)) =
            make(Protocol::Shadowsocks, None, Security::Tls, &name)
        {
            s.method = method;
            s.password = ss_key(method).to_string();
            cases.push((name, Profile::Shadowsocks(s)));
        }
    }

    // 4. VLESS × every TLS security mode (TCP).
    for sec in Security::iter() {
        let name = format!("sec/vless-{}", wire(&sec));
        if let Some(p) = make(Protocol::Vless, Some(Network::Tcp), sec, &name) {
            cases.push((name, p));
        }
    }

    // 5. VLESS-WS carrying an ECH config. Share links ship the raw base64
    // ECHConfigList; this exercises the emitted `ech` option against the core.
    {
        let name = "ech/vless-ws".to_string();
        if let Some(Profile::Vless(mut v)) =
            make(Protocol::Vless, Some(Network::Ws), Security::Tls, &name)
        {
            v.tls.ech = ECH_CONFIG_LIST_B64.to_string();
            cases.push((name, Profile::Vless(v)));
        }
    }

    // ── explicit per-field cases ──
    //
    // The enum sweeps above exercise every protocol/transport/security/cipher with
    // DEFAULT field values. The cases below set non-default values that trigger
    // distinct builder branches the sweeps don't reach — each branch produces a
    // different config shape that a schema-drift could break silently.

    // 6. VMess × every cipher variant (the enum sweep only hits the default).
    for enc in VmessEnc::iter() {
        let name = format!("vmess-cipher/{}", wire(&enc));
        if let Some(Profile::Vmess(mut v)) =
            make(Protocol::Vmess, Some(Network::Tcp), Security::Tls, &name)
        {
            v.encryption = enc;
            cases.push((name, Profile::Vmess(v)));
        }
    }

    // 7. VMess advanced fields: non-zero alter_id + global_padding +
    //    authenticated_length + packet_encoding.
    {
        let name = "vmess-advanced".to_string();
        if let Some(Profile::Vmess(mut v)) =
            make(Protocol::Vmess, Some(Network::Tcp), Security::Tls, &name)
        {
            v.alter_id = 64;
            v.vmess_global_padding = true;
            v.vmess_authenticated_length = true;
            v.packet_encoding = PacketEncoding::Xudp;
            cases.push((name, Profile::Vmess(v)));
        }
    }

    // 8. VLESS packet_encoding variants the builder branches on (`packetaddr`
    //    produces a different outbound field than the default xudp).
    {
        let name = "vless-packetaddr".to_string();
        if let Some(Profile::Vless(mut v)) =
            make(Protocol::Vless, Some(Network::Tcp), Security::Tls, &name)
        {
            v.packet_encoding = PacketEncoding::Packetaddr;
            cases.push((name, Profile::Vless(v)));
        }
    }

    // 9. VLESS flow = xtls-rprx-vision-udp443 (the sweep only covers vision).
    {
        let name = "vless-vision-udp443".to_string();
        if let Some(Profile::Vless(mut v)) = make(
            Protocol::Vless,
            Some(Network::Tcp),
            Security::Reality,
            &name,
        ) {
            v.flow = Flow::VisionUdp443;
            cases.push((name, Profile::Vless(v)));
        }
    }

    // 10. VLESS outbound mux on (profile-level), grpc transport.
    {
        let name = "vless-mux-grpc".to_string();
        if let Some(Profile::Vless(mut v)) =
            make(Protocol::Vless, Some(Network::Grpc), Security::Tls, &name)
        {
            v.mux_enabled = true;
            cases.push((name, Profile::Vless(v)));
        }
    }

    // 11. TLS extra knobs: cipher suites, curve preferences, min/max version,
    //     PCS hex, non-default uTLS fingerprint.
    {
        let name = "tls-knobs".to_string();
        if let Some(Profile::Vless(mut v)) =
            make(Protocol::Vless, Some(Network::Tcp), Security::Tls, &name)
        {
            v.tls.tls_min_version = "1.2".into();
            v.tls.tls_max_version = "1.3".into();
            v.tls.tls_cipher_suites = vec!["TLS_AES_128_GCM_SHA256".into()];
            v.tls.tls_curve_preferences = vec!["X25519".into()];
            v.tls.pcs = PCS_HEX.into();
            v.tls.fingerprint = Fingerprint::Firefox;
            cases.push((name, Profile::Vless(v)));
        }
    }

    // 12. Shadowsocks over TCP carrying an HTTP fake-header obfuscation.
    {
        let name = "ss-obfs-http".to_string();
        if let Some(Profile::Shadowsocks(mut s)) =
            make(Protocol::Shadowsocks, None, Security::Tls, &name)
        {
            s.transport = Transport::Tcp(TcpTransport {
                header_type: HeaderType::Http,
                host: "cdn.example".into(),
                path: "/".into(),
            });
            cases.push((name, Profile::Shadowsocks(s)));
        }
    }

    // 13. WireGuard explicit knobs (psk, reserved, mtu, keepalive, workers).
    {
        let name = "wireguard-knobs".to_string();
        if let Some(Profile::Wireguard(mut w)) =
            make(Protocol::Wireguard, None, Security::None, &name)
        {
            w.pre_shared_key = WG_PSK.into();
            w.reserved = vec![1, 2, 3];
            w.mtu = 1420;
            w.persistent_keepalive = 25;
            w.workers = 2;
            w.local_address = "172.16.0.2/32, fd00::2/128".into();
            cases.push((name, Profile::Wireguard(w)));
        }
    }

    // 14. Custom profile carrying a valid raw xray config.
    {
        let name = "custom/raw".to_string();
        let raw = json!({
            "log": { "loglevel": "warning" },
            "inbounds": [{ "port": 1080, "listen": "127.0.0.1", "protocol": "socks",
                           "settings": { "auth": "noauth" } }],
            "outbounds": [{ "protocol": "freedom", "tag": "direct" }]
        })
        .to_string();
        let p: Profile = serde_json::from_value(json!({
            "protocol": "custom",
            "meta": { "id": "", "remarks": name, "groupId": "g-main" },
            "raw": raw,
        }))
        .unwrap();
        cases.push((name, p));
    }

    cases
}

/// One validation case: a profile plus optional settings / rules / chain peers.
/// `needs_geo` marks cases referencing geoip / geosite data files (skipped when no
/// `geoip.dat` is staged next to the core).
struct Case {
    name: String,
    profile: Profile,
    settings: AdvancedSettings,
    rules: Vec<RoutingRule>,
    others: Vec<Profile>,
    needs_geo: bool,
}

fn plain(name: &str, profile: Profile) -> Case {
    Case {
        name: name.to_string(),
        profile,
        settings: AdvancedSettings::default(),
        rules: vec![],
        others: vec![],
        needs_geo: false,
    }
}

fn rule(id: &str, outbound: &str) -> RoutingRule {
    RoutingRule {
        id: id.into(),
        remarks: id.into(),
        enabled: true,
        outbound_tag: outbound.into(),
        domain: None,
        ip: None,
        port: None,
        network: None,
        protocol: None,
        process: None,
        package_name: None,
        source_ip: None,
    }
}

fn settings_cases() -> Vec<Case> {
    let base = || make(Protocol::Vless, Some(Network::Tcp), Security::Tls, "s").unwrap();
    let mut cases = Vec::new();

    // Fragment on (fragmented TLS handshake mask).
    {
        let s = AdvancedSettings {
            fragment: true,
            fragment_packets: "tlshello".into(),
            fragment_length: Some("100-200".into()),
            fragment_delay: Some("10".into()),
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/fragment".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // Mux on + xudp443 rejection + non-localhost listen + custom ports + socks auth.
    {
        let s = AdvancedSettings {
            mux: true,
            mux_concurrency: 8,
            mux_xudp_concurrency: Some(4),
            mux_xudp443: Some(MuxXudp443::Reject),
            allow_non_localhost: true,
            local_socks_port: Some(10808),
            local_http_port: Some(10809),
            socks_username: Some("user".into()),
            socks_password: Some("pw".into()),
            domain_strategy: DomainStrategy::IpIfNonMatch,
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/mux-inbound".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // DNS split (Exclave-style): remote over TCP through the proxy + domestic
    // direct resolvers + ipv6, answering the captured port-53 traffic via
    // `dns-out` instead of raw forwarding it.
    {
        let s = AdvancedSettings {
            dns_via_proxy: true,
            ipv6_enabled: Some(true),
            remote_dns: Some("tcp://1.1.1.1".into()),
            domestic_dns: Some("tcp://1.1.1.1".into()),
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/dns".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // Domestic DNS = system resolver, and remote resolvers dialled direct
    // (DNS-via-proxy off).
    {
        let s = AdvancedSettings {
            dns_via_proxy: false,
            domestic_dns: Some("localhost".into()),
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/dns-localhost-direct".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // Rules mode: a direct rule's domains join the domestic DNS ownership.
    {
        let s = AdvancedSettings {
            routing_mode: RoutingMode::Rules,
            ..Default::default()
        };
        let mut bypass = rule("bypass", "direct");
        bypass.domain = Some(vec!["example.com".into(), "geosite:cn".into()]);
        cases.push(Case {
            name: "settings/dns-rules-direct".into(),
            profile: base(),
            settings: s,
            rules: vec![bypass],
            others: vec![],
            needs_geo: true,
        });
    }

    // Fake-DNS: pool + `fakedns` allocator + sniffer override (no geo dependency).
    {
        let s = AdvancedSettings {
            fake_dns: true,
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/dns-fake".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // Log levels.
    for level in [
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warning,
        LogLevel::Error,
        LogLevel::None,
    ] {
        let s = AdvancedSettings {
            log_level: Some(level),
            ..Default::default()
        };
        cases.push(Case {
            name: format!("settings/log-{}", wire(&level)),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    // Routing modes + a rules-mode case with domain/ip/port rules.
    for mode in [RoutingMode::Global, RoutingMode::Custom, RoutingMode::Rules] {
        let mut r1 = rule("r1", "direct");
        r1.domain = Some(vec![
            "domain:example.com".into(),
            "full:exact.example".into(),
        ]);
        r1.port = Some("80,443".into());
        let mut r2 = rule("r2", "block");
        r2.ip = Some(vec!["192.168.0.0/16".into()]);
        let settings = AdvancedSettings {
            routing_mode: mode,
            ..Default::default()
        };
        cases.push(Case {
            name: format!("settings/routing-{}", wire(&mode)),
            profile: base(),
            settings,
            rules: vec![r1, r2],
            others: vec![],
            needs_geo: false,
        });
    }

    // Geo-dependent rule (needs staged geoip/geosite or is skipped).
    {
        let mut r = rule("geo", "block");
        r.ip = Some(vec!["geoip:private".into()]);
        r.domain = Some(vec!["geosite:cn".into()]);
        cases.push(Case {
            name: "settings/routing-geo".into(),
            profile: base(),
            settings: AdvancedSettings::default(),
            rules: vec![r],
            others: vec![],
            needs_geo: true,
        });
    }

    // Custom raw routing JSON string.
    {
        let s = AdvancedSettings {
            custom_routing: Some(
                json!({ "domainStrategy": "IPIfNonMatch",
                        "rules": [{ "type": "field", "outboundTag": "direct",
                                    "domain": ["geosite:cn"] }] })
                .to_string(),
            ),
            ..Default::default()
        };
        cases.push(Case {
            name: "settings/custom-routing".into(),
            profile: base(),
            settings: s,
            rules: vec![],
            others: vec![],
            needs_geo: false,
        });
    }

    cases
}

fn chain_cases() -> Vec<Case> {
    let with_id = |mut p: Profile, id: &str, via: Option<&str>| {
        p.meta_mut().id = id.into();
        p.meta_mut().via = via.map(str::to_string);
        p
    };
    let exit = make(Protocol::Vless, Some(Network::Ws), Security::Tls, "exit").unwrap();
    let mut cases = Vec::new();
    for (proto, net) in [
        (Protocol::Trojan, Some(Network::Tcp)),
        (Protocol::Wireguard, None),
        (Protocol::Shadowsocks, None),
    ] {
        let mid = with_id(
            make(proto, net, Security::Tls, "mid").unwrap(),
            "mid",
            Some("entry"),
        );
        let entry = with_id(
            make(Protocol::Vmess, Some(Network::Grpc), Security::Tls, "entry").unwrap(),
            "entry",
            None,
        );
        let exit = with_id(exit.clone(), "exit", Some("mid"));
        cases.push(Case {
            name: format!("chain/{}", wire(&proto)),
            profile: exit,
            settings: AdvancedSettings::default(),
            rules: vec![],
            others: vec![mid, entry],
            needs_geo: false,
        });
    }
    cases
}

fn binaries_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin")
}

fn find_core(env_var: &str, prefix: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_var) {
        let path = PathBuf::from(p);
        return path.is_file().then_some(path);
    }
    let dir = binaries_dir();
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && !name.ends_with(".dll") {
            return Some(e.path());
        }
    }
    None
}

/// Build one case's config; `None` when our builder declines the combo.
fn build_config(
    profile: &Profile,
    others: &[Profile],
    settings: &AdvancedSettings,
    rules: &[RoutingRule],
) -> Option<Value> {
    let profiles: Vec<Profile> = std::iter::once(profile.clone())
        .chain(others.iter().cloned())
        .collect();
    build_core_config(profile, settings, rules, &profiles)
        .ok()
        .map(|c| c.config)
}

fn write_config(cfg: &Value) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, serde_json::to_string_pretty(cfg).unwrap()).unwrap();
    (dir, path)
}

fn validate(bin: &Path, cfg: &Path, asset_dir: &Path) -> (bool, String) {
    let out = Command::new(bin)
        .args(["run", "-test", "-c"])
        .arg(cfg)
        .env("XRAY_LOCATION_ASSET", asset_dir)
        .output()
        .expect("spawn xray");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), combined)
}

/// Build every case and run it past the staged core (skipping everything when no
/// core is staged, as in a plain CI checkout).
fn validate_all(cases: Vec<Case>) {
    let xray = find_core("KASUMI_XRAY_BIN", "xray");
    let cores_staged = xray.is_some();
    if !cores_staged {
        eprintln!(
            "no staged xray binary in {} (run scripts/fetch-binaries.sh) — static sweep only",
            binaries_dir().display()
        );
    }

    let asset_dir = binaries_dir();
    let has_geo = asset_dir.join("geoip.dat").is_file();

    let mut core_failures = Vec::new();
    let mut built = 0;
    let mut checked = 0;
    let mut skipped_geo = 0;
    for Case {
        name,
        profile,
        settings,
        rules,
        needs_geo,
        others,
    } in cases
    {
        let Some(cfg_value) = build_config(&profile, &others, &settings, &rules) else {
            continue; // our builder declined this combo — not a core problem.
        };
        built += 1;
        if !cores_staged {
            continue;
        }
        if needs_geo && !has_geo {
            skipped_geo += 1;
            continue;
        }
        let Some(bin) = xray.as_deref() else { continue };
        let (_keep, cfg) = write_config(&cfg_value);
        let (ok, output) = validate(bin, &cfg, asset_dir.as_path());
        checked += 1;
        if !ok {
            core_failures.push(format!("[{name}] Xray rejected:\n{}", output.trim()));
        }
    }

    eprintln!(
        "config sweep: {built} built; {}/{checked} accepted by xray{}",
        checked - core_failures.len(),
        if skipped_geo > 0 {
            format!(
                ", {skipped_geo} geo-dependent skipped (no geoip.dat in {})",
                asset_dir.display()
            )
        } else {
            String::new()
        }
    );
    let failures = core_failures;
    assert!(
        failures.is_empty(),
        "{} problems across {built} generated configs ({checked} core-validated):\n\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
    assert!(
        built > 0,
        "no cases built — generator declined every combo?"
    );
    assert!(
        !cores_staged || checked > 0,
        "no cases validated — staged binary unreadable?"
    );
}

#[test]
fn protocol_matrix_validates_against_real_cores() {
    let cases = generate()
        .into_iter()
        .map(|(name, p)| plain(&name, p))
        .collect();
    validate_all(cases);
}

#[test]
fn chain_matrix_validates_against_real_cores() {
    validate_all(chain_cases());
}

#[test]
fn settings_matrix_validates_against_real_cores() {
    validate_all(settings_cases());
}

/// The pinned config differs from the plain one only in the dial address being a
/// literal (`DialPin`): the core must still accept it (and the name fields must
/// keep the hostname — asserted before validation).
#[test]
fn a_pinned_address_config_validates_against_real_cores() {
    let Some(bin) = find_core("KASUMI_XRAY_BIN", "xray") else {
        eprintln!("no staged xray binary — skipping");
        return;
    };
    let Some(mut profile) = make(Protocol::Vless, Some(Network::Ws), Security::Tls, "pinned")
    else {
        return;
    };
    if let Profile::Vless(v) = &mut profile {
        v.endpoint.address = "vpn.example".to_string();
    }
    let pin = DialPin {
        host: "vpn.example".to_string(),
        ip: "203.0.113.7".to_string(),
    };
    let cfg = build_core_config_pinned(
        &profile,
        &AdvancedSettings::default(),
        &[],
        std::slice::from_ref(&profile),
        Some(&pin),
    )
    .unwrap()
    .config;
    let outbound = &cfg["outbounds"][0];
    assert_eq!(outbound["settings"]["vnext"][0]["address"], "203.0.113.7");
    assert_eq!(
        outbound["streamSettings"]["wsSettings"]["host"],
        "cdn.example"
    );
    let (_keep, path) = write_config(&cfg);
    let (ok, output) = validate(&bin, &path, binaries_dir().as_path());
    assert!(ok, "Xray rejected the pinned config:\n{}", output.trim());
}
