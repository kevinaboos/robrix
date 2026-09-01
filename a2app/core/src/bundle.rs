//! Mini-apps as portable files: one app in, one app out.
//!
//! A generated app is just a Splash script plus a scrap of metadata, so a
//! bundle is a single JSON file — small enough to paste into a chat message,
//! self-contained enough to drop in a folder and hand to someone.
//!
//! Import also accepts a **bare Splash script** (`.splash`, or anything that
//! isn't JSON): the `// name:` / `// icon:` / `// tint:` header the generator
//! already writes carries everything a manifest needs, and everything else has
//! a sane default. That means an app you exported, an app you hand-wrote, and
//! an app the agent printed into a chat all import the same way.
//!
//! Neither direction touches the registry — the caller decides what to do
//! with the manifest it gets back.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    data_root,
    manifest::{MiniAppManifest, WidgetManifest},
};

/// Extension of an exported bundle. Deliberately not `.json`: an export is a
/// *thing you can install*, and the file listing says so.
pub const BUNDLE_EXT: &str = "splashapp";

/// Bumped only for a change old readers can't cope with. Readers accept
/// anything ≤ this; a newer bundle is rejected with a message rather than
/// silently importing half of it.
const FORMAT: u32 = 1;

/// The wire form. Flat and boring on purpose — someone will read this in a
/// text editor.
#[derive(Serialize, Deserialize)]
struct BundleFile {
    format: u32,
    /// The exporter's id. A suggestion only: the importer re-uniques it
    /// against what's already installed.
    id: String,
    name: String,
    icon: String,
    tint: u32,
    #[serde(default)]
    allow_net: bool,
    /// Declared permission ids. Declarations only; the importing user's
    /// grants never travel in a bundle.
    #[serde(default)]
    permissions: Vec<String>,
    /// The app's stated reason per permission, shown at install time.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    permission_reasons: std::collections::BTreeMap<String, String>,
    /// Capability ids narrowing the declared groups (see `capabilities`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<String>,
    #[serde(default)]
    shortcuts: Vec<String>,
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    widget: Option<BundleWidget>,
}

#[derive(Serialize, Deserialize)]
struct BundleWidget {
    source: String,
    default_span: (u8, u8),
    min_span: (u8, u8),
}

/// Serializes an app to bundle text. Pretty-printed: bundles get pasted into
/// chats and diffed, and the source is one long escaped string either way.
pub fn to_text(manifest: &MiniAppManifest) -> String {
    let file = BundleFile {
        format: FORMAT,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        icon: manifest.icon.clone(),
        tint: manifest.tint,
        allow_net: manifest.allow_net,
        permissions: manifest.permissions.clone(),
        permission_reasons: manifest.permission_reasons.clone(),
        capabilities: manifest.capabilities.clone(),
        shortcuts: manifest.shortcuts.clone(),
        source: manifest.source.clone(),
        widget: manifest.widget.as_ref().map(|w| BundleWidget {
            source: w.source.clone(),
            default_span: w.default_span,
            min_span: w.min_span,
        }),
    };
    // Infallible in practice (plain owned data); fall back rather than panic
    // in a UI handler.
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Parses bundle text — or a bare Splash script — into a manifest.
///
/// The returned `id` is whatever the bundle suggested (or a kebab-case of the
/// name); the caller must re-unique it against the installed set. `builtin` is
/// always false: importing can't mint a protected app. `scope` is always
/// Account: room attachment is local state and never travels in a file.
pub fn parse(text: &str) -> Result<MiniAppManifest, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("nothing to import".to_string());
    }
    if trimmed.starts_with('{') {
        return parse_bundle(trimmed);
    }
    parse_bare_source(trimmed)
}

fn parse_bundle(text: &str) -> Result<MiniAppManifest, String> {
    let file: BundleFile = serde_json::from_str(text)
        .map_err(|e| format!("not a valid app bundle: {}", first_line(&e.to_string())))?;
    if file.format > FORMAT {
        return Err(format!(
            "bundle format {} is newer than this build understands",
            file.format
        ));
    }
    if file.source.trim().is_empty() {
        return Err("the bundle has no app source".to_string());
    }
    let mut manifest = MiniAppManifest {
        id: sanitize_id(&file.id, &file.name),
        name: clamp_name(&file.name),
        icon: clamp_icon(&file.icon),
        tint: file.tint,
        source: file.source,
        allow_net: file.allow_net,
        permissions: sanitize_permissions(&file.permissions),
        // Clamp each reason: it is untrusted text rendered in host chrome.
        permission_reasons: file
            .permission_reasons
            .into_iter()
            .map(|(k, v)| (k, clamp_reason(&v)))
            .collect(),
        capabilities: file.capabilities,
        builtin: false,
        widget: file.widget.map(|w| WidgetManifest {
            source: w.source,
            default_span: w.default_span,
            min_span: w.min_span,
        }),
        shortcuts: file.shortcuts,
        scope: Default::default(),
    };
    manifest.normalize_permissions();
    Ok(manifest)
}

/// Keeps only permission ids this build knows, deduped. Unknown ids couldn't
/// be granted anyway; dropping them keeps the app's info honest about what
/// the app can ever do.
fn sanitize_permissions(perms: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in perms {
        if crate::permissions::Permission::from_str(p).is_some() && !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// A bare `.splash` script: the header comments are the manifest.
fn parse_bare_source(source: &str) -> Result<MiniAppManifest, String> {
    let header = crate::header::parse_app_header(source);
    let name = header.name.unwrap_or_else(|| "Imported App".to_string());
    let mut manifest = MiniAppManifest {
        id: sanitize_id("", &name),
        name: clamp_name(&name),
        icon: clamp_icon(&header.icon.unwrap_or_else(|| "📦".to_string())),
        tint: header.tint.unwrap_or(0x7c6cf0),
        source: source.to_string(),
        allow_net: false,
        // A hand-written script declares in its header exactly like a
        // generated one; dropping that here would install an app whose own
        // code can never work.
        permissions: sanitize_permissions(&header.permissions),
        capabilities: header.capabilities,
        permission_reasons: header
            .permission_reasons
            .into_iter()
            .map(|(k, v)| (k, clamp_reason(&v)))
            .collect(),
        builtin: false,
        widget: None,
        shortcuts: Vec::new(),
        scope: Default::default(),
    };
    manifest.normalize_permissions();
    Ok(manifest)
}

/// Kebab-cases whatever the bundle claimed, falling back to the name. Never
/// trusts the file: an id becomes a directory name under `apps/`, so anything
/// that isn't `[a-z0-9-]` is dropped here rather than caught downstream.
pub fn sanitize_id(id: &str, name: &str) -> String {
    let from = if id.trim().is_empty() { name } else { id };
    let kebab = from
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if kebab.is_empty() {
        "imported".to_string()
    } else {
        kebab.chars().take(40).collect()
    }
}

/// Trims an app-supplied permission reason to something that fits a prompt
/// line. It is the app's own words rendered in host chrome, so it is clamped
/// hard and stripped of newlines — a reason cannot become a wall of text or
/// fake extra dialog copy.
fn clamp_reason(reason: &str) -> String {
    reason
        .replace(['\n', '\r'], " ")
        .trim()
        .chars()
        .take(120)
        .collect()
}

fn clamp_name(name: &str) -> String {
    let n: String = name.trim().chars().take(18).collect();
    if n.is_empty() { "Imported App".to_string() } else { n }
}

fn clamp_icon(icon: &str) -> String {
    let i: String = icon.trim().chars().take(12).collect();
    if i.is_empty() { "📦".to_string() } else { i }
}

fn first_line(msg: &str) -> String {
    msg.lines().next().unwrap_or(msg).chars().take(90).collect()
}

// ---------------------------------------------------------------------------
// The exchange folder
// ---------------------------------------------------------------------------

/// Where exports are written and imports are looked for. One folder for both
/// directions: exporting an app makes it visible to Import on the next device
/// that syncs the folder, and dropping a file in makes it importable here
/// without any picker.
pub fn exchange_dir() -> PathBuf {
    data_root().join("exchange")
}

/// Writes `manifest` into the exchange folder, returning the path. Overwrites
/// a previous export of the same app — the folder is a drop box, not history.
pub fn write_export(manifest: &MiniAppManifest) -> Result<PathBuf, String> {
    let dir = exchange_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.{BUNDLE_EXT}", sanitize_id(&manifest.id, &manifest.name)));
    std::fs::write(&path, to_text(manifest).as_bytes())
        .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(path)
}

/// One importable file found in the exchange folder.
#[derive(Clone, Debug)]
pub struct ImportEntry {
    pub path: PathBuf,
    /// The app's display name, read from the file (not the file name).
    pub name: String,
    pub icon: String,
    /// The file it came from, for the row's second line.
    pub detail: String,
    /// Permission ids the file's manifest declares. Carried through so the
    /// row can say what the app may ever ask for BEFORE it's installed —
    /// declarations are the only honest thing to show at that point.
    pub permissions: Vec<String>,
}

/// Everything importable in the exchange folder, by name. Files that don't
/// parse are left out rather than listed as broken rows: the folder is
/// user-writable and may hold anything.
pub fn list_importable() -> Vec<ImportEntry> {
    let Ok(entries) = std::fs::read_dir(exchange_dir()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, BUNDLE_EXT | "splash" | "json") {
            continue;
        }
        // Cap the read: this folder is user-writable, and a stray gigabyte
        // file must not stall the picker.
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > 1_000_000 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(manifest) = parse(&text) else { continue };
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        found.push(ImportEntry {
            name: manifest.name,
            icon: manifest.icon,
            detail: file_name.clone(),
            permissions: manifest.permissions,
            path,
        });
    }
    found.sort_by_key(|e| e.name.to_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MiniAppManifest {
        MiniAppManifest {
            id: "pomodoro".to_string(),
            name: "Pomodoro".to_string(),
            icon: "🍅".to_string(),
            tint: 0xE84D3D,
            source: "// name: Pomodoro\nView{}".to_string(),
            allow_net: false,
            permissions: vec!["network".to_string(), "open-url".to_string()],
            permission_reasons: Default::default(),
            capabilities: Vec::new(),
            builtin: true,
            widget: None,
            shortcuts: vec!["Start".to_string()],
            scope: Default::default(),
        }
    }

    #[test]
    fn round_trips_a_bundle() {
        let m = parse(&to_text(&sample())).unwrap();
        assert_eq!(m.id, "pomodoro");
        assert_eq!(m.name, "Pomodoro");
        assert_eq!(m.icon, "🍅");
        assert_eq!(m.tint, 0xE84D3D);
        assert_eq!(m.shortcuts, vec!["Start".to_string()]);
        // Declarations travel; grants never do (they're host state).
        assert_eq!(m.permissions, vec!["network".to_string(), "open-url".to_string()]);
        // Legacy readers keep working: network declared -> allow_net mirrored.
        assert!(m.allow_net);
        // An import can never mint a protected app, whatever the file says.
        assert!(!m.builtin);
    }

    #[test]
    fn unknown_and_duplicate_permissions_are_dropped_on_import() {
        let text = r#"{"format":1,"id":"x","name":"X","icon":"x","tint":0,"source":"View{}",
            "permissions":["network","made-up-cap","network","location"]}"#;
        let m = parse(text).unwrap();
        assert_eq!(m.permissions, vec!["network".to_string(), "location".to_string()]);
    }

    #[test]
    fn imports_a_bare_splash_script() {
        let m = parse("// name: Tip Jar\n// icon: 💰\n// tint: #112233\nView{}").unwrap();
        assert_eq!(m.name, "Tip Jar");
        assert_eq!(m.id, "tip-jar");
        assert_eq!(m.icon, "💰");
        assert_eq!(m.tint, 0x112233);
    }

    #[test]
    fn rejects_junk_and_future_formats() {
        assert!(parse("   ").is_err());
        assert!(parse("{ not json").is_err());
        assert!(parse(r#"{"format":99,"id":"x","name":"X","icon":"x","tint":0,"source":"View{}"}"#)
            .is_err());
        assert!(parse(r#"{"format":1,"id":"x","name":"X","icon":"x","tint":0,"source":"  "}"#)
            .is_err());
    }

    #[test]
    fn ids_from_a_bundle_can_never_escape_the_apps_dir() {
        let text = r#"{"format":1,"id":"../../etc","name":"X","icon":"x","tint":0,"source":"View{}"}"#;
        assert_eq!(parse(text).unwrap().id, "etc");
    }
}
