//! Debug helper (not shipped): dump generated Xray configs for the local E2E
//! harnesses so the real core can be pointed at them.
//!
//! Usage:
//!   dump_config <outdir>                              — the fixed sample profiles
//!   dump_config <outdir> <share-link>                 — the link's config
//!   dump_config <outdir> <share-link> <pin-host> <pin-ip>
//!                                                     — also a pinned variant
use std::env;
use std::fs;

use kasumi_core::profile::Profile;
use kasumi_core::share::parse_share_link;
use kasumi_core::state::AdvancedSettings;
use kasumi_core::xray_config::{DialPin, build_xray_config_pinned};

fn dump(p: &Profile, s: &AdvancedSettings, pin: Option<&DialPin>, dir: &str, name: &str) {
    let cfg = build_xray_config_pinned(p, s, &[], std::slice::from_ref(p), pin).unwrap();
    let path = format!("{dir}/{name}.json");
    fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
    println!("wrote {path}");
}

fn main() {
    let mut args = env::args().skip(1);
    let dir = args
        .next()
        .expect("usage: dump_config <outdir> [share-link [pin-host pin-ip]]");
    let base = AdvancedSettings {
        local_socks_port: Some(20808),
        local_http_port: Some(20809),
        ..Default::default()
    };
    match args.next() {
        None => {
            let p = parse_share_link(
                "vless://11111111-1111-1111-1111-111111111111@example.com:443?type=tcp&security=tls&sni=example.com",
                None,
            )
            .unwrap();
            dump(&p, &base, None, &dir, "default");
            let fake = AdvancedSettings {
                fake_dns: true,
                ..base.clone()
            };
            dump(&p, &fake, None, &dir, "fake");
        }
        Some(link) => {
            let p = parse_share_link(&link, None).unwrap();
            dump(&p, &base, None, &dir, "link");
            if let (Some(host), Some(ip)) = (args.next(), args.next()) {
                let pin = DialPin { host, ip };
                dump(&p, &base, Some(&pin), &dir, "link-pinned");
            }
        }
    }
}
