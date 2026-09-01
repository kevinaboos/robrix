//! The catalog of everything a mini-app and Robrix can do to each other,
//! tagged so the user can allow or deny each single ability per app.
//!
//! A capability is one concrete action: an OUTGOING request the app makes
//! of Robrix (a broker service), or an INCOMING event/hook Robrix delivers
//! into the app. Each belongs to a coarse permission group (the thing the
//! user is prompted about); the group is the default answer and the user
//! can override any single capability underneath it in App Info.

use crate::permissions::Permission;

/// Whether the ability only observes, changes something durable, does
/// both in one call, or just triggers a transient host/OS side effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
    /// A transient effect that neither reveals nor persists user data
    /// (navigate, open a dialog, show a popup, resize its own pane).
    Act,
}

/// Who initiates: the app asking Robrix, or Robrix telling the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// mini-app -> Robrix (a `host.request` service).
    Outgoing,
    /// Robrix -> mini-app (a top-level `fn on_...` hook or subscription).
    Incoming,
}

/// How much of the user's world the ability touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// This one running isolate (its pane, focus, subscriptions).
    Instance,
    /// The user's own account (identity, account data, settings).
    Account,
    /// Another Matrix user addressed by id, independent of any room.
    User,
    /// The one room the instance is bound to.
    Room,
    /// Several rooms at once (lists, search, cross-room reads).
    MultiRoom,
    /// A space and its hierarchy.
    Space,
    /// The device/OS (clipboard, files, location, notifications, network).
    Device,
    /// The app's own sandbox and host plumbing; nothing of the user's.
    App,
    /// Other installed mini-apps on this device.
    Apps,
    /// Robrix's own UI state on this device: navigation, composer, prefs.
    Client,
}

/// Whether Robrix implements it in this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Available,
    /// Shipped, but refused without prompting while its global switch is
    /// off (today: the matrix read-only mode).
    RefusedBySwitch,
    /// Robrix already has the SDK call or update stream; needs a service or
    /// hook, a worker arm, and delivery.
    PlannedMachinery,
    /// Needs new plumbing below Robrix (SDK, robius, or splash side) first.
    PlannedNewPlumbing,
    /// Deliberately never offered, whatever a manifest says; listed so the
    /// boundary is explicit.
    Never,
}

/// How much harm a misuse could do, for the user's read of the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Risk {
    Low,
    Medium,
    High,
    Critical,
}

pub struct Capability {
    /// Stable id, `family.noun.verb` for outgoing and `on_family_event` for
    /// incoming. What manifests, grants and `host.has()` refer to.
    pub id: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    pub access: Access,
    pub direction: Direction,
    pub scope: Scope,
    /// The permission group gating it. `None` is plumbing every app gets.
    pub group: Option<Permission>,
    pub status: Status,
    pub risk: Risk,
    /// Broker service ids (outgoing) or the script hook name (incoming)
    /// that carry it on the wire. Wire ids never change; capability ids
    /// are the user-facing layer over them.
    pub wire: &'static [&'static str],
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::ReadWrite => "read+write",
            Access::Act => "act",
        }
    }
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Outgoing => "app → Robrix",
            Direction::Incoming => "Robrix → app",
        }
    }
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Instance => "this instance",
            Scope::Account => "account",
            Scope::User => "a user",
            Scope::Room => "this room",
            Scope::MultiRoom => "many rooms",
            Scope::Space => "space",
            Scope::Device => "device",
            Scope::App => "app-local",
            Scope::Apps => "other apps",
            Scope::Client => "Robrix UI",
        }
    }
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Available => "available",
            Status::RefusedBySwitch => "off by switch",
            Status::PlannedMachinery | Status::PlannedNewPlumbing => "planned",
            Status::Never => "never",
        }
    }
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
            Risk::Critical => "critical",
        }
    }
}

impl Capability {
    /// Reachable in this build (a switch may still refuse it).
    pub fn is_available(&self) -> bool {
        matches!(self.status, Status::Available | Status::RefusedBySwitch)
    }

    /// The compact tag line shown under a capability row; risk is only
    /// called out when it is worth a second look.
    pub fn tags(&self) -> String {
        let mut tags = format!("{} · {} · {}", self.access.as_str(), self.direction.as_str(), self.scope.as_str());
        if self.risk >= Risk::High {
            tags.push_str(" · ");
            tags.push_str(self.risk.as_str());
            tags.push_str(" risk");
        }
        tags
    }
}

macro_rules! cap {
    ($id:expr, $title:expr, $blurb:expr, $access:ident, $dir:ident, $scope:ident, $group:expr, $status:ident, $risk:ident, [$($wire:expr),*]) => {
        Capability {
            id: $id,
            title: $title,
            blurb: $blurb,
            access: Access::$access,
            direction: Direction::$dir,
            scope: Scope::$scope,
            group: $group,
            status: Status::$status,
            risk: Risk::$risk,
            wire: &[$($wire),*],
        }
    };
}

use Permission as P;

/// Every capability, grouped by permission in display order. Available ones
/// map 1:1 onto the broker services and hooks that exist today.
/// Every capability, grouped by permission in display order. Available
/// rows map 1:1 onto the broker services and hooks that exist today; the
/// planned and never rows document the full intended surface.
pub const CATALOG: &[Capability] = &[
    // ----- core -----
    cap!("host.env.read", "App environment", "Its app id, whether a room is attached, and (planned) instance tag, surface, platform.", Read, Outgoing, Instance, None, Available, Low, ["env"]),
    cap!("permissions.query", "Check its grants", "State of every declared group (and, with detail, every capability).", Read, Outgoing, App, None, Available, Low, ["permissions.query"]),
    cap!("permissions.request", "Ask for a permission", "Raise the prompt for one declared GROUP ahead of first use; never a single capability, so the prompt unit stays the group.", Act, Outgoing, App, None, Available, Low, ["permissions.request"]),
    cap!("storage.file.read", "Read private storage", "fs.read / fs.exists / fs.list inside the app's own jail, which is per APP and shared by all its instances.", Read, Outgoing, App, None, Available, Low, []),
    cap!("storage.file.write", "Write private storage", "fs.write / append / remove / mkdir in the app's jail, which is per app and shared by all its instances.", Write, Outgoing, App, None, Available, Low, []),
    cap!("storage.instance.write", "Per-instance storage", "A reserved /instance/ prefix private to this app@room pair, ending the cross-instance clobber.", ReadWrite, Outgoing, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("storage.quota.read", "Storage usage", "Bytes used and the cap, so an app can trim before writes fail.", Read, Outgoing, App, None, PlannedMachinery, Low, []),
    cap!("timer.schedule", "Timers", "start_timeout / start_interval inside the isolate; runs on the UI thread under the per-call instruction limit.", Write, Outgoing, App, None, Available, Low, []),
    cap!("ipc.self.send", "Message its other instances", "ipc.send with to:'self' broadcasts to this app's OTHER isolates (never echoes). Inside one sandbox, so no grant.", Write, Outgoing, App, None, Available, Low, []),
    cap!("events.subscribe", "Subscribe to an event family", "Turn on one gated incoming family with a filter. Checked against that family's capability, charged, records Used, per instance, dies with the isolate.", Act, Outgoing, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("events.unsubscribe", "Unsubscribe", "Stop one family, or all with event:'*'; queued deliveries are dropped.", Act, Outgoing, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("ui.pane.read", "Its pane state", "Surface (dock/tab/modal), dock side, content size, minimized, foreground.", Read, Outgoing, Instance, None, PlannedMachinery, Low, []),
    cap!("ui.pane.close", "Quit itself", "Tear down this instance, same as the Close button. Runs after the broker drain, never mid-dispatch.", Act, Outgoing, Instance, None, PlannedMachinery, Low, []),
    cap!("ui.dialog.confirm", "Host confirm dialog", "A Robrix-drawn yes/no with the app's text, attributed to the app in chrome it cannot fake. Foreground-only, one at a time.", Act, Outgoing, Client, None, PlannedMachinery, Low, []),
    cap!("on_app_resize", "Pane resized", "Called with the new content box whenever it changes by more than 0.5pt; queued during draw, delivered at the next event.", Read, Incoming, Instance, None, Available, Low, ["on_app_resize"]),
    cap!("on_permissions_changed", "Grants changed", "Called with the current caps list after any grant, revoke, expiry or restriction; caps also re-pushed via set_host_caps. A network change restarts the app instead.", Read, Incoming, App, None, Available, Low, ["on_permissions_changed"]),
    cap!("on_focus_changed", "Foreground changed", "Called when this instance becomes, or stops being, the pane the user is looking at.", Read, Incoming, Instance, None, PlannedMachinery, Low, []),
    cap!("on_visibility_changed", "Visibility changed", "visible / minimized / hidden-tab / offscreen-room; the cue to pause timers and network.", Read, Incoming, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("on_surface_changed", "Moved between surfaces", "Called after break-out, return-to-room, or a dock side change.", Read, Incoming, Instance, None, PlannedMachinery, Low, []),
    cap!("on_suspend", "Suspend", "Called when minimized, tab hidden, or Robrix backgrounded; the host pauses its timers after the call returns. One event-pass budget.", Read, Incoming, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("on_resume", "Resume", "Called after suspend with the count of subscribed events dropped meanwhile (no replay).", Read, Incoming, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("on_quit", "Quit", "Best-effort before teardown (close, restart on caps change); skipped on force stop and Restrict; capped to one event pass; fs writes are the only sane use.", Read, Incoming, Instance, None, PlannedNewPlumbing, Low, []),
    cap!("on_low_memory", "Memory pressure", "Called when the host is about to collect harder; drop caches or be stopped.", Read, Incoming, Instance, None, PlannedNewPlumbing, Low, []),
    // ----- network -----
    cap!("network.http", "HTTP requests", "mod.net.http_request from the isolate; traps in a netless isolate, which is why a grant change restarts the app.", ReadWrite, Outgoing, Device, Some(P::Network), Available, High, []),
    // ----- location -----
    cap!("device.location.read", "Current location", "One fix: CoreLocation first, city-level IP geolocation on failure; waiting requests share one fix.", Read, Outgoing, Device, Some(P::Location), Available, High, ["location.get"]),
    cap!("on_location", "Location updates", "A fix stream after events.subscribe('on_location', {min_interval_secs}); stops while suspended.", Read, Incoming, Device, Some(P::Location), PlannedNewPlumbing, High, []),
    // ----- notifications -----
    cap!("notifications.post", "Show a popup", "A popup in Robrix's chrome prefixed with the app name; text flattened and clamped to 200 chars.", Act, Outgoing, Client, Some(P::Notifications), Available, Low, ["notify.post"]),
    cap!("notifications.clear", "Clear its popups", "Accepted for compatibility; a shown popup cannot be recalled today.", Act, Outgoing, Client, Some(P::Notifications), Available, Low, ["notify.clear"]),
    cap!("notifications.system.post", "System notification", "A real OS notification when Robrix is backgrounded; tapping it reopens the app in its room.", Act, Outgoing, Device, Some(P::Notifications), PlannedNewPlumbing, Medium, []),
    cap!("on_notification_activated", "Popup tapped", "Called when the user taps this app's popup or notification; the host also foregrounds the app.", Read, Incoming, Client, Some(P::Notifications), PlannedNewPlumbing, Low, []),
    // ----- clipboard-read -----
    cap!("device.clipboard.read", "Read clipboard", "Clipboard text, 64KB cap; macOS only today (pbpaste off-thread), other platforms answer an error.", Read, Outgoing, Device, Some(P::ClipboardRead), Available, High, ["clipboard.read"]),
    // ----- ipc -----
    cap!("ipc.send", "Message another app", "Fire-and-forget JSON to another installed app's running isolates; refused unless the target declares ipc and is not denied.", Write, Outgoing, Apps, Some(P::Ipc), Available, Medium, ["ipc.send"]),
    cap!("on_ipc_message", "Receive app messages", "Called when another app or a sibling instance sends to this one. Opted into by declaring ipc and defining the hook; a Denied ipc shuts the inbox. No queue bound today.", Read, Incoming, Apps, Some(P::Ipc), Available, Low, ["on_ipc_message"]),
    cap!("ipc.request", "Ask another app", "Send and await one reply with a timeout; the target's on_ipc_request returns the answer. A pending table keyed (heap, req_id) means a dead target cannot hang the caller.", ReadWrite, Outgoing, Apps, Some(P::Ipc), PlannedNewPlumbing, Medium, []),
    cap!("on_ipc_request", "Answer another app", "Called with a request from another app; the return value is the reply.", Read, Incoming, Apps, Some(P::Ipc), PlannedNewPlumbing, Low, []),
    cap!("ipc.apps.list", "Apps that accept messages", "Ids, names and running flag of installed apps that declare ipc. Gated by ipc because it leaks the installed-app list.", Read, Outgoing, Apps, Some(P::Ipc), PlannedMachinery, Medium, []),
    // ----- matrix-room-read -----
    cap!("matrix.room.messages.read", "Recent messages", "Latest text messages, oldest first, bodies cut at 500 chars; event cache first, pagination fallback.", Read, Outgoing, Room, Some(P::MatrixRoomRead), Available, High, ["matrix.read_messages"]),
    cap!("matrix.room.members.read", "Member list", "Joined members with power level; store first, /members sync only when sparse.", Read, Outgoing, Room, Some(P::MatrixRoomRead), Available, Medium, ["matrix.room_members"]),
    cap!("matrix.room.pins.read", "Pinned messages", "The attached room's pinned messages.", Read, Outgoing, Room, Some(P::MatrixRoomRead), Available, Medium, ["matrix.pinned_events"]),
    cap!("matrix.room.threads.read", "Thread list", "Thread root messages, newest first.", Read, Outgoing, Room, Some(P::MatrixRoomRead), Available, High, ["matrix.room_threads"]),
    cap!("matrix.room.thread.read", "Thread replies", "Replies inside one thread, oldest first.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedMachinery, High, []),
    cap!("matrix.room.messages.paginate", "Older history", "One page further back than the recent window.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedMachinery, High, []),
    cap!("matrix.room.event.read", "One message", "A single event by id with sender, body, timestamp, reactions and edit state.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedMachinery, High, []),
    cap!("matrix.room.events.read", "Events by type", "Recent events of declared types (custom app types, m.sticker, ...); type-qualified declaration matched by prefix.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedNewPlumbing, High, []),
    cap!("matrix.room.relations.read", "Reactions, edits, replies", "Relations of one event: reaction counts, edit history, thread replies.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedNewPlumbing, Medium, []),
    cap!("matrix.room.state.read", "Room state", "One state event by type and key, from declared `type#key` patterns (m.room.member reveals people, so medium).", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedNewPlumbing, Medium, []),
    cap!("matrix.room.receipts.read", "Read positions", "How far a given person has read, honoring show_read_receipts.", Read, Outgoing, Room, Some(P::MatrixRoomRead), PlannedMachinery, Medium, []),
    // ----- matrix-room-send -----
    cap!("matrix.room.message.send", "Send a message", "Post text to the attached room as you; one per user action; format beyond plain is planned via the slash-command content builders.", Write, Outgoing, Room, Some(P::MatrixRoomSend), RefusedBySwitch, High, ["matrix.send_message"]),
    cap!("matrix.room.message.reply", "Reply to a message", "Post a reply to a specific event.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedMachinery, High, []),
    cap!("matrix.room.thread.reply", "Reply in a thread", "Post into a thread.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedMachinery, High, []),
    cap!("matrix.room.message.edit", "Edit its own message", "Change the text of an event THIS APP sent; the host keeps a per-instance sent-event set and refuses everything else.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedNewPlumbing, High, []),
    cap!("matrix.room.message.redact", "Delete its own message", "Redact an event THIS APP sent; never the user's other messages, never others'.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedNewPlumbing, Critical, []),
    cap!("matrix.room.attachment.send", "Send a file", "Upload a jail file or pick_binary handle into the room; file name shown on the prompt, host progress UI.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedMachinery, High, []),
    cap!("matrix.room.event.send", "Send a custom event", "A non-message event of a declared type (matrix.room.event.send:<type>); m.room.* types refused.", Write, Outgoing, Room, Some(P::MatrixRoomSend), PlannedNewPlumbing, High, []),
    // ----- clipboard-write -----
    cap!("device.clipboard.write", "Write clipboard", "Replace the clipboard with app text.", Write, Outgoing, Device, Some(P::ClipboardWrite), Available, Low, ["clipboard.write"]),
    // ----- open-url -----
    cap!("device.url.open", "Open a link", "http(s)/mailto in the system handler; scheme allowlist, 2048 chars, foreground-only. matrix.to links route to host.nav.link instead.", Act, Outgoing, Device, Some(P::OpenUrl), Available, Medium, ["url.open"]),
    // ----- files -----
    cap!("device.files.pick", "Pick a text file", "OS open dialog returning UTF-8 text up to 1MB. Modal, one at a time, foreground-only.", Read, Outgoing, Device, Some(P::Files), Available, Medium, ["files.pick"]),
    cap!("device.files.save", "Save a file", "OS save dialog with a suggested name for app text.", Write, Outgoing, Device, Some(P::Files), Available, Low, ["files.save"]),
    cap!("device.files.pick_binary", "Pick a binary file", "Open dialog returning an opaque handle the app can pass to share.file or attachment.send, so bytes never enter the script heap; base64 only on request.", Read, Outgoing, Device, Some(P::Files), PlannedNewPlumbing, Medium, []),
    // ----- share -----
    cap!("device.share", "Share text", "System share sheet with text.", Act, Outgoing, Device, Some(P::Share), Available, Low, ["share"]),
    cap!("device.share.file", "Share a file", "Share sheet with a file from the jail or a pick_binary handle; the path is resolved inside the jail host-side.", Act, Outgoing, Device, Some(P::Share), PlannedNewPlumbing, Low, []),
    // ----- auth -----
    cap!("device.auth.check", "Confirm it's you", "Biometric/password prompt with the app's reason; the app learns only pass/fail. Modal, one at a time.", Act, Outgoing, Device, Some(P::Auth), Available, Low, ["auth.check"]),
    // ----- matrix-room-info -----
    cap!("matrix.room.info.read", "Room details", "Name, topic, member count, encryption, join rule, history visibility of the attached room.", Read, Outgoing, Room, Some(P::MatrixRoomInfo), Available, Low, ["matrix.room_info"]),
    cap!("matrix.room.unread.read", "Unread count", "Unread and mention counts and the marked-unread flag.", Read, Outgoing, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("matrix.room.power_levels.read", "Room permissions", "What you may do here: invite, kick, ban, redact, notify room.", Read, Outgoing, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("matrix.room.link.create", "Permalink", "A matrix.to or matrix: link to the attached room or one of its events.", Read, Outgoing, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("matrix.room.successor.read", "Read the upgraded room", "Learn where an upgraded (tombstoned) room continued.", Read, Outgoing, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("on_room_info_changed", "Room details changed", "Renamed, new topic or avatar, encrypted, tagged, or upgraded; coalesced.", Read, Incoming, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("on_room_unread_changed", "Unread count changed", "Latest unread and mention counts, coalesced.", Read, Incoming, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    cap!("on_room_pins_changed", "Pins changed", "The new pinned event ids (contents need matrix-room-read).", Read, Incoming, Room, Some(P::MatrixRoomInfo), PlannedMachinery, Low, []),
    // ----- matrix-profile -----
    cap!("matrix.profile.read", "Your identity", "Your own user id and display name (avatar mxc planned).", Read, Outgoing, Account, Some(P::MatrixProfile), Available, Medium, ["matrix.profile"]),
    cap!("on_profile_changed", "Identity changed", "Called when your display name or avatar changes.", Read, Incoming, Account, Some(P::MatrixProfile), PlannedMachinery, Low, []),
    // ----- device-info -----
    cap!("device.info.read", "Device facts", "Platform, OS family, locale, time zone, desktop view mode, UI zoom.", Read, Outgoing, Device, Some(P::DeviceInfo), PlannedMachinery, Low, []),
    cap!("network.status.read", "Online or offline", "Whether Robrix currently has connectivity, from the sync loop's view.", Read, Outgoing, Device, Some(P::DeviceInfo), PlannedNewPlumbing, Low, []),
    cap!("on_network_changed", "Connectivity changed", "Called when Robrix goes offline or online; coalesced to one per pass.", Read, Incoming, Device, Some(P::DeviceInfo), PlannedNewPlumbing, Low, []),
    cap!("on_sync_state_changed", "Sync state changed", "connecting / syncing / offline / error, coalesced.", Read, Incoming, Account, Some(P::DeviceInfo), PlannedNewPlumbing, Low, []),
    // ----- camera -----
    cap!("device.camera.capture", "Take a photo", "OS camera returning a handle or base64; until it exists, files.pick_binary(accept:'image') is the honest route.", Read, Outgoing, Device, Some(P::Camera), PlannedNewPlumbing, High, []),
    // ----- microphone -----
    cap!("device.microphone.record", "Record audio", "OS recorder returning a clip handle for share or attachment.", Read, Outgoing, Device, Some(P::Microphone), PlannedNewPlumbing, High, []),
    // ----- matrix-account-read -----
    cap!("matrix.account.device.read", "This device", "Device id, name, and verification state.", Read, Outgoing, Account, Some(P::MatrixAccountRead), PlannedMachinery, Medium, []),
    cap!("matrix.account.info.read", "Account info", "Homeserver URL and account-management URL.", Read, Outgoing, Account, Some(P::MatrixAccountRead), PlannedMachinery, Medium, []),
    cap!("matrix.account.data.read", "Read app account data", "A global account-data event of an app-namespaced type (rs.robius.a2app.<app_id>.*); m.* types are refused.", Read, Outgoing, Account, Some(P::MatrixAccountRead), PlannedNewPlumbing, Medium, []),
    cap!("matrix.account.ignored.read", "Read your ignore list", "See which users you have ignored.", Read, Outgoing, Account, Some(P::MatrixAccountRead), PlannedMachinery, Medium, []),
    cap!("on_account_data_changed", "App account data changed", "Called when a subscribed app-namespaced type changes on any device.", Read, Incoming, Account, Some(P::MatrixAccountRead), PlannedNewPlumbing, Medium, []),
    // ----- matrix-account-write -----
    cap!("matrix.account.display_name.set", "Change your name", "Set your global display name; the new name is shown on the prompt.", Write, Outgoing, Account, Some(P::MatrixAccountWrite), PlannedMachinery, Critical, []),
    cap!("matrix.account.avatar.set", "Change your avatar", "Upload and set, or clear, your global avatar.", Write, Outgoing, Account, Some(P::MatrixAccountWrite), PlannedMachinery, Critical, []),
    cap!("matrix.user.ignore.set", "Block or unblock someone", "Add or remove a user from the account-wide ignore list.", Write, Outgoing, User, Some(P::MatrixAccountWrite), PlannedMachinery, Critical, []),
    cap!("matrix.account.data.write", "Store app account data", "Write an app-namespaced account-data type so app state follows the user across devices.", Write, Outgoing, Account, Some(P::MatrixAccountWrite), PlannedNewPlumbing, High, []),
    // ----- matrix-users -----
    cap!("matrix.user.profile.read", "Look up a user", "Display name, avatar and ignored flag for any user id through your homeserver.", Read, Outgoing, User, Some(P::MatrixUsers), PlannedMachinery, Medium, []),
    cap!("matrix.user.dm.find", "Find an existing DM", "The DM room with a user if one exists; never creates one.", Read, Outgoing, User, Some(P::MatrixUsers), PlannedMachinery, Medium, []),
    cap!("matrix.user.search", "Find people", "Search the homeserver's user directory.", Read, Outgoing, User, Some(P::MatrixUsers), PlannedNewPlumbing, Medium, []),
    cap!("matrix.user.presence.read", "Presence", "Online / idle / offline for a user, plus the on_user_presence hook.", Read, Outgoing, User, Some(P::MatrixUsers), PlannedNewPlumbing, Medium, []),
    // ----- matrix-room-watch -----
    cap!("on_room_message", "New messages", "New text/notice/emote (media as {msgtype, body} without bytes), batched per event pass, bounded FIFO with drop count.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, High, []),
    cap!("on_room_message_changed", "Message edited or removed", "An existing message was edited or redacted.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, High, []),
    cap!("on_room_reaction", "Reactions", "A reaction added or removed on any event.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, Medium, []),
    cap!("on_room_event", "Events of a type", "Each new event of a declared type, including custom app types; declared as on_room_event:<type>.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedNewPlumbing, High, []),
    cap!("on_room_state", "State changed", "A state event of a declared type changed; declared as on_room_state:<type>.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedNewPlumbing, Medium, []),
    cap!("on_room_typing", "Who is typing", "The current set of typing users, latest-only per pass. Subscription started only while an instance asked, refcounted with RoomScreen's.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, Medium, []),
    cap!("on_room_receipt", "Read receipts moved", "Someone's read position moved (own and others).", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, Medium, []),
    cap!("on_room_members_changed", "Members changed", "Join, leave, invite, kick, ban in the attached room; bounded queue.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedMachinery, Medium, []),
    cap!("on_room_mention", "You were mentioned", "An event the push rules say should notify you, in the attached room.", Read, Incoming, Room, Some(P::MatrixRoomWatch), PlannedNewPlumbing, High, []),
    // ----- matrix-room-interact -----
    cap!("matrix.room.reaction.toggle", "React", "Add or remove your reaction on an event.", ReadWrite, Outgoing, Room, Some(P::MatrixRoomInteract), PlannedMachinery, Medium, []),
    cap!("matrix.room.typing.send", "Typing indicator", "Show you as typing; auto-expires; limiter-priced.", Write, Outgoing, Room, Some(P::MatrixRoomInteract), PlannedMachinery, Medium, []),
    cap!("matrix.room.receipt.send", "Mark as read", "Send a read receipt up to an event, or mark the room fully read; honors read_receipts_privacy.", Write, Outgoing, Room, Some(P::MatrixRoomInteract), PlannedMachinery, Medium, []),
    // ----- matrix-room-app-data -----
    cap!("matrix.room.app_event.send", "Save app data to the room", "Post this app's own state as an rs.robius.a2app.data event with app_id stamped by the host; other members' copies see it.", Write, Outgoing, Room, Some(P::MatrixRoomAppData), PlannedNewPlumbing, Medium, []),
    cap!("matrix.room.app_event.read", "Load app data from the room", "Latest N of this app's own data events, for state replay on boot.", Read, Outgoing, Room, Some(P::MatrixRoomAppData), PlannedNewPlumbing, Low, []),
    cap!("on_room_app_event", "App data arrived", "Another copy of this app saved state into the room.", Read, Incoming, Room, Some(P::MatrixRoomAppData), PlannedNewPlumbing, Low, []),
    // ----- matrix-room-manage -----
    cap!("matrix.room.pin.set", "Pin or unpin", "Pin or unpin an event; the SDK enforces power level.", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedMachinery, Medium, []),
    cap!("matrix.room.favorite.set", "Favorite", "Add or remove this room from your favorites.", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedMachinery, Low, []),
    cap!("matrix.room.low_priority.set", "Low priority", "Mark this room low priority or restore it.", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedMachinery, Low, []),
    cap!("matrix.room.unread.set", "Flag unread", "Flag this room unread, or clear the flag, without sending a receipt.", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedMachinery, Low, []),
    cap!("matrix.room.state.send", "Change room settings", "A state event of a declared type (name, topic, avatar, or an app-owned state type); power levels and membership types refused.", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedNewPlumbing, Critical, []),
    cap!("matrix.room.account_data.read", "Read room notes", "App-owned per-room account data.", Read, Outgoing, Room, Some(P::MatrixRoomManage), PlannedNewPlumbing, Medium, []),
    cap!("matrix.room.account_data.write", "Store room notes", "Write app-owned per-room account data (never m.* types).", Write, Outgoing, Room, Some(P::MatrixRoomManage), PlannedNewPlumbing, Medium, []),
    // ----- matrix-room-invite -----
    cap!("matrix.room.invite.send", "Invite someone", "Invite a user to the attached room as you; the user id is shown on every prompt.", Write, Outgoing, Room, Some(P::MatrixRoomInvite), PlannedMachinery, High, []),
    // ----- matrix-media -----
    cap!("matrix.media.download", "Fetch an attachment", "Bytes of an attachment or thumbnail from the attached room, size-capped, base64 or handle.", Read, Outgoing, Room, Some(P::MatrixMedia), PlannedMachinery, High, []),
    cap!("matrix.media.save", "Download to disk", "Save an attachment from the attached room to Downloads with host progress UI.", Act, Outgoing, Room, Some(P::MatrixMedia), PlannedMachinery, Medium, []),
    cap!("matrix.media.share", "Share an attachment", "Hand a room attachment to the system share sheet.", Act, Outgoing, Room, Some(P::MatrixMedia), PlannedMachinery, Medium, []),
    cap!("matrix.media.avatar.read", "Avatars", "A member's or the room's avatar thumbnail.", Read, Outgoing, Room, Some(P::MatrixMedia), PlannedMachinery, Low, []),
    cap!("matrix.media.url_preview.read", "Link preview", "Title, description and image of a URL via the homeserver (the homeserver sees the URL).", Read, Outgoing, Account, Some(P::MatrixMedia), PlannedMachinery, Medium, []),
    cap!("matrix.media.upload", "Upload media", "Upload bytes to the media repo and get an mxc uri without sending an event.", Write, Outgoing, Account, Some(P::MatrixMedia), PlannedMachinery, Medium, []),
    cap!("on_upload_progress", "Upload progress hook", "Be told how an upload it started is progressing, and cancel it.", Read, Incoming, Room, Some(P::MatrixMedia), PlannedMachinery, Low, []),
    // ----- matrix-rooms-list -----
    cap!("matrix.rooms.list", "Your rooms", "Joined rooms and DMs with name, unread, mentions, tags, is_direct, is_space; no message previews; optionally limited to a space.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, High, []),
    cap!("matrix.rooms.search", "Find rooms", "Your rooms and spaces whose name or alias match a query.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, Medium, []),
    cap!("matrix.rooms.invites.list", "Pending invites", "Rooms you are invited to, with inviter.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, Medium, []),
    cap!("matrix.rooms.preview.read", "Preview a room", "Public details of any room by id or alias, joined or not.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, Low, []),
    cap!("on_rooms_changed", "Room list changed", "Rooms joined, left, renamed, reordered, or unread counts changed; coalesced diff per pass.", Read, Incoming, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, High, []),
    cap!("on_invite_received", "Invite received", "A new room invite arrived; joining stays a user action unless matrix-membership is granted.", Read, Incoming, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, Medium, []),
    cap!("on_unread_totals_changed", "Unread totals changed", "Account-wide unread and mention totals without per-room detail.", Read, Incoming, MultiRoom, Some(P::MatrixRoomsList), PlannedMachinery, Low, []),
    // ----- matrix-rooms-read -----
    cap!("matrix.rooms.info.read", "Details of another room", "Room details for a room in this app's allowlist.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsRead), PlannedMachinery, Medium, []),
    cap!("matrix.rooms.messages.read", "Messages in another room", "Recent messages in a room in this app's allowlist.", Read, Outgoing, MultiRoom, Some(P::MatrixRoomsRead), PlannedMachinery, Critical, []),
    cap!("on_rooms_message", "New messages in picked rooms", "New messages in allowlisted rooms; only rooms whose timeline Robrix has built deliver, so a firehose over every room is not offered.", Read, Incoming, MultiRoom, Some(P::MatrixRoomsRead), PlannedNewPlumbing, Critical, []),
    // ----- matrix-rooms-send -----
    cap!("matrix.rooms.message.send", "Send to another room", "Post text to an allowlisted room as you; room name and text shown on every prompt.", Write, Outgoing, MultiRoom, Some(P::MatrixRoomsSend), PlannedMachinery, Critical, []),
    // ----- matrix-membership -----
    cap!("matrix.rooms.join", "Join or knock", "Join a room by id or alias, or knock if invite-only; room name shown on every prompt.", Write, Outgoing, MultiRoom, Some(P::MatrixMembership), PlannedMachinery, High, []),
    cap!("matrix.invites.respond", "Answer an invite", "Accept or decline a pending invite.", Write, Outgoing, MultiRoom, Some(P::MatrixMembership), PlannedMachinery, High, []),
    cap!("matrix.user.dm.open", "Start a direct message", "Open, or create if needed, a DM with someone and navigate there; creation is what makes this membership.", ReadWrite, Outgoing, User, Some(P::MatrixMembership), PlannedMachinery, High, []),
    cap!("matrix.room.leave", "Leave this room", "Leave the ATTACHED room as you; Robrix's own leave confirmation is raised on every call, and success kills the instance. Leaving any other room stays never.", Write, Outgoing, Room, Some(P::MatrixMembership), PlannedMachinery, Critical, []),
    cap!("matrix.rooms.create", "Create a room", "Create a room with a name and invitees.", Write, Outgoing, MultiRoom, Some(P::MatrixMembership), PlannedNewPlumbing, High, []),
    // ----- matrix-spaces -----
    cap!("matrix.spaces.list", "Your spaces", "The spaces you have joined.", Read, Outgoing, Space, Some(P::MatrixSpaces), PlannedMachinery, Medium, []),
    cap!("matrix.space.info.read", "Space details", "Name, topic, member and room counts, join rule, world-readable.", Read, Outgoing, Space, Some(P::MatrixSpaces), PlannedMachinery, Medium, []),
    cap!("matrix.space.rooms.list", "Rooms in a space", "Child rooms and subspaces with joined flag.", Read, Outgoing, Space, Some(P::MatrixSpaces), PlannedMachinery, Medium, []),
    cap!("on_space_changed", "Space changed", "A subscribed space's children or your space list changed.", Read, Incoming, Space, Some(P::MatrixSpaces), PlannedMachinery, Low, []),
    // ----- robrix-navigation -----
    cap!("host.nav.room", "Open a room", "Switch Robrix to a joined room; unknown or unjoined refused.", Act, Outgoing, MultiRoom, Some(P::RobrixNavigation), PlannedMachinery, Medium, []),
    cap!("host.nav.event", "Jump to a message", "Scroll the attached room to an event and highlight it.", Act, Outgoing, Room, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    cap!("host.nav.thread", "Open a thread", "Open a thread of the attached room.", Act, Outgoing, Room, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    cap!("host.nav.user", "Show a profile", "Open the user profile pane for a user.", Act, Outgoing, User, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    cap!("host.nav.space", "Open a space", "Go to a space's lobby.", Act, Outgoing, Space, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    cap!("host.nav.screen", "Open a screen", "Home, add/join room, Mini Apps, Settings.", Act, Outgoing, Client, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    cap!("host.nav.link", "Open a Matrix link in-app", "Resolve a matrix.to / matrix: URI to a room, event or user inside Robrix.", Act, Outgoing, MultiRoom, Some(P::RobrixNavigation), PlannedNewPlumbing, Medium, []),
    cap!("host.nav.app", "Open another mini-app", "Open an installed app in this room's dock; target must be installed and unrestricted.", Act, Outgoing, Apps, Some(P::RobrixNavigation), PlannedMachinery, Low, []),
    // ----- robrix-composer -----
    cap!("host.composer.insert", "Draft a message for you", "Put text into the attached room's message box, optionally as a reply; the user still presses send.", Act, Outgoing, Room, Some(P::RobrixComposer), PlannedNewPlumbing, Medium, []),
    cap!("host.composer.reply_to", "Set reply target", "Put the composer into reply or thread-reply mode for an event.", Act, Outgoing, Room, Some(P::RobrixComposer), PlannedMachinery, Low, []),
    // ----- robrix-ui -----
    cap!("ui.pane.request_size", "Preferred size", "Hint the size along the pane's resizable axis; the dock clamps and the user's drag always wins; rate-limited harder since it reflows the timeline.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedNewPlumbing, Low, []),
    cap!("ui.pane.set_side", "Choose dock side", "Move to top/bottom/left/right, same as the CycleEdge button.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedMachinery, Low, []),
    cap!("ui.pane.minimize", "Minimize to chip", "Collapse into the chips row; an app may shrink itself but never un-minimize itself.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedMachinery, Low, []),
    cap!("ui.pane.break_out", "Break out to a tab", "Move to its own desktop dock tab; refused in mobile view.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedMachinery, Low, []),
    cap!("ui.pane.set_title", "Pane title", "Override the tab/pane title line; always prefixed with the real app name so it cannot spoof another app.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedNewPlumbing, Low, []),
    cap!("ui.pane.set_badge", "Badge its chip or tab", "A count or dot on the minimized chip and tab, the quiet alternative to a popup.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedNewPlumbing, Low, []),
    cap!("ui.focus.request", "Keyboard focus", "Ask for key focus inside the pane; refused unless foreground, so it cannot steal focus from the composer.", Act, Outgoing, Instance, Some(P::RobrixUi), PlannedNewPlumbing, Medium, []),
    // ----- robrix-preferences -----
    cap!("host.prefs.read", "Display settings", "View mode, UI zoom, send-on-enter, thumbnail size, show_read_receipts; never the privacy prefs and never a setter.", Read, Outgoing, Client, Some(P::RobrixPreferences), PlannedMachinery, Low, []),
    cap!("host.theme.read", "Theme tokens", "Scheme and colour tokens so an app can match Robrix instead of hardcoding.", Read, Outgoing, Client, Some(P::RobrixPreferences), PlannedNewPlumbing, Low, []),
    cap!("on_prefs_changed", "Display settings changed", "One of those settings changed.", Read, Incoming, Client, Some(P::RobrixPreferences), PlannedMachinery, Low, []),
    cap!("on_theme_changed", "Theme changed", "Scheme or zoom changed (ScriptReapply).", Read, Incoming, Client, Some(P::RobrixPreferences), PlannedNewPlumbing, Low, []),
    // ----- robrix-observe -----
    cap!("on_active_room_changed", "You switched rooms", "Which room, thread, or nothing Robrix now shows; latest-only per pass. Reveals browsing, so it prompts.", Read, Incoming, MultiRoom, Some(P::RobrixObserve), PlannedMachinery, High, []),
    cap!("on_navigation_changed", "You switched screens", "Home, a space, settings, mini apps; latest-only per pass.", Read, Incoming, Client, Some(P::RobrixObserve), PlannedMachinery, Medium, []),
    cap!("on_launch", "Launch context hook", "Be told how the instance was opened: slash command, timeline card, list, or tab.", Read, Incoming, Instance, None, PlannedMachinery, Low, []),
    // ----- never -----
    cap!("apps.manage", "Manage installed apps", "Never: install, uninstall, archive, restore, export or import apps stays a user-only surface.", Write, Outgoing, Apps, None, Never, Critical, []),
    cap!("matrix.space.leave", "Leave a space", "Never: leaving a space and its children is irreversible and user-only.", Write, Outgoing, Space, None, Never, Critical, []),
    cap!("matrix.session.manage", "Log out, verify, manage the account", "Never: no app logs you out, starts device verification, opens the account portal, or changes the session.", Write, Outgoing, Account, None, Never, Critical, []),
    cap!("matrix.rooms.leave", "Leave any room by id", "Never in v1: leaving is destructive and the app cannot see what it would cost you. Only the attached room, host-confirmed, is offered.", Write, Outgoing, MultiRoom, None, Never, Critical, []),
    cap!("matrix.room.app.share", "Share itself into a room", "Never app-initiated: an app posting its own bundle into rooms is a worm vector. Stays a user action (/miniapp share, App Info), still behind the write switch.", Write, Outgoing, MultiRoom, None, Never, Critical, []),
    cap!("matrix.room.message.redact_others", "Delete other people's messages", "Never: moderation stays with the user even when their power level allows it; also covers the user's own non-app messages.", Write, Outgoing, Room, None, Never, Critical, []),
    cap!("matrix.room.power_levels.set", "Change power levels or membership state", "Never: m.room.power_levels, m.room.member, kick and ban are refused by matrix.room.state.send.", Write, Outgoing, Room, None, Never, Critical, []),
    cap!("host.composer.get", "Read the unsent draft", "Never: what the user is typing is theirs until they send it.", Read, Outgoing, Room, None, Never, High, []),
    cap!("on_composer_send", "Rewrite outgoing messages", "Never: a synchronous hook over the user's own speech is a content-injection vector; host.composer.insert is the visible substitute.", ReadWrite, Incoming, Room, None, Never, Critical, []),
    cap!("host.prefs.set", "Change Robrix settings", "Never: no app changes view mode, zoom, read-receipt privacy, or any preference.", Write, Outgoing, Client, None, Never, High, []),
    cap!("host.cache.clear", "Clear caches or run diagnostics", "Never: maintenance actions that can stall Robrix stay out of reach.", Act, Outgoing, Client, None, Never, High, []),
    cap!("host.ui.screenshot", "Screenshot the host", "Never: it would leak every room and pane on screen; Splash screencapture also breaks render commits (makepad 6cf59e1).", Read, Outgoing, Client, None, Never, Critical, []),
    cap!("matrix.to_device.send", "To-device messaging", "Never until there is an E2EE policy for app-visible to-device traffic (covers the on_to_device hook too).", Write, Outgoing, Account, None, Never, Critical, []),
    cap!("matrix.account.openid.get", "Identity token", "Never until a per-call consent sheet names the recipient host; an OpenID token proves your identity to a third party.", Read, Outgoing, Account, None, Never, Critical, []),
];

/// A capability by its id.
pub fn by_id(id: &str) -> Option<&'static Capability> {
    CATALOG.iter().find(|c| c.id == id)
}

/// The capability a broker service carries, if it is one.
pub fn for_service(service: &str) -> Option<&'static Capability> {
    CATALOG.iter().find(|c| c.direction == Direction::Outgoing && c.wire.contains(&service))
}

/// The capability an incoming hook carries, if it is one.
pub fn for_hook(hook: &str) -> Option<&'static Capability> {
    CATALOG.iter().find(|c| c.direction == Direction::Incoming && c.wire.contains(&hook))
}

/// The capabilities gated by one permission group, in catalog order.
pub fn in_group(perm: Permission) -> impl Iterator<Item = &'static Capability> {
    CATALOG.iter().filter(move |c| c.group == Some(perm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_wires_are_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len(), "duplicate capability id");
        let mut wires: Vec<&str> = CATALOG.iter().flat_map(|c| c.wire.iter().copied()).collect();
        wires.sort_unstable();
        wires.dedup();
        let total: usize = CATALOG.iter().map(|c| c.wire.len()).sum();
        assert_eq!(wires.len(), total, "a wire id carried by two capabilities");
    }

    #[test]
    fn every_group_has_capabilities() {
        for perm in Permission::ALL {
            assert!(in_group(perm).next().is_some(), "{} has no capabilities", perm.as_str());
        }
    }

    #[test]
    fn services_resolve() {
        assert_eq!(for_service("matrix.room_members").unwrap().id, "matrix.room.members.read");
        assert_eq!(for_hook("on_ipc_message").unwrap().group, Some(Permission::Ipc));
        assert!(for_service("nope").is_none());
    }
}
