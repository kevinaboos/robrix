//! A minimal Agent Client Protocol (ACP) client over stdio.
//!
//! Speaks newline-delimited JSON-RPC 2.0 to a spawned agent process (by default
//! `octos acp`, but any ACP agent binary works — the protocol is the standard
//! one from <https://agentclientprotocol.com>). Deliberately dependency-light:
//! plain `std::process` + `std::thread` + `mpsc`, no async runtime. Incoming
//! events are queued for the UI thread, which drains them from `handle_event`;
//! each queued event is followed by a `SignalToUI` wakeup so replies that land
//! while the app is idle don't sit in the channel until the next input event.
//!
//! One client == one child process == one generation. The pipeline spawns a
//! fresh agent per create-app request and drops it when the request completes,
//! so there is no reconnect/restart state to manage; a cancel is just a
//! `session/cancel` (and ultimately a kill on drop).
//!
//! Threading: THREE threads touch the child, and none of them may block
//! another. All stdin writes go through a dedicated writer thread fed by an
//! unbounded channel — the UI thread and the reader thread only ever
//! `send()`, which cannot block, so a stalled/wedged child can never freeze
//! the UI (or deadlock the reader against its own refusal replies). `Drop`
//! kills the child FIRST (kill takes no locks), which closes the pipes and
//! unblocks any thread stuck mid-write/mid-read.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use makepad_widgets::SignalToUI;
use serde_json::{json, Value};

/// Cap on one incoming NDJSON line. A frame past this is not a protocol we
/// can parse anyway (real replies are a few KB) — treat it as a dead agent
/// rather than buffering without bound.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Events surfaced to the UI thread, already reduced from raw JSON-RPC to what
/// the generation pipeline cares about.
#[derive(Debug)]
pub enum AcpEvent {
    /// `initialize` + `session/new` completed; the agent is ready for a prompt.
    SessionReady,
    /// A streamed chunk of the agent's reply text (`agent_message_chunk`).
    Chunk(String),
    /// The agent invoked a tool (title only — shown as status, not blocked on).
    ToolCall(String),
    /// A chunk of the agent's extended *thinking* (`agent_thought_chunk`).
    /// This is the only thing a reasoning model emits during the long quiet
    /// stretch before it starts writing, so dropping it (as this client used
    /// to) left the console frozen on one line for a minute at a time.
    Thought(String),
    /// The agent's plan (`plan`) — Claude Code turns its TodoWrite calls into
    /// this, so it's a real, ordered account of what it's about to do.
    Plan(Vec<PlanStep>),
    /// A session update that carries no content we show (a tool-call status
    /// change, a mode/command list). Surfaced anyway as proof of life: the
    /// stall watchdog measures the gap since the last event, and silently
    /// dropping these made a busy agent look hung.
    Tick,
    /// The prompt turn finished with this ACP stop reason (e.g. `end_turn`,
    /// `cancelled`, `refusal`), plus the full accumulated reply text.
    TurnDone { stop_reason: String, text: String },
    /// A JSON-RPC error reply, or a protocol-level failure.
    Error(String),
    /// The agent process exited (or its stdout closed). Carries a short
    /// diagnostic assembled from captured stderr — this is how "octos isn't
    /// installed / no provider configured" reaches the user.
    ProcessGone(String),
}

/// One step of the agent's plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub content: String,
    /// `pending` | `in_progress` | `completed` (ACP's vocabulary).
    pub status: String,
}

/// Which JSON-RPC request an outstanding id belongs to. The pipeline runs
/// strictly one request at a time (initialize → session/new → prompt → ...),
/// so a single pending slot replaces a request table.
#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Initialize,
    NewSession,
    Prompt,
}

/// State shared between the UI-side handle and the reader thread. The reader
/// thread advances the handshake itself (initialize reply → send session/new)
/// so the UI only ever sees `SessionReady`.
struct Shared {
    /// Feed to the writer thread; `send` never blocks. `None` after shutdown.
    write_tx: Mutex<Option<Sender<String>>>,
    next_id: AtomicU64,
    /// The id and kind of the single in-flight request, if any.
    pending: Mutex<Option<(u64, Pending)>>,
    /// The negotiated session id, once `session/new` returns.
    session_id: Mutex<Option<String>>,
    /// The agent's reply text accumulated from streamed chunks this turn.
    turn_text: Mutex<String>,
    /// Absolute workspace dir sent as the session's `cwd`.
    workspace: String,
}

impl Shared {
    fn write_line(&self, line: String) {
        if let Some(tx) = self.write_tx.lock().unwrap().as_ref() {
            let _ = tx.send(line);
        }
    }

    fn send_request(&self, kind: Pending, method: &str, params: Value) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        *self.pending.lock().unwrap() = Some((id, kind));
        self.write_line(
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        );
    }
}

/// A live connection to a spawned ACP agent process.
pub struct AcpClient {
    child: Child,
    events: Receiver<AcpEvent>,
    shared: Arc<Shared>,
    /// Human-readable command line, for diagnostics.
    cmd_desc: String,
}

impl AcpClient {
    /// Spawns `cmd_line` (whitespace-split; first token is the binary) with
    /// `workspace` as its working directory, and starts the ACP handshake.
    /// `env` is layered on top of the inherited environment — that's how the
    /// UI's model/effort/thinking picks reach the agent, since these are the
    /// agent's own env-var knobs rather than anything in the ACP protocol.
    /// Returns an error string suitable to show the user if the process cannot
    /// be spawned at all.
    pub fn spawn(
        cmd_line: &str,
        workspace: &std::path::Path,
        env: &[(String, String)],
        extra_args: &[String],
    ) -> Result<Self, String> {
        let mut parts = cmd_line.split_whitespace();
        let bin = parts.next().ok_or("agent command is empty")?;
        let mut args: Vec<String> = parts.map(str::to_string).collect();
        args.extend(extra_args.iter().cloned());

        std::fs::create_dir_all(workspace).ok();
        let mut child = Command::new(bin)
            .args(&args)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            // Robrix may itself be running from inside a Claude Code
            // session (dev workflows); the claude-code-acp adapter refuses to
            // start when it sees CLAUDECODE, thinking it's being nested. Robrix
            // isn't Claude Code — drop the marker for the child (the
            // adapter's own documented bypass).
            .env_remove("CLAUDECODE")
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("couldn't start `{cmd_line}`: {e}"))?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let mut stdin = child.stdin.take().expect("piped stdin");

        let (tx, events) = std::sync::mpsc::channel::<AcpEvent>();
        let (write_tx, write_rx) = std::sync::mpsc::channel::<String>();
        let shared = Arc::new(Shared {
            write_tx: Mutex::new(Some(write_tx)),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(None),
            session_id: Mutex::new(None),
            turn_text: Mutex::new(String::new()),
            workspace: workspace.to_string_lossy().into_owned(),
        });

        // Writer thread: sole owner of the child's stdin. Exits when every
        // Sender is gone (client dropped) or the pipe breaks (child died).
        std::thread::spawn(move || {
            for line in write_rx {
                if stdin
                    .write_all(line.as_bytes())
                    .and_then(|_| stdin.write_all(b"\n"))
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });

        // Stderr drain: keep the tail for diagnostics (a missing provider
        // config makes the agent exit immediately with the reason on stderr).
        // Read lossily — a stray invalid-UTF-8 byte must not kill the drain.
        let err_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_done = {
            let err_tail = err_tail.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buf = Vec::new();
                while read_capped_line(&mut reader, &mut buf) {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim_end_matches(['\r', '\n']);
                    let mut tail = err_tail.lock().unwrap();
                    tail.push(line.to_string());
                    let excess = tail.len().saturating_sub(12);
                    if excess > 0 {
                        tail.drain(..excess);
                    }
                }
            })
        };

        // Reader thread: NDJSON lines → reduced AcpEvents → queue + UI signal.
        // Also read lossily and with a per-line cap, so a malformed agent
        // can't wedge or balloon the host app.
        {
            let tx = tx.clone();
            let shared = shared.clone();
            let err_tail = err_tail.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf = Vec::new();
                loop {
                    if !read_capped_line(&mut reader, &mut buf) {
                        break;
                    }
                    if buf.len() >= MAX_LINE_BYTES {
                        let _ = tx.send(AcpEvent::ProcessGone(
                            "agent sent an oversized protocol frame".to_string(),
                        ));
                        SignalToUI::set_ui_signal();
                        return;
                    }
                    let line = String::from_utf8_lossy(&buf);
                    if line.trim().is_empty() {
                        continue;
                    }
                    for event in reduce_line(&shared, &line) {
                        if tx.send(event).is_err() {
                            return; // client dropped; stop reading
                        }
                        SignalToUI::set_ui_signal();
                    }
                }
                // Stdout closed: the process died or shut down. Wait briefly
                // for the stderr drain to finish flushing the reason (bounded:
                // stderr may stay open in exotic cases, so don't join blindly).
                for _ in 0..20 {
                    if stderr_done.is_finished() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let tail = err_tail.lock().unwrap().join("\n");
                let msg = if tail.trim().is_empty() {
                    "agent process exited".to_string()
                } else {
                    format!("agent process exited: {}", tail.trim())
                };
                let _ = tx.send(AcpEvent::ProcessGone(msg));
                SignalToUI::set_ui_signal();
            });
        }

        // Kick off the handshake; the reader thread chains session/new when
        // the initialize reply arrives.
        shared.send_request(
            Pending::Initialize,
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {"name": "robrix", "version": env!("CARGO_PKG_VERSION")},
            }),
        );

        Ok(Self { child, events, shared, cmd_desc: cmd_line.to_string() })
    }

    /// Drains every event queued by the reader thread. Call from the UI
    /// thread's event handler (a `SignalToUI` wakeup guarantees one fires).
    pub fn drain_events(&mut self) -> Vec<AcpEvent> {
        let mut out = Vec::new();
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
        out
    }

    /// Sends the user's prompt for this session. `SessionReady` must have been
    /// received first. Repair prompts reuse the same session, so the agent
    /// keeps the conversation history across attempts.
    pub fn send_prompt(&mut self, text: &str) {
        let Some(session_id) = self.shared.session_id.lock().unwrap().clone() else {
            return;
        };
        self.shared.turn_text.lock().unwrap().clear();
        self.shared.send_request(
            Pending::Prompt,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": text}],
            }),
        );
    }

    /// Requests cancellation of the in-flight turn (`session/cancel`). The
    /// turn still ends with a `TurnDone{stop_reason:"cancelled"}` reply.
    pub fn cancel(&mut self) {
        let Some(session_id) = self.shared.session_id.lock().unwrap().clone() else {
            return;
        };
        self.shared.write_line(
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {"sessionId": session_id},
            })
            .to_string(),
        );
    }

    /// The command line this client was spawned with (for error messages).
    pub fn cmd_desc(&self) -> &str {
        &self.cmd_desc
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Kill FIRST: it takes no locks and closes the pipes, so any thread
        // blocked on the child (reader mid-read, writer mid-write) unwedges.
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Dropping the sender lets the writer thread exit.
        self.shared.write_tx.lock().unwrap().take();
    }
}

/// Reads one `\n`-terminated line into `buf` (cleared first), lossily and
/// capped at `MAX_LINE_BYTES`. Returns false on EOF/error with nothing read.
/// A line hitting the cap is returned as-is (caller checks `buf.len()`).
fn read_capped_line(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> bool {
    buf.clear();
    let mut limited = reader.take(MAX_LINE_BYTES as u64);
    match limited.read_until(b'\n', buf) {
        Ok(0) => false,
        Ok(_) => {
            while buf.last() == Some(&b'\n') || buf.last() == Some(&b'\r') {
                buf.pop();
            }
            true
        }
        Err(_) => false,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Regression test for the review-confirmed deadlock: an agent that
    /// floods client-bound requests WITHOUT reading its stdin used to wedge
    /// the reader thread inside a blocking refusal write (holding the stdin
    /// lock), which froze cancel/Drop — and the UI thread with them. With the
    /// writer thread + kill-first Drop, teardown must stay prompt no matter
    /// what the child does.
    #[test]
    fn burst_agent_cannot_wedge_teardown() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("hl_acp_burst_test");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("burst.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             i=0\n\
             while [ $i -lt 3000 ]; do\n\
               echo '{\"jsonrpc\":\"2.0\",\"id\":\"r'$i'\",\"method\":\"nag\",\"params\":{}}'\n\
               i=$((i+1))\n\
             done\n\
             sleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut client = AcpClient::spawn(script.to_str().unwrap(), &dir, &[], &[]).unwrap();
        // Let the flood arrive (the client refuses each request via the
        // writer channel; the child never drains stdin, so the pipe fills).
        std::thread::sleep(std::time::Duration::from_millis(600));
        let _ = client.drain_events();
        let start = std::time::Instant::now();
        client.cancel(); // no session yet — must be a prompt no-op
        drop(client); // kill-first: must unwedge everything
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "teardown wedged on a non-draining agent"
        );
    }
}

/// Reduces one incoming JSON-RPC line to zero or more `AcpEvent`s, advancing
/// the handshake as a side effect. Runs on the reader thread; only touches
/// `Shared`, never the UI.
fn reduce_line(shared: &Shared, line: &str) -> Vec<AcpEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };

    // Notifications: session/update carries the streamed turn content.
    if value.get("method").and_then(Value::as_str) == Some("session/update") {
        let Some(update) = value.pointer("/params/update") else {
            return vec![];
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                let text = update
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if text.is_empty() {
                    return vec![];
                }
                shared.turn_text.lock().unwrap().push_str(text);
                return vec![AcpEvent::Chunk(text.to_string())];
            }
            Some("agent_thought_chunk") => {
                let text = update
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if text.is_empty() {
                    return vec![];
                }
                // NOT appended to `turn_text`: thinking is not part of the
                // reply, and folding it in would feed the fence extractor
                // whatever code the model mused about mid-thought.
                return vec![AcpEvent::Thought(text.to_string())];
            }
            Some("tool_call") => {
                let title = update
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                return vec![AcpEvent::ToolCall(title)];
            }
            Some("plan") => {
                let Some(entries) = update.get("entries").and_then(Value::as_array) else {
                    return vec![AcpEvent::Tick];
                };
                let steps: Vec<PlanStep> = entries
                    .iter()
                    .filter_map(|e| {
                        let content = e.get("content").and_then(Value::as_str)?.trim();
                        (!content.is_empty()).then(|| PlanStep {
                            content: content.to_string(),
                            status: e
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or("pending")
                                .to_string(),
                        })
                    })
                    .collect();
                if steps.is_empty() {
                    return vec![AcpEvent::Tick];
                }
                return vec![AcpEvent::Plan(steps)];
            }
            // Everything else (tool_call_update, current_mode_update,
            // available_commands_update, user_message_chunk…): no content to
            // show, but still proof the agent is alive.
            _ => return vec![AcpEvent::Tick],
        }
    }

    // Requests FROM the agent (e.g. session/request_permission, fs/*) are not
    // expected from `octos acp` v1; refuse politely so the agent isn't left
    // waiting forever if one ever arrives. JSON-RPC ids may be numbers OR
    // strings — echo whichever the agent used.
    if value.get("method").is_some() {
        if let Some(id) = value.get("id").filter(|id| !id.is_null()) {
            let method = value.get("method").and_then(Value::as_str).unwrap_or("?");
            shared.write_line(
                json!({
                    "jsonrpc": "2.0",
                    "id": id.clone(),
                    "error": {"code": -32601, "message": format!("client does not support {method}")},
                })
                .to_string(),
            );
        }
        // Method + no id = an unrecognized notification; nothing to do.
        return vec![];
    }

    // Replies: routed by the single outstanding request id (ours are numeric).
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return vec![];
    };
    let pending = {
        let mut p = shared.pending.lock().unwrap();
        match *p {
            Some((pid, kind)) if pid == id => {
                *p = None;
                Some(kind)
            }
            _ => None,
        }
    };
    let Some(pending) = pending else { return vec![] };

    if let Some(err) = value.get("error") {
        // Forward the WHOLE error object, not just `message`. JSON-RPC's
        // `message` is the transport's own generic string — octos sends
        // "Internal error" — while the provider's actual sentence ("You've
        // reached your usage limit…") lives in `data`. Taking `message` alone
        // discarded it here, before anything downstream could read it, which is
        // why the UI could only ever say "internal error".
        // `pipeline::short_reason` digs the specific message back out.
        let msg = if err.get("data").is_some() {
            err.to_string()
        } else {
            err.get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown agent error")
                .to_string()
        };
        return vec![AcpEvent::Error(msg)];
    }

    match pending {
        Pending::Initialize => {
            // Handshake step 2, driven from here so the UI needn't care.
            shared.send_request(
                Pending::NewSession,
                "session/new",
                json!({"cwd": shared.workspace, "mcpServers": []}),
            );
            vec![]
        }
        Pending::NewSession => {
            let Some(sid) = value
                .pointer("/result/sessionId")
                .and_then(Value::as_str)
            else {
                return vec![AcpEvent::Error("session/new reply had no sessionId".into())];
            };
            *shared.session_id.lock().unwrap() = Some(sid.to_string());
            vec![AcpEvent::SessionReady]
        }
        Pending::Prompt => {
            let stop_reason = value
                .pointer("/result/stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string();
            let text = std::mem::take(&mut *shared.turn_text.lock().unwrap());
            vec![AcpEvent::TurnDone { stop_reason, text }]
        }
    }
}
