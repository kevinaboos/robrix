//! The mini-app permission model: what an app may ask for, what the user has
//! answered, and what that nets out to.
//!
//! Deny-by-default, Android-style declaration, iOS-style runtime prompts:
//! a capability is reachable only when the app DECLARES it (manifest) AND the
//! user's stored answer (or the tier default) allows it. Grants are host
//! state in `<data_root>/permissions.json`, deliberately outside every app's
//! own storage jail.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::{MiniAppId, MiniAppManifest};

/// Every capability a mini-app can declare. Kebab-case ids are the manifest /
/// wire form; keep them stable, exported bundles carry them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Network,
    Location,
    Notifications,
    ClipboardRead,
    Ipc,
    /// Read the attached room's recent messages.
    MatrixRoomRead,
    /// Send messages into the attached room as the user.
    MatrixRoomSend,
    ClipboardWrite,
    OpenUrl,
    Files,
    Share,
    Auth,
    /// See the attached room's name, topic, and member count.
    MatrixRoomInfo,
    /// Know the user's own Matrix display name and user id.
    MatrixProfile,
    /// Low-sensitivity facts about this device and connection: platform, locale, time zone, onlin
    DeviceInfo,
    /// Take a photo through the system camera.
    Camera,
    /// Record an audio clip through the system recorder.
    Microphone,
    /// See this device's ID and verification state, your homeserver, and app-owned account settin
    MatrixAccountRead,
    /// Change your display name or avatar, block people, or store app settings on your account. A
    MatrixAccountWrite,
    /// Fetch other Matrix users' public profiles and find existing chats with them.
    MatrixUsers,
    /// Be told as messages, edits, reactions, typing, receipts, joins and mentions happen in this
    MatrixRoomWatch,
    /// React, show typing, and mark this room read as you. Off unless 'Apps may write to rooms' i
    MatrixRoomInteract,
    /// Store and read this app's own data as events in this room so everyone using it sees the sa
    MatrixRoomAppData,
    /// Pin messages, favorite this room, flag it unread, or change its settings. Writes are off u
    MatrixRoomManage,
    /// Invite people to this room as you. Asks every time and is off unless 'Apps may change room
    MatrixRoomInvite,
    /// Fetch images and files from this room, get link previews, and upload files to your homeser
    MatrixMedia,
    /// See which rooms, DMs and invites you have with unread counts, and be told when that change
    MatrixRoomsList,
    /// Read rooms you pick beyond the one this app is attached to.
    MatrixRoomsRead,
    /// Post to rooms you pick, as you. Asks every time and is off unless 'Apps may write to rooms
    MatrixRoomsSend,
    /// Join rooms, answer invites, open direct messages, or leave this room as you. Asks every ti
    MatrixMembership,
    /// See your spaces and the rooms inside them.
    MatrixSpaces,
    /// Take you to a room, message, thread, person, space, screen or another mini-app.
    RobrixNavigation,
    /// Put a draft in this room's message box for you to review and send. Nothing is sent by the 
    RobrixComposer,
    /// Resize, move, minimize or break out its pane, badge its tab, and ask for keyboard focus.
    RobrixUi,
    /// Know display settings like view mode, zoom and theme so the app can match Robrix.
    RobrixPreferences,
    /// Be told which room or screen you switch to.
    RobrixObserve,
}

/// Runtime permissions prompt the user on first use; normal ones auto-grant
/// on declaration (both stay user-revocable at any time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Normal,
    Runtime,
}

impl Permission {
    pub const ALL: [Permission; 36] = [
        Permission::Network,
        Permission::Location,
        Permission::Notifications,
        Permission::ClipboardRead,
        Permission::Ipc,
        Permission::MatrixRoomRead,
        Permission::MatrixRoomSend,
        Permission::ClipboardWrite,
        Permission::OpenUrl,
        Permission::Files,
        Permission::Share,
        Permission::Auth,
        Permission::MatrixRoomInfo,
        Permission::MatrixProfile,
        Permission::DeviceInfo,
        Permission::Camera,
        Permission::Microphone,
        Permission::MatrixAccountRead,
        Permission::MatrixAccountWrite,
        Permission::MatrixUsers,
        Permission::MatrixRoomWatch,
        Permission::MatrixRoomInteract,
        Permission::MatrixRoomAppData,
        Permission::MatrixRoomManage,
        Permission::MatrixRoomInvite,
        Permission::MatrixMedia,
        Permission::MatrixRoomsList,
        Permission::MatrixRoomsRead,
        Permission::MatrixRoomsSend,
        Permission::MatrixMembership,
        Permission::MatrixSpaces,
        Permission::RobrixNavigation,
        Permission::RobrixComposer,
        Permission::RobrixUi,
        Permission::RobrixPreferences,
        Permission::RobrixObserve,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Network => "network",
            Permission::Location => "location",
            Permission::Notifications => "notifications",
            Permission::ClipboardRead => "clipboard-read",
            Permission::Ipc => "ipc",
            Permission::MatrixRoomRead => "matrix-room-read",
            Permission::MatrixRoomSend => "matrix-room-send",
            Permission::ClipboardWrite => "clipboard-write",
            Permission::OpenUrl => "open-url",
            Permission::Files => "files",
            Permission::Share => "share",
            Permission::Auth => "auth",
            Permission::MatrixRoomInfo => "matrix-room-info",
            Permission::MatrixProfile => "matrix-profile",
            Permission::DeviceInfo => "device-info",
            Permission::Camera => "camera",
            Permission::Microphone => "microphone",
            Permission::MatrixAccountRead => "matrix-account-read",
            Permission::MatrixAccountWrite => "matrix-account-write",
            Permission::MatrixUsers => "matrix-users",
            Permission::MatrixRoomWatch => "matrix-room-watch",
            Permission::MatrixRoomInteract => "matrix-room-interact",
            Permission::MatrixRoomAppData => "matrix-room-app-data",
            Permission::MatrixRoomManage => "matrix-room-manage",
            Permission::MatrixRoomInvite => "matrix-room-invite",
            Permission::MatrixMedia => "matrix-media",
            Permission::MatrixRoomsList => "matrix-rooms-list",
            Permission::MatrixRoomsRead => "matrix-rooms-read",
            Permission::MatrixRoomsSend => "matrix-rooms-send",
            Permission::MatrixMembership => "matrix-membership",
            Permission::MatrixSpaces => "matrix-spaces",
            Permission::RobrixNavigation => "robrix-navigation",
            Permission::RobrixComposer => "robrix-composer",
            Permission::RobrixUi => "robrix-ui",
            Permission::RobrixPreferences => "robrix-preferences",
            Permission::RobrixObserve => "robrix-observe",
        }
    }

    // Not the FromStr trait: this is infallible-to-Option, not Result.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Permission> {
        Permission::ALL.into_iter().find(|p| p.as_str() == s)
    }

    pub fn tier(self) -> Tier {
        match self {
            Permission::Network
            | Permission::Location
            | Permission::Notifications
            | Permission::ClipboardRead
            | Permission::Ipc
            // Reading and speaking in a room are serious enough to always
            // ask, even though the app was opened from that very room.
            | Permission::MatrixRoomRead
            | Permission::MatrixRoomSend
            | Permission::Camera
            | Permission::Microphone
            | Permission::MatrixAccountRead
            | Permission::MatrixAccountWrite
            | Permission::MatrixUsers
            | Permission::MatrixRoomWatch
            | Permission::MatrixRoomInteract
            | Permission::MatrixRoomAppData
            | Permission::MatrixRoomManage
            | Permission::MatrixRoomInvite
            | Permission::MatrixMedia
            | Permission::MatrixRoomsList
            | Permission::MatrixRoomsRead
            | Permission::MatrixRoomsSend
            | Permission::MatrixMembership
            | Permission::MatrixSpaces
            | Permission::RobrixNavigation
            | Permission::RobrixComposer
            | Permission::RobrixUi
            | Permission::RobrixObserve => Tier::Runtime,
            Permission::ClipboardWrite
            | Permission::OpenUrl
            | Permission::Files
            | Permission::Share
            | Permission::Auth
            | Permission::MatrixRoomInfo
            | Permission::MatrixProfile
            | Permission::DeviceInfo
            | Permission::RobrixPreferences => Tier::Normal,
        }
    }

    /// Short human name for rows and prompts.
    pub fn title(self) -> &'static str {
        match self {
            Permission::Network => "Network",
            Permission::Location => "Location",
            Permission::Notifications => "Notifications",
            Permission::ClipboardRead => "Read Clipboard",
            Permission::Ipc => "App Messaging",
            Permission::MatrixRoomRead => "Read room content",
            Permission::MatrixRoomSend => "Send room messages",
            Permission::ClipboardWrite => "Write Clipboard",
            Permission::OpenUrl => "Open Links",
            Permission::Files => "Files",
            Permission::Share => "Share",
            Permission::Auth => "Authentication",
            Permission::MatrixRoomInfo => "Room details",
            Permission::MatrixProfile => "Your identity",
            Permission::DeviceInfo => "Device details",
            Permission::Camera => "Camera",
            Permission::Microphone => "Microphone",
            Permission::MatrixAccountRead => "Your account details",
            Permission::MatrixAccountWrite => "Change your account",
            Permission::MatrixUsers => "Look up people",
            Permission::MatrixRoomWatch => "Watch this room live",
            Permission::MatrixRoomInteract => "React in this room",
            Permission::MatrixRoomAppData => "Sync app data in this room",
            Permission::MatrixRoomManage => "Manage this room",
            Permission::MatrixRoomInvite => "Invite to this room",
            Permission::MatrixMedia => "Media",
            Permission::MatrixRoomsList => "Your room list",
            Permission::MatrixRoomsRead => "Read other rooms",
            Permission::MatrixRoomsSend => "Send to other rooms",
            Permission::MatrixMembership => "Join, leave and start chats",
            Permission::MatrixSpaces => "Spaces",
            Permission::RobrixNavigation => "Navigate Robrix",
            Permission::RobrixComposer => "Prepare messages",
            Permission::RobrixUi => "Its own pane",
            Permission::RobrixPreferences => "Robrix settings",
            Permission::RobrixObserve => "Watch what you're doing",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Permission::Network => "🌐",
            Permission::Location => "📍",
            Permission::Notifications => "🔔",
            Permission::ClipboardRead => "📋",
            Permission::Ipc => "✉️",
            Permission::MatrixRoomRead => "📖",
            Permission::MatrixRoomSend => "💬",
            Permission::ClipboardWrite => "📋",
            Permission::OpenUrl => "🔗",
            Permission::Files => "📁",
            Permission::Share => "📤",
            Permission::Auth => "🔒",
            Permission::MatrixRoomInfo => "🏷",
            Permission::MatrixProfile => "👤",
            Permission::DeviceInfo => "📱",
            Permission::Camera => "📷",
            Permission::Microphone => "🎙",
            Permission::MatrixAccountRead => "🪪",
            Permission::MatrixAccountWrite => "✏️",
            Permission::MatrixUsers => "🧑‍🤝‍🧑",
            Permission::MatrixRoomWatch => "👁",
            Permission::MatrixRoomInteract => "👍",
            Permission::MatrixRoomAppData => "🧩",
            Permission::MatrixRoomManage => "🛠",
            Permission::MatrixRoomInvite => "➕",
            Permission::MatrixMedia => "🖼",
            Permission::MatrixRoomsList => "🗂",
            Permission::MatrixRoomsRead => "📚",
            Permission::MatrixRoomsSend => "📣",
            Permission::MatrixMembership => "🚪",
            Permission::MatrixSpaces => "🌌",
            Permission::RobrixNavigation => "🧭",
            Permission::RobrixComposer => "⌨️",
            Permission::RobrixUi => "🪟",
            Permission::RobrixPreferences => "⚙️",
            Permission::RobrixObserve => "📡",
        }
    }

    /// What granting actually means, in the user's terms; shown on the prompt
    /// and under the app's permission rows.
    pub fn blurb(self) -> &'static str {
        match self {
            Permission::Network => "Connect to the internet to fetch and send data.",
            Permission::Location => "See your approximate location.",
            Permission::Notifications => "Show popup notifications from this app.",
            Permission::ClipboardRead => "Read whatever is on your clipboard.",
            Permission::Ipc => "Send messages to your other mini-apps.",
            Permission::MatrixRoomRead => "Read this room's messages, members, pins, and threads.",
            Permission::MatrixRoomSend => "Send messages to this room as you.",
            Permission::ClipboardWrite => "Put text on your clipboard.",
            Permission::OpenUrl => "Open web links in your browser.",
            Permission::Files => "Open and save files you pick in the system dialog.",
            Permission::Share => "Open the system share sheet.",
            Permission::Auth => "Ask you to authenticate (Touch ID / password).",
            Permission::MatrixRoomInfo => "See this room's name, topic, and member count.",
            Permission::MatrixProfile => "Know your Matrix display name and user ID.",
            Permission::DeviceInfo => "Low-sensitivity facts about this device and connection: platform, locale, time zone, online/offline, sync state.",
            Permission::Camera => "Take a photo through the system camera.",
            Permission::Microphone => "Record an audio clip through the system recorder.",
            Permission::MatrixAccountRead => "See this device's ID and verification state, your homeserver, and app-owned account settings.",
            Permission::MatrixAccountWrite => "Change your display name or avatar, block people, or store app settings on your account. Asks every time and is off unless 'Apps may change your account' is on.",
            Permission::MatrixUsers => "Fetch other Matrix users' public profiles and find existing chats with them.",
            Permission::MatrixRoomWatch => "Be told as messages, edits, reactions, typing, receipts, joins and mentions happen in this room.",
            Permission::MatrixRoomInteract => "React, show typing, and mark this room read as you. Off unless 'Apps may write to rooms' is on.",
            Permission::MatrixRoomAppData => "Store and read this app's own data as events in this room so everyone using it sees the same state. Saving is off unless 'Apps may write to rooms' is on.",
            Permission::MatrixRoomManage => "Pin messages, favorite this room, flag it unread, or change its settings. Writes are off unless 'Apps may write to rooms' is on.",
            Permission::MatrixRoomInvite => "Invite people to this room as you. Asks every time and is off unless 'Apps may change room membership' is on.",
            Permission::MatrixMedia => "Fetch images and files from this room, get link previews, and upload files to your homeserver.",
            Permission::MatrixRoomsList => "See which rooms, DMs and invites you have with unread counts, and be told when that changes.",
            Permission::MatrixRoomsRead => "Read rooms you pick beyond the one this app is attached to.",
            Permission::MatrixRoomsSend => "Post to rooms you pick, as you. Asks every time and is off unless 'Apps may write to rooms' is on.",
            Permission::MatrixMembership => "Join rooms, answer invites, open direct messages, or leave this room as you. Asks every time and is off unless 'Apps may change room membership' is on.",
            Permission::MatrixSpaces => "See your spaces and the rooms inside them.",
            Permission::RobrixNavigation => "Take you to a room, message, thread, person, space, screen or another mini-app.",
            Permission::RobrixComposer => "Put a draft in this room's message box for you to review and send. Nothing is sent by the app.",
            Permission::RobrixUi => "Resize, move, minimize or break out its pane, badge its tab, and ask for keyboard focus.",
            Permission::RobrixPreferences => "Know display settings like view mode, zoom and theme so the app can match Robrix.",
            Permission::RobrixObserve => "Be told which room or screen you switch to.",
        }
    }
}

/// The user's stored answer for one (app, permission).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GrantState {
    /// Never asked (or reset): runtime tiers prompt, normal tiers auto-grant.
    #[default]
    Ask,
    Granted,
    Denied,
}

/// What a request nets out to right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effective {
    Granted,
    Denied,
    /// Declared, runtime-tier, still Ask: park the request and prompt.
    NeedsPrompt,
    /// Not in the app's manifest: refuse without ever prompting.
    Undeclared,
}

/// One recorded use of a capability, for the "recent access" line the
/// permission UI shows. Cheap and bounded — this is a privacy receipt, not
/// telemetry: it never leaves the device and holds no request contents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessRecord {
    pub app_id: MiniAppId,
    /// Permission id (string so an unknown-to-this-build entry survives).
    pub perm: String,
    /// Unix seconds.
    pub at: u64,
}

/// Most access records kept. A few hundred covers "what has this thing been
/// doing lately" without turning permissions.json's sibling into a log file.
pub const MAX_ACCESS_RECORDS: usize = 240;

/// All grants, keyed app id -> permission id. Owned by the host, persisted
/// whole-file on every change (it is tiny, and a lost file just re-asks).
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct PermissionStore {
    grants: BTreeMap<MiniAppId, BTreeMap<String, GrantState>>,
    /// Per-capability answers layered under the group grants: `Ask` follows
    /// the group, `Granted`/`Denied` override it (a group `Denied` still
    /// wins, so blocking a group is a true kill switch).
    #[serde(default)]
    cap_overrides: BTreeMap<MiniAppId, BTreeMap<String, GrantState>>,
    /// Newest-last ring of capability uses (see [`AccessRecord`]).
    #[serde(default)]
    access: Vec<AccessRecord>,
    /// One-time grants: live for this session only and never hit disk, so
    /// "Allow Once" cannot silently become forever. Cleared when the app's
    /// isolates are torn down, exactly like a phone dropping a one-shot
    /// grant when the app closes.
    #[serde(skip)]
    once: std::collections::HashSet<(MiniAppId, Permission)>,
    /// Grants that expire on the clock ("Allow for 1 hour"), as unix seconds.
    /// Persisted: an expiry survives a restart precisely because it ends by
    /// itself, so nothing is silently extended.
    #[serde(default)]
    until: BTreeMap<MiniAppId, BTreeMap<String, u64>>,
    /// How many times each app has actually exercised each capability.
    #[serde(default)]
    uses: BTreeMap<MiniAppId, BTreeMap<String, u64>>,
    /// Strict mode: normal-tier permissions stop auto-granting, so EVERY
    /// capability has to be allowed explicitly. Off by default — it trades
    /// convenience for control, which should be the user's choice.
    #[serde(default)]
    strict: bool,
    /// Apps the host stopped for abusing the bridge, and why. Persisted
    /// deliberately: an app that hammered its way to a stop must not get a
    /// clean slate by being restarted, or the escalation means nothing.
    /// Only the user clears this.
    #[serde(default)]
    restricted: BTreeMap<MiniAppId, Restriction>,
}

/// Why an app is barred from running, kept so the user is told what happened
/// rather than just finding a dead app.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Restriction {
    /// User-facing sentence, e.g. "made too many requests to the system".
    pub reason: String,
    /// When the host stopped it, unix seconds.
    pub at: u64,
    /// How many of its requests had already been refused when it was stopped.
    /// Recorded here rather than read live, because stopping the app clears
    /// the run's counters — and this is the number that explains the stop.
    #[serde(default)]
    pub refusals: u64,
}

impl PermissionStore {
    pub fn state(&self, app_id: &str, perm: Permission) -> GrantState {
        self.grants
            .get(app_id)
            .and_then(|m| m.get(perm.as_str()))
            .copied()
            .unwrap_or_default()
    }

    pub fn set(&mut self, app_id: &str, perm: Permission, state: GrantState) {
        // A durable answer supersedes any one-time grant for the same pair.
        self.once.remove(&(app_id.to_string(), perm));
        self.grants
            .entry(app_id.to_string())
            .or_default()
            .insert(perm.as_str().to_string(), state);
    }

    pub fn strict(&self) -> bool {
        self.strict
    }

    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    /// Grants a capability until `until_unix` (the sheet's "Allow for 1 hour").
    pub fn grant_until(&mut self, app_id: &str, perm: Permission, until_unix: u64) {
        self.once.remove(&(app_id.to_string(), perm));
        self.grants
            .entry(app_id.to_string())
            .or_default()
            .remove(perm.as_str());
        self.until
            .entry(app_id.to_string())
            .or_default()
            .insert(perm.as_str().to_string(), until_unix);
    }

    /// When a timed grant for this pair runs out, if one is live.
    pub fn timed_until(&self, app_id: &str, perm: Permission, now: u64) -> Option<u64> {
        self.until
            .get(app_id)
            .and_then(|m| m.get(perm.as_str()))
            .copied()
            .filter(|until| *until > now)
    }

    /// Drops expired timed grants; returns what changed (so the caller can
    /// republish the snapshot and re-apply to running isolates).
    pub fn expire_timed(&mut self, now: u64) -> Vec<(MiniAppId, Permission)> {
        let mut expired = Vec::new();
        for (app, perms) in self.until.iter_mut() {
            perms.retain(|id, until| {
                if *until > now {
                    return true;
                }
                if let Some(perm) = Permission::from_str(id) {
                    expired.push((app.clone(), perm));
                }
                false
            });
        }
        self.until.retain(|_, perms| !perms.is_empty());
        expired
    }

    /// Blocks every capability an app declares — the "Block all" action.
    pub fn block_all(&mut self, manifest: &MiniAppManifest) {
        for perm in Permission::ALL {
            if manifest.declares(perm) {
                self.set(&manifest.id, perm, GrantState::Denied);
            }
        }
    }

    /// Forgets every answer for every app: back to first-run. Restrictions
    /// survive on purpose — an app the host stopped for abuse should not be
    /// quietly freed as a side effect of tidying up grants. Letting it run
    /// again is its own deliberate choice.
    pub fn reset_all(&mut self) {
        self.grants.clear();
        self.cap_overrides.clear();
        self.until.clear();
        self.once.clear();
    }

    /// Bars an app from running after it abused the host bridge. This is the
    /// end of the escalation ladder, not a permission decision: no capability
    /// is involved, the app simply does not get to run until the user says so.
    pub fn restrict(&mut self, app_id: &str, reason: &str, at: u64, refusals: u64) {
        self.restricted.insert(
            app_id.to_string(),
            Restriction { reason: reason.to_string(), at, refusals },
        );
    }

    /// Lets a restricted app run again — only ever from the user.
    pub fn unrestrict(&mut self, app_id: &str) {
        self.restricted.remove(app_id);
    }

    /// Why this app is barred, if it is.
    pub fn restriction(&self, app_id: &str) -> Option<&Restriction> {
        self.restricted.get(app_id)
    }

    pub fn is_restricted(&self, app_id: &str) -> bool {
        self.restricted.contains_key(app_id)
    }

    /// Every barred app, for the permission manager's notice.
    pub fn restricted_apps(&self) -> Vec<MiniAppId> {
        self.restricted.keys().cloned().collect()
    }

    /// How many times an app has used a capability.
    pub fn use_count(&self, app_id: &str, perm: Permission) -> u64 {
        self.uses
            .get(app_id)
            .and_then(|m| m.get(perm.as_str()))
            .copied()
            .unwrap_or(0)
    }

    /// Grants a capability for this session only (the prompt's "Allow Once").
    pub fn grant_once(&mut self, app_id: &str, perm: Permission) {
        self.once.insert((app_id.to_string(), perm));
    }

    pub fn has_once(&self, app_id: &str, perm: Permission) -> bool {
        self.once.contains(&(app_id.to_string(), perm))
    }

    /// Drops every one-time grant for an app — called when its isolates go
    /// away (force stop, uninstall, restart), which ends "this once".
    /// Reports whether anything was actually dropped, so callers can skip a
    /// snapshot republish when nothing changed.
    pub fn clear_once_for(&mut self, app_id: &str) -> bool {
        let before = self.once.len();
        self.once.retain(|(id, _)| id != app_id);
        before != self.once.len()
    }

    /// Forget an app entirely (uninstall). A reinstall starts from Ask.
    pub fn remove_app(&mut self, app_id: &str) {
        self.grants.remove(app_id);
        self.cap_overrides.remove(app_id);
        self.until.remove(app_id);
        self.uses.remove(app_id);
        self.clear_once_for(app_id);
        self.access.retain(|r| r.app_id != app_id);
    }

    /// Records that an app actually exercised a capability.
    pub fn record_access(&mut self, app_id: &str, perm: Permission, at: u64) {
        *self
            .uses
            .entry(app_id.to_string())
            .or_default()
            .entry(perm.as_str().to_string())
            .or_insert(0) += 1;
        // Collapse a burst: repeated use within a minute updates the last
        // record instead of filling the ring with near-identical rows.
        if let Some(last) = self.access.last_mut() {
            if last.app_id == app_id && last.perm == perm.as_str() && at.saturating_sub(last.at) < 60
            {
                last.at = at;
                return;
            }
        }
        self.access.push(AccessRecord {
            app_id: app_id.to_string(),
            perm: perm.as_str().to_string(),
            at,
        });
        if self.access.len() > MAX_ACCESS_RECORDS {
            let cut = self.access.len() - MAX_ACCESS_RECORDS;
            self.access.drain(..cut);
        }
    }

    /// When an app last used a capability, if ever.
    pub fn last_access(&self, app_id: &str, perm: Permission) -> Option<u64> {
        self.access
            .iter()
            .rev()
            .find(|r| r.app_id == app_id && r.perm == perm.as_str())
            .map(|r| r.at)
    }

    /// Recent uses, newest first.
    pub fn recent_access(&self, limit: usize) -> Vec<AccessRecord> {
        self.access.iter().rev().take(limit).cloned().collect()
    }

    pub fn effective(&self, manifest: &MiniAppManifest, perm: Permission) -> Effective {
        if !manifest.declares(perm) {
            return Effective::Undeclared;
        }
        // A restricted app holds nothing, whatever it was granted before. It
        // should not be running at all, but this is the choke point every
        // capability check goes through, so it is where the guarantee belongs
        // rather than in whichever caller remembers to ask.
        if self.is_restricted(&manifest.id) {
            return Effective::Denied;
        }
        let stored = self.state(&manifest.id, perm);
        // Session and timed grants outrank Ask but never a stored Denied:
        // "just this once" must not resurrect something you turned off.
        if stored != GrantState::Denied {
            if self.has_once(&manifest.id, perm) {
                return Effective::Granted;
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if self.timed_until(&manifest.id, perm, now).is_some() {
                return Effective::Granted;
            }
        }
        match (stored, perm.tier()) {
            (GrantState::Granted, _) => Effective::Granted,
            (GrantState::Denied, _) => Effective::Denied,
            // Strict mode stops normal tiers auto-granting: everything has to
            // be allowed on purpose.
            (GrantState::Ask, Tier::Normal) if !self.strict => Effective::Granted,
            (GrantState::Ask, _) => Effective::NeedsPrompt,
        }
    }

    /// Whether the capability is usable right now (prompt-pending counts as no).
    pub fn is_granted(&self, manifest: &MiniAppManifest, perm: Permission) -> bool {
        self.effective(manifest, perm) == Effective::Granted
    }

    /// The user's stored answer for one (app, capability); `Ask` = follows
    /// the group.
    pub fn capability_state(&self, app_id: &str, cap_id: &str) -> GrantState {
        self.cap_overrides
            .get(app_id)
            .and_then(|m| m.get(cap_id))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_capability(&mut self, app_id: &str, cap_id: &str, state: GrantState) {
        let per_app = self.cap_overrides.entry(app_id.to_string()).or_default();
        if state == GrantState::Ask {
            per_app.remove(cap_id);
        } else {
            per_app.insert(cap_id.to_string(), state);
        }
    }

    /// What one capability nets out to: its own override first, then its
    /// group's answer; a group `Denied` (or a restriction) beats everything.
    pub fn effective_capability(
        &self,
        manifest: &MiniAppManifest,
        cap: &crate::capabilities::Capability,
    ) -> Effective {
        if !cap.is_available() {
            return Effective::Undeclared;
        }
        if !manifest.declares_capability(cap) {
            return Effective::Undeclared;
        }
        if self.is_restricted(&manifest.id) {
            return Effective::Denied;
        }
        let Some(group) = cap.group else { return Effective::Granted };
        let group_effective = self.effective(manifest, group);
        match self.capability_state(&manifest.id, cap.id) {
            GrantState::Denied => Effective::Denied,
            GrantState::Granted => match group_effective {
                Effective::Denied | Effective::Undeclared => group_effective,
                _ => Effective::Granted,
            },
            GrantState::Ask => group_effective,
        }
    }

    /// The names currently usable — every granted permission group id plus
    /// every granted capability id — what `host.capabilities()` reports
    /// inside the app's isolate, so `host.has("network")` and
    /// `host.has("matrix.room.members.read")` both work.
    pub fn granted_caps(&self, manifest: &MiniAppManifest) -> Vec<String> {
        let mut out: Vec<String> = Permission::ALL
            .into_iter()
            .filter(|p| self.is_granted(manifest, *p))
            .map(|p| p.as_str().to_string())
            .collect();
        out.extend(
            crate::capabilities::CATALOG
                .iter()
                .filter(|c| c.group.is_some())
                .filter(|c| self.effective_capability(manifest, c) == Effective::Granted)
                .map(|c| c.id.to_string()),
        );
        out
    }

    /// Declared permissions with their stored states, in declaration order,
    /// for the app's permission rows. Unknown declared ids are skipped (a
    /// newer bundle's permission this build doesn't know can't be granted
    /// anyway), and duplicates collapse so a malformed manifest can't
    /// overflow the fixed row budget.
    pub fn declared_states(&self, manifest: &MiniAppManifest) -> Vec<(Permission, GrantState)> {
        let mut seen = Vec::new();
        for perm in manifest.permissions.iter().filter_map(|s| Permission::from_str(s)) {
            if !seen.iter().any(|(p, _)| *p == perm) {
                seen.push((perm, self.state(&manifest.id, perm)));
            }
        }
        seen
    }

    /// Every app that declares `perm`, with what it nets out to — the data
    /// behind the per-permission view ("who can see my location?").
    pub fn apps_declaring(
        &self,
        registry: &crate::manifest::AppRegistry,
        perm: Permission,
    ) -> Vec<(MiniAppManifest, Effective)> {
        let mut out: Vec<(MiniAppManifest, Effective)> = registry
            .iter()
            .filter(|m| m.declares(perm))
            .map(|m| (m.clone(), self.effective(m, perm)))
            .collect();
        // Allowed first, then the ones still asking, then blocked; name-sorted
        // inside each group so the list doesn't reshuffle as grants change.
        out.sort_by(|a, b| {
            let rank = |e: &Effective| match e {
                Effective::Granted => 0,
                Effective::NeedsPrompt => 1,
                Effective::Denied => 2,
                Effective::Undeclared => 3,
            };
            rank(&a.1)
                .cmp(&rank(&b.1))
                .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
        });
        out
    }

    /// How many apps declare `perm`, and how many of those can use it now.
    pub fn permission_tally(
        &self,
        registry: &crate::manifest::AppRegistry,
        perm: Permission,
    ) -> (usize, usize) {
        let apps = self.apps_declaring(registry, perm);
        let allowed = apps.iter().filter(|(_, e)| *e == Effective::Granted).count();
        (allowed, apps.len())
    }
}

std::thread_local! {
    /// app id -> currently granted capability names. Widgets that create
    /// Splash isolates read this instead of threading host state through
    /// every open path; the host republishes it every event pass, so it can
    /// never go stale.
    static GRANT_SNAPSHOT: std::cell::RefCell<std::collections::HashMap<MiniAppId, Vec<String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

impl PermissionStore {
    /// The full app -> granted-caps map for [`publish_snapshot`].
    pub fn snapshot(
        &self,
        registry: &crate::manifest::AppRegistry,
    ) -> std::collections::HashMap<MiniAppId, Vec<String>> {
        registry
            .iter()
            .map(|m| (m.id.clone(), self.granted_caps(m)))
            .collect()
    }
}

pub fn publish_snapshot(map: std::collections::HashMap<MiniAppId, Vec<String>>) {
    GRANT_SNAPSHOT.with(|s| *s.borrow_mut() = map);
}

/// The capability names currently granted to an app, per the last published
/// snapshot. Empty for unknown apps: deny-by-default extends to any isolate
/// created before the first publish.
pub fn snapshot_grants_for(app_id: &str) -> Vec<String> {
    GRANT_SNAPSHOT.with(|s| s.borrow().get(app_id).cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(perms: &[&str]) -> MiniAppManifest {
        MiniAppManifest {
            id: "t".into(),
            name: "T".into(),
            icon: "t".into(),
            tint: 0,
            source: String::new(),
            allow_net: false,
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            permission_reasons: Default::default(),
            capabilities: Vec::new(),
            builtin: false,
            widget: None,
            shortcuts: vec![],
            scope: Default::default(),
        }
    }

    #[test]
    fn undeclared_is_never_grantable() {
        let mut store = PermissionStore::default();
        let m = manifest(&[]);
        assert_eq!(store.effective(&m, Permission::Network), Effective::Undeclared);
        // Even a (stray) stored grant can't override a missing declaration.
        store.set("t", Permission::Network, GrantState::Granted);
        assert_eq!(store.effective(&m, Permission::Network), Effective::Undeclared);
        assert!(store.granted_caps(&m).is_empty());
    }

    #[test]
    fn tiers_default_correctly_and_answers_stick() {
        let mut store = PermissionStore::default();
        let m = manifest(&["network", "open-url"]);
        // Runtime tier prompts; normal tier auto-grants.
        assert_eq!(store.effective(&m, Permission::Network), Effective::NeedsPrompt);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Granted);
        store.set("t", Permission::Network, GrantState::Granted);
        assert_eq!(store.effective(&m, Permission::Network), Effective::Granted);
        // Group ids first, then the capability ids they unlock.
        let caps = store.granted_caps(&m);
        assert!(caps.iter().any(|c| c == "network") && caps.iter().any(|c| c == "open-url"));
        assert!(caps.iter().any(|c| c == "network.http") && caps.iter().any(|c| c == "device.url.open"));
        // The user can shut off a normal-tier permission too.
        store.set("t", Permission::OpenUrl, GrantState::Denied);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Denied);
        let caps = store.granted_caps(&m);
        assert!(caps.iter().any(|c| c == "network"));
        assert!(!caps.iter().any(|c| c == "open-url" || c == "device.url.open"));
    }

    #[test]
    fn matrix_tiers_split_read_write_from_info() {
        let store = PermissionStore::default();
        let m = manifest(&["matrix-room-info", "matrix-room-read", "matrix-room-send", "matrix-profile"]);
        // Room details and identity auto-grant; reading and sending prompt.
        assert_eq!(store.effective(&m, Permission::MatrixRoomInfo), Effective::Granted);
        assert_eq!(store.effective(&m, Permission::MatrixProfile), Effective::Granted);
        assert_eq!(store.effective(&m, Permission::MatrixRoomRead), Effective::NeedsPrompt);
        assert_eq!(store.effective(&m, Permission::MatrixRoomSend), Effective::NeedsPrompt);
    }

    #[test]
    fn normalize_translates_legacy_allow_net_both_ways() {
        let mut m = manifest(&[]);
        m.allow_net = true;
        m.normalize_permissions();
        assert!(m.declares(Permission::Network));
        assert!(m.allow_net);

        let mut m2 = manifest(&["network"]);
        m2.allow_net = false;
        m2.normalize_permissions();
        assert!(m2.allow_net, "declaration backfills the legacy flag");
    }

    #[test]
    fn uninstall_resets_to_ask() {
        let mut store = PermissionStore::default();
        let m = manifest(&["location"]);
        store.set("t", Permission::Location, GrantState::Granted);
        assert!(store.is_granted(&m, Permission::Location));
        store.remove_app("t");
        assert_eq!(store.effective(&m, Permission::Location), Effective::NeedsPrompt);
    }

    /// Strict mode is the answer to "an imported app got open-url for free":
    /// with it on, even a normal tier has to be allowed on purpose.
    #[test]
    fn strict_mode_stops_normal_tiers_auto_granting() {
        let mut store = PermissionStore::default();
        let m = manifest(&["open-url"]);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Granted);
        store.set_strict(true);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::NeedsPrompt);
        // An explicit answer still wins over the default either way.
        store.set("t", Permission::OpenUrl, GrantState::Granted);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Granted);
    }

    /// A timed grant works until its clock runs out, then simply stops —
    /// without turning into a stored "denied" the user never chose.
    #[test]
    fn timed_grants_expire_on_their_own() {
        let mut store = PermissionStore::default();
        let now = 1_000_000;
        store.grant_until("t", Permission::Location, now + 3600);
        assert!(store.timed_until("t", Permission::Location, now).is_some());
        let expired = store.expire_timed(now + 10);
        assert!(expired.is_empty(), "not due yet");
        let expired = store.expire_timed(now + 3601);
        assert_eq!(expired, vec![("t".to_string(), Permission::Location)]);
        assert!(store.timed_until("t", Permission::Location, now + 3601).is_none());
        assert_eq!(store.state("t", Permission::Location), GrantState::Ask);
    }

    /// Bulk actions: one tap to shut an app out, one to start over.
    #[test]
    fn block_all_and_reset_all() {
        let mut store = PermissionStore::default();
        let m = manifest(&["network", "open-url"]);
        store.block_all(&m);
        assert_eq!(store.effective(&m, Permission::Network), Effective::Denied);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Denied);
        store.reset_all();
        assert_eq!(store.effective(&m, Permission::Network), Effective::NeedsPrompt);
        assert_eq!(store.effective(&m, Permission::OpenUrl), Effective::Granted);
    }

    /// Uses are counted per capability, and an uninstall forgets them.
    #[test]
    fn use_counts_accumulate_and_reset_with_the_app() {
        let mut store = PermissionStore::default();
        store.record_access("t", Permission::Location, 100);
        store.record_access("t", Permission::Location, 500);
        assert_eq!(store.use_count("t", Permission::Location), 2);
        store.remove_app("t");
        assert_eq!(store.use_count("t", Permission::Location), 0);
    }

    #[test]
    fn ids_round_trip() {
        for p in Permission::ALL {
            assert_eq!(Permission::from_str(p.as_str()), Some(p));
        }
        assert_eq!(Permission::from_str("nope"), None);
        // Dropped host_launcher ids stay dropped, so old bundles carrying
        // them lose the declaration instead of gaining a fake capability.
        assert_eq!(Permission::from_str("background"), None);
        assert_eq!(Permission::from_str("storage-large"), None);
    }

    /// A restricted app holds nothing, whatever it was granted before —
    /// checked at `effective`, so every capability path inherits it.
    #[test]
    fn a_restricted_app_loses_every_capability() {
        let m = manifest(&["location", "clipboard-write"]);
        let mut store = PermissionStore::default();
        store.set("t", Permission::Location, GrantState::Granted);
        assert_eq!(store.effective(&m, Permission::Location), Effective::Granted);
        assert!(store.granted_caps(&m).iter().any(|c| c == "location"));

        store.restrict("t", "made far too many requests", 1000, 42);
        assert_eq!(store.effective(&m, Permission::Location), Effective::Denied);
        assert_eq!(
            store.effective(&m, Permission::ClipboardWrite),
            Effective::Denied,
            "an auto-granted normal tier is off too"
        );
        assert!(store.granted_caps(&m).is_empty());

        // Lifting it restores exactly what was there before, nothing more.
        store.unrestrict("t");
        assert_eq!(store.effective(&m, Permission::Location), Effective::Granted);
        assert!(store.granted_caps(&m).iter().any(|c| c == "location"));
    }

    /// The record survives a restart (that is the whole point) and carries
    /// what the user needs to be told.
    #[test]
    fn a_restriction_persists_with_its_reason() {
        let mut store = PermissionStore::default();
        store.restrict("t", "made far too many requests", 1000, 42);
        let json = serde_json::to_string(&store).unwrap();
        let reloaded: PermissionStore = serde_json::from_str(&json).unwrap();
        let r = reloaded.restriction("t").expect("restriction survives a reload");
        assert_eq!(r.reason, "made far too many requests");
        assert_eq!(r.at, 1000);
        assert_eq!(r.refusals, 42);
        assert_eq!(reloaded.restricted_apps(), vec!["t".to_string()]);
    }

    /// Tidying up grants must not quietly free an app the host stopped.
    #[test]
    fn resetting_grants_leaves_restrictions_alone() {
        let mut store = PermissionStore::default();
        store.set("t", Permission::Location, GrantState::Granted);
        store.restrict("t", "misbehaved", 1000, 1);
        store.reset_all();
        assert!(store.is_restricted("t"), "a stop is not a grant");
        assert_eq!(store.state("t", Permission::Location), GrantState::Ask);
    }

    /// A capability answer sits under its group: Ask follows the group,
    /// an explicit answer overrides it, and a blocked group wins regardless.
    #[test]
    fn capability_overrides_layer_under_groups() {
        use crate::capabilities::by_id;
        let mut store = PermissionStore::default();
        let m = manifest(&["matrix-room-read"]);
        let members = by_id("matrix.room.members.read").unwrap();
        let pins = by_id("matrix.room.pins.read").unwrap();
        assert_eq!(store.effective_capability(&m, members), Effective::NeedsPrompt);
        store.set("t", Permission::MatrixRoomRead, GrantState::Granted);
        assert_eq!(store.effective_capability(&m, members), Effective::Granted);
        store.set_capability("t", members.id, GrantState::Denied);
        assert_eq!(store.effective_capability(&m, members), Effective::Denied);
        assert_eq!(store.effective_capability(&m, pins), Effective::Granted);
        store.set_capability("t", pins.id, GrantState::Granted);
        store.set("t", Permission::MatrixRoomRead, GrantState::Denied);
        assert_eq!(store.effective_capability(&m, pins), Effective::Denied);
        let other = by_id("device.clipboard.read").unwrap();
        assert_eq!(store.effective_capability(&m, other), Effective::Undeclared);
        // Back to Ask clears the override entirely.
        store.set_capability("t", members.id, GrantState::Ask);
        assert_eq!(store.capability_state("t", members.id), GrantState::Ask);
    }

    #[test]
    fn a_narrowed_manifest_declares_only_the_listed_capabilities() {
        use crate::capabilities::by_id;
        let mut m = manifest(&["matrix-room-read"]);
        m.capabilities = vec!["matrix.room.members.read".to_string()];
        m.normalize_permissions();
        assert!(m.declares_capability(by_id("matrix.room.members.read").unwrap()));
        assert!(!m.declares_capability(by_id("matrix.room.pins.read").unwrap()));
        assert!(m.declares_capability(by_id("host.env.read").unwrap()));
        // Listing a capability alone pulls its group in.
        let mut n = manifest(&[]);
        n.capabilities = vec!["device.clipboard.write".to_string()];
        n.normalize_permissions();
        assert!(n.declares(Permission::ClipboardWrite));
    }
}
