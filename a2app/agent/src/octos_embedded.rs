//! `embedded`: the octos agent running IN-PROCESS, no child process.
//!
//! Links the octos agent core (provider registry, tool registry + sandbox,
//! agent loop) directly and drives it on a dedicated thread that owns a tokio
//! runtime — the same shape `octos acp` itself uses, minus the process
//! boundary. This is the only local option on iOS, where `exec()` is
//! prohibited; on desktop it also removes the "install octos first" step
//! (only `~/.octos/config.json` — an `octos init` from any machine — is
//! needed for the provider).
//!
//! The agent itself is built by octos, not by us: `AcpCommand::factory()` is
//! the same factory `octos acp` serves over stdio, so this backend gets
//! provider fallback routing, the auth store, `keychain:` markers, MCP,
//! plugins, skills, memory-bank tools and the config precedence rules
//! identically — and keeps getting them as octos changes.
//!
//! It used to reimplement that assembly by hand against octos-llm/-memory,
//! which meant a subset that drifted: a key in the auth store or behind a
//! `keychain:` marker was invisible, and the backend reported "no provider"
//! where the child process would have run fine. What is left here is only the
//! parts octos does NOT own — the thread and its runtime, the command loop,
//! the reporter that reduces octos's progress events to the pipeline's
//! vocabulary, and the per-turn history rules.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use makepad_widgets::SignalToUI;

use octos_cli::commands::acp::AcpCommand;

use crate::acp_client::AcpEvent;
use crate::prefs::AgentPrefs;
use crate::AgentTransport;

/// Tool-call iterations allowed per turn. Lower than octos's own default of
/// 20: a turn here writes one Splash file and stops, so a budget that large
/// only buys a runaway loop more rope.
const MAX_ITERATIONS: u32 = 12;

enum Cmd {
    Prompt(String),
}

/// Cancellation, which has to work before the agent exists.
///
/// The agent's shutdown flag is created by octos inside `factory.build()` and
/// wired into the loop, so we can't hand ours in — we adopt theirs once it
/// arrives. Until then a cancel or a drop still has to be remembered, or
/// tearing down during the (network-bound) build would be silently ignored and
/// the first turn would run anyway. Both fields sit under one lock so adopting
/// can't race a concurrent cancel.
#[derive(Default)]
struct Shutdown {
    inner: std::sync::Mutex<ShutdownInner>,
}

#[derive(Default)]
struct ShutdownInner {
    requested: bool,
    agent: Option<Arc<AtomicBool>>,
}

impl Shutdown {
    fn set(&self, value: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.requested = value;
        if let Some(flag) = &inner.agent {
            flag.store(value, Ordering::Release);
        }
    }

    /// Takes ownership of the agent's flag, applying whatever was asked for
    /// while it didn't exist yet.
    fn adopt(&self, flag: Arc<AtomicBool>) {
        let mut inner = self.inner.lock().unwrap();
        flag.store(inner.requested, Ordering::Release);
        inner.agent = Some(flag);
    }

    fn is_set(&self) -> bool {
        self.inner.lock().unwrap().requested
    }
}

/// In-process octos agent behind the same event interface as the ACP client.
pub struct EmbeddedOctos {
    events: Receiver<AcpEvent>,
    cmd_tx: Sender<Cmd>,
    shutdown: Arc<Shutdown>,
}

impl EmbeddedOctos {
    pub fn start(workspace: &Path, prefs: &AgentPrefs) -> Result<Self, String> {
        std::fs::create_dir_all(workspace).ok();
        let (evt_tx, events) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let shutdown = Arc::new(Shutdown::default());
        let ws = workspace.to_path_buf();
        let prefs = prefs.clone();
        let sd = shutdown.clone();
        std::thread::spawn(move || agent_thread(ws, prefs, cmd_rx, evt_tx, sd));
        Ok(Self { events, cmd_tx, shutdown })
    }
}

impl AgentTransport for EmbeddedOctos {
    fn drain_events(&mut self) -> Vec<AcpEvent> {
        let mut out = Vec::new();
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
        out
    }

    fn send_prompt(&mut self, text: &str) {
        // Clear a stale cancel flag HERE (not inside the turn) so a cancel or
        // drop that lands after this queue-up still wins — mirroring octos
        // acp's own dispatch-loop reset ordering.
        self.shutdown.set(false);
        let _ = self.cmd_tx.send(Cmd::Prompt(text.to_string()));
    }

    fn cancel(&mut self) {
        // The agent loop checks this at each iteration/stream-chunk boundary.
        self.shutdown.set(true);
    }

    fn desc(&self) -> &str {
        "octos (in-process)"
    }
}

impl Drop for EmbeddedOctos {
    fn drop(&mut self) {
        // Without this, dropping the client mid-turn (stall watchdog, app
        // teardown) leaves the agent turn running detached — burning tokens
        // invisibly for up to max_iterations. The flag aborts it at the next
        // loop/stream checkpoint; the thread then sees the closed command
        // channel and exits, taking the runtime with it.
        self.shutdown.set(true);
    }
}

fn send(evt_tx: &Sender<AcpEvent>, event: AcpEvent) {
    let _ = evt_tx.send(event);
    SignalToUI::set_ui_signal();
}

/// The agent thread: build once, then serve prompt turns until the client
/// (and thus the command channel) is dropped.
fn agent_thread(
    workspace: PathBuf,
    prefs: AgentPrefs,
    cmd_rx: Receiver<Cmd>,
    evt_tx: Sender<AcpEvent>,
    shutdown: Arc<Shutdown>,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        // Deep agent futures; octos's own entrypoints use an 8MB stack.
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            send(&evt_tx, AcpEvent::ProcessGone(format!("tokio runtime: {e}")));
            return;
        }
    };

    let agent = match rt.block_on(build_agent(&workspace, &prefs, &shutdown)) {
        Ok(agent) => agent,
        Err(e) => {
            send(&evt_tx, AcpEvent::ProcessGone(e));
            return;
        }
    };

    send(&evt_tx, AcpEvent::SessionReady);

    // Conversation history across turns (repair prompts rely on it), kept by
    // octos's exact append rules (see run_turn).
    let mut history: Vec<octos_core::Message> = Vec::new();

    // Blocking command loop OUTSIDE the runtime: recv() parks this thread;
    // each turn runs to completion on the runtime.
    while let Ok(Cmd::Prompt(text)) = cmd_rx.recv() {
        rt.block_on(run_turn(&agent, &shutdown, &evt_tx, &mut history, &text));
    }
}

/// Builds the agent through octos's own ACP factory.
///
/// Everything that used to live here by hand — reading config.json, resolving
/// the provider and its key, constructing the tool registry and episode store
/// — is octos's job now, done exactly as `octos acp` does it.
///
/// Note what is NOT overridden: the data dir. It is tempting to point the
/// episode store at a private scratch dir so a user running their own
/// octos can't contend for the redb lock — this code used to. But an explicit
/// `data_dir` also makes octos treat the context as explicit and moves
/// `config_home` to that same dir (`resolve_config_context`), so it looks for
/// `config.json` in the scratch dir, finds none, and reports "no LLM provider
/// configured" no matter how the user actually set octos up. That isolation is
/// for tenants, and we are not one.
///
/// Sharing `~/.octos` is what the child process does anyway — `octos acp` is
/// spawned without `--data-dir` — so this is parity, not a regression. It is
/// also safe to share now: the episode store's redb lock is exclusive, and
/// octos degrades to a memory-less handle rather than failing when someone
/// else holds it (octos-org/octos#1914). Before that it failed outright, and
/// the holder was usually us — an agent's thread only releases the store when
/// it reaches a checkpoint and drops, so Stop-then-Send landed inside that
/// window and died.
async fn build_agent(
    workspace: &Path,
    prefs: &AgentPrefs,
    shutdown: &Arc<Shutdown>,
) -> Result<Arc<octos_agent::Agent>, String> {
    let cwd = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let command = AcpCommand {
        cwd: Some(cwd.clone()),
        max_iterations: MAX_ITERATIONS,
        // The pane's picks, honoured HERE too. The child process gets them as
        // `--provider` / `--model` on its command line; without passing them
        // the embedded agent read config.json and quietly ignored both, so
        // choosing a provider in the Providers page changed nothing and the
        // page still reported it as the one in use.
        //
        // `model_override`, NOT `prefs.model`: the saved pick is global and
        // may belong to a provider that isn't this one. See its docs — passing
        // it blindly is what produced "invalid temperature: only 1 is allowed
        // for this model" against the Kimi coding plan.
        provider: crate::providers::session_provider(),
        model: crate::prefs::Backend::detect().model_override(prefs),
        ..Default::default()
    };

    let factory = command.factory().map_err(|e| e.to_string())?;

    // Deliberately NO wipe-and-retry on failure. An earlier version cleared the
    // data dir and tried again, to recover from an episodes.redb truncated by a
    // crash — safe only while that dir was our own private scratch. It is the
    // user's own ~/.octos now, and their episode history is not ours to delete.
    //
    let built = factory.build(cwd).await.map_err(|e| e.to_string())?;
    finish_agent(built, shutdown)
}

/// Adopts the agent's shutdown flag and applies our own additions.
fn finish_agent(
    built: (Arc<octos_agent::Agent>, Arc<AtomicBool>),
    shutdown: &Arc<Shutdown>,
) -> Result<Arc<octos_agent::Agent>, String> {
    let (agent, flag) = built;
    // From here a cancel reaches the running loop — including one that arrived
    // while the build was still in flight.
    shutdown.adopt(flag);

    // With persistent-guide the guide lives in the system prompt for the whole
    // session (the in-process analogue of the .octos/AGENTS.md bootstrap
    // file), and per-turn prompts go slim. `append_system_prompt` takes &self,
    // so this still works on octos's already-built agent.
    #[cfg(feature = "persistent-guide")]
    {
        agent.append_system_prompt(crate::SPLASH_GUIDE);
        crate::skills::mark_deployed();
    }

    Ok(agent)
}

/// Per-turn reporter: octos's own StreamChunk/Response dedupe, reduced to the
/// pipeline's event vocabulary.
struct Reporter {
    evt_tx: Sender<AcpEvent>,
    streamed: AtomicBool,
}

impl octos_agent::ProgressReporter for Reporter {
    /// Mirrors `progress_event_to_acp` in octos's own ACP command, so the
    /// console shows the same run whichever backend produced it.
    fn report(&self, event: octos_agent::ProgressEvent) {
        use octos_agent::ProgressEvent as E;
        match event {
            E::StreamChunk { text, .. } => {
                self.streamed.store(true, Ordering::Release);
                send(&self.evt_tx, AcpEvent::Chunk(text));
            }
            // The loop emits streaming deltas AND a final full Response with
            // the same text; forward the Response only when nothing streamed
            // (non-streaming providers).
            E::Response { content, .. } => {
                if !self.streamed.load(Ordering::Acquire) {
                    send(&self.evt_tx, AcpEvent::Chunk(content));
                }
            }
            // THE reasoning text. Dropping this is what made an embedded run
            // look hung: a thinking model — Kimi's k3 has thinking always on —
            // spends most of a turn emitting these and nothing else, so the UI
            // sat on one status with an empty console until the code finally
            // started. octos's ACP path maps it to `agent_thought_chunk`, which
            // is where the child process gets its 💭 from.
            E::ReasoningChunk { text, .. } => {
                send(&self.evt_tx, AcpEvent::Thought(text));
            }
            E::ToolStarted { name, .. } => {
                send(&self.evt_tx, AcpEvent::ToolCall(name));
            }
            // Nothing to render, but they are proof the agent is alive, and
            // the pipeline's stall watchdog measures the gap since the LAST
            // event of any kind. Dropping them meant a long think looked
            // identical to a dead provider, and a turn that thought for longer
            // than the stall timeout was killed for being slow.
            E::Thinking { .. }
            | E::TaskStarted { .. }
            | E::LlmStatus { .. }
            | E::ToolProgress { .. }
            | E::ToolCompleted { .. }
            | E::FileModified { .. }
            | E::PlanUpdated { .. }
            | E::TokenUsage { .. }
            | E::CostUpdate { .. }
            | E::StreamDone { .. }
            | E::StreamRetry { .. } => send(&self.evt_tx, AcpEvent::Tick),
            _ => {}
        }
    }
}

/// One prompt turn, following `octos acp`'s run_prompt_turn to the letter:
/// stale-cancel reset before the turn, cancelled-Err mapped to a cancel (not
/// an error), and the two-guard history append.
async fn run_turn(
    agent: &Arc<octos_agent::Agent>,
    shutdown: &Arc<Shutdown>,
    evt_tx: &Sender<AcpEvent>,
    history: &mut Vec<octos_core::Message>,
    text: &str,
) {
    // NOTE: the stale-cancel reset happens in send_prompt (UI side), BEFORE
    // the command is queued — so a cancel/drop arriving while the turn waits
    // in the queue is never clobbered here.
    agent.set_reporter(Arc::new(Reporter {
        evt_tx: evt_tx.clone(),
        streamed: AtomicBool::new(false),
    }));

    let snapshot = history.clone();
    let outcome = agent.process_message(text, &snapshot, vec![]).await;
    let cancelled = shutdown.is_set();

    match outcome {
        Ok(resp) => {
            let assistant_reply = resp.content.clone();
            history.extend(resp.messages);
            let already = matches!(
                history.last(),
                Some(last) if last.role == octos_core::MessageRole::Assistant
                    && last.content == assistant_reply
            );
            if !cancelled && !assistant_reply.is_empty() && !already {
                history.push(octos_core::Message {
                    role: octos_core::MessageRole::Assistant,
                    content: assistant_reply.clone(),
                    media: vec![],
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                    client_message_id: None,
                    thread_id: None,
                    timestamp: chrono::Utc::now(),
                });
            }
            let stop_reason = if cancelled { "cancelled" } else { "end_turn" };
            send(
                evt_tx,
                AcpEvent::TurnDone {
                    stop_reason: stop_reason.to_string(),
                    text: if cancelled { String::new() } else { assistant_reply },
                },
            );
        }
        Err(e) => {
            if cancelled {
                send(
                    evt_tx,
                    AcpEvent::TurnDone { stop_reason: "cancelled".to_string(), text: String::new() },
                );
            } else {
                send(evt_tx, AcpEvent::Error(format!("agent turn failed: {e}")));
            }
        }
    }
}
