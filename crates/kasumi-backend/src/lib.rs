//! Kasumi backend — the transport-neutral orchestration layer.
//!
//! Above [`kasumi_core`]'s pure domain logic, this crate owns everything that
//! touches the host: the [`Platform`] trait (OS operations each shell supplies),
//! process/filesystem/network primitives, the typed command dispatch, the
//! data-path lifecycle and the `Service` that owns it. The Android module's daemon
//! exposes these over a token-gated WS; there is no control socket.

pub mod asset_update;
pub mod commands;
pub mod fs;
pub mod fsjson;
pub mod jobs;
pub mod lifecycle;
pub mod net;
pub mod platform;
pub mod proc;
pub mod service;
pub mod state;
pub mod state_mw;
pub mod updater;

#[cfg(test)]
mod testutil;

pub use commands::{Command, CommandError, Response, dispatch};
pub use platform::{AppInfo, BackendPaths, Platform, PlatformCapabilities};
pub use service::Service;
pub use updater::LifecycleControl;
