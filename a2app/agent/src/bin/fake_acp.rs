//! A fake ACP agent for offline tests of the create-app pipeline.
//!
//! Speaks just enough newline-delimited JSON-RPC to stand in for `octos acp`:
//! `initialize` → `session/new` → `session/prompt` with streamed
//! `session/update` chunks and a final `stopReason` reply. Deterministic and
//! LLM-free; the scenario is chosen by what the USER'S REQUEST contains —
//! the tail after the prompt's "User request:" / "User's change request:"
//! marker, never the whole prompt. A refine prompt embeds the app's current
//! source, so scanning everything let an app's own comments hijack the
//! scenario (a mini-app that mentioned refusing a request made every refine
//! come back as a refusal):
//!
//! - contains `"refuse"` → replies with `stopReason: "refusal"`.
//! - contains `"broken"` → first prompt streams an invalid Splash script (the
//!   validator must reject it); the repair prompt (recognized by the phrase
//!   "failed to compile") then streams a valid one.
//! - contains `"slow"` → publishes a plan, streams thinking, fires a burst of
//!   tool calls, then idles (the in-flight console state).
//! - contains `"hopeless"` → streams an invalid script EVERY turn, so the
//!   repair budget runs out and the generation fails (the case Retry
//!   exists for).
//! - anything else → streams a valid pomodoro-ish app, split across two
//!   chunks mid-fence to exercise accumulation.
//!
//! Point the app at it with
//! `ROBRIX_AGENT_CMD=$PWD/target/debug/fake_acp`.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// The user's own words: everything after the last request marker the prompt
/// builders emit. Falls back to the whole prompt when neither appears (the
/// repair prompt, which carries no user text and is matched separately).
fn user_request(prompt: &str) -> &str {
    for marker in ["User's change request:", "User request:"] {
        if let Some(at) = prompt.rfind(marker) {
            return &prompt[at + marker.len()..];
        }
    }
    prompt
}

const GOOD_APP: &str = "// name: Pomodoro\n// icon: 🍅\n// tint: #E84D3D\n\
let secs = 1500\n\
fn show(){ ui.time.set_text(\"\" + secs) }\n\
View{\n\
    width: Fill height: Fit flow: Down spacing: 12 padding: 16\n\
    align: Align{x: 0.5}\n\
    glass.H1{text: \"Pomodoro\"}\n\
    time := Label{text: \"1500\"}\n\
    glass.GlassButton{text: \"Tick\" width: 90 height: 44 on_click: || { secs -= 1 show() }}\n\
}";

const BROKEN_APP: &str = "// name: Pomodoro\n// icon: 🍅\n// tint: #E84D3D\n\
DefinitelyNotAWidget{ &&& this does not parse }";

/// What a refine turn returns: the good app with a renamed header + title, so
/// tests can watch the installed app's identity change in place.
const REFINED_APP: &str = "// name: Pomodoro Pro\n// icon: 🍅\n// tint: #E84D3D\n\
// permissions: files\n\
// why-files: Saves your session log where you choose.\n\
let secs = 1500\n\
fn show(){ ui.time.set_text(\"\" + secs) }\n\
View{\n\
    width: Fill height: Fit flow: Down spacing: 12 padding: 16\n\
    align: Align{x: 0.5}\n\
    glass.H1{text: \"Pomodoro Pro\"}\n\
    time := Label{text: \"1500\"}\n\
    glass.GlassButton{text: \"Tick\" width: 90 height: 44 on_click: || { secs -= 1 show() }}\n\
    glass.GlassButton{text: \"Reset\" width: 90 height: 44 on_click: || { secs = 1500 show() }}\n\
}";

fn send(out: &mut impl Write, v: Value) {
    let mut line = v.to_string();
    line.push('\n');
    out.write_all(line.as_bytes()).unwrap();
    out.flush().unwrap();
}

fn chunk(out: &mut impl Write, session_id: &str, text: &str) {
    send(
        out,
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": text},
                },
            },
        }),
    );
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let session_id = "fake-session-1";
    // Sticky per-session: once a refine prompt is seen, its REPAIR turns must
    // also answer with the refined identity (a real agent keeps the session's
    // context; answering a refine-repair with the create app would silently
    // pass the wrong manifest through the update path).
    let mut refine_session = false;
    let mut hopeless_session = false;

    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => send(
                &mut stdout,
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"protocolVersion": 1, "agentCapabilities": {}},
                }),
            ),
            "session/new" => send(
                &mut stdout,
                json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"sessionId": session_id},
                }),
            ),
            "session/prompt" => {
                let text = msg
                    .pointer("/params/prompt/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // Scenario keywords come from the user's words only; `text`
                // stays available for structural checks on the whole prompt.
                let ask = user_request(text);
                if ask.contains("refuse") {
                    send(
                        &mut stdout,
                        json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": {"stopReason": "refusal"},
                        }),
                    );
                    continue;
                }
                // "slow" requests idle first, giving UI tests a window to
                // interact with the in-flight state (the console, Stop). The
                // burst of tool calls first fills the console well past what
                // fits, so tests can check it keeps the whole run rather than
                // the last few lines.
                if ask.contains("slow") {
                    // A plan and some thinking first — the shapes a real
                    // reasoning agent emits during the long opening stretch,
                    // and the ones the console has to render.
                    send(
                        &mut stdout,
                        json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "plan",
                                    "entries": [
                                        {"content": "Read the Splash guide", "status": "in_progress", "priority": "medium"},
                                        {"content": "Write the app", "status": "pending", "priority": "medium"},
                                    ],
                                },
                            },
                        }),
                    );
                    for part in ["Let me think about ", "the timer layout."] {
                        send(
                            &mut stdout,
                            json!({
                                "jsonrpc": "2.0",
                                "method": "session/update",
                                "params": {
                                    "sessionId": session_id,
                                    "update": {
                                        "sessionUpdate": "agent_thought_chunk",
                                        "content": {"type": "text", "text": part},
                                    },
                                },
                            }),
                        );
                    }
                    for i in 1 ..= 20 {
                        send(
                            &mut stdout,
                            json!({
                                "jsonrpc": "2.0",
                                "method": "session/update",
                                "params": {
                                    "sessionId": session_id,
                                    "update": {
                                        "sessionUpdate": "tool_call",
                                        "toolCallId": format!("call-{i}"),
                                        "title": format!("Read splash_guide.md ({i})"),
                                        "status": "completed",
                                    },
                                },
                            }),
                        );
                    }
                    // Long enough for a test to walk the whole console UI
                    // while the run is genuinely in flight — each assertion
                    // costs a widget-snapshot round trip, and those get slow
                    // under a loaded full-suite run. Tests end such a run with
                    // Stop rather than waiting this out.
                    std::thread::sleep(std::time::Duration::from_secs(120));
                }

                // Before answering, fire an agent-originated request with a
                // STRING id (legal JSON-RPC) — the client must refuse it and
                // carry on; a client that drops it would leave a real agent
                // hanging. The reply itself is read below and discarded.
                send(
                    &mut stdout,
                    json!({
                        "jsonrpc": "2.0", "id": "agent-req-1",
                        "method": "session/request_permission",
                        "params": {"sessionId": session_id},
                    }),
                );

                // The repair prompt is recognizable by the pipeline's own
                // copy; a refine prompt by its current-source section.
                let is_repair = text.contains("failed to compile");
                if text.contains("The app's current source") {
                    refine_session = true;
                }
                // "hopeless" never repairs: the repair budget runs out and
                // the generation fails for real. The marker only survives in
                // the FIRST prompt, so the session remembers it.
                if ask.contains("hopeless") {
                    hopeless_session = true;
                }
                // A hopeless run has to be *observably* in flight: it's the
                // Retry test's subject, and a run that starts and fails
                // between two snapshot polls can't be told from one that never
                // started. Three turns at this pace keep the console on screen
                // for a few seconds, which survives a loaded full-suite run.
                if hopeless_session {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                }
                let source = if hopeless_session {
                    BROKEN_APP
                } else if refine_session {
                    REFINED_APP
                } else if ask.contains("broken") && !is_repair {
                    BROKEN_APP
                } else {
                    GOOD_APP
                };
                let reply = format!("```splash\n{source}\n```");
                // Split mid-reply so the client has to accumulate chunks.
                let mid = reply.len() / 2;
                let mid = (mid..reply.len())
                    .find(|&i| reply.is_char_boundary(i))
                    .unwrap_or(reply.len());
                chunk(&mut stdout, session_id, &reply[..mid]);
                chunk(&mut stdout, session_id, &reply[mid..]);
                send(
                    &mut stdout,
                    json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"stopReason": "end_turn"},
                    }),
                );
            }
            // No method: it's the client's RESPONSE to our agent-originated
            // request above — consume silently (replying to a reply is a
            // JSON-RPC violation).
            "" => {}
            // Notifications (session/cancel) and unknown requests: unknown
            // *requests* still get a reply so nothing hangs.
            _ => {
                if !id.is_null() {
                    send(
                        &mut stdout,
                        json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": {"code": -32601, "message": "not supported"},
                        }),
                    );
                }
            }
        }
    }
}
