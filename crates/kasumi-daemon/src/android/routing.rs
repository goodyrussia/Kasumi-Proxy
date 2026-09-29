//! Packet routing for the xray data-path: iptables marking + ip rules/tables +
//! fwmark.

use std::collections::BTreeMap;

use kasumi_core::state::{AppCaptureMode, AppFilterMode};
use kasumi_core::tun::{TUN_IPV4_CIDR, TUN_IPV6_CIDR, TUN2_IPV4_CIDR, TUN2_IPV6_CIDR};

use super::paths::{CMD, IP, IP6TABLES, IPTABLES};
use super::{default_uplink, run_out, silent};

pub const FWMARK: u32 = 255;
const RULE_PRIORITY: &str = "1000";
const MARK_CHAIN: &str = "KASUMI_PROXY_MARK";

// Our own route-table numbers (v4 and v6 share them).
const TUN_TABLE: &str = "1100";
const TUN_TABLE_FORCE: &str = "1101";
const PRIO_TUN: &str = "1010";
const PRIO_TUN_FORCE: &str = "1011";

// A legacy strict-route carve-out preference an older build installed; only the
// cleanup path reads it now (a preference band above the OS one).
const STRICT_CARVEOUT_PREF: &str = "8500";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Add,
    Del,
}

/// The per-app capture configuration the routing rules are built from.
pub struct AppFilter {
    pub capture_mode: AppCaptureMode,
    /// `"pkg:uid"` → mode.
    pub entries: BTreeMap<String, AppFilterMode>,
    /// Kill-switch: mark every uid but root, not just system + apps.
    pub strict: bool,
}

pub struct RoutingState {
    pub tun_iface: Option<String>,
    pub tun2_iface: Option<String>,
    pub filter: AppFilter,
    pub socks_port: u16,
    pub http_port: u16,
}

pub fn has_force_proxy(f: &AppFilter) -> bool {
    f.entries.values().any(|m| *m == AppFilterMode::ForceProxy)
}

fn uid_of(key: &str) -> Option<&str> {
    let uid = key.rsplit(':').next()?;
    (!uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit())).then_some(uid)
}

/// `ip [-6] <args>`.
async fn ip_rule(v6: bool, args: &[&str]) -> i32 {
    let mut a: Vec<&str> = vec![IP];
    if v6 {
        a.push("-6");
    }
    a.extend_from_slice(args);
    silent(&a).await
}

async fn mark_uid(ipt: &str, range: &str) {
    silent(&[
        ipt,
        "-t",
        "mangle",
        "-A",
        MARK_CHAIN,
        "-m",
        "owner",
        "--uid-owner",
        range,
        "-j",
        "MARK",
        "--set-xmark",
        "1",
    ])
    .await;
}

/// Append the catch-all uid capture: strict marks every uid but root (1-max);
/// otherwise capture "all" marks system (1000) + apps (9999+) + the OS
/// connectivity-check uid(s). Per-uid bypass / force-proxy rules and the
/// local/REPLY exclusions are added before this, so they take precedence.
/// capture "none" adds nothing.
async fn capture_mark_rules(ipt: &str, filter: &AppFilter, netstack: &[u32]) {
    if filter.strict {
        mark_uid(ipt, "1-2147483647").await;
    } else if filter.capture_mode == AppCaptureMode::All {
        mark_uid(ipt, "1000").await;
        mark_uid(ipt, "9999-2147483647").await;
        for uid in netstack {
            mark_uid(ipt, &uid.to_string()).await;
        }
    }
}

/// Uids to mark for the OS connectivity checks, or none when this capture
/// configuration doesn't capture the system band anyway (strict captures every
/// uid; "none" captures nothing by explicit choice).
async fn connectivity_uids_if_captured(filter: &AppFilter) -> Vec<u32> {
    if !filter.strict && filter.capture_mode == AppCaptureMode::All {
        connectivity_uids().await
    } else {
        Vec::new()
    }
}

/// AOSP's fixed uid of the NetworkStack app (`Process.NETWORK_STACK_UID`), used
/// when the package-manager lookup yields nothing.
const NETWORK_STACK_UID_FALLBACK: u32 = 1073;

/// Uids of Android's connectivity-check process. NetworkMonitor — the captive
/// portal / internet-validation probes (`generate_204` and friends) — runs in
/// the NetworkStack app, whose uid sits inside the system band that read-only
/// capture otherwise leaves direct. Left uncaptured it bypasses the tunnel, so
/// on links where only tunnelled traffic works (zero-rated SIMs, captive
/// intermediaries) every probe fails and the OS marks the network
/// unvalidated/"no internet" — surfacing as Chromium's sticky offline bar even
/// though real traffic flows. The app and its tethering twin share one uid;
/// overlays are separate packages and skipped.
async fn connectivity_uids() -> Vec<u32> {
    let (code, out) = run_out(&[CMD, "package", "list", "packages", "-U", "networkstack"]).await;
    if code == 0 {
        let uids = parse_networkstack_uids(&out);
        if !uids.is_empty() {
            return uids;
        }
    }
    vec![NETWORK_STACK_UID_FALLBACK]
}

/// Parse `cmd package list packages -U networkstack` output (`package:<name>
/// uid:<n>` per line).
fn parse_networkstack_uids(out: &str) -> Vec<u32> {
    let mut uids = Vec::new();
    for line in out.lines() {
        let Some(rest) = line.trim().strip_prefix("package:") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let (Some(pkg), Some(uid)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(uid) = uid.strip_prefix("uid:").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        if !matches!(
            pkg,
            "com.android.networkstack"
                | "com.google.android.networkstack"
                | "com.android.networkstack.tethering"
                | "com.google.android.networkstack.tethering"
        ) {
            continue;
        }
        if !uids.contains(&uid) {
            uids.push(uid);
        }
    }
    uids
}

/// Legacy cleanup: remove any strict-route carve-out rule an older build
/// installed. The xray data-path has no equivalent (its REPLY-direction RETURN
/// already spares incoming traffic).
async fn clear_strict_carveouts() {
    for v6 in [false, true] {
        for _ in 0..4 {
            if ip_rule(v6, &["rule", "del", "pref", STRICT_CARVEOUT_PREF]).await != 0 {
                break;
            }
        }
    }
}

pub async fn remove_mark_rule() {
    ip_rule(
        false,
        &["rule", "del", "fwmark", "255", "priority", RULE_PRIORITY],
    )
    .await;
    ip_rule(
        true,
        &["rule", "del", "fwmark", "255", "priority", RULE_PRIORITY],
    )
    .await;
}

/// Bind the proxy fwmark to the active uplink's route table.
pub async fn apply_mark_rule(iface: &str) {
    if iface.is_empty() {
        return;
    }
    remove_mark_rule().await;
    ip_rule(
        false,
        &[
            "rule",
            "add",
            "fwmark",
            "255",
            "table",
            iface,
            "priority",
            RULE_PRIORITY,
        ],
    )
    .await;
    ip_rule(
        true,
        &[
            "rule",
            "add",
            "fwmark",
            "255",
            "table",
            iface,
            "priority",
            RULE_PRIORITY,
        ],
    )
    .await;
}

async fn app_uid_rules(ipt: &str, filter: &AppFilter) {
    for (key, mode) in &filter.entries {
        let Some(uid) = uid_of(key) else {
            continue;
        };
        match mode {
            AppFilterMode::Bypass => {
                silent(&[
                    ipt,
                    "-t",
                    "mangle",
                    "-A",
                    MARK_CHAIN,
                    "-m",
                    "owner",
                    "--uid-owner",
                    uid,
                    "-j",
                    "RETURN",
                ])
                .await;
            }
            AppFilterMode::ForceProxy => {
                silent(&[
                    ipt,
                    "-t",
                    "mangle",
                    "-A",
                    MARK_CHAIN,
                    "-m",
                    "owner",
                    "--uid-owner",
                    uid,
                    "-j",
                    "MARK",
                    "--set-xmark",
                    "2",
                ])
                .await;
                silent(&[
                    ipt,
                    "-t",
                    "mangle",
                    "-A",
                    MARK_CHAIN,
                    "-m",
                    "owner",
                    "--uid-owner",
                    uid,
                    "-j",
                    "RETURN",
                ])
                .await;
            }
        }
    }
}

async fn local_ipv4_exclusions() {
    for cidr in [
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "224.0.0.0/4",
        "255.255.255.255/32",
    ] {
        silent(&[
            IPTABLES, "-t", "mangle", "-A", MARK_CHAIN, "-d", cidr, "-j", "RETURN",
        ])
        .await;
    }
}

async fn local_ipv6_exclusions() {
    for cidr in ["::1/128", "fc00::/7", "fe80::/10", "ff00::/8"] {
        silent(&[
            IP6TABLES, "-t", "mangle", "-A", MARK_CHAIN, "-d", cidr, "-j", "RETURN",
        ])
        .await;
    }
}

/// Reject loopback proxy-port access from bypass-mode apps so they can't probe the
/// running proxy. Removes existing rules first to avoid stacking.
pub async fn protect_local_ports(
    action: Action,
    filter: &AppFilter,
    socks_port: u16,
    http_port: u16,
) {
    let socks = socks_port.to_string();
    let http = http_port.to_string();
    for (key, mode) in &filter.entries {
        if *mode != AppFilterMode::Bypass {
            continue;
        }
        let Some(uid) = uid_of(key) else {
            continue;
        };
        for port in [socks.as_str(), http.as_str()] {
            for ipt in [IPTABLES, IP6TABLES] {
                let rule = [
                    "-o",
                    "lo",
                    "-p",
                    "tcp",
                    "--dport",
                    port,
                    "-m",
                    "owner",
                    "--uid-owner",
                    uid,
                    "-j",
                    "REJECT",
                    "--reject-with",
                    "tcp-reset",
                ];
                let mut del = vec![ipt, "-D", "OUTPUT"];
                del.extend_from_slice(&rule);
                silent(&del).await;
                if action == Action::Add {
                    let mut add = vec![ipt, "-A", "OUTPUT"];
                    add.extend_from_slice(&rule);
                    silent(&add).await;
                }
            }
        }
    }
}

/// Tear down every rule/table/device the xray data-path installed.
pub async fn clear_routing_rules(st: &RoutingState) {
    remove_mark_rule().await;
    clear_strict_carveouts().await;
    protect_local_ports(Action::Del, &st.filter, st.socks_port, st.http_port).await;

    // IPv4 mark chain
    silent(&[IPTABLES, "-t", "mangle", "-D", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-t", "mangle", "-F", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-t", "mangle", "-X", MARK_CHAIN]).await;
    ip_rule(
        false,
        &[
            "rule", "del", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    ip_rule(
        false,
        &[
            "rule",
            "del",
            "fwmark",
            "2",
            "table",
            TUN_TABLE_FORCE,
            "priority",
            PRIO_TUN_FORCE,
        ],
    )
    .await;
    ip_rule(
        true,
        &[
            "rule",
            "del",
            "fwmark",
            "2",
            "table",
            TUN_TABLE_FORCE,
            "priority",
            PRIO_TUN_FORCE,
        ],
    )
    .await;

    // Our LAN-bypass rules: pref 5020-5022 send RFC1918 sources to the uplink
    // table, 5030-5050 to our tun table. Delete by priority — the band is ours
    // (the OS lives at pref 10000+); the loop clears any stacked duplicate.
    for pref in ["5020", "5021", "5022", "5030", "5040", "5050"] {
        for _ in 0..4 {
            if ip_rule(false, &["rule", "del", "pref", pref]).await != 0 {
                break;
            }
        }
    }
    for table in [TUN_TABLE, TUN_TABLE_FORCE] {
        ip_rule(false, &["route", "flush", "table", table]).await;
        ip_rule(true, &["route", "flush", "table", table]).await;
    }
    if let Some(tun) = &st.tun_iface {
        silent(&[IPTABLES, "-D", "FORWARD", "-o", tun, "-j", "ACCEPT"]).await;
        silent(&[IPTABLES, "-D", "FORWARD", "-i", tun, "-j", "ACCEPT"]).await;
        silent(&[
            IPTABLES,
            "-t",
            "mangle",
            "-D",
            "FORWARD",
            "-o",
            tun,
            "-p",
            "tcp",
            "--tcp-flags",
            "SYN,RST",
            "SYN",
            "-j",
            "TCPMSS",
            "--set-mss",
            "1350",
        ])
        .await;
    }
    if let Some(tun2) = &st.tun2_iface {
        silent(&[IPTABLES, "-D", "FORWARD", "-o", tun2, "-j", "ACCEPT"]).await;
        silent(&[IPTABLES, "-D", "FORWARD", "-i", tun2, "-j", "ACCEPT"]).await;
        silent(&[IP, "link", "delete", "dev", tun2]).await;
    }

    // IPv6 mark chain
    silent(&[IP6TABLES, "-t", "mangle", "-D", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[IP6TABLES, "-t", "mangle", "-F", MARK_CHAIN]).await;
    silent(&[IP6TABLES, "-t", "mangle", "-X", MARK_CHAIN]).await;
    ip_rule(
        true,
        &[
            "rule", "del", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    silent(&[
        IP6TABLES,
        "-D",
        "FORWARD",
        "-j",
        "REJECT",
        "--reject-with",
        "icmp6-no-route",
    ])
    .await;

    if let Some(tun) = &st.tun_iface {
        silent(&[IP, "link", "delete", "dev", tun]).await;
    }
}

/// Bring up tun device addresses/routes/rules and the xray marking chain.
pub async fn apply_external_tun_routing(st: &RoutingState) {
    let Some(tun) = st.tun_iface.as_deref() else {
        return;
    };
    let tun2 = st.tun2_iface.as_deref();
    let netstack = connectivity_uids_if_captured(&st.filter).await;

    silent(&[IP, "addr", "add", TUN_IPV4_CIDR, "dev", tun]).await;
    silent(&[IP, "link", "set", "dev", tun, "up"]).await;
    silent(&[
        IP, "route", "replace", "default", "dev", tun, "table", TUN_TABLE,
    ])
    .await;
    ip_rule(
        false,
        &[
            "rule", "del", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    ip_rule(
        false,
        &[
            "rule", "add", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    if let Some(tun2) = tun2 {
        silent(&[IP, "addr", "add", TUN2_IPV4_CIDR, "dev", tun2]).await;
        silent(&[IP, "link", "set", "dev", tun2, "up"]).await;
        silent(&[
            IP,
            "route",
            "replace",
            "default",
            "dev",
            tun2,
            "table",
            TUN_TABLE_FORCE,
        ])
        .await;
        ip_rule(
            false,
            &[
                "rule",
                "del",
                "fwmark",
                "2",
                "table",
                TUN_TABLE_FORCE,
                "priority",
                PRIO_TUN_FORCE,
            ],
        )
        .await;
        ip_rule(
            false,
            &[
                "rule",
                "add",
                "fwmark",
                "2",
                "table",
                TUN_TABLE_FORCE,
                "priority",
                PRIO_TUN_FORCE,
            ],
        )
        .await;
    }

    // IPv4 marking chain
    silent(&[IPTABLES, "-t", "mangle", "-F", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-t", "mangle", "-D", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-t", "mangle", "-X", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-t", "mangle", "-N", MARK_CHAIN]).await;
    silent(&[
        IPTABLES, "-t", "mangle", "-A", MARK_CHAIN, "-m", "mark", "--mark", "255", "-j", "RETURN",
    ])
    .await;
    silent(&[
        IPTABLES,
        "-t",
        "mangle",
        "-A",
        MARK_CHAIN,
        "-m",
        "conntrack",
        "--ctdir",
        "REPLY",
        "-j",
        "RETURN",
    ])
    .await;
    local_ipv4_exclusions().await;
    app_uid_rules(IPTABLES, &st.filter).await;
    capture_mark_rules(IPTABLES, &st.filter, &netstack).await;
    silent(&[IPTABLES, "-t", "mangle", "-A", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[IPTABLES, "-I", "FORWARD", "-o", tun, "-j", "ACCEPT"]).await;
    silent(&[IPTABLES, "-I", "FORWARD", "-i", tun, "-j", "ACCEPT"]).await;

    // Pin local-origin traffic to the physical uplink so it doesn't loop the tun.
    if let Some(uplink) = default_uplink().await {
        for (src, pref) in [
            ("10.0.0.0/8", "5020"),
            ("172.16.0.0/12", "5021"),
            ("192.168.0.0/16", "5022"),
        ] {
            ip_rule(
                false,
                &[
                    "rule", "del", "from", src, "iif", "lo", "lookup", &uplink, "pref", pref,
                ],
            )
            .await;
            ip_rule(
                false,
                &[
                    "rule", "add", "from", src, "iif", "lo", "lookup", &uplink, "pref", pref,
                ],
            )
            .await;
        }
    }
    for (src, pref) in [
        ("10.0.0.0/8", "5030"),
        ("172.16.0.0/12", "5040"),
        ("192.168.0.0/16", "5050"),
    ] {
        ip_rule(
            false,
            &[
                "rule", "del", "from", src, "lookup", TUN_TABLE, "pref", pref,
            ],
        )
        .await;
        ip_rule(
            false,
            &[
                "rule", "add", "from", src, "lookup", TUN_TABLE, "pref", pref,
            ],
        )
        .await;
    }
    silent(&[
        IPTABLES,
        "-t",
        "mangle",
        "-I",
        "FORWARD",
        "-o",
        tun,
        "-p",
        "tcp",
        "--tcp-flags",
        "SYN,RST",
        "SYN",
        "-j",
        "TCPMSS",
        "--set-mss",
        "1350",
    ])
    .await;

    // IPv6 addresses/routes + marking chain
    silent(&[IP, "-6", "addr", "add", TUN_IPV6_CIDR, "dev", tun]).await;
    silent(&[IP, "-6", "link", "set", "dev", tun, "up"]).await;
    silent(&[
        IP, "-6", "route", "replace", "default", "dev", tun, "table", TUN_TABLE,
    ])
    .await;
    ip_rule(
        true,
        &[
            "rule", "del", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    ip_rule(
        true,
        &[
            "rule", "add", "fwmark", "1", "table", TUN_TABLE, "priority", PRIO_TUN,
        ],
    )
    .await;
    if let Some(tun2) = tun2 {
        silent(&[IP, "-6", "addr", "add", TUN2_IPV6_CIDR, "dev", tun2]).await;
        silent(&[IP, "-6", "link", "set", "dev", tun2, "up"]).await;
        silent(&[
            IP,
            "-6",
            "route",
            "replace",
            "default",
            "dev",
            tun2,
            "table",
            TUN_TABLE_FORCE,
        ])
        .await;
        ip_rule(
            true,
            &[
                "rule",
                "del",
                "fwmark",
                "2",
                "table",
                TUN_TABLE_FORCE,
                "priority",
                PRIO_TUN_FORCE,
            ],
        )
        .await;
        ip_rule(
            true,
            &[
                "rule",
                "add",
                "fwmark",
                "2",
                "table",
                TUN_TABLE_FORCE,
                "priority",
                PRIO_TUN_FORCE,
            ],
        )
        .await;
    }
    silent(&[IP6TABLES, "-t", "mangle", "-F", MARK_CHAIN]).await;
    silent(&[IP6TABLES, "-t", "mangle", "-D", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[IP6TABLES, "-t", "mangle", "-X", MARK_CHAIN]).await;
    silent(&[IP6TABLES, "-t", "mangle", "-N", MARK_CHAIN]).await;
    silent(&[
        IP6TABLES, "-t", "mangle", "-A", MARK_CHAIN, "-m", "mark", "--mark", "255", "-j", "RETURN",
    ])
    .await;
    silent(&[
        IP6TABLES,
        "-t",
        "mangle",
        "-A",
        MARK_CHAIN,
        "-m",
        "conntrack",
        "--ctdir",
        "REPLY",
        "-j",
        "RETURN",
    ])
    .await;
    local_ipv6_exclusions().await;
    app_uid_rules(IP6TABLES, &st.filter).await;
    capture_mark_rules(IP6TABLES, &st.filter, &netstack).await;
    silent(&[IP6TABLES, "-t", "mangle", "-A", "OUTPUT", "-j", MARK_CHAIN]).await;
    silent(&[
        IP6TABLES,
        "-I",
        "FORWARD",
        "-j",
        "REJECT",
        "--reject-with",
        "icmp6-no-route",
    ])
    .await;
}

/// Reload xray app-filter rules without a core restart (reload-app-filter).
pub async fn reload_app_filter_rules(filter: &AppFilter) {
    let netstack = connectivity_uids_if_captured(filter).await;
    for ipt in [IPTABLES, IP6TABLES] {
        silent(&[ipt, "-t", "mangle", "-F", MARK_CHAIN]).await;
        app_uid_rules(ipt, filter).await;
        capture_mark_rules(ipt, filter, &netstack).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_of_extracts_numeric_suffix() {
        assert_eq!(uid_of("com.app:10123"), Some("10123"));
        assert_eq!(uid_of("10123"), Some("10123"));
        assert_eq!(uid_of("com.app:abc"), None);
        assert_eq!(uid_of("com.app:"), None);
    }

    #[test]
    fn parses_networkstack_uids_skipping_overlays() {
        // Real shape, as listed by `cmd package list packages -U networkstack`:
        // the probe app and its tethering twin share one uid; overlays carry
        // their own and are skipped.
        let out = "\
package:com.android.networkstack.tethering.overlay.ncm uid:10202
package:com.android.networkstack.overlay uid:10182
package:com.android.networkstack uid:1073
package:com.android.networkstack.tethering uid:1073
";
        assert_eq!(parse_networkstack_uids(out), vec![1073]);
        assert_eq!(
            parse_networkstack_uids("package:com.google.android.networkstack uid:1073\n"),
            vec![1073]
        );
        assert!(parse_networkstack_uids("").is_empty());
        assert!(parse_networkstack_uids("package:com.example uid:10123\n").is_empty());
        assert!(parse_networkstack_uids("garbage").is_empty());
    }

    #[test]
    fn has_force_proxy_detects_mode() {
        let mut entries = BTreeMap::new();
        entries.insert("a:1".to_string(), AppFilterMode::Bypass);
        let f = AppFilter {
            capture_mode: AppCaptureMode::All,
            entries: entries.clone(),
            strict: false,
        };
        assert!(!has_force_proxy(&f));
        entries.insert("b:2".to_string(), AppFilterMode::ForceProxy);
        let f2 = AppFilter {
            capture_mode: AppCaptureMode::All,
            entries,
            strict: false,
        };
        assert!(has_force_proxy(&f2));
    }
}
