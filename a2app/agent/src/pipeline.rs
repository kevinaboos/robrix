//! The create-app generation pipeline: prompt → streamed reply → extract the
//! fenced Splash source → validate it against the real parser → repair loop →
//! a ready-to-install `MiniAppManifest`.
//!
//! Modeled on octos-one's card pipeline (single fenced block as the ENTIRE
//! reply, client-side extraction, lint → one-shot repair), but the validator
//! here is the actual Splash parser/evaluator running in a throwaway isolate
//! with a captured-error sink — not a regex lint.

use makepad_widgets::*;
use serde_json::Value;

use a2app_core::header::parse_app_header;
use a2app_core::manifest::{A2AppScope, MiniAppId, MiniAppManifest};

use crate::acp_client::{AcpEvent, PlanStep};
use crate::prefs::AgentPrefs;
use crate::AgentTransport;

/// How many repair turns a generation may take after the first attempt.
const MAX_REPAIRS: u32 = 2;

/// Symbol glyphs the app fonts are known to lack (they render as
/// tofu boxes). Generated apps must use emoji / words / × instead; seen in
/// the wild when models reach for ✕-style icons.
const TOFU_GLYPHS: &[char] = &['✕', '✗', '✘', '⤡', '⤢', '➜', '↻', '⟳'];

/// A generation with NO agent events for this long is declared stalled. LLM
/// turns legitimately take a while, so this is generous — it only catches an
/// agent that is alive but silent (hung provider, dead network) which would
/// otherwise leave the UI busy until the user hits Stop.
const STALL_SECS: u64 = 240;

/// The two kinds of output an agent streams, which the retained transcript
/// keeps apart with a heading — they arrive interleaved and read as gibberish
/// run together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Thinking,
    Writing,
}

impl StreamKind {
    fn heading(self) -> &'static str {
        match self {
            Self::Thinking => "💭 thinking",
            Self::Writing => "✍️ writing",
        }
    }
}

/// Where the generation pipeline currently is; drives the create UI's status.
#[derive(Debug, Clone, PartialEq)]
pub enum GenPhase {
    /// Agent spawned, handshake in flight.
    Connecting,
    /// The prompt (or a repair prompt) is out; the reply is streaming in.
    /// `attempt` is 0 for the first try, 1.. for repairs.
    Generating { attempt: u32 },
    /// Nothing in flight (after success/failure the pipeline is dropped).
    Done,
}

/// The outcome the app layer acts on after feeding an event batch through.
pub enum GenOutcome {
    /// Still working; the status line may have changed.
    Working,
    /// Success. `refine_of` is set when this UPDATES an existing app (the
    /// manifest keeps that app's id) rather than installing a new one.
    Ready {
        manifest: Box<MiniAppManifest>,
        refine_of: Option<MiniAppId>,
    },
    /// Failure, with a short human-readable reason.
    Failed(String),
}

/// Whether this generation creates a new app or modifies an existing one.
enum GenMode {
    Create,
    /// Refine `base`: the prompt carries its current source, and the result
    /// keeps its id (name/icon/tint may change via the header).
    Refine { base: Box<MiniAppManifest> },
}

/// One in-flight "create app" (or "refine app") request.
pub struct Generation {
    client: Box<dyn AgentTransport>,
    /// The user's request, verbatim.
    request: String,
    mode: GenMode,
    /// The scope the built manifest gets (a refine keeps its base's).
    scope: A2AppScope,
    phase: GenPhase,
    /// Repair turns used so far.
    repairs: u32,
    /// Live status line for the UI.
    status: String,
    /// Ids already taken (so a fresh id can be made unique against the registry).
    taken_ids: Vec<MiniAppId>,
    /// When the last agent event arrived, for the stall watchdog.
    last_event: std::time::Instant,
    /// Whether prompts may omit the inline guide (persistent-guide active AND this
    /// generation's backend is one that carries the persistent copy — a
    /// foreign agent via ROBRIX_AGENT_CMD may ignore AGENTS.md, so the
    /// env override always inlines).
    slim_prompts: bool,
    /// Human-readable activity trail for the drop-down panel: phase changes,
    /// tool calls, validation errors. Capped; newest last.
    activity: Vec<String>,
    /// The current turn's streamed reply text (the code being written),
    /// accumulated for the panel's live tail. Cleared per turn.
    stream: String,
    /// The current turn's streamed *thinking*, same idea. Kept apart from
    /// `stream` so it can never reach the fence extractor, and so the panel can
    /// show it only while there's no real output yet.
    thought: String,
    /// EVERYTHING the agent has emitted this run, in arrival order — thinking
    /// and code alike, across every repair turn.
    ///
    /// Separate from `stream`/`thought` because those are working buffers: the
    /// fence extractor consumes `stream`, so it has to be cleared at each turn
    /// boundary. This never is. The console is the only record of what the
    /// agent did, and it used to show a rolling 700-byte window of the current
    /// turn, so everything before that — the reasoning, the code that failed to
    /// compile, the earlier attempts — was gone before anyone could read it.
    transcript: String,
    /// What the transcript last carried, so switching between thinking and code
    /// gets a heading instead of the two running together mid-word.
    transcript_kind: Option<StreamKind>,
    /// The plan the agent last published, so an update can be diffed against it
    /// and only the CHANGES logged (it republishes the whole list every time).
    plan: Vec<PlanStep>,
}

impl Generation {
    /// Starts the agent backend and kicks off a generation for `request`.
    /// `taken_ids` is the current set of installed app ids; `scope` is what
    /// the built manifest gets (account-wide, or attached to one room).
    pub fn start(
        request: String,
        taken_ids: Vec<MiniAppId>,
        scope: A2AppScope,
        prefs: AgentPrefs,
    ) -> Result<Self, String> {
        Self::start_with_mode(request, taken_ids, scope, GenMode::Create, prefs)
    }

    /// Starts a refine of an existing app: same machinery, but the prompt
    /// includes the app's current source and the result keeps its id.
    pub fn start_refine(
        request: String,
        base: MiniAppManifest,
        prefs: AgentPrefs,
    ) -> Result<Self, String> {
        let scope = base.scope.clone();
        let mode = GenMode::Refine { base: Box::new(base) };
        Self::start_with_mode(request, Vec::new(), scope, mode, prefs)
    }

    fn start_with_mode(
        request: String,
        taken_ids: Vec<MiniAppId>,
        scope: A2AppScope,
        mode: GenMode,
        prefs: AgentPrefs,
    ) -> Result<Self, String> {
        let workspace = agent_workspace_dir();
        // Persist the dialect guide on the agent side so prompts can go slim.
        #[cfg(feature = "persistent-guide")]
        crate::skills::deploy_guide(&workspace);
        let client = crate::start_backend(&workspace, &prefs)?;
        // Slim prompts only for backends known to carry the persistent guide:
        // the default octos spawn / the in-process agent. An explicit
        // ROBRIX_AGENT_CMD may be any ACP agent, which likely ignores
        // our AGENTS.md — inline the guide for those.
        #[cfg(feature = "persistent-guide")]
        let slim_prompts = std::env::var("ROBRIX_AGENT_CMD").is_err()
            && crate::skills::guide_is_deployed();
        #[cfg(not(feature = "persistent-guide"))]
        let slim_prompts = false;
        Ok(Self {
            client,
            request,
            mode,
            scope,
            phase: GenPhase::Connecting,
            repairs: 0,
            status: "Contacting agent…".to_string(),
            taken_ids,
            last_event: std::time::Instant::now(),
            thought: String::new(),
            plan: Vec::new(),
            slim_prompts,
            activity: vec!["Starting agent…".to_string()],
            stream: String::new(),
            transcript: String::new(),
            transcript_kind: None,
        })
    }

    /// The user's request, verbatim — recorded as the version-history note
    /// when a modification lands.
    pub fn request(&self) -> &str {
        &self.request
    }

    /// The app this run modifies, or `None` when it creates a new one. The
    /// UI's Retry needs it to re-run the same kind of generation.
    pub fn refine_target(&self) -> Option<&MiniAppId> {
        match &self.mode {
            GenMode::Create => None,
            GenMode::Refine { base } => Some(&base.id),
        }
    }

    /// The run's full activity trail, oldest first — what the console shows.
    pub fn activity(&self) -> &[String] {
        &self.activity
    }

    /// Everything the agent has produced this run, oldest first — what the
    /// console shows below the trail, and scrolls.
    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    /// What the create UI shows while a generation is live.
    ///
    /// No run clock: a ticking `m:ss` next to the status invited the user to
    /// watch a number instead of the work, and said nothing about progress —
    /// the spinner already carries "still going", and the status text itself
    /// changes as the run moves through its phases.
    pub fn status_line(&self) -> String {
        self.status.clone()
    }

    /// Appends to the run's trail. Nothing is ever dropped: the console shows
    /// the whole history and scrolls, and a pipeline lives only as long as the
    /// generation it's driving, so the trail is bounded by the run itself.
    fn log(&mut self, line: impl Into<String>) {
        self.activity.push(line.into());
    }

    /// Appends agent output to the retained transcript, heading it whenever the
    /// kind changes so a stretch of reasoning doesn't run straight into code.
    fn transcribe(&mut self, kind: StreamKind, text: &str) {
        if self.transcript_kind != Some(kind) {
            if !self.transcript.is_empty() {
                self.transcript.push_str("\n\n");
            }
            self.transcript.push_str(kind.heading());
            self.transcript.push('\n');
            self.transcript_kind = Some(kind);
        }
        self.transcript.push_str(text);
    }

    /// Marks a turn boundary in the transcript. Each repair re-asks the agent
    /// from scratch, and without this the new attempt's code runs straight on
    /// from the failed one's with nothing to say which is which.
    fn transcribe_turn(&mut self, heading: &str) {
        if !self.transcript.is_empty() {
            self.transcript.push_str("\n\n");
        }
        self.transcript.push_str(heading);
        // Force a heading on the next chunk whatever kind it is.
        self.transcript_kind = None;
    }

    /// The current status line for the create UI.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Asks the agent to abandon the in-flight turn. The caller drops the
    /// pipeline right after; this just lets the agent stop burning tokens.
    pub fn cancel(&mut self) {
        self.client.cancel();
    }

    /// Whether the agent has gone silent past the stall budget. Checked from a
    /// periodic timer, since a silent agent by definition produces no events
    /// that would otherwise wake the pipeline.
    pub fn is_stalled(&self) -> bool {
        self.phase != GenPhase::Done && self.last_event.elapsed().as_secs() > STALL_SECS
    }

    /// Feeds queued agent events through the state machine. Call on every UI
    /// event (cheap when idle); `cx` is needed to run the validator.
    pub fn advance(&mut self, cx: &mut Cx) -> GenOutcome {
        let events = self.client.drain_events();
        if !events.is_empty() {
            self.last_event = std::time::Instant::now();
        }
        for event in events {
            match event {
                AcpEvent::SessionReady => {
                    let prompt = match &self.mode {
                        GenMode::Create => build_initial_prompt(&self.request, self.slim_prompts),
                        GenMode::Refine { base } => {
                            build_refine_prompt(&self.request, base, self.slim_prompts)
                        }
                    };
                    self.client.send_prompt(&prompt);
                    self.phase = GenPhase::Generating { attempt: 0 };
                    self.status = "Generating your app…".to_string();
                    self.log("Agent connected — writing the app");
                    self.stream.clear();
                    self.thought.clear();
                    self.transcript_kind = None;
                }
                AcpEvent::Chunk(text) => {
                    self.stream.push_str(&text);
                    self.transcribe(StreamKind::Writing, &text);
                    // Streaming progress. Length is a decent proxy for life.
                    if let GenPhase::Generating { attempt } = self.phase {
                        self.status = if attempt == 0 {
                            "Writing the app…".to_string()
                        } else {
                            format!("Fixing the app (try {attempt})…")
                        };
                    }
                }
                AcpEvent::ToolCall(title) => {
                    self.status = format!("Agent: {title}…");
                    self.log(format!("🔧 {title}"));
                }
                AcpEvent::Thought(text) => {
                    // The trail gets ONE line per thinking stretch, not one per
                    // chunk — the chunks are token-sized. The text itself goes
                    // to the live tail, where it's replaced by the code as soon
                    // as the agent starts writing.
                    if self.thought.is_empty() {
                        self.log("💭 Thinking…");
                    }
                    self.thought.push_str(&text);
                    self.transcribe(StreamKind::Thinking, &text);
                    if self.stream.is_empty() {
                        self.status = match self.phase {
                            GenPhase::Generating { attempt } if attempt > 0 => {
                                format!("Thinking about the fix (try {attempt})…")
                            }
                            _ => "Thinking…".to_string(),
                        };
                    }
                }
                AcpEvent::Plan(steps) => {
                    // The agent republishes the whole plan on every change, so
                    // log the diff: new steps once, and each step again when it
                    // starts or finishes. Otherwise a five-step plan floods the
                    // console with five copies of itself per update.
                    for step in &steps {
                        let was = self.plan.iter().find(|p| p.content == step.content);
                        if was.map(|p| &p.status) == Some(&step.status) {
                            continue;
                        }
                        match step.status.as_str() {
                            "completed" => self.log(format!("✓ {}", step.content)),
                            "in_progress" => {
                                self.status = format!("{}…", step.content);
                                self.log(format!("→ {}", step.content));
                            }
                            _ if was.is_none() => self.log(format!("📋 {}", step.content)),
                            _ => {}
                        }
                    }
                    self.plan = steps;
                }
                // No content, but it moved the stall clock (see `advance`).
                AcpEvent::Tick => {}
                AcpEvent::TurnDone { stop_reason, text } => {
                    if stop_reason == "cancelled" {
                        return GenOutcome::Failed("Cancelled".to_string());
                    }
                    if stop_reason == "refusal" {
                        return GenOutcome::Failed("The agent declined that request".to_string());
                    }
                    match self.finish_turn(cx, &text) {
                        TurnVerdict::Installed(manifest) => {
                            self.phase = GenPhase::Done;
                            let refine_of = match &self.mode {
                                GenMode::Create => None,
                                GenMode::Refine { base } => Some(base.id.clone()),
                            };
                            return GenOutcome::Ready { manifest, refine_of };
                        }
                        TurnVerdict::NeedsRepair(errors) => {
                            self.repairs += 1;
                            if self.repairs > MAX_REPAIRS {
                                return GenOutcome::Failed(
                                    "The generated app kept failing to compile".to_string(),
                                );
                            }
                            for e in errors.iter().take(3) {
                                self.log(format!("⚠ {e}"));
                            }
                            let attempt = self.repairs;
                            self.log(format!("Sending errors back (repair {attempt})"));
                            self.client.send_prompt(&build_repair_prompt(&errors));
                            self.phase = GenPhase::Generating { attempt };
                            self.status = format!("Fixing the app (try {attempt})…");
                            self.stream.clear();
                            self.thought.clear();
                            self.transcribe_turn(&format!(
                                "──────── repair {attempt}: {} error(s) sent back ────────",
                                errors.len()
                            ));
                        }
                        TurnVerdict::Malformed(reason) => {
                            // No fenced code at all — one retry with a nudge,
                            // sharing the repair budget so this can't loop.
                            self.repairs += 1;
                            if self.repairs > MAX_REPAIRS {
                                return GenOutcome::Failed(reason);
                            }
                            let attempt = self.repairs;
                            self.log("Reply had no code block — asking again");
                            self.client.send_prompt(&build_nudge_prompt());
                            self.phase = GenPhase::Generating { attempt };
                            self.status = "Asking for the code again…".to_string();
                            self.stream.clear();
                            self.thought.clear();
                            self.transcribe_turn(
                                "──────── retry: reply had no code block ────────",
                            );
                        }
                    }
                }
                AcpEvent::Error(msg) => {
                    // A failed *spawn* arrives here, not as ProcessGone — the
                    // process never existed to go away — so it needs the same
                    // "install octos" translation rather than a raw errno.
                    return GenOutcome::Failed(process_gone_reason(&msg, self.client.desc()));
                }
                AcpEvent::ProcessGone(msg) => {
                    return GenOutcome::Failed(process_gone_reason(&msg, self.client.desc()));
                }
            }
        }
        GenOutcome::Working
    }

    /// Extract + validate a finished turn. On success, returns the manifest.
    fn finish_turn(&mut self, cx: &mut Cx, reply: &str) -> TurnVerdict {
        let Some(block) = extract_splash_block(reply) else {
            return TurnVerdict::Malformed(
                "The agent's reply had no ```splash code block".to_string(),
            );
        };
        // The header comments stay in the source, so `block` IS the source.
        let header = parse_app_header(&block);
        let source = block;

        if let Some(banned) = forbidden_construct(&source) {
            return TurnVerdict::NeedsRepair(vec![format!(
                "the script uses `{banned}`, which is not available in this dialect \
                 — remove it and use only constructs from the guide"
            )]);
        }
        // Runtime-invisible but user-visible: glyphs the app fonts don't have
        // render as tofu boxes. Caught here (the parser can't) so the repair
        // turn swaps them out.
        if let Some(c) = source.chars().find(|c| TOFU_GLYPHS.contains(c)) {
            return TurnVerdict::NeedsRepair(vec![format!(
                "the app font has no glyph for '{c}' (renders as an empty box) — \
                 use an emoji, a plain word, or × (U+00D7) instead"
            )]);
        }

        self.status = "Checking the app…".to_string();
        self.log("Validating with the Splash parser");
        let errors = validate_splash(cx, &source);
        if !errors.is_empty() {
            return TurnVerdict::NeedsRepair(errors);
        }
        self.log("Compiles clean — installing");

        // A refine keeps the app's identity; the header may restyle it. A
        // create mints everything fresh.
        let manifest = match &self.mode {
            GenMode::Create => {
                let name = header.name.unwrap_or_else(|| default_name(&self.request));
                let id = unique_id(&name, &self.taken_ids);
                MiniAppManifest {
                    id,
                    name,
                    icon: header.icon.unwrap_or_else(|| "✨".to_string()),
                    tint: header.tint.unwrap_or(0x7c6cf0),
                    source,
                    allow_net: false,
                    // Generated apps start fully sandboxed; the user (or a
                    // later refine) adds declarations deliberately.
                    // What the agent asked for, sanitized. Declaring is not
                    // granting: runtime tiers prompt on first use and App Info
                    // lists every one of them.
                    permissions: header.permissions.clone(),
                    permission_reasons: header.permission_reasons.clone(),
                    builtin: false,
                    widget: None,
                    shortcuts: Vec::new(),
                    scope: self.scope.clone(),
                }
            }
            GenMode::Refine { base } => MiniAppManifest {
                id: base.id.clone(),
                name: header.name.unwrap_or_else(|| base.name.clone()),
                icon: header.icon.unwrap_or_else(|| base.icon.clone()),
                tint: header.tint.unwrap_or(base.tint),
                source,
                allow_net: base.allow_net,
                // Declarations survive a refine like the id does: the user
                // granted against them, and a rewrite mustn't silently widen
                // or narrow what the app may ask for.
                // A refine may ADD a capability the rewrite needs (the user
                // asked for the change, and it lands in App Info like any
                // other); it never silently drops one they already granted
                // against. Removing is App Info's "Remove capability".
                permissions: union_permissions(&base.permissions, &header.permissions),
                permission_reasons: {
                    let mut r = base.permission_reasons.clone();
                    r.extend(header.permission_reasons.clone());
                    r
                },
                // Keep the flag: a modified BUILT-IN stays built-in (its
                // override just shadows the stock app). Dropping it here would
                // make it uninstallable-then-resurrectable — and would strip
                // the protection the menu relies on.
                builtin: base.builtin,
                // Robrix never runs widget scripts, so nothing to carry over.
                widget: None,
                shortcuts: Vec::new(),
                scope: base.scope.clone(),
            },
        };
        TurnVerdict::Installed(Box::new(manifest))
    }
}

enum TurnVerdict {
    Installed(Box<MiniAppManifest>),
    NeedsRepair(Vec<String>),
    Malformed(String),
}

/// The harmless directory the agent's tools are rooted at (octos insists on a
/// cwd for its sandbox), under the a2app data root.
pub fn agent_workspace_dir() -> std::path::PathBuf {
    a2app_core::data_root().join("agent_workspace")
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

use crate::SPLASH_GUIDE;

/// With `research`, the agent may research before answering (web search /
/// fetch — octos's own tools), baking what it finds into the app as
/// constants. Without it, generation is a single pure text turn.
#[cfg(feature = "research")]
const TOOL_POLICY: &str = "You MAY use your tools first (e.g. web search/fetch) to look up real \
     data the app should carry — exchange rates, schedules, trivia — and bake \
     the results into the script as plain constants (the app itself has no \
     network access). Your FINAL message must still be exactly the one fenced \
     block, nothing else.";
#[cfg(not(feature = "research"))]
const TOOL_POLICY: &str = "Do not use tools; reply directly.";

/// The reply contract shared by create, refine, and repair turns.
fn reply_contract() -> String {
    format!(
        "## Reply format (MANDATORY)\n\
         \n\
         Reply with EXACTLY ONE fenced code block and nothing else — no prose \
         before or after, no extra fences. {TOOL_POLICY}\n\
         \n\
         ```splash\n\
         // name: <Short App Name, max 18 chars>\n\
         // icon: <one emoji>\n\
         // tint: <hex color like #4A90D9 that suits the app>\n\
         <the complete script>\n\
         ```"
    )
}

/// The dialect guide, unless this generation's backend carries a persistent
/// copy (`persistent-guide` deploys one — see `crate::skills`), in which case a
/// one-line pointer replaces the ~6KB text.
fn guide_section(slim: bool) -> String {
    if slim {
        return "Follow the Splash dialect guide in your workspace's AGENTS.md \
                (also in your system prompt) EXACTLY — only constructs shown \
                there exist."
            .to_string();
    }
    SPLASH_GUIDE.to_string()
}

/// Stated in the task itself, not only the guide: the host shows apps
/// fullscreen, in split-screen panes, and in freely resized windows, and an
/// app that only works phone-shaped is a broken deliverable.
const RESPONSIVE_POLICY: &str = "The app MUST lay out well and stay fully usable at ANY host size — \
     fullscreen phone, a narrow (~190pt) or short (~250pt) split-screen pane, \
     or a wide desktop window. Cap+center the content column with \
     `width: Fill{max: N}` under an `align: Align{x: 0.5}` parent, and when \
     fixed sizes or fonts must differ by size, define `fn on_app_resize(w, h)` \
     and toggle pre-declared tier Views (see the guide's \"runs at ANY size\" \
     section).";

/// Stated in EVERY prompt, slim or not: an agent that never sees the full
/// guide still has to get consent right, and "works only if granted" is the
/// most likely way a generated app is broken on arrival.
const PERMISSION_POLICY: &str = "The app is SANDBOXED. Anything beyond its own UI and \
     its private `fs` jail (network, location, clipboard, notifications, links, files, \
     share, auth, messaging other apps, Matrix room data and messages, the user's \
     Matrix identity) needs a capability DECLARED in the header \
     (`// permissions: network, location`) with a reason per capability \
     (`// why-network: ...`). Declaring is not granting: the user is asked, and can \
     refuse or revoke at any time. So the app MUST be fully usable with everything \
     denied — real fallback content, never a blank screen or a dead button — must \
     handle `r.is_ok == false` in every `host.request` callback, must re-check \
     `host.has(\"x\")` at use time rather than caching it at boot, and should define \
     `fn on_permissions_changed(caps)` to re-sync anything gated. Ask for the least \
     it needs. The host also limits HOW OFTEN an app may ask: requests are charged \
     against a per-app budget, so ask on user action or when data goes stale — never \
     poll in a loop, never request from a fast timer, never retry a failure \
     immediately. Over-budget requests come back as `r.is_ok == false` (handle them \
     like a denial), and an app that keeps hammering is STOPPED by the host. \
     File pickers, save dialogs and `auth.check` work only while the app is on \
     screen, one at a time. Apps also \
     SHARE the processor, memory, timers and downloads: an app on its own is not \
     limited, and is only trimmed when others compete for the same thing. Still \
     write for the edges — `start_interval` returns nil if refused (check it if \
     you create timers in a loop), an interval faster than the floor is SLOWED \
     rather than refused, and nothing should accumulate without bound.\n\
     \n\
     The `matrix.*` services (see the guide's Matrix services section) work only \
     when the app is attached to a room, and fail like a denial without one. Send \
     ONLY what the user explicitly asked to send, one message per user action, \
     never on a timer.";

fn build_initial_prompt(request: &str, slim: bool) -> String {
    format!(
        "You are the app generator for a phone launcher. Build a small, polished, \
         self-contained mini-app in the Makepad Splash dialect described below. \
         {RESPONSIVE_POLICY}\n\
         \n\
         {PERMISSION_POLICY}\n\
         \n\
         {}\n\
         \n\
         {}\n\
         \n\
         User request: {request}",
        guide_section(slim),
        reply_contract(),
    )
}

fn build_refine_prompt(request: &str, base: &MiniAppManifest, slim: bool) -> String {
    format!(
        "You are the app generator for a phone launcher. MODIFY an existing \
         mini-app written in the Makepad Splash dialect described below. Keep \
         everything the user didn't ask to change — including its responsive \
         layout: {RESPONSIVE_POLICY}\n\
         \n\
         {PERMISSION_POLICY}\n\
         \n\
         {}\n\
         \n\
         ## The app's current source ({})\n\
         \n\
         ````splash\n{}\n````\n\
         \n\
         {}\n\
         \n\
         User's change request: {request}",
        guide_section(slim),
        base.name,
        base.source,
        reply_contract(),
    )
}

fn build_repair_prompt(errors: &[String]) -> String {
    let list = errors
        .iter()
        .map(|e| format!("- {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Your script failed to compile. Errors:\n{list}\n\n\
         Re-read the guide's rules and reply again with the corrected COMPLETE \
         script as EXACTLY ONE ```splash fenced block (same // name/icon/tint \
         header), nothing else."
    )
}

fn build_nudge_prompt() -> String {
    "Your reply did not contain a ```splash fenced code block. Reply with \
     EXACTLY ONE ```splash fenced block containing the complete script \
     (starting with the // name / // icon / // tint header lines), and \
     nothing else."
        .to_string()
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Pulls the body of the ```splash fence out of the reply. The agent is asked
/// for exactly one, but replies sometimes grow prose that *mentions* the fence
/// (before or after the real one) — so every tagged fence is scanned and the
/// longest body wins, which is robust against both a quoted-format preamble
/// and a trailing "as requested, one ```splash block" remark. Falls back to a
/// bare ``` fence if no tagged one exists. CRLF endings are normalized.
fn extract_splash_block(reply: &str) -> Option<String> {
    fn body_after(reply: &str, open_idx: usize) -> Option<&str> {
        let after = &reply[open_idx..];
        let body_start = after.find('\n')? + 1;
        let body = &after[body_start..];
        let end = body.find("```").unwrap_or(body.len());
        Some(body[..end].trim_end())
    }

    let mut best: Option<&str> = None;
    let mut from = 0;
    while let Some(rel) = reply[from..].find("```splash") {
        let idx = from + rel;
        if let Some(body) = body_after(reply, idx) {
            if best.is_none_or(|b| body.len() > b.len()) {
                best = Some(body);
            }
        }
        from = idx + "```splash".len();
    }
    let body = match best {
        Some(b) => b,
        // Fallback: a single untagged fence.
        None => body_after(reply, reply.find("```")?)?,
    };
    if body.trim().is_empty() {
        return None;
    }
    Some(body.replace("\r\n", "\n"))
}

/// Base declarations plus anything the rewrite added, order preserved.
fn union_permissions(base: &[String], added: &[String]) -> Vec<String> {
    let mut out = base.to_vec();
    for p in added {
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// Cheap pre-parse ban list: constructs from OTHER dialects (octos-one's
/// `sys.*` helpers, web substrates, raw module imports) that would either
/// parse-fail confusingly or signal the agent drifted off-dialect — caught
/// here with a clearer message than the parser would give. This is a
/// steering aid, NOT the sandbox: actual containment is the isolate itself
/// (no net runtime, and `fs`/`run`/`res` stripped from its namespace).
/// String literals are blanked before scanning so an app whose *label text*
/// mentions e.g. "import " isn't rejected.
fn forbidden_construct(source: &str) -> Option<&'static str> {
    const BANNED: &[&str] = &[
        "sys.", "live_design", "Root{", "Root {", "Window{", "Window {",
        "use mod", "import ", "<script", "<div",
    ];
    let bare = blank_string_literals(source);
    BANNED.iter().copied().find(|b| bare.contains(b))
}

/// Replaces the contents of double-quoted string literals with spaces
/// (honoring `\"` escapes), so bans only match actual code.
fn blank_string_literals(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_str = false;
    let mut escaped = false;
    for c in source.chars() {
        if in_str {
            if escaped {
                escaped = false;
                out.push(' ');
            } else if c == '\\' {
                escaped = true;
                out.push(' ');
            } else if c == '"' {
                in_str = false;
                out.push('"');
            } else {
                out.push(' ');
            }
        } else {
            if c == '"' {
                in_str = true;
            }
            out.push(c);
        }
    }
    out
}

/// A short display name derived from the request when the header lacks one.
fn default_name(request: &str) -> String {
    let mut words = request.split_whitespace().collect::<Vec<_>>();
    words.retain(|w| !matches!(w.to_ascii_lowercase().as_str(),
        "a" | "an" | "the" | "app" | "make" | "create" | "build" | "me" | "for" | "with"));
    let mut name = words.into_iter().take(2).collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        name = "Generated".to_string();
    }
    // Title-case each word.
    name.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(18)
        .collect()
}

/// Kebab-case id from the name, made unique against the installed set.
/// Also used by the install path to re-unique against the LIVE state, since
/// the snapshot taken when the generation started can go stale (apps
/// installed while it ran, or a second generation racing this one).
pub fn unique_id(name: &str, taken: &[MiniAppId]) -> MiniAppId {
    let mut base: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if base.is_empty() {
        base = "generated".to_string();
    }
    if !taken.iter().any(|t| t == &base) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Evaluates `source` the exact way the Splash widget will (same prelude
/// prefix, no network) in a throwaway isolate, and returns the formatted
/// errors. Empty == the script parses and its root evaluates. Runtime errors
/// inside handlers can still happen later — this catches the compile/eval
/// class that octos-one's lint+repair loop targets, but with the real parser.
pub fn validate_splash(cx: &mut Cx, source: &str) -> Vec<String> {
    makepad_widgets::splash::validate_splash_body(cx, source, false)
}

// ---------------------------------------------------------------------------
// Error copy
// ---------------------------------------------------------------------------

/// First line, trimmed, capped — status-bar sized.
fn short_reason(msg: &str) -> String {
    // Scan the WHOLE message for a provider envelope, not just the first line:
    // octos reports a failure as a headline ("Internal error") with the API's
    // JSON on a later line, so a first-line-only scan threw away every word
    // that said what actually happened and showed the headline alone.
    if let Some(inner) = json_message(msg) {
        return inner;
    }
    // Nothing specific in there: hand back the WHOLE payload, not its first
    // line. It is ugly, but something ugly the user can paste into a bug
    // report beats "internal error", which is unactionable by design — and a
    // first-line-only fallback drops the JSON that agents print underneath it.
    let raw = one_line(msg);
    if raw.is_empty() { "agent error".to_string() } else { raw }
}

/// The sentence a human can act on, pulled out of a provider's error envelope.
///
/// Agents surface API failures as prose with a JSON blob stapled on:
/// `Internal error: Failed to authenticate. API Error: 403 {"error":{"type":
/// "permission_error","message":"You've reached your usage limit"}}`. The only
/// part that says what to DO is `message`, and it comes LAST — so any length
/// cap eats exactly the useful bit and leaves the punctuation. Returned in
/// full; shortening for a one-line strip is the display layer's business.
///
/// Three tiers, in order of how trustworthy the result is:
///
/// 1. **Parse it.** The live path — an ACP failure is a JSON-RPC envelope
///    built by `serde_json`, so it is always well-formed. Walking the parsed
///    tree handles nesting and escaping by construction, which hand-scanning
///    kept getting wrong.
/// 2. **Hand-scan it.** Agents hand us blobs they already truncated
///    (`…"message":"You've reached you`), which no parser will accept and
///    which are exactly the case this needs to work for.
/// 3. **Hand-scan it again, unescaped.** A payload that arrived as a *string*
///    of JSON has `\"` where a scan wants `"`. Tried last because a message
///    may legitimately contain an escaped quote, and unescaping that one cuts
///    the sentence short.
fn json_message(msg: &str) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(msg.trim()) {
        if let Some(found) = deepest_message(&parsed) {
            return Some(found);
        }
    }
    if let Some(found) = json_message_scan(msg) {
        return Some(found);
    }
    if msg.contains("\\\"") {
        let un = msg.replace("\\\"", "\"").replace("\\n", " ");
        if let Some(found) = json_message_scan(&un) {
            return Some(found);
        }
    }
    // Nothing anywhere carried a specific sentence. Say so; `short_reason`
    // decides what to show instead — the raw payload beats a generic word.
    None
}

/// Boilerplate the transport supplies whatever went wrong. Reporting one of
/// these is reporting nothing, so they never win against a real sentence.
fn is_generic(m: &str) -> bool {
    let l = m.trim().to_ascii_lowercase();
    l == "internal error" || l == "request failed" || l == "error" || l.is_empty()
}

/// The most deeply nested human sentence in a parsed error envelope.
///
/// Depth is the ranking that matters: the transport wraps the provider, which
/// wraps the API, so the further down a string sits the more specific it is.
/// `{"code":-32603,"message":"Internal error","data":"…{\"error\":{\"message\":
/// \"quota exhausted\"}}"}` has to resolve to `quota exhausted`, three layers
/// in and behind a string that is itself a document.
fn deepest_message(v: &Value) -> Option<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    collect_messages(v, 0, &mut found);
    found
        .into_iter()
        .filter(|(_, m)| !is_generic(m))
        // Deepest first; longest breaks a tie, since siblings at one depth are
        // a terse `type` and the prose that explains it.
        .max_by_key(|(depth, m)| (*depth, m.len()))
        .map(|(_, m)| m)
}

/// Keys whose value is meant for a person to read. `type`/`code`/`param` are
/// deliberately absent: `permission_error` is not a sentence.
const MESSAGE_KEYS: [&str; 5] = ["message", "data", "details", "detail", "description"];

fn collect_messages(v: &Value, depth: usize, out: &mut Vec<(usize, String)>) {
    match v {
        Value::Object(map) => {
            for (key, val) in map {
                match val {
                    Value::String(s) => {
                        // A string that is itself an envelope — the shape octos
                        // sends. Its contents sit one level deeper than the
                        // string that carries them, so they outrank it.
                        let nested = serde_json::from_str::<Value>(s.trim())
                            .ok()
                            .and_then(|inner| deepest_message(&inner))
                            .or_else(|| json_message_scan(s));
                        if let Some(nested) = nested {
                            out.push((depth + 2, nested));
                        }
                        if MESSAGE_KEYS.contains(&key.as_str()) {
                            out.push((depth + 1, one_line(s)));
                        }
                    }
                    _ => collect_messages(val, depth + 1, out),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_messages(item, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ").trim().to_string()
}

/// Every `"message"` in a blob too broken to parse, best one returned.
///
/// Scans for ALL of them, not just the first: an ACP failure is wrapped in
/// JSON-RPC, whose envelope carries a generic
/// `{"code":-32603,"message":"Internal error"}` — so taking the first match
/// reported "internal error" and threw away the provider's actual sentence,
/// which is nested deeper and therefore appears later.
fn json_message_scan(msg: &str) -> Option<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = msg;
    while let Some(at) = rest.find(r#""message""#) {
        rest = &rest[at + r#""message""#.len() ..];
        let Some(open) = rest.find('"') else { break };
        let out = json_string_body(&rest[open + 1 ..]);
        if !out.is_empty() {
            found.push(out);
        }
        rest = &rest[open + 1 ..];
    }
    // Skip the transport's boilerplate, then take the longest — reliably the
    // provider's own explanation.
    found
        .into_iter()
        .filter(|m| !is_generic(m))
        .max_by_key(String::len)
}

/// Reads a JSON string body up to its closing quote, honouring escapes.
///
/// Escape handling is the whole point: `{\"error\":…` stuffed into a string
/// field means the first bare `"` a naive scan meets belongs to the *nested*
/// document, not to the end of this one. Stopping there cut every provider
/// error off at `HTTP 403 - {\`.
fn json_string_body(body: &str) -> String {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') | Some('r') | Some('t') => out.push(' '),
                Some(esc) => out.push(esc),
                None => break,
            },
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Friendlier copy for the two setup failure modes everyone will hit first:
/// the binary missing, and the provider not configured.
fn process_gone_reason(msg: &str, cmd: &str) -> String {
    if msg.contains("no LLM provider configured") {
        "No LLM provider — export an API key (e.g. ANTHROPIC_API_KEY) or run `octos init`"
            .to_string()
    } else if msg.contains("couldn't start") {
        format!("`{cmd}` isn't installed — `cargo install octos`, or add a Kimi Coding key in Providers")
    } else {
        short_reason(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2app_core::header::parse_hex_color;

    /// A generated app declares its capabilities in the header, or it can
    /// never be granted them — an undeclared capability is refused outright.
    #[test]
    fn header_carries_permissions_and_reasons() {
        let src = "// name: Sunrise\n\
                   // icon: 🌅\n\
                   // permissions: network, location, made-up-cap, network\n\
                   // why-location: Uses your city.\n\
                   // why-made-up-cap: ignored\n\
                   View{}";
        let h = parse_app_header(src);
        // Unknown ids and repeats are dropped: neither can ever be granted,
        // and both would sit in App Info promising something fake.
        assert_eq!(h.permissions, vec!["network".to_string(), "location".to_string()]);
        assert_eq!(h.permission_reasons.get("location").unwrap(), "Uses your city.");
        assert!(h.permission_reasons.get("made-up-cap").is_none());
    }

    /// No header line means no capabilities, which is the safe default for
    /// every app that never asks for anything.
    #[test]
    fn header_without_permissions_declares_nothing() {
        let h = parse_app_header("// name: Tip\nView{}");
        assert!(h.permissions.is_empty());
        assert!(h.permission_reasons.is_empty());
    }

    /// A refine may ADD what the rewrite needs but never drops a declaration
    /// the user has already granted against.
    #[test]
    fn refine_unions_declarations() {
        let base = vec!["network".to_string(), "location".to_string()];
        let added = vec!["location".to_string(), "clipboard-write".to_string()];
        assert_eq!(
            union_permissions(&base, &added),
            vec![
                "network".to_string(),
                "location".to_string(),
                "clipboard-write".to_string()
            ]
        );
    }

    #[test]
    fn extracts_tagged_fence() {
        let reply = "sure!\n```splash\n// name: X\nView{}\n```\nthanks";
        assert_eq!(extract_splash_block(reply).unwrap(), "// name: X\nView{}");
    }

    #[test]
    fn extracts_longest_tagged_fence() {
        // Trailing prose that merely MENTIONS the fence must not beat the
        // real (longer) block, wherever each appears.
        let reply = "```splash\nlet a = 1\nView{}\n```\nas requested, one ```splash block";
        assert_eq!(extract_splash_block(reply).unwrap(), "let a = 1\nView{}");
        let reply2 = "a ```splash\nstub\n``` first, then\n```splash\nlet real = 1\nView{}\n```";
        assert_eq!(extract_splash_block(reply2).unwrap(), "let real = 1\nView{}");
    }

    #[test]
    fn extraction_normalizes_crlf() {
        let reply = "```splash\r\nView{}\r\n```";
        assert_eq!(extract_splash_block(reply).unwrap(), "View{}");
    }

    #[test]
    fn extracts_untagged_fallback_and_rejects_none() {
        assert_eq!(extract_splash_block("```\nBody\n```").unwrap(), "Body");
        assert!(extract_splash_block("no code here").is_none());
    }

    #[test]
    fn unterminated_fence_still_extracts() {
        let reply = "```splash\nView{}";
        assert_eq!(extract_splash_block(reply).unwrap(), "View{}");
    }

    #[test]
    fn parses_header_comments() {
        let block = "// name: Tip Calc\n// icon: 💰\n// tint: #4A90D9\nView{}";
        let h = parse_app_header(block);
        assert_eq!(h.name.as_deref(), Some("Tip Calc"));
        assert_eq!(h.icon.as_deref(), Some("💰"));
        assert_eq!(h.tint, Some(0x4A90D9));
    }

    #[test]
    fn header_variants_parse() {
        assert_eq!(parse_hex_color("#aabbcc"), Some(0xaabbcc));
        assert_eq!(parse_hex_color("0xAABBCC"), Some(0xAABBCC));
        assert_eq!(parse_hex_color("nope"), None);
    }

    /// The real 403 that started this: the actionable sentence is at the END
    /// of a JSON blob, so the old fixed-length cut kept the HTTP status and
    /// threw away the only part that says what happened.
    #[test]
    fn an_api_error_reports_the_providers_own_message() {
        let raw = r#"Internal error: Failed to authenticate. API Error: 403 {"error":{"type":"permission_error","message":"You've reached your usage limit for this month"}}"#;
        assert_eq!(short_reason(raw), "You've reached your usage limit for this month");
    }

    /// Agents often hand us a blob that they themselves already truncated, so
    /// this can't depend on the JSON being well-formed.
    #[test]
    fn a_truncated_error_blob_still_yields_its_message() {
        let cut = r#"API Error: 403 {"error":{"type":"permission_error","message":"You've reached you"#;
        assert_eq!(short_reason(cut), "You've reached you");
        // octos reports the headline on one line and the API's JSON on the
        // next. Scanning only the first line showed "Internal error" alone.
        let octos = concat!(
            "Internal error\n",
            "API error (moonshot-coding@api/k3): provider quota exhausted - HTTP 403 - ",
            r#"{"error":{"message":"You've reached your usage limit for this billing cycle"}}"#,
        );
        assert_eq!(short_reason(octos), "You've reached your usage limit for this billing cycle");
        // The real shape: an ACP failure is wrapped in JSON-RPC, whose envelope
        // says "Internal error". Reporting THAT is reporting nothing, which is
        // exactly what the UI used to show.
        let rpc = concat!(
            r#"{"code":-32603,"message":"Internal error","data":"#,
            r#"{"error":{"type":"quota","message":"You've reached your usage limit "#,
            r#"for this billing cycle"}}}"#,
        );
        assert_eq!(
            short_reason(rpc),
            "You've reached your usage limit for this billing cycle"
        );
        // ...and when the envelope carries nothing specific, show the RAW
        // payload rather than the useless generic word.
        let bare = r#"{"code":-32603,"message":"Internal error"}"#;
        assert_eq!(short_reason(bare), bare);
        // The real shape from octos: the payload is a STRING of escaped JSON,
        // so every quote inside it arrives as \" and a naive scan misses it.
        let escaped = concat!(
            r#"prompt turn failed: API error (moonshot-coding@api/k3): "#,
            r#"provider quota exhausted - HTTP 403 - {"error":{"message":""#,
            r#"You've reached your usage limit for this billing cycle"}}"#,
        );
        assert_eq!(
            short_reason(escaped),
            "You've reached your usage limit for this billing cycle"
        );
        // A `data` string is the other place agents put the detail.
        let with_data = r#"{"code":-32603,"message":"Internal error","data":"spawn octos ENOENT"}"#;
        assert_eq!(short_reason(with_data), "spawn octos ENOENT");
        // Escapes are unwrapped rather than shown raw.
        let escaped = r#"{"message":"the \"model\" field\nis wrong"}"#;
        assert_eq!(short_reason(escaped), r#"the "model" field is wrong"#);
    }

    /// The shape that actually comes off the wire, reproduced exactly.
    ///
    /// `octos acp` reports a failed turn with `util::internal_error(…)`, which
    /// puts the whole thing in `data` AS A STRING — prose with the provider's
    /// JSON stapled on the end. Our client re-serializes that envelope, so by
    /// the time it reaches here every quote inside `data` is escaped.
    ///
    /// This is what shipped truncated at `HTTP 403 - {\`: the `"data"` scan
    /// stopped at the first bare quote it saw, which belongs to the *nested*
    /// document, and it ran before the pass that would have found the real
    /// sentence — so it won with garbage.
    #[test]
    fn the_real_octos_envelope_yields_the_providers_sentence() {
        // Built the way the client builds it, not typed by hand: an escaping
        // mistake in the fixture would make this test agree with a bug.
        let detail = concat!(
            "prompt turn failed: API error (moonshot-coding@api/k3): ",
            r#"provider quota exhausted - HTTP 403 - {"error":{"message":""#,
            r#"Your account has run out of credits; top up at kimi.com","type":"quota"}}"#,
        );
        let wire = serde_json::json!({
            "code": -32603,
            "message": "Internal error",
            "data": detail,
        })
        .to_string();
        // Precondition: the fixture really does contain the escaped nesting.
        assert!(wire.contains(r#"HTTP 403 - {\"error\""#), "fixture lost its escaping: {wire}");

        assert_eq!(
            short_reason(&wire),
            "Your account has run out of credits; top up at kimi.com"
        );
        // And nothing anywhere may end mid-escape.
        assert!(!short_reason(&wire).ends_with('\\'));
    }

    /// The literal frame `octos acp` sent, copied off the wire.
    ///
    /// Note the end: octos caps a provider's response body at 200 chars
    /// (`octos-llm/src/error.rs`), so the nested document arrives with its
    /// string UNTERMINATED — no closing quote, no closing braces. Nothing may
    /// depend on that JSON being well-formed, which is why the message scan is
    /// hand-rolled. The tail past octos's cap is gone for good; what has to
    /// survive is the sentence that says what to do about it.
    #[test]
    fn the_frame_octos_actually_sent_reads_as_a_sentence() {
        let wire = concat!(
            r#"{"code":-32603,"message":"Internal error","data":"prompt turn failed: "#,
            r#"API error (moonshot-coding@api/k3): provider quota exhausted — HTTP 403 - "#,
            r#"{\"error\":{\"message\":\"You've reached your usage limit for this billing "#,
            r#"cycle. Your quota will be refreshed in the next cycle. To continue now, "#,
            r#"purchase extra usage or upgrade your plan: https://www.kim"}"#,
        );
        assert_eq!(
            short_reason(wire),
            "You've reached your usage limit for this billing cycle. Your quota will be \
             refreshed in the next cycle. To continue now, purchase extra usage or \
             upgrade your plan: https://www.kim"
        );
    }

    /// Same envelope, but the provider gave no nested JSON — then the prose in
    /// `data` IS the answer, and it has to survive whole.
    #[test]
    fn a_data_only_envelope_keeps_the_whole_sentence() {
        let wire = serde_json::json!({
            "code": -32603,
            "message": "Internal error",
            "data": "prompt turn failed: no API key for provider `moonshot-coding` \
                     (set KIMI_CODING_API_KEY)",
        })
        .to_string();
        assert_eq!(
            short_reason(&wire),
            "prompt turn failed: no API key for provider `moonshot-coding` (set KIMI_CODING_API_KEY)"
        );
    }

    /// A plain error has no envelope and must survive untouched — and an empty
    /// one still has to say something.
    #[test]
    fn plain_errors_pass_through() {
        assert_eq!(short_reason("connection reset by peer"), "connection reset by peer");
        assert_eq!(short_reason("   "), "agent error");
    }

    #[test]
    fn bans_foreign_dialects() {
        assert_eq!(forbidden_construct("View{ sys.weather() }"), Some("sys."));
        assert_eq!(forbidden_construct("let x = 1\nView{}"), None);
    }

    #[test]
    fn ban_scan_ignores_string_literals() {
        // A label whose TEXT mentions a banned token is a legit app.
        assert_eq!(
            forbidden_construct("View{ Label{text: \"how to import  sys. stuff\"} }"),
            None
        );
        // ...but the same token as code still trips.
        assert_eq!(
            forbidden_construct("Label{text: \"hi\"}\nsys.weather()"),
            Some("sys.")
        );
        // Escaped quotes don't desync the scanner.
        assert_eq!(
            forbidden_construct("Label{text: \"a \\\" quote\"}\nimport x"),
            Some("import ")
        );
    }

    #[test]
    fn icon_header_keeps_multi_codepoint_emoji() {
        let h = parse_app_header("// icon: 👨‍👩‍👧\nView{}");
        assert_eq!(h.icon.as_deref(), Some("👨‍👩‍👧"));
    }

    #[test]
    fn ids_unique_against_taken() {
        let taken = vec!["tip-calc".to_string(), "tip-calc-2".to_string()];
        assert_eq!(unique_id("Tip Calc", &taken), "tip-calc-3");
        assert_eq!(unique_id("Fresh", &taken), "fresh");
        assert_eq!(unique_id("!!!", &[]), "generated");
    }

    #[test]
    fn default_names_from_request() {
        assert_eq!(default_name("make me a pomodoro timer app"), "Pomodoro Timer");
        assert_eq!(default_name(""), "Generated");
    }
}
