//! Debug helper (not shipped): dump generated Xray configs for the local E2E
//! DNS harness so the real core can be pointed at them.
use std::env;
use std::fs;

use kasumi_core::profile::Profile;
use kasumi_core::share::parse_share_link;
use kasumi_core::state::AdvancedSettings;
use kasumi_core::xray_config::build_xray_config;

fn dump(p: &Profile, s: &AdvancedSettings, dir: &str, name: &str) {
    let cfg = build_xray_config(p, s, &[], std::slice::from_ref(p)).unwrap();
    let path = format!("{dir}/{name}.json");
    fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
    println!("wrote {path}");
}

fn main() {
    let dir = env::args().nth(1).expect("usage: dump_config <outdir>");
    let p = parse_share_link(
        "vless://11111111-1111-1111-1111-111111111111@example.com:443?type=tcp&security=tls&sni=example.com",
        None,
    )
    .unwrap();
    let base = AdvancedSettings {
        local_socks_port: Some(20808),
        local_http_port: Some(20809),
        ..Default::default()
    };
    dump(&p, &base, &dir, "default");
    let fake = AdvancedSettings {
        fake_dns: true,
        ..base.clone()
    };
    dump(&p, &fake, &dir, "fake");
}
