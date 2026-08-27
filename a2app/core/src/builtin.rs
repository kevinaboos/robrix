//! The manifests of the pre-installed mini-apps.
//!
//! Splash sources live in the `apps/` directory. They're baked into the binary,
//! but in a dev checkout we prefer reading them from disk so `.splash` edits
//! show up on the next app launch without a rebuild.

use crate::manifest::MiniAppManifest;

fn load_source(file: &str, baked: &'static str) -> String {
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("apps")
        .join(file);
    std::fs::read_to_string(dev_path).unwrap_or_else(|_| baked.to_string())
}

macro_rules! app_source {
    ($file:literal) => {
        load_source($file, include_str!(concat!("../apps/", $file)))
    };
}

fn app(id: &str, name: &str, icon: &str, tint: u32, source: String) -> MiniAppManifest {
    let mut manifest = MiniAppManifest {
        id: id.to_string(),
        name: name.to_string(),
        icon: icon.to_string(),
        tint,
        source,
        allow_net: false,
        permissions: permissions_for(id),
        permission_reasons: reasons_for(id),
        builtin: true,
        widget: None,
        shortcuts: Vec::new(),
        scope: Default::default(),
    };
    manifest.normalize_permissions();
    manifest
}

/// What a built-in DECLARES, for anyone holding a saved copy of one.
///
/// A user-modified built-in is stored on disk and that copy shadows the code,
/// so a copy saved before this build had a permission carries none — and a
/// built-in's declarations cannot be edited, so there is no way back. The
/// loader unions these in; see `load_user_app`.
pub fn declared_permissions(id: &str) -> Vec<String> {
    permissions_for(id)
}

/// The stock reasons for a built-in's declarations, same purpose.
pub fn declared_reasons(id: &str) -> std::collections::BTreeMap<String, String> {
    reasons_for(id)
}

/// What each stock app DECLARES. Declaring is not granting: runtime-tier
/// entries still prompt on first use. Apps absent here are fully sandboxed
/// on purpose — don't add "just in case" entries.
fn permissions_for(id: &str) -> Vec<String> {
    let p: &[&str] = match id {
        "room-peek" => &["matrix-room-info", "matrix-room-read", "matrix-room-send"],
        "roll-call" => &["matrix-profile", "matrix-room-send"],
        "room-info" => &["matrix-room-info"],
        "room-members" | "room-pins" | "room-threads" => &["matrix-room-read"],
        _ => &[],
    };
    p.iter().map(|x| x.to_string()).collect()
}

/// Why each stock app wants what it declares, in the APP's voice — iOS's
/// usage strings. Kept in step with the `// why-` lines in each app's own
/// `.splash` header.
fn reasons_for(id: &str) -> std::collections::BTreeMap<String, String> {
    let r: &[(&str, &str)] = match id {
        "room-peek" => &[
            ("matrix-room-info", "Shows this room's name and member count."),
            ("matrix-room-read", "Lists the latest messages in this room."),
            ("matrix-room-send", "Sends the message you type into this room."),
        ],
        "roll-call" => &[
            ("matrix-profile", "Shows who is rolling."),
            ("matrix-room-send", "Posts your roll into this room."),
        ],
        "room-info" => &[
            ("matrix-room-info", "Shows this room's name, topic, and settings."),
        ],
        "room-members" => &[
            ("matrix-room-read", "Lists who is in this room."),
        ],
        "room-pins" => &[
            ("matrix-room-read", "Shows this room's pinned messages."),
        ],
        "room-threads" => &[
            ("matrix-room-read", "Lists the discussion threads in this room."),
        ],
        _ => &[],
    };
    r.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// The pre-installed apps: always present, can't be uninstalled.
pub fn builtin_apps() -> Vec<MiniAppManifest> {
    vec![
        app("room-peek", "Room Peek", "👀", 0x4A90D9, app_source!("room_peek.splash")),
        app("roll-call", "Roll Call", "🎲", 0x7C6CF0, app_source!("roll_call.splash")),
        app("room-info", "Room Info", "🏷", 0x2E86AB, app_source!("room_info.splash")),
        app("room-members", "Room Members", "👥", 0x6C8E3A, app_source!("room_members.splash")),
        app("room-pins", "Pinned Events", "📌", 0xC0533E, app_source!("room_pins.splash")),
        app("room-threads", "Room Threads", "🧵", 0x8A5CA8, app_source!("room_threads.splash")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hardcoded catalog must agree with each app's own `.splash` header,
    /// or the prompt would show different reasons than the app declares.
    #[test]
    fn catalog_matches_the_splash_headers() {
        let apps = builtin_apps();
        assert_eq!(apps.len(), 6);
        for m in &apps {
            assert!(m.builtin);
            assert!(m.widget.is_none());
            let h = crate::header::parse_app_header(&m.source);
            assert_eq!(h.name.as_deref(), Some(m.name.as_str()), "{}", m.id);
            assert_eq!(h.tint, Some(m.tint), "{}", m.id);
            assert_eq!(h.permissions, m.permissions, "{}", m.id);
            assert_eq!(h.permission_reasons, m.permission_reasons, "{}", m.id);
        }
    }
}
