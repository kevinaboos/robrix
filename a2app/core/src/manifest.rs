//! Mini-app manifests and the in-memory registry of installed apps.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// A stable identifier for an installed mini-app, e.g. `"roll-call"`.
pub type MiniAppId = String;

/// Everything the host knows about one installed mini-app.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MiniAppManifest {
    pub id: MiniAppId,
    /// Display name shown under the icon.
    pub name: String,
    /// Emoji drawn as the icon glyph (rendered via the built-in emoji font).
    pub icon: String,
    /// Tint color of the icon tile, as 0xRRGGBB.
    pub tint: u32,
    /// The Splash source code of the app itself.
    pub source: String,
    /// Legacy pre-permissions field; normalized into `permissions` at every
    /// load and kept in sync so older builds still read exported apps right.
    pub allow_net: bool,
    /// Permission ids this app DECLARES it may use. Undeclared capabilities
    /// are ungrantable. Grant state is the user's, lives in the host's
    /// permission store, and never travels with the app.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Why this app wants each permission, in ITS words — iOS's usage strings.
    /// Keyed by permission id; missing entries fall back to the host's
    /// generic description. Shown on the prompt, always attributed to the app
    /// so a persuasive string can't pose as the system.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permission_reasons: BTreeMap<String, String>,
    /// Pre-installed apps cannot be uninstalled.
    pub builtin: bool,
    /// A widget-form Splash script some host_launcher bundles carry. Kept for
    /// bundle compat; Robrix never runs widget scripts.
    pub widget: Option<WidgetManifest>,
    /// Quick-action labels from host_launcher bundles; bundle compat only.
    #[serde(default)]
    pub shortcuts: Vec<String>,
    /// Where this app lives: the whole account, or attached to one room.
    #[serde(default)]
    pub scope: A2AppScope,
}

impl MiniAppManifest {
    /// Reconciles the legacy `allow_net` flag with the `permissions` list, in
    /// both directions: an old export's `allow_net: true` becomes a declared
    /// `network`, and `allow_net` mirrors the declaration so downgrades and
    /// old readers keep working. Call at every point a manifest enters the
    /// system (builtins, disk load, import, generation).
    pub fn normalize_permissions(&mut self) {
        if self.allow_net && !self.declares(crate::permissions::Permission::Network) {
            self.permissions.push("network".to_string());
        }
        // Drop unknown and duplicate ids at the door: an id this build can't
        // grant is noise in every list, and a repeat would burn a fixed UI row.
        let mut seen: Vec<String> = Vec::new();
        for p in std::mem::take(&mut self.permissions) {
            if crate::permissions::Permission::from_str(&p).is_some() && !seen.contains(&p) {
                seen.push(p);
            }
        }
        self.permissions = seen;
        self.permission_reasons
            .retain(|id, _| self.permissions.iter().any(|p| p == id));
        self.allow_net = self.declares(crate::permissions::Permission::Network);
    }

    /// Whether this app declares a permission (the precondition for it ever
    /// being granted).
    pub fn declares(&self, perm: crate::permissions::Permission) -> bool {
        self.permissions.iter().any(|p| p == perm.as_str())
    }

    /// The app's own explanation for wanting a permission, if it gave one.
    pub fn reason_for(&self, perm: crate::permissions::Permission) -> Option<&str> {
        self.permission_reasons
            .get(perm.as_str())
            .map(|s| s.as_str())
            .filter(|s| !s.trim().is_empty())
    }

    /// Adds a declaration (the capability editor for apps the user owns).
    /// No-op when already declared; keeps `allow_net` in step.
    pub fn declare(&mut self, perm: crate::permissions::Permission) {
        if !self.declares(perm) {
            self.permissions.push(perm.as_str().to_string());
            self.normalize_permissions();
        }
    }

    /// Removes a declaration, which also makes the capability ungrantable.
    pub fn undeclare(&mut self, perm: crate::permissions::Permission) {
        self.permissions.retain(|p| p != perm.as_str());
        self.permission_reasons.remove(perm.as_str());
        self.normalize_permissions();
    }
}

/// A widget provided by a mini-app: a separate, smaller Splash script.
/// Bundle compat only; Robrix never runs it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WidgetManifest {
    /// The Splash source code of the widget form of the app.
    pub source: String,
    /// Default size in grid cells (cols, rows).
    pub default_span: (u8, u8),
    /// Minimum size in grid cells.
    pub min_span: (u8, u8),
}

/// What a mini-app is scoped to. An account app opens standalone; a room app
/// belongs to one room and gets that room's services.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum A2AppScope {
    #[default]
    Account,
    Room { room_id: String },
}

/// The full app registry: every installed app, in a stable order.
#[derive(Default)]
pub struct AppRegistry {
    apps: Vec<MiniAppManifest>,
    index: HashMap<MiniAppId, usize>,
}

impl AppRegistry {
    pub fn new(apps: Vec<MiniAppManifest>) -> Self {
        let mut registry = Self::default();
        for app in apps {
            registry.insert(app);
        }
        registry
    }

    pub fn insert(&mut self, app: MiniAppManifest) {
        if let Some(&i) = self.index.get(&app.id) {
            self.apps[i] = app;
        } else {
            self.index.insert(app.id.clone(), self.apps.len());
            self.apps.push(app);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<MiniAppManifest> {
        let i = self.index.remove(id)?;
        let app = self.apps.remove(i);
        // Reindex everything after the removed entry.
        for (j, a) in self.apps.iter().enumerate().skip(i) {
            self.index.insert(a.id.clone(), j);
        }
        Some(app)
    }

    pub fn get(&self, id: &str) -> Option<&MiniAppManifest> {
        self.index.get(id).map(|&i| &self.apps[i])
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut MiniAppManifest> {
        self.index.get(id).map(|&i| &mut self.apps[i])
    }

    pub fn contains(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &MiniAppManifest> {
        self.apps.iter()
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }
}

/// Separator between an app id and its room in a host instance tag. App ids
/// are slug-safe and room ids never contain '@', so the split is unambiguous.
pub const INSTANCE_TAG_SEP: char = '@';

/// The host tag for one running instance of an app.
pub fn instance_tag(app_id: &str, room_id: Option<&str>) -> String {
    match room_id {
        Some(room) => format!("{app_id}{INSTANCE_TAG_SEP}{room}"),
        None => app_id.to_string(),
    }
}

/// Splits an instance tag back into (app_id, room_id).
pub fn split_instance_tag(tag: &str) -> (&str, Option<&str>) {
    match tag.split_once(INSTANCE_TAG_SEP) {
        Some((app, room)) => (app, Some(room)),
        None => (tag, None),
    }
}
