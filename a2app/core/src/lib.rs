//! Core mini-app support for Robrix: the launcher-independent half of
//! hosting sandboxed Splash mini-apps, ported from `host_launcher`.
//!
//! The host app (Robrix) injects a data root via [`set_data_root`] before
//! using anything else; all on-disk state lives under it:
//! `apps/<id>/` (manifest + source + versions), `app_data/<id>/` (the app's
//! private fs jail), `permissions.json`, `a2app_state.json`, `exchange/`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The manifest header comment format (`// name:` etc.) shared by the
/// generation pipeline and bare-source imports.
pub mod header;
/// Mini-app manifests, scopes, and the in-memory registry.
pub mod manifest;
/// The `.splashapp` bundle format for exporting/importing/sharing apps.
pub mod bundle;
/// Version history snapshots for modified apps.
pub mod versions;
/// The permission model: declarations, grants, prompts, restrictions.
pub mod permissions;
/// Built-in sample apps.
pub mod builtin;
/// On-disk persistence for apps, grants, and registry state.
pub mod persistence;
/// The host-service broker that answers `host.request(...)` calls from isolates.
pub mod services;

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Sets the directory that all a2app state lives under. Call once at startup;
/// later calls are ignored.
pub fn set_data_root(root: PathBuf) {
    let _ = DATA_ROOT.set(root);
}

/// The a2app data root. Falls back to a per-process temp dir if the host
/// never set one, so tests can't touch a real profile.
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(|| {
        std::env::temp_dir().join(format!("robrix_a2app_{}", std::process::id()))
    })
}

/// The private storage jail for one mini-app, enforced by the Splash layer
/// via `Splash::set_sandbox_dir`.
pub fn app_sandbox_dir(app_id: &str) -> PathBuf {
    data_root().join("app_data").join(app_id)
}
