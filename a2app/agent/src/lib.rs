//! AI app generation for Robrix's mini-apps. An agent writes Splash source;
//! the pipeline validates and packages it. The agent sits behind the
//! [`AgentTransport`] trait — the default backend spawns an external ACP
//! process (octos by default; `ssh host octos acp` works too, since stdio
//! composes), and the `embedded` cargo feature adds an in-process backend
//! that links the octos agent crates directly (no child process — the only
//! option on iOS, where exec() is prohibited).

pub mod acp_client;
pub mod intent;
#[cfg(feature = "embedded")]
mod octos_embedded;
pub mod pipeline;
pub mod prefs;
pub mod providers;
pub mod setup;
#[cfg(feature = "persistent-guide")]
pub mod skills;

use acp_client::{AcpClient, AcpEvent};

/// The Splash dialect guide — the entire "app-card memory" teaching the agent
/// THIS repo's dialect. Inlined into prompts by default; `persistent-guide`
/// instead injects it persistently on the agent side and sends slim prompts.
pub(crate) const SPLASH_GUIDE: &str = include_str!("splash_guide.md");

/// What the generation pipeline needs from an agent, regardless of where it
/// runs. Implementations push `AcpEvent`s to an internal queue from their own
/// threads (waking the UI via `SignalToUI`); the UI thread drains.
pub trait AgentTransport: Send {
    /// Drains every queued event. Called from the UI thread's event handler.
    fn drain_events(&mut self) -> Vec<AcpEvent>;
    /// Sends the user's (or a repair) prompt on the current session.
    fn send_prompt(&mut self, text: &str);
    /// Asks the agent to abandon the in-flight turn.
    fn cancel(&mut self);
    /// Human-readable description of the backend, for error messages.
    fn desc(&self) -> &str;
}

impl AgentTransport for AcpClient {
    fn drain_events(&mut self) -> Vec<AcpEvent> {
        AcpClient::drain_events(self)
    }
    fn send_prompt(&mut self, text: &str) {
        AcpClient::send_prompt(self, text)
    }
    fn cancel(&mut self) {
        AcpClient::cancel(self)
    }
    fn desc(&self) -> &str {
        self.cmd_desc()
    }
}

/// Well-known provider API-key env vars → the octos provider they imply, in
/// preference order. Lets the app work with ZERO octos setup: a key
/// already exported in the shell is enough — the provider name is inferred
/// here and passed as a flag (external agent) or used directly (in-process);
/// the key itself is read from the environment by octos's provider, never
/// stored or forwarded by us.
pub(crate) const PROVIDER_KEY_ENVS: &[(&str, &str)] = &[
    ("ANTHROPIC_API_KEY", "anthropic"),
    ("OPENAI_API_KEY", "openai"),
    ("GEMINI_API_KEY", "gemini"),
    ("OPENROUTER_API_KEY", "openrouter"),
    ("DEEPSEEK_API_KEY", "deepseek"),
    ("GROQ_API_KEY", "groq"),
    ("MOONSHOT_API_KEY", "moonshot"),
    // Kimi's subscription "Coding Plan" is a separate octos provider on a
    // separate host (api.kimi.com/coding/v1); its keys are rejected by the
    // regular Moonshot endpoints, so it must never collapse into `moonshot`.
    ("KIMI_CODING_API_KEY", "moonshot-coding"),
];

/// Serializes tests that point `OCTOS_CONFIG_DIR` at a scratch directory.
/// `set_var` is process-global, so without this the parallel test runner has
/// one test's config dir swapped out from under another mid-assertion.
#[cfg(test)]
pub(crate) static CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The env-var name octos expects a provider's key under.
pub(crate) fn key_env_for(provider: &str) -> String {
    PROVIDER_KEY_ENVS
        .iter()
        .find(|(_, p)| *p == provider)
        .map(|(var, _)| var.to_string())
        .unwrap_or_else(|| format!("{}_API_KEY", provider.to_uppercase().replace('-', "_")))
}

/// The provider implied by the environment, when no octos config chose one.
pub(crate) fn provider_from_env() -> Option<&'static str> {
    provider_from_lookup(|var| std::env::var(var).ok())
}

/// The provider implied by octos's auth store (`octos auth login` pastes a
/// key into `auth.json` without writing a config), so logged-in users need no
/// further setup either. Prefers anthropic, else the first stored provider.
pub(crate) fn provider_from_auth_store() -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("OCTOS_CONFIG_DIR") {
        candidates.push(std::path::PathBuf::from(dir).join("auth.json"));
    }
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        candidates.push(home.join(".config").join("octos").join("auth.json"));
        candidates.push(home.join(".octos").join("auth.json"));
    }
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(creds) = value.get("credentials").and_then(|c| c.as_object()) else {
            continue;
        };
        if creds.contains_key("anthropic") {
            return Some("anthropic".to_string());
        }
        if let Some(name) = creds.keys().next() {
            return Some(name.clone());
        }
    }
    None
}

fn provider_from_lookup(get: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    PROVIDER_KEY_ENVS
        .iter()
        .find(|(var, _)| get(var).is_some_and(|v| !v.is_empty()))
        .map(|(_, provider)| *provider)
}

/// Fully local, no-key path: if an Ollama server is running on the default
/// port, use it (octos's `ollama` provider needs no key). Returns the model
/// to use, picked from what's actually pulled — coder-ish models first, since
/// the job is writing Splash code. Std-only probe: a short TCP connect + a
/// minimal HTTP/1.0 GET (which forces a non-chunked, close-delimited reply),
/// bounded to ~0.7s worst case — and it only runs when every other detection
/// came up empty.
pub(crate) fn ollama_model() -> Option<String> {
    ollama_model_at(11434)
}

fn ollama_model_at(port: u16) -> Option<String> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream =
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .ok()?;
    stream
        .write_all(b"GET /api/tags HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .ok()?;
    let mut response = Vec::new();
    let _ = stream.take(1 << 20).read_to_end(&mut response);
    let response = String::from_utf8_lossy(&response);
    let body = response.split_once("\r\n\r\n").map(|(_, b)| b)?;
    pick_ollama_model(body)
}

/// Chooses the best generation model from an Ollama `/api/tags` body.
fn pick_ollama_model(tags_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(tags_json).ok()?;
    let names: Vec<String> = value
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    // Splash generation is a coding task; prefer code-tuned models.
    for hint in ["coder", "qwen", "deepseek", "codellama", "llama"] {
        if let Some(name) = names.iter().find(|n| n.to_lowercase().contains(hint)) {
            return Some(name.clone());
        }
    }
    names.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The install line is what the user pastes into a terminal, so it has to
    /// be one self-contained command that needs no clone and no `cd`.
    #[test]
    fn the_install_command_is_copy_pasteable() {
        assert!(!OCTOS_INSTALL_CMD.contains('\n'), "one line, or Copy hands over a script");
        assert!(!OCTOS_INSTALL_CMD.contains("git clone"), "cargo install takes the URL directly");
        assert!(OCTOS_INSTALL_CMD.starts_with("cargo install --git "));
    }

    /// Every blocker has to name the missing piece in the one-line hint, and
    /// the page copy must not tell the user to go where they already are.
    #[test]
    fn blockers_explain_themselves() {
        for b in [Blocker::NoProvider, Blocker::OctosMissing] {
            assert!(!b.headline().is_empty() && !b.title().is_empty() && !b.detail().is_empty());
            assert!(b.headline().contains("tap"), "the hint has to point somewhere: {b:?}");
            assert!(!b.title().contains("tap"), "the page IS there: {b:?}");
        }
        // The whole point: say the name of the program and how to get it.
        let missing = Blocker::OctosMissing;
        assert!(missing.headline().contains("octos"));
        assert!(missing.detail().contains("octos"));
        assert_eq!(missing.command(), Some(OCTOS_INSTALL_CMD));
        // Nothing to install for a missing key, so no button offering to.
        assert_eq!(Blocker::NoProvider.command(), None);
    }

    #[test]
    fn provider_detection_prefers_anthropic_and_skips_empty() {
        let get = |var: &str| match var {
            "ANTHROPIC_API_KEY" => Some("sk-ant-x".to_string()),
            "OPENAI_API_KEY" => Some("sk-x".to_string()),
            _ => None,
        };
        assert_eq!(provider_from_lookup(get), Some("anthropic"));

        // An exported-but-empty key must not win.
        let get = |var: &str| match var {
            "ANTHROPIC_API_KEY" => Some(String::new()),
            "GROQ_API_KEY" => Some("gsk-x".to_string()),
            _ => None,
        };
        assert_eq!(provider_from_lookup(get), Some("groq"));

        assert_eq!(provider_from_lookup(|_| None), None);
    }

    #[test]
    fn ollama_model_pick_prefers_coder_models() {
        let tags = r#"{"models":[
            {"name":"llama3.2:latest"},
            {"name":"qwen2.5-coder:14b"},
            {"name":"mistral:7b"}
        ]}"#;
        assert_eq!(pick_ollama_model(tags).as_deref(), Some("qwen2.5-coder:14b"));

        let tags = r#"{"models":[{"name":"mistral:7b"},{"name":"llama3.1:8b"}]}"#;
        assert_eq!(pick_ollama_model(tags).as_deref(), Some("llama3.1:8b"));

        // No hint match: first model wins; empty list: none.
        let tags = r#"{"models":[{"name":"gemma:2b"}]}"#;
        assert_eq!(pick_ollama_model(tags).as_deref(), Some("gemma:2b"));
        assert_eq!(pick_ollama_model(r#"{"models":[]}"#), None);
        assert_eq!(pick_ollama_model("not json"), None);
    }

    /// The probe against a real (fake-Ollama) listener, and against a closed
    /// port — both must resolve promptly.
    #[test]
    fn ollama_probe_reads_tags_and_fails_fast() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf); // the GET
                let body = r#"{"models":[{"name":"qwen2.5-coder:14b"}]}"#;
                let _ = write!(
                    sock,
                    "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{body}"
                );
            }
        });
        assert_eq!(ollama_model_at(port).as_deref(), Some("qwen2.5-coder:14b"));

        // Closed port: prompt None (localhost refuses immediately).
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port();
        drop(closed);
        let start = std::time::Instant::now();
        assert_eq!(ollama_model_at(closed_port), None);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}

/// Whether any octos config file exists (same resolution order octos uses).
/// When one does, the user's own setup wins and no auto-detection happens.
pub(crate) fn octos_config_exists() -> bool {
    octos_config_candidates().iter().any(|p| p.exists())
}

/// Where octos looks for `config.json`, most specific first.
pub(crate) fn octos_config_candidates() -> Vec<std::path::PathBuf> {
    // OCTOS_CONFIG_DIR is AUTHORITATIVE, not merely first: octos treats an
    // explicit config dir as its own context and never falls back to the home
    // paths from there, and neither may we. Returning the home paths as
    // fallbacks meant that pointing at a dir with no config.json yet — which
    // is exactly what a test fixture is — silently resolved to the real
    // ~/.octos/config.json and wrote the user's own file.
    if let Some(dir) = std::env::var_os("OCTOS_CONFIG_DIR") {
        return vec![std::path::PathBuf::from(dir).join("config.json")];
    }
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        candidates.push(home.join(".config").join("octos").join("config.json"));
        candidates.push(home.join(".octos").join("config.json"));
    }
    candidates
}

/// The provider octos will actually use, read from the config file octos will
/// actually read (first existing candidate — it stops at the first one too).
///
/// `Backend::detect` has to agree with that file rather than with the
/// environment: the setup modal writes a provider into config.json without
/// exporting anything, so a UI that fell back to "anthropic" would offer a
/// Claude model picker and then send the pick to a completely different
/// provider. `None` when no config exists or it names no provider — octos can
/// also infer one from a bare `model`, which we don't try to second-guess.
pub(crate) fn provider_from_octos_config() -> Option<String> {
    let path = octos_config_candidates().into_iter().find(|p| p.exists())?;
    let text = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    let provider = value.get("provider")?.as_str()?.trim();
    (!provider.is_empty()).then(|| provider.to_string())
}

/// Whether `name` is runnable as a bare command.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
    })
}

/// Providers whose endpoint speaks the Anthropic API, and the base URL to
/// point at. Anything here can be driven by `claude-code-acp` directly.
const ANTHROPIC_COMPATIBLE: &[(&str, &str)] =
    &[("moonshot-coding", "https://api.kimi.com/coding/")];

/// A way to run without octos installed.
///
/// octos is a Rust binary many people won't have, while `claude-code-acp` is
/// commonly already there — and it is an Anthropic-API client, so any provider
/// serving that API can drive it just by repointing its base URL. Kimi's
/// Coding Plan is exactly that. Without this, pasting a perfectly good
/// `sk-kimi-…` key into the setup field ends at "couldn't start `octos acp`",
/// which is true and useless.
///
/// Deliberately only when octos is genuinely absent: when it IS installed it
/// stays the backend, because it's the one that honours the model/effort picks.
/// Which provider the bridge would run, if the bridge is what would run.
///
/// Separate from [`anthropic_compatible_bridge`] so the UI can name the
/// backend without building a command line and, more importantly, without
/// putting the user's API key in scope just to draw a label.
pub fn bridged_provider() -> Option<String> {
    if on_path("octos") || !on_path("claude-code-acp") {
        return None;
    }
    ANTHROPIC_COMPATIBLE
        .iter()
        .find(|(id, _)| providers::key_for(id).is_some())
        .map(|(id, _)| id.to_string())
}

fn anthropic_compatible_bridge() -> Option<(String, Vec<(String, String)>)> {
    if on_path("octos") || !on_path("claude-code-acp") {
        return None;
    }
    let (_, base) = ANTHROPIC_COMPATIBLE
        .iter()
        .find(|(id, _)| providers::key_for(id).is_some())?;
    let key = ANTHROPIC_COMPATIBLE
        .iter()
        .find_map(|(id, _)| providers::key_for(id))?;
    Some((
        "claude-code-acp".to_string(),
        vec![
            ("ANTHROPIC_BASE_URL".to_string(), base.to_string()),
            // The adapter reads a token, not an API key: an OAuth-style
            // subscription token and a pasted provider key arrive the same way.
            ("ANTHROPIC_AUTH_TOKEN".to_string(), key),
        ],
    ))
}

/// The exact commands that install octos, kept in one place so the console,
/// the Providers page and the docs can't drift apart.
pub const OCTOS_INSTALL_CMD: &str =
    "cargo install --git https://github.com/octos-org/octos octos-cli";

/// Why a generation cannot start — worked out BEFORE anything is spawned.
///
/// The point is that "the thing that runs your key isn't on this machine" is
/// knowable up front. Finding out via a failed `exec` turns a one-line install
/// instruction into `No such file or directory (os error 2)`, which names
/// neither the missing program nor the fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blocker {
    /// Nothing to generate with: no key, no agent command, no local model.
    NoProvider,
    /// A provider is set up, but octos — the program that would use it — is
    /// not installed, and no other backend can stand in.
    OctosMissing,
}

impl Blocker {
    /// One line for the create UI, which has room for exactly that. It points
    /// at the page, because the hint has nowhere to put a fix.
    pub fn headline(&self) -> String {
        match self {
            Self::NoProvider => "No AI provider set up yet — tap to add one".to_string(),
            Self::OctosMissing => "octos isn't installed — tap to see how".to_string(),
        }
    }

    /// The same problem stated on the Providers page, where "tap to see how"
    /// would be telling the user to go where they already are.
    pub fn title(&self) -> String {
        match self {
            Self::NoProvider => "No AI provider set up yet".to_string(),
            Self::OctosMissing => "octos isn't installed".to_string(),
        }
    }

    /// The full explanation for the Providers page: what is missing, why it
    /// matters, and the command that fixes it.
    pub fn detail(&self) -> String {
        match self {
            Self::NoProvider => {
                "Pick a provider below and paste its API key to start making apps.".to_string()
            }
            Self::OctosMissing => "octos is the program that turns your API key into a \
                 running agent. It isn't installed, so a key saved here has nothing to \
                 run it.\n\nInstall it once, then restart Robrix:"
                .to_string(),
        }
    }

    /// The command that fixes this, if one does. Kept out of [`Self::detail`]
    /// so it can be set in a monospace line of its own — wrapped into prose at
    /// the body font, the URL broke across lines and read as a typo.
    pub fn command(&self) -> Option<&'static str> {
        match self {
            Self::NoProvider => None,
            Self::OctosMissing => Some(OCTOS_INSTALL_CMD),
        }
    }
}

/// Something the user should know that is NOT stopping them.
///
/// Separate from [`Blocker`] because it must not gate anything: reaching for
/// generation with a working bridge has to open the composer, not the setup
/// page. But "it works" is not the whole truth here — the run is going through
/// a different program than the one the key belongs to, which is visible (a
/// Claude Code completion sound on a Kimi generation) and unexplainable from
/// the UI unless the UI says so.
pub fn advisory() -> Option<(String, String, &'static str)> {
    let provider = bridged_provider()?;
    Some((
        format!("Running {} through Claude Code", providers::label_for(&provider)),
        "octos isn't installed, so Robrix is driving the `claude-code-acp` \
         adapter and pointing it at your provider's endpoint. Your key and your \
         provider are what answer — but Claude Code is the program running, which \
         is why you hear its completion sound.\n\n\
         Install octos to talk to the provider directly:"
            .to_string(),
        OCTOS_INSTALL_CMD,
    ))
}

/// What stands between the user and a generation right now, if anything.
///
/// Consulted before a run starts and by the Providers page, so both agree —
/// and so an unusable setup is reported when the user reaches for the prompt
/// rather than sixty seconds into a run that was never going to work.
pub fn blocker() -> Option<Blocker> {
    // An explicit agent command is the user's own choice of backend; octos is
    // not consulted at all in that case (see `start_backend`).
    if providers::agent_command().is_some() {
        return None;
    }
    // Compiled in — there is no binary to be missing.
    #[cfg(feature = "embedded")]
    {
        return (!providers::any_configured()).then_some(Blocker::NoProvider);
    }
    #[cfg(not(feature = "embedded"))]
    {
        // A key that the bridge can run is a complete setup on its own.
        if anthropic_compatible_bridge().is_some() {
            return None;
        }
        if !on_path("octos") {
            return Some(Blocker::OctosMissing);
        }
        (!providers::any_configured()).then_some(Blocker::NoProvider)
    }
}

/// Picks and starts the agent backend for one generation.
///
/// Selection: `ROBRIX_AGENT_CMD` always wins and always means "spawn
/// this external ACP command" — the explicit override, and how the offline
/// test agent (`fake_acp`) is injected even in `embedded` builds. With no
/// override, the in-process backend is used when compiled in, else the
/// Anthropic-compatible bridge, else an external `octos acp` — with the
/// provider auto-detected from the environment when octos was never
/// configured, so an exported API key is the ONLY setup needed.
pub fn start_backend(
    workspace: &std::path::Path,
    prefs: &prefs::AgentPrefs,
) -> Result<Box<dyn AgentTransport>, String> {
    // Refuse before spawning rather than translating an errno afterwards: the
    // check knows WHICH program is missing, so it can name it and the install.
    if let Some(blocked) = blocker() {
        return Err(blocked.headline());
    }
    let backend = prefs::Backend::detect();
    let env = backend.env(prefs);
    let extra = backend.args(prefs);
    // octos takes its reasoning effort from its own config file rather than a
    // flag, so delivering that pick means editing that file (see
    // prefs::apply_octos_effort).
    if let prefs::Backend::Octos { .. } = backend {
        if let Err(e) = prefs::apply_octos_effort(prefs.effort.as_deref()) {
            makepad_widgets::error!("couldn't set octos reasoning effort: {e}");
        }
    }
    if let Ok(cmd) = std::env::var("ROBRIX_AGENT_CMD") {
        return Ok(Box::new(AcpClient::spawn(&cmd, workspace, &env, &extra)?));
    }
    #[cfg(feature = "embedded")]
    {
        return Ok(Box::new(octos_embedded::EmbeddedOctos::start(workspace, prefs)?));
    }
    #[cfg(not(feature = "embedded"))]
    {
        // Before falling back to a command that may not exist at all.
        if let Some((cmd, bridge_env)) = anthropic_compatible_bridge() {
            // The bridge gets its own env only: `backend.env(prefs)` carries
            // octos's knobs, and the model/effort names in it mean nothing to
            // another provider's endpoint.
            return Ok(Box::new(AcpClient::spawn(&cmd, workspace, &bridge_env, &[])?));
        }
        Ok(Box::new(AcpClient::spawn(&octos_acp_command(prefs), workspace, &env, &extra)?))
    }
}

/// The `octos acp` command line a run would be spawned with.
///
/// Shared by `start_backend` and by `runtime()`, which is what the Providers
/// page reports — two copies of this would drift, and the page would then be
/// confidently wrong about the thing it exists to explain.
#[cfg_attr(feature = "embedded", allow(dead_code))]
fn octos_acp_command(prefs: &prefs::AgentPrefs) -> String {
    // A pick made in the Providers page this session overrides the config
    // without rewriting it, so it has to be passed on the command line —
    // octos would otherwise read the saved default and quietly ignore it.
    if let Some(provider) = providers::session_provider() {
        format!("octos acp --provider {provider}")
    } else if !octos_config_exists() {
        if let Some(provider) =
            provider_from_env().map(str::to_string).or_else(provider_from_auth_store)
        {
            format!("octos acp --provider {provider}")
        } else if let Some(model) = ollama_model().filter(|_| prefs.model.is_none()) {
            // Fully local fallback: a running Ollama needs no key at all.
            format!("octos acp --provider ollama --model {model}")
        } else {
            "octos acp".to_string() // fails with octos's own setup hint
        }
    } else {
        "octos acp".to_string()
    }
}

/// How a generation will actually be executed — what the Providers page shows.
///
/// Deliberately about the PLUMBING, not the provider: the pane already names
/// which service answers, and knowing that a key is configured tells you
/// nothing about whether the agent is a child process, this process, or some
/// binary the environment pointed us at.
#[derive(Debug, Clone, PartialEq)]
pub enum Runtime {
    /// octos compiled in, running on a thread of this process — no child.
    Embedded,
    /// A child process we spawn, and the exact command line.
    Child(String),
    /// A child process the USER chose via `ROBRIX_AGENT_CMD`.
    Override(String),
}

impl Runtime {
    /// One line for the page. Says where the agent runs first, because that is
    /// the part nothing else on screen reveals.
    pub fn summary(&self) -> String {
        match self {
            Self::Embedded => "Runs inside this app — octos is compiled in, no child process".into(),
            Self::Child(cmd) => format!("Runs as a child process — `{cmd}`"),
            Self::Override(cmd) => {
                format!("Runs as a child process — `{cmd}` (from ROBRIX_AGENT_CMD)")
            }
        }
    }
}

/// Worked out exactly the way `start_backend` decides, in the same order.
pub fn runtime(prefs: &prefs::AgentPrefs) -> Runtime {
    if let Some(cmd) = providers::agent_command() {
        return Runtime::Override(cmd);
    }
    #[cfg(feature = "embedded")]
    {
        let _ = prefs;
        Runtime::Embedded
    }
    #[cfg(not(feature = "embedded"))]
    {
        if let Some((cmd, _)) = anthropic_compatible_bridge() {
            return Runtime::Child(cmd);
        }
        Runtime::Child(octos_acp_command(prefs))
    }
}
