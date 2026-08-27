//! The host-service broker: the host half of the `host.request(...)` bridge.
//! Drains queued requests from every Splash isolate, applies the permission
//! policy, does the platform work through the robius crates, and answers back
//! into the requesting isolate.
//!
//! Split of responsibilities with the host app: the broker decides and
//! executes everything it can locally; anything that must touch host state or
//! widgets (showing a prompt, delivering IPC into other isolates, popup
//! notifications, the matrix services) is returned as a [`BrokerAsk`] for the
//! host's event-handling code to perform. Robius callbacks land on other
//! threads, so they come home through an mpsc channel drained on the next
//! event pass (`SignalToUI` wakes one promptly).

pub mod limits;

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use makepad_widgets::splash_host::{
    splash_host_respond, take_splash_host_requests, SplashHostRequest,
};
use makepad_widgets::*;

use crate::manifest::{AppRegistry, MiniAppId};
use crate::permissions::{Effective, Permission, PermissionStore};

/// The IP-geolocation fallback endpoint (city-level, no key needed).
const GEO_URL: &str = "https://ipapi.co/json/";

/// Where a service answer must go: one isolate, one request.
#[derive(Clone, Copy, Debug)]
pub struct Reply {
    pub heap_key: usize,
    pub req_id: u64,
}

impl Reply {
    fn of(req: &SplashHostRequest) -> Reply {
        Reply { heap_key: req.heap_key, req_id: req.req_id }
    }
}

/// Whether a dispatch spends from the app's request budget. Fresh requests
/// do; a request replayed after the user answered a permission prompt does
/// not, since it was already paid for when the app first asked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charge {
    Yes,
    No,
}

/// Appends one line to the service trace (`ROBRIX_A2APP_TRACE_SERVICES=1`).
/// A file, not stdout: this has to work inside a test harness that swallows
/// the app's console output.
fn trace_line(line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/robrix_a2app_services.log")
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn trace_on() -> bool {
    std::env::var("ROBRIX_A2APP_TRACE_SERVICES").is_ok()
}

/// Answers one request. `Ok` carries the `data` JSON, `Err` the user-visible
/// error string.
pub fn respond(cx: &mut Cx, reply: Reply, result: Result<&str, &str>) {
    let outcome = splash_host_respond(cx, reply.heap_key, reply.req_id, result);
    if trace_on() {
        let mut preview = match result {
            Ok(data) => data.to_string(),
            Err(e) => format!("ERR {e}"),
        };
        preview.truncate(200);
        trace_line(&format!(
            "respond heap={} req={} ok={} outcome={:?} data={}\n",
            reply.heap_key,
            reply.req_id,
            result.is_ok(),
            outcome,
            preview
        ));
    }
}

/// Work only the host can do, returned from [`Broker::process`].
pub enum BrokerAsk {
    /// Queue a runtime-permission prompt. `request`, when present, is parked
    /// until the user answers (re-dispatch on allow, deny-respond on deny).
    Prompt {
        app_id: MiniAppId,
        perm: Permission,
        request: Option<SplashHostRequest>,
    },
    /// Deliver an (already policy-checked) IPC message to `to`'s running
    /// isolates, then answer `reply` with the delivered count. `from_heap`
    /// identifies the SENDING isolate so a `to: "self"` broadcast doesn't
    /// echo the message back to its own sender.
    IpcDeliver {
        reply: Reply,
        from: MiniAppId,
        from_heap: usize,
        to: MiniAppId,
        data_json: String,
    },
    /// Show a popup notification for this app. The request is already
    /// answered; `summary` is the popup text, clamped broker-side.
    Notify { app_id: MiniAppId, summary: String },
    /// An app actually USED a capability: record it for the access log and
    /// light the in-use indicator. Only real, policy-passed uses reach here.
    Used { app_id: MiniAppId, perm: Permission },
    /// This app has abused the bridge past the point of being refused
    /// politely: stop it, mark it restricted, and tell the user. The broker
    /// has already answered the offending request with an error.
    Restrict { app_id: MiniAppId, reason: String },
    /// Run a validated matrix.* call against the SDK, then answer `reply`
    /// with [`respond`] on the UI thread when the result comes back.
    Matrix {
        reply: Reply,
        app_id: MiniAppId,
        /// The room this instance is bound to (from its host tag).
        room: Option<String>,
        call: MatrixServiceCall,
    },
}

/// A matrix.* service call, parsed and validated by the broker.
pub enum MatrixServiceCall {
    /// `{room_id, room_name, topic, member_count}` for the attached room.
    RoomInfo,
    /// The latest `limit` text messages, oldest first (limit already 1..=30).
    ReadMessages { limit: u32 },
    /// Send a plain text message to the attached room as the user.
    SendMessage { body: String },
    /// `{user_id, display_name}` — the user's own identity.
    Profile,
    /// `{count, members: [{name, user_id, power}]}` for the attached room.
    Members { limit: u32 },
    /// `{pinned: [{sender, body}]}` — the room's pinned messages.
    PinnedEvents,
    /// `{threads: [{sender, body}]}` — thread roots, newest first.
    Threads { limit: u32 },
}

/// Async results coming home from robius callbacks on other threads.
enum Completion {
    Respond(Reply, Result<String, String>),
    /// CoreLocation produced a fix: answer every waiting location request.
    LocationFix { lat: f64, lon: f64 },
    /// CoreLocation failed (denied / unavailable): fall back to IP geolocation.
    LocationFailed,
}

/// The host state the broker reads while dispatching, borrowed fresh each
/// event pass.
#[derive(Clone, Copy)]
pub struct BrokerCtx<'a> {
    pub registry: &'a AppRegistry,
    pub permissions: &'a PermissionStore,
    /// The app whose host pane is currently shown, gating the UI services
    /// and the one-at-a-time modal guard.
    pub foreground_app: Option<&'a str>,
}

pub struct Broker {
    tx: Sender<Completion>,
    rx: Receiver<Completion>,
    /// Kept alive so CoreLocation's delegate keeps reporting; created lazily
    /// on first use (must be on the main thread, which `process` is).
    location_manager: Option<robius_location::Manager>,
    /// Location requests waiting on CoreLocation or the IP fallback.
    pending_locations: Vec<Reply>,
    /// In-flight host-side IP-geolocation fetches, by request LiveId.
    pending_geo: HashMap<LiveId, Reply>,
    /// Per-app request budgets and strikes (`limits`): what keeps a hostile
    /// app from turning the bridge into a denial-of-service on the host.
    limits: limits::AbuseLimiter,
    /// Which app owns each on-screen OS dialog, so the one-at-a-time guard
    /// can be released when the completion comes home from another thread.
    dialog_owner: HashMap<(usize, u64), MiniAppId>,
}

impl Default for Broker {
    fn default() -> Self {
        Self::new()
    }
}

impl Broker {
    pub fn new() -> Broker {
        let (tx, rx) = channel();
        Broker {
            tx,
            rx,
            location_manager: None,
            pending_locations: Vec::new(),
            pending_geo: HashMap::new(),
            limits: limits::AbuseLimiter::default(),
            dialog_owner: HashMap::new(),
        }
    }

    /// Drops an app's rate-limit budget, strikes and dialog guard. Called when
    /// its isolates are torn down (force stop, uninstall, or the user allowing
    /// a restricted app to run again) — a fresh run starts with a clean sheet,
    /// while the persisted restriction is what remembers the abuse.
    pub fn forget_app(&mut self, app_id: &str) {
        self.limits.forget(app_id);
        self.dialog_owner.retain(|_, owner| owner != app_id);
    }

    /// How many of this app's requests have been refused this run, for the
    /// app's info page.
    pub fn refusal_count(&self, app_id: &str) -> u64 {
        self.limits.refusals(app_id)
    }

    /// Marks an app's OS dialog as on screen, turning on the one-at-a-time
    /// guard — for hosts that raise a modal on an app's behalf.
    pub fn dialog_started(&mut self, app_id: &str) {
        self.limits.dialog_started(app_id);
    }

    /// Marks that dialog as finished (answered, cancelled or failed).
    pub fn dialog_finished(&mut self, app_id: &str) {
        self.limits.dialog_finished(app_id);
    }

    /// One full pass: everything isolates asked since the last event, plus
    /// every async completion that came home. Call once per `handle_event`.
    pub fn process(&mut self, cx: &mut Cx, ctx: BrokerCtx) -> Vec<BrokerAsk> {
        let mut asks = Vec::new();
        // Apps condemned during THIS drain. Once the host has decided to
        // stop an app, the rest of its batch is dropped rather than answered:
        // every answer is a synchronous re-entry into an isolate that is about
        // to be torn down in this same event pass, and a script that answers
        // by touching its UI leaves paused threads and queued widget calls
        // behind it. Dropping is the bridge's documented behaviour for an
        // undrained request — it simply never resolves — and the callbacks are
        // reaped with the isolate moments later.
        let mut condemned: Vec<MiniAppId> = Vec::new();
        for req in take_splash_host_requests() {
            let (app_part, _) = crate::manifest::split_instance_tag(&req.app_tag);
            if condemned.iter().any(|c| c == app_part) {
                continue;
            }
            let before = asks.len();
            self.dispatch(cx, ctx, req, &mut asks, Charge::Yes);
            for ask in &asks[before..] {
                if let BrokerAsk::Restrict { app_id, .. } = ask {
                    condemned.push(app_id.clone());
                }
            }
        }
        self.drain_completions(cx);
        asks
    }

    /// Re-runs a parked request after the user granted its permission. Not
    /// charged again: the app asked once, and the delay since was the user
    /// reading a prompt.
    pub fn dispatch_after_grant(&mut self, cx: &mut Cx, ctx: BrokerCtx, req: SplashHostRequest) -> Vec<BrokerAsk> {
        let mut asks = Vec::new();
        self.dispatch(cx, ctx, req, &mut asks, Charge::No);
        asks
    }

    /// Answers a parked request after the user denied its permission.
    /// `permissions.request` gets its documented `{granted: false}` shape (a
    /// denial IS its answer); everything else gets the error.
    pub fn respond_denied(cx: &mut Cx, req: &SplashHostRequest) {
        if req.service == "permissions.request" {
            return respond(cx, Reply::of(req), Ok("{\"granted\": false}"));
        }
        let msg = match service_permission(&req.service) {
            Some(perm) => format!("permission denied: {}", perm.as_str()),
            None => "permission denied".to_string(),
        };
        respond(cx, Reply::of(req), Err(&msg));
    }

    fn dispatch(
        &mut self,
        cx: &mut Cx,
        ctx: BrokerCtx,
        req: SplashHostRequest,
        asks: &mut Vec<BrokerAsk>,
        charge: Charge,
    ) {
        if trace_on() {
            trace_line(&format!(
                "dispatch app={} svc={} heap={} req={} prompt={} grants={:?} args={}\n",
                req.app_tag,
                req.service,
                req.heap_key,
                req.req_id,
                req.may_prompt,
                crate::permissions::snapshot_grants_for(
                    crate::manifest::split_instance_tag(&req.app_tag).0
                ),
                req.args_json
            ));
        }
        let reply = Reply::of(&req);
        // Untagged isolates (previews, validation dry-runs) get a clean
        // refusal; the tag is host-assigned, so this cannot be spoofed away.
        if req.app_tag.is_empty() {
            return respond(cx, reply, Err("host services are not available here"));
        }
        // The tag names one INSTANCE: the app plus the room it is bound to.
        let (app_part, room_part) = crate::manifest::split_instance_tag(&req.app_tag);
        let instance_room = room_part.map(str::to_string);
        let Some(manifest) = ctx.registry.get(app_part).cloned() else {
            return respond(cx, reply, Err("unknown app"));
        };
        // A restricted app has already been stopped, so anything still queued
        // from it is a leftover from the isolate that was torn down. Drop it
        // without answering: there is nothing alive to answer, and re-entering
        // a dead isolate's heap is how you corrupt someone else's.
        if ctx.permissions.is_restricted(&manifest.id) {
            if trace_on() {
                trace_line(&format!(
                    "drop app={} svc={} reason=restricted\n",
                    manifest.id, req.service
                ));
            }
            return;
        }
        // Abuse control comes BEFORE the permission check, because refusing a
        // request is itself work and an app in a tight loop must not be able
        // to make the host do it forever.
        if charge == Charge::Yes {
            let foreground = ctx.foreground_app == Some(manifest.id.as_str());
            let verdict = self.limits.check(&manifest.id, &req.service, foreground);
            if trace_on() {
                if let limits::Verdict::Refuse(why) | limits::Verdict::Stop(why) = &verdict {
                    trace_line(&format!(
                        "limit app={} svc={} refused={:?} stop={} total_refusals={}\n",
                        manifest.id,
                        req.service,
                        why,
                        matches!(verdict, limits::Verdict::Stop(_)),
                        self.limits.refusals(&manifest.id),
                    ));
                }
            }
            match verdict {
                limits::Verdict::Allow => {}
                limits::Verdict::Refuse(why) => return respond(cx, reply, Err(why.message())),
                limits::Verdict::Stop(why) => {
                    // Answer the offending call, then hand the app to the host
                    // to be stopped: the broker cannot tear down widgets.
                    respond(cx, reply, Err(why.message()));
                    asks.push(BrokerAsk::Restrict {
                        app_id: manifest.id.clone(),
                        reason: "made far too many requests to the system".to_string(),
                    });
                    return;
                }
            }
        }
        let args: serde_json::Value =
            serde_json::from_str(&req.args_json).unwrap_or(serde_json::Value::Null);

        // Same-app IPC is inside one sandbox: no permission involved.
        let ipc_target = (req.service == "ipc.send").then(|| {
            let to = args["to"].as_str().unwrap_or_default().to_string();
            if to.is_empty() || to == "self" { manifest.id.clone() } else { to }
        });
        let needs = match req.service.as_str() {
            "env" | "permissions.query" | "permissions.request" => None,
            "ipc.send" if ipc_target.as_deref() == Some(manifest.id.as_str()) => None,
            other => match service_permission(other) {
                Some(perm) => Some(perm),
                None => return respond(cx, reply, Err(&format!("unknown service '{other}'"))),
            },
        };
        if let Some(perm) = needs {
            match ctx.permissions.effective(&manifest, perm) {
                // A capability actually being exercised — the only place a
                // "used" record can honestly come from.
                Effective::Granted => {
                    asks.push(BrokerAsk::Used { app_id: manifest.id.clone(), perm });
                }
                Effective::Denied => return Self::respond_denied(cx, &req),
                Effective::Undeclared => {
                    return respond(
                        cx,
                        reply,
                        Err(&format!("permission not declared: {}", perm.as_str())),
                    );
                }
                Effective::NeedsPrompt => {
                    // Surfaces that may not prompt never pop consent dialogs:
                    // their Ask-state requests fail cleanly and the script
                    // falls back.
                    if !req.may_prompt {
                        return Self::respond_denied(cx, &req);
                    }
                    asks.push(BrokerAsk::Prompt {
                        app_id: manifest.id.clone(),
                        perm,
                        request: Some(req),
                    });
                    return;
                }
            }
        }

        match req.service.as_str() {
            "env" => {
                let data = serde_json::json!({
                    "app_id": manifest.id,
                    "room_attached": instance_room.is_some(),
                });
                respond(cx, reply, Ok(&data.to_string()));
            }
            "permissions.query" => {
                let mut map = serde_json::Map::new();
                for (perm, _) in ctx.permissions.declared_states(&manifest) {
                    let s = match ctx.permissions.effective(&manifest, perm) {
                        Effective::Granted => "granted",
                        Effective::Denied => "denied",
                        _ => "ask",
                    };
                    map.insert(perm.as_str().to_string(), s.into());
                }
                respond(cx, reply, Ok(&serde_json::Value::Object(map).to_string()));
            }
            "permissions.request" => {
                let Some(perm) = args["perm"].as_str().and_then(Permission::from_str) else {
                    return respond(cx, reply, Err("unknown permission"));
                };
                match ctx.permissions.effective(&manifest, perm) {
                    Effective::Granted => respond(cx, reply, Ok("{\"granted\": true}")),
                    Effective::Denied => respond(cx, reply, Ok("{\"granted\": false}")),
                    Effective::Undeclared => {
                        respond(cx, reply, Err(&format!("permission not declared: {}", perm.as_str())));
                    }
                    Effective::NeedsPrompt if !req.may_prompt => {
                        respond(cx, reply, Ok("{\"granted\": false}"));
                    }
                    Effective::NeedsPrompt => {
                        asks.push(BrokerAsk::Prompt {
                            app_id: manifest.id.clone(),
                            perm,
                            request: Some(req),
                        });
                    }
                }
            }
            "location.get" => self.location_get(cx, reply),
            "clipboard.read" => {
                // Off-thread: pbpaste is usually instant but it IS a child
                // process, and a wedged pasteboard must not stall the UI.
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let out = read_clipboard()
                        .map(|text| serde_json::json!({ "text": text }).to_string());
                    tx.send(Completion::Respond(reply, out)).ok();
                    SignalToUI::set_ui_signal();
                });
            }
            "clipboard.write" => {
                let Some(text) = args["text"].as_str() else {
                    return respond(cx, reply, Err("clipboard.write needs {text}"));
                };
                cx.copy_to_clipboard(text);
                respond(cx, reply, Ok("{}"));
            }
            "url.open" => {
                let Some(url) = args["url"].as_str() else {
                    return respond(cx, reply, Err("url.open needs {url}"));
                };
                // Scheme allowlist: `file:` and friends reach places a
                // sandboxed app must not send the user.
                let ok_scheme = ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|s| url.starts_with(s));
                if !ok_scheme || url.len() > 2048 {
                    return respond(cx, reply, Err("only http(s) and mailto links can be opened"));
                }
                match robius_open::Uri::new(url).open() {
                    Ok(()) => respond(cx, reply, Ok("{}")),
                    Err(e) => respond(cx, reply, Err(&format!("couldn't open link: {e:?}"))),
                }
            }
            "share" => {
                let Some(text) = args["text"].as_str() else {
                    return respond(cx, reply, Err("share needs {text}"));
                };
                match robius_share::ShareSheet::new().add_text(text).share() {
                    Ok(()) => respond(cx, reply, Ok("{}")),
                    Err(e) => respond(cx, reply, Err(&format!("couldn't share: {e:?}"))),
                }
            }
            "notify.post" => {
                // Popup text is the app's own words rendered in host chrome,
                // so it is flattened and clamped like a permission reason.
                let title = args["title"].as_str().unwrap_or("").trim();
                let body = args["body"].as_str().unwrap_or("").trim();
                let raw = match (title.is_empty(), body.is_empty()) {
                    (false, false) => format!("{title}: {body}"),
                    (false, true) => title.to_string(),
                    (true, false) => body.to_string(),
                    (true, true) => match args["count"].as_u64() {
                        Some(n) => format!("{} notifications", n.min(999)),
                        None => "New notification".to_string(),
                    },
                };
                let summary: String =
                    raw.replace(['\n', '\r'], " ").chars().take(200).collect();
                asks.push(BrokerAsk::Notify { app_id: manifest.id.clone(), summary });
                respond(cx, reply, Ok("{}"));
            }
            "notify.clear" => {
                // A shown popup can't be recalled; answered ok so scripts
                // written against the badge-count host keep working.
                respond(cx, reply, Ok("{}"));
            }
            "files.pick" => {
                let tx = self.tx.clone();
                let result = robius_file_picker::FileDialog::new().pick_file(move |res| {
                    tx.send(Completion::Respond(reply, picked_to_json(res))).ok();
                    SignalToUI::set_ui_signal();
                });
                match result {
                    Ok(()) => self.dialog_launched(&manifest.id, reply),
                    Err(e) => {
                        respond(cx, reply, Err(&format!("couldn't open the file picker: {e:?}")))
                    }
                }
            }
            "files.save" => {
                let Some(name) = args["name"].as_str().filter(|n| !n.is_empty()) else {
                    return respond(cx, reply, Err("files.save needs {name, data}"));
                };
                let Some(data) = args["data"].as_str() else {
                    return respond(cx, reply, Err("files.save needs {name, data}"));
                };
                let tx = self.tx.clone();
                let bytes = data.as_bytes().to_vec();
                let result = robius_file_picker::FileDialog::new()
                    .set_file_name(name)
                    .save_data(bytes, move |res| {
                        let out = match res {
                            Ok(Some(_)) => Ok("{\"saved\": true}".to_string()),
                            Ok(None) => Ok("{\"saved\": false}".to_string()),
                            Err(e) => Err(format!("save failed: {e:?}")),
                        };
                        tx.send(Completion::Respond(reply, out)).ok();
                        SignalToUI::set_ui_signal();
                    });
                match result {
                    Ok(()) => self.dialog_launched(&manifest.id, reply),
                    Err(e) => {
                        respond(cx, reply, Err(&format!("couldn't open the save dialog: {e:?}")))
                    }
                }
            }
            "auth.check" => {
                let reason = args["reason"].as_str().unwrap_or("Confirm it's you").to_string();
                if self.auth_check(cx, reply, &manifest.name, &reason) {
                    self.dialog_launched(&manifest.id, reply);
                }
            }
            "ipc.send" => {
                let to = ipc_target.unwrap_or_default();
                let data_json = args["data"].to_string();
                if to != manifest.id {
                    // Cross-app consent is asymmetric on purpose: SENDING is
                    // the privileged act (prompted, sender-side). RECEIVING is
                    // opted into by declaring ipc + defining the hook, and the
                    // user can still shut a receiver off by denying its ipc.
                    let Some(target) = ctx.registry.get(&to) else {
                        return respond(cx, reply, Err("no such app"));
                    };
                    let blocked = !target.declares(Permission::Ipc)
                        || ctx.permissions.state(&to, Permission::Ipc)
                            == crate::permissions::GrantState::Denied;
                    if blocked {
                        return respond(cx, reply, Err("that app doesn't accept messages"));
                    }
                }
                asks.push(BrokerAsk::IpcDeliver {
                    reply,
                    from: manifest.id.clone(),
                    from_heap: req.heap_key,
                    to,
                    data_json,
                });
            }
            "matrix.room_info" | "matrix.read_messages" | "matrix.send_message"
            | "matrix.profile" | "matrix.room_members" | "matrix.pinned_events"
            | "matrix.room_threads" => {
                // Room-scoped calls need an attached room; checked AFTER the
                // permission gate so a first-use prompt still reads sensibly.
                if req.service != "matrix.profile" && instance_room.is_none() {
                    return respond(cx, reply, Err("this mini-app is not attached to a room"));
                }
                let call = match req.service.as_str() {
                    "matrix.room_info" => MatrixServiceCall::RoomInfo,
                    "matrix.profile" => MatrixServiceCall::Profile,
                    "matrix.read_messages" => MatrixServiceCall::ReadMessages {
                        limit: args["limit"].as_u64().unwrap_or(10).clamp(1, 30) as u32,
                    },
                    "matrix.room_members" => MatrixServiceCall::Members {
                        limit: args["limit"].as_u64().unwrap_or(50).clamp(1, 200) as u32,
                    },
                    "matrix.pinned_events" => MatrixServiceCall::PinnedEvents,
                    "matrix.room_threads" => MatrixServiceCall::Threads {
                        limit: args["limit"].as_u64().unwrap_or(20).clamp(1, 50) as u32,
                    },
                    _ => {
                        let body = args["body"].as_str().map(str::trim).unwrap_or_default();
                        if body.is_empty() {
                            return respond(cx, reply, Err("matrix.send_message needs {body}"));
                        }
                        if body.chars().count() > 4096 {
                            return respond(
                                cx,
                                reply,
                                Err("message is too long (4096 characters max)"),
                            );
                        }
                        MatrixServiceCall::SendMessage { body: body.to_string() }
                    }
                };
                asks.push(BrokerAsk::Matrix {
                    reply,
                    app_id: manifest.id.clone(),
                    room: instance_room.clone(),
                    call,
                });
            }
            _ => unreachable!("service list checked above"),
        }
    }

    /// CoreLocation first; on any failure every waiting request falls back to
    /// IP geolocation (city-level) via the host's own HTTP stack.
    fn location_get(&mut self, cx: &mut Cx, reply: Reply) {
        let already_waiting = !self.pending_locations.is_empty();
        self.pending_locations.push(reply);
        if already_waiting {
            return;
        }
        if self.location_manager.is_none() {
            self.location_manager = robius_location::Manager::new(LocationHandler {
                tx: Mutex::new(self.tx.clone()),
            })
            .ok();
        }
        let ok = self
            .location_manager
            .as_ref()
            .is_some_and(|m| m.update_once().is_ok());
        if !ok {
            // No manager (or a sync failure): go straight to the fallback.
            self.start_ip_geolocation(cx);
        }
    }

    fn start_ip_geolocation(&mut self, cx: &mut Cx) {
        for reply in std::mem::take(&mut self.pending_locations) {
            let id = LiveId::unique();
            self.pending_geo.insert(id, reply);
            cx.http_request(id, HttpRequest::new(GEO_URL.to_string(), HttpMethod::GET));
        }
    }

    /// Records that an OS dialog is now on screen for `app_id`, so the app
    /// cannot stack a second one behind it. Released in `drain_completions`
    /// when the user answers, cancels, or the dialog fails.
    fn dialog_launched(&mut self, app_id: &MiniAppId, reply: Reply) {
        self.limits.dialog_started(app_id);
        self.dialog_owner.insert((reply.heap_key, reply.req_id), app_id.clone());
    }

    /// Returns whether a prompt actually went up (so the caller knows whether
    /// to start the one-at-a-time guard); errors are answered here.
    fn auth_check(&mut self, cx: &mut Cx, reply: Reply, app_name: &str, reason: &str) -> bool {
        let Some(policy) = robius_authentication::PolicyBuilder::new().build() else {
            respond(cx, reply, Err("authentication is not available here"));
            return false;
        };
        let text = robius_authentication::Text {
            android: robius_authentication::AndroidText {
                title: app_name,
                subtitle: None,
                description: Some(reason),
            },
            apple: reason,
            // Truncating constructor: the reason is script-supplied, and the
            // plain new() panics past Windows' length caps.
            windows: robius_authentication::WindowsText::new_truncated(app_name, reason),
        };
        let tx = self.tx.clone();
        let context = robius_authentication::Context::new(());
        let result = context.authenticate(text, &policy, move |res| {
            let out = match res {
                Ok(()) => Ok("{}".to_string()),
                Err(e) => Err(format!("not authenticated: {e:?}")),
            };
            tx.send(Completion::Respond(reply, out)).ok();
            SignalToUI::set_ui_signal();
        });
        if let Err(e) = result {
            respond(cx, reply, Err(&format!("authentication unavailable: {e:?}")));
            return false;
        }
        true
    }

    fn drain_completions(&mut self, cx: &mut Cx) {
        while let Ok(done) = self.rx.try_recv() {
            // Any answer to a modal service means its dialog is off screen,
            // whether the user accepted, cancelled, or it failed outright.
            if let Completion::Respond(reply, _) = &done {
                if let Some(owner) = self.dialog_owner.remove(&(reply.heap_key, reply.req_id)) {
                    self.limits.dialog_finished(&owner);
                }
            }
            match done {
                Completion::Respond(reply, Ok(json)) => respond(cx, reply, Ok(&json)),
                Completion::Respond(reply, Err(e)) => respond(cx, reply, Err(&e)),
                Completion::LocationFix { lat, lon } => {
                    let data = serde_json::json!({
                        "lat": lat, "lon": lon, "city": "", "source": "gps",
                    })
                    .to_string();
                    for reply in std::mem::take(&mut self.pending_locations) {
                        respond(cx, reply, Ok(&data));
                    }
                }
                Completion::LocationFailed => self.start_ip_geolocation(cx),
            }
        }
    }

    /// Host-side network completions (the IP-geolocation fallback). Call from
    /// `Event::NetworkResponses`; unrelated request ids are left alone.
    pub fn handle_network(&mut self, cx: &mut Cx, responses: &NetworkResponsesEvent) {
        for response in responses {
            let request_id = match response {
                NetworkResponse::HttpResponse { request_id, .. }
                | NetworkResponse::HttpError { request_id, .. } => *request_id,
                _ => continue,
            };
            let Some(reply) = self.pending_geo.remove(&request_id) else {
                continue;
            };
            match response {
                NetworkResponse::HttpResponse { response, .. } => {
                    let parsed = response
                        .get_string_body()
                        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
                        .and_then(|v| geo_json_to_location(&v));
                    match parsed {
                        Some(data) => respond(cx, reply, Ok(&data)),
                        None => respond(cx, reply, Err("couldn't determine your location")),
                    }
                }
                _ => respond(cx, reply, Err("couldn't determine your location")),
            }
        }
    }
}

/// The permission each service needs. `None` for the free ones and for
/// anything unknown; a same-app `ipc.send` skips the check in dispatch.
fn service_permission(service: &str) -> Option<Permission> {
    match service {
        "location.get" => Some(Permission::Location),
        "clipboard.read" => Some(Permission::ClipboardRead),
        "clipboard.write" => Some(Permission::ClipboardWrite),
        "url.open" => Some(Permission::OpenUrl),
        "share" => Some(Permission::Share),
        "notify.post" | "notify.clear" => Some(Permission::Notifications),
        "files.pick" | "files.save" => Some(Permission::Files),
        "auth.check" => Some(Permission::Auth),
        "ipc.send" => Some(Permission::Ipc),
        "matrix.room_info" => Some(Permission::MatrixRoomInfo),
        "matrix.read_messages" | "matrix.room_members" | "matrix.pinned_events"
        | "matrix.room_threads" => Some(Permission::MatrixRoomRead),
        "matrix.send_message" => Some(Permission::MatrixRoomSend),
        "matrix.profile" => Some(Permission::MatrixProfile),
        _ => None,
    }
}

struct LocationHandler {
    tx: Mutex<Sender<Completion>>,
}

impl robius_location::Handler for LocationHandler {
    fn handle(&self, location: robius_location::Location<'_>) {
        let tx = self.tx.lock().unwrap();
        match location.coordinates() {
            Ok(coords) => {
                tx.send(Completion::LocationFix { lat: coords.latitude, lon: coords.longitude })
                    .ok();
            }
            // A fix without coordinates still has to answer the waiters, or
            // every later location.get piles behind them forever.
            Err(_) => {
                tx.send(Completion::LocationFailed).ok();
            }
        }
        SignalToUI::set_ui_signal();
    }

    fn error(&self, _error: robius_location::Error) {
        let tx = self.tx.lock().unwrap();
        tx.send(Completion::LocationFailed).ok();
        SignalToUI::set_ui_signal();
    }
}

/// Accepts every common IP-geolocation response shape (ipapi.co and
/// ip-api.com) and normalizes to the service's `{lat, lon, city}`.
fn geo_json_to_location(v: &serde_json::Value) -> Option<String> {
    let lat = v["lat"].as_f64().or_else(|| v["latitude"].as_f64())?;
    let lon = v["lon"].as_f64().or_else(|| v["longitude"].as_f64())?;
    let city = v["city"].as_str().unwrap_or("");
    Some(serde_json::json!({ "lat": lat, "lon": lon, "city": city, "source": "ip" }).to_string())
}

fn picked_to_json(
    res: Result<Option<robius_file_picker::PickedFile>, robius_file_picker::Error>,
) -> Result<String, String> {
    /// Read cap: results ride through a script heap as one string.
    const MAX_PICK_BYTES: u64 = 1024 * 1024;
    match res {
        Ok(None) => Ok("{\"cancelled\": true}".to_string()),
        Ok(Some(file)) => {
            // size() can be None on desktop; fall back to fs metadata so a
            // huge file is rejected BEFORE read_bytes buffers all of it.
            let size = file.size().or_else(|| {
                file.path()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len())
            });
            if size.is_some_and(|n| n > MAX_PICK_BYTES) {
                return Err("file is too large (1MB max)".to_string());
            }
            let bytes = file.read_bytes().map_err(|e| format!("couldn't read the file: {e:?}"))?;
            if bytes.len() as u64 > MAX_PICK_BYTES {
                return Err("file is too large (1MB max)".to_string());
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| "only text files are supported for now".to_string())?;
            let name = file.display_name().unwrap_or("file").to_string();
            Ok(serde_json::json!({
                "name": name,
                "size": text.len(),
                "text": text,
            })
            .to_string())
        }
        Err(e) => Err(format!("file picker failed: {e:?}")),
    }
}

/// Clipboard read has no makepad API; on macOS the system `pbpaste` is the
/// host-side (never script-side) door. 64KB cap keeps a giant clipboard from
/// flooding a script heap; bytes are capped BEFORE the lossy conversion so a
/// multibyte boundary can never panic a String::truncate.
fn read_clipboard() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/pbpaste")
            .output()
            .map_err(|e| format!("clipboard unavailable: {e}"))?;
        let mut bytes = out.stdout;
        bytes.truncate(64 * 1024);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("clipboard read is not available on this platform".to_string())
    }
}
