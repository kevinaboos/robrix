# Splash mini-apps in Robrix (the `a2app` feature)

AI-generated, sandboxed [Splash] mini-apps that run inside Robrix — in their own
isolated script VMs, with a mobile-OS-style permission system, and optional access
to Matrix itself (read/send messages in the room they're attached to).

Everything is gated behind the `a2app` cargo feature:

```bash
cargo run --features a2app
```

## What you get

- A **Mini Apps** button in the navigation bar (sparkle icon; can be hidden in
  App Settings) that opens the Mini Apps screen: create apps with AI, run them,
  and manage each one's permissions, versions, storage, and source.
- A **create bar**: describe an app ("a pomodoro timer") and an ACP agent
  (`octos acp` by default; Claude Code and any other ACP agent work too)
  writes it in the Splash dialect, validated with the real parser and
  auto-repaired for up to two turns.
- **Per-app isolation**: each app runs in its own Splash isolate with nothing
  by default: no filesystem beyond its private jail, no network, no host access.
  Capabilities are declared in the app's manifest, prompted at first use
  (Allow / Allow Once / Don't Allow / Not Now), revocable at any time, and a
  request-flooding app gets stopped and restricted.
- **Matrix services** for apps attached to a room: `matrix.room_info`,
  `matrix.read_messages`, `matrix.send_message`, and `matrix.profile`, each
  behind its own permission.
- **Sharing**: export any app as a `.splashapp` bundle (file + clipboard), or
  post it into a room with `/miniapp share <name>`, where it renders as a
  card other Robrix users can install and run.
- Two built-in demo apps: **Room Peek** (room info + recent messages + send)
  and **Roll Call** (dice roller that can post its roll to the room).

## Crates

| Crate | What it holds |
|---|---|
| `a2app/core` | Manifests, registry, `.splashapp` bundles, version history, the permission model + store, the host-service broker (platform services via `robius-*` crates), request-budget abuse control, persistence. |
| `a2app/agent` | The ACP client (JSON-RPC over stdio), the generation/repair pipeline, the Splash dialect guide, create-vs-modify intent classification, provider/model selection and key management, plus an optional in-process octos backend. |

Robrix-side UI and glue live in `src/a2app/` (feature-gated), with invisible
stub widgets in `src/a2app_dummy/` so non-`a2app` builds still resolve the
shared DSL names.

## Agent setup (once)

1. Install the octos CLI: `cargo install --git https://github.com/octos-org/octos octos-cli`
2. Give it an LLM provider, whichever is least effort:
   - the **AI Providers** page in the Mini Apps screen (pick a provider, paste a key),
   - a key already exported in your shell (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …),
   - `octos auth login -p <provider>`,
   - or a local [Ollama](https://ollama.com) with a code model pulled — detected
     automatically, no key needed.

Any other ACP agent works via `ROBRIX_AGENT_CMD`, e.g. a Claude Code
subscription: `ROBRIX_AGENT_CMD="claude-code-acp" cargo run --features a2app`
(stdio composes, so `ROBRIX_AGENT_CMD="ssh myserver octos acp"` works too).
`ROBRIX_AGENT_MODEL=<model>` is forwarded to that agent as `ANTHROPIC_MODEL`
(the Claude-based agents read it); use `opus` when testing generation.

## Extra cargo features

| Feature | Effect |
|---|---|
| `a2app-embedded-agent` | Link the octos agent **in-process** instead of spawning `octos acp` (required on iOS, where `exec()` is prohibited). |
| `a2app-persistent-guide` | Install the Splash dialect guide on the agent once, so per-turn prompts shrink to a pointer line. |
| `a2app-research` | Let the agent research with its tools (web search/fetch) before generating, baking found data into the app as constants. |

## Not included yet

- Splash resource limiting (CPU/memory/timer shares) — needs makepad#1189.
- Agent option knobs (model/effort/thinking controls); env vars
  (`ANTHROPIC_MODEL`, `CLAUDE_CODE_EFFORT_LEVEL`, `MAX_THINKING_TOKENS`) still
  seed the defaults.
- Live mini-apps embedded directly inside timeline items (shared apps render
  as install/run cards instead).

[Splash]: https://github.com/makepad/makepad
