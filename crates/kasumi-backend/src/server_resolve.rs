//! Connect-time resolution of the proxy server's hostname ("pinning").
//!
//! The core dials the raw hostname on every new connection. On a root-module
//! setup that lookup bottoms out in netd, which runs privileged and is therefore
//! never inside the module's UID capture — bad carrier-DNS windows then stall
//! fresh connections (images/thumbnails). Resolving here — once per connect,
//! with retries and a last-good cache — lets the config dial a literal address
//! while the hostname lives on in every name-carrying field (SNI, ws `Host`).

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use kasumi_core::xray_config::DialPin;

use crate::fsjson::{read_json, write_json_atomic};
use crate::platform::Platform;

/// A cached address younger than this is served without touching DNS.
const CACHE_TTL_SECS: u64 = 300;
/// Fresh-resolution attempts and the per-attempt timeout.
const ATTEMPTS: u32 = 3;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);
/// Pause between attempts so a fast NXDOMAIN doesn't burn them instantly.
const RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServerCache {
    #[serde(default)]
    hosts: BTreeMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    ip: String,
    ts: u64,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn cache_path(platform: &dyn Platform) -> PathBuf {
    platform.paths().data_dir.join("server-cache.json")
}

/// Resolve `host` to a [`DialPin`], or `None` when it is already an IP literal
/// or nothing could be resolved (the config then keeps the hostname).
pub async fn resolve_dial_pin(platform: &dyn Platform, host: &str, port: u16) -> Option<DialPin> {
    let host = host.trim();
    if host.is_empty() || host.parse::<IpAddr>().is_ok() {
        return None;
    }
    let path = cache_path(platform);
    let mut cache: ServerCache = read_json(&path).await.unwrap_or_default();
    let cached = cache.hosts.get(host).cloned();
    if let Some(entry) = &cached
        && now_unix().saturating_sub(entry.ts) < CACHE_TTL_SECS
    {
        return Some(DialPin {
            host: host.to_string(),
            ip: entry.ip.clone(),
        });
    }
    match resolve(host, port).await {
        Some(ip) => {
            if cached.as_ref().is_none_or(|c| c.ip != ip) {
                log::info!("server address {host} resolves to {ip}");
            }
            cache.hosts.insert(
                host.to_string(),
                CacheEntry {
                    ip: ip.clone(),
                    ts: now_unix(),
                },
            );
            if let Err(e) = write_json_atomic(&path, &cache).await {
                log::warn!("server address cache write failed: {e}");
            }
            Some(DialPin {
                host: host.to_string(),
                ip,
            })
        }
        None => match cached {
            Some(entry) => {
                log::warn!(
                    "server address {host} could not be resolved; using cached {}",
                    entry.ip
                );
                Some(DialPin {
                    host: host.to_string(),
                    ip: entry.ip,
                })
            }
            None => {
                log::warn!("server address {host} could not be resolved; keeping the hostname");
                None
            }
        },
    }
}

/// Best-effort fresh resolution with bounded retries; prefers IPv4.
async fn resolve(host: &str, port: u16) -> Option<String> {
    for attempt in 1..=ATTEMPTS {
        match tokio::time::timeout(ATTEMPT_TIMEOUT, tokio::net::lookup_host((host, port))).await {
            Ok(Ok(addrs)) => {
                let mut v4: Option<IpAddr> = None;
                let mut any: Option<IpAddr> = None;
                for a in addrs {
                    let ip = a.ip();
                    if ip.is_ipv4() && v4.is_none() {
                        v4 = Some(ip);
                    }
                    if any.is_none() {
                        any = Some(ip);
                    }
                }
                if let Some(ip) = v4.or(any) {
                    return Some(ip.to_string());
                }
                log::debug!("server address {host}: no addresses (attempt {attempt})");
            }
            Ok(Err(e)) => {
                log::debug!("server address {host}: resolve failed (attempt {attempt}): {e}");
            }
            Err(_) => {
                log::debug!("server address {host}: resolve timed out (attempt {attempt})");
            }
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestPlatform;

    #[tokio::test]
    async fn ip_literals_are_not_pinned() {
        let (p, _d) = TestPlatform::new();
        assert!(resolve_dial_pin(&p, "217.154.33.118", 443).await.is_none());
        assert!(resolve_dial_pin(&p, "   ", 443).await.is_none());
    }

    #[tokio::test]
    async fn a_fresh_cache_entry_is_served_without_network() {
        let (p, _d) = TestPlatform::new();
        let cache = ServerCache {
            hosts: BTreeMap::from([(
                "unresolvable.invalid".to_string(),
                CacheEntry {
                    ip: "203.0.113.9".to_string(),
                    ts: now_unix(),
                },
            )]),
        };
        write_json_atomic(cache_path(&p), &cache).await.unwrap();
        let pin = resolve_dial_pin(&p, "unresolvable.invalid", 443)
            .await
            .unwrap();
        assert_eq!(pin.ip, "203.0.113.9");
    }

    #[tokio::test]
    async fn a_stale_entry_is_served_when_resolution_fails() {
        let (p, _d) = TestPlatform::new();
        let cache = ServerCache {
            hosts: BTreeMap::from([(
                "host.invalid".to_string(),
                CacheEntry {
                    ip: "203.0.113.10".to_string(),
                    ts: now_unix().saturating_sub(CACHE_TTL_SECS * 10),
                },
            )]),
        };
        write_json_atomic(cache_path(&p), &cache).await.unwrap();
        let pin = resolve_dial_pin(&p, "host.invalid", 443).await.unwrap();
        assert_eq!(pin.ip, "203.0.113.10");
    }

    #[tokio::test]
    async fn successful_resolution_updates_the_cache() {
        let (p, _d) = TestPlatform::new();
        let pin = resolve_dial_pin(&p, "localhost", 443).await.unwrap();
        assert_eq!(pin.ip, "127.0.0.1");
        let cache: ServerCache = read_json(cache_path(&p)).await.unwrap();
        assert_eq!(cache.hosts["localhost"].ip, "127.0.0.1");
    }

    #[tokio::test]
    async fn unresolved_hosts_without_a_cache_stay_unpinned() {
        let (p, _d) = TestPlatform::new();
        assert!(resolve_dial_pin(&p, "host.invalid", 443).await.is_none());
    }
}
