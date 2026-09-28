// The `serde_json::json!` macro in the profile fixtures expands deeply (a profile
// has ~50 flat keys); lift the default 128 recursion limit for those tests.
#![recursion_limit = "512"]

//! Kasumi domain core.
//!
//! The neutral, IO-free domain logic shared by every shell: profile/state types
//! (serde + `specta::Type`), share-link parse/build, the xray config builder and
//! constants. The config builder is pinned byte-for-byte against committed
//! reference fixtures (compared as `serde_json::Value`, so key order is
//! irrelevant).
//!
//! Modules:
//! - [`contract`] — daemon↔UI wire types + test-port constants
//! - [`enums`] — tun-engine/transport/TLS value sets
//! - [`mixins`] — shared field groups (meta/endpoint/transport/tls)
//! - [`profile`] — the 8-protocol `Profile` discriminated union
//! - [`state`] — groups/rules/assets + settings/AppState
//! - [`share`] — share-link parse/build

pub mod chain;
pub mod config_shared;
pub mod contract;
pub mod core_config;
pub mod data_path_state;
pub mod enums;
pub mod hev_config;
pub mod migrate;
pub mod mixins;
pub mod mutate;
pub mod normalize;
pub mod profile;
pub mod share;
pub mod state;
pub mod tun;
pub mod tun2socks_config;
pub mod uid;
pub mod xray_config;
