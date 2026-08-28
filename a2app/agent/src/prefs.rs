//! What to ask the agent for — and, crucially, *which* knobs even exist.
//!
//! There is no cross-provider standard for this: Claude Code takes model,
//! effort and a thinking budget through environment variables it reads itself;
//! octos takes a model as a CLI flag and exposes nothing else we can drive;
//! a foreign ACP agent (`ROBRIX_AGENT_CMD`) is a black box. So the UI
//! doesn't show one fixed set of controls — it asks the ACTIVE backend what it
//! supports and renders that. A knob no one can deliver is worse than a
//! missing one: it looks like it works.
//!
//! Adding a provider's knob is a table entry in `Backend::knobs` plus a rule in
//! `env`/`args` for how it's delivered. Please don't add one without checking
//! the delivery mechanism actually exists.

use serde::{Deserialize, Serialize};

/// Anthropic model ids offered wherever Claude is the provider. A model the
/// CLI doesn't know is passed through untouched (the agent errors, not us), so
/// this list can lag a release without blocking anyone.
pub const CLAUDE_MODELS: &[(&str, &str)] = &[
    ("Opus 5", "claude-opus-5"),
    ("Sonnet 5", "claude-sonnet-5"),
    ("Haiku 4.5", "claude-haiku-4-5"),
];

/// Effort levels *this* Claude Code build accepts, cheapest first. NOT the
/// full API ladder — the API also has `xhigh` between `high` and `max`, but the
/// bundled CLI validates against exactly this list and drops anything else.
pub const CLAUDE_EFFORTS: &[(&str, &str)] =
    &[("Low", "low"), ("Medium", "medium"), ("High", "high"), ("Max", "max")];

/// `xhigh` sits between `high` and `max` in the API's ladder, but whether it
/// works depends on the Claude Code build the ACP adapter runs: an unknown
/// level is SILENTLY dropped (falling back to the default), so offering it
/// blindly would be a control that quietly does nothing. Probed, not assumed.
pub const CLAUDE_EFFORT_XHIGH: (&str, &str) = ("X-High", "xhigh");

/// The effort ladder for the Claude Code runtime actually installed. Cached:
/// the probe reads a file off disk.
pub fn claude_efforts() -> &'static [(&'static str, &'static str)] {
    static LADDER: std::sync::OnceLock<Vec<(&'static str, &'static str)>> =
        std::sync::OnceLock::new();
    LADDER
        .get_or_init(|| {
            let mut ladder = CLAUDE_EFFORTS.to_vec();
            if runtime_supports_xhigh() {
                // Between `high` and `max`, matching the API's own ordering.
                ladder.insert(3, CLAUDE_EFFORT_XHIGH);
            }
            ladder
        })
        .as_slice()
}

/// Whether the Claude Code that will actually run knows the `xhigh` level.
///
/// Read from the runtime rather than inferred from a version number, because
/// the binary in play isn't necessarily the `claude` on PATH: the adapter runs
/// the CLI bundled inside `@anthropic-ai/claude-agent-sdk` unless
/// `CLAUDE_CODE_EXECUTABLE` overrides it. Fails CLOSED — if nothing is found,
/// the level isn't offered.
fn runtime_supports_xhigh() -> bool {
    claude_runtime_paths()
        .into_iter()
        .filter(|p| p.exists())
        .any(|p| file_mentions(&p, "xhigh"))
}

/// Files that would carry the runtime's effort ladder, best candidate first.
fn claude_runtime_paths() -> Vec<std::path::PathBuf> {
    // An explicit override wins — it's the binary the adapter will spawn.
    if let Some(explicit) = std::env::var_os("CLAUDE_CODE_EXECUTABLE") {
        return vec![std::path::PathBuf::from(explicit)];
    }
    // Otherwise the SDK bundled beside the adapter on PATH. Layouts vary by
    // installer, so try the common ones and give up quietly.
    let Some(adapter) = which_on_path("claude-code-acp") else {
        return Vec::new();
    };
    let adapter = std::fs::canonicalize(&adapter).unwrap_or(adapter);
    let mut paths = Vec::new();
    for base in adapter.ancestors().skip(1).take(4) {
        let sdk = base.join("node_modules/@anthropic-ai/claude-agent-sdk");
        paths.push(sdk.join("sdk.d.ts"));
        paths.push(sdk.join("cli.js"));
    }
    paths
}

fn which_on_path(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

/// Whether `needle` appears in `path`, streamed so a multi-megabyte bundle
/// doesn't have to be held in memory.
fn file_mentions(path: &std::path::Path, needle: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut window = Vec::new();
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        match file.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(n) => {
                // Keep a needle-sized tail so a match spanning chunks is found.
                window.extend_from_slice(&chunk[..n]);
                if window
                    .windows(needle.len())
                    .any(|w| w == needle.as_bytes())
                {
                    return true;
                }
                let keep = needle.len().saturating_sub(1);
                if window.len() > keep {
                    window.drain(..window.len() - keep);
                }
            }
        }
    }
}

/// octos's `ReasoningEffort`, serialized snake_case into its config. Same four
/// names as Claude Code's ladder by coincidence, not by contract — they're
/// different enums in different tools, so they get their own table.
pub const OCTOS_EFFORTS: &[(&str, &str)] =
    &[("Low", "low"), ("Medium", "medium"), ("High", "high"), ("Max", "max")];

/// Kimi's Coding Plan ladder (`k3`, `kimi-for-coding*`). octos emits
/// `reasoning_effort` for these as **low | high | max** — there is no medium
/// tier, and it clamps a Medium pick UP to "high" (octos-llm `openai.rs`,
/// `ReasoningStyle::EffortLowHighMax`). Offering Medium would be a control
/// with two settings that do the same thing.
pub const KIMI_EFFORTS: &[(&str, &str)] = &[("Low", "low"), ("High", "high"), ("Max", "max")];

/// Thinking budget used when extended thinking is switched on. Generous enough
/// to matter on a hard app, small enough not to dominate a simple one.
pub const THINKING_TOKENS: u32 = 16000;

/// Which knob a control drives. The set is deliberately small and shared:
/// providers differ in *whether* they expose one and in the values it takes,
/// not in the underlying concept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobId {
    Model,
    Effort,
    Thinking,
}

impl KnobId {
    /// Which row of the UI's options this knob owns. FIXED per knob, not by
    /// position in the backend's list: a segmented control takes its labels
    /// from the DSL, so row 1 is always the effort ladder. A backend that
    /// offers effort but no model (octos + openai) must still land in row 1,
    /// or it would render the effort setting with the model's labels.
    pub fn row(self) -> usize {
        match self {
            Self::Model => 0,
            Self::Effort => 1,
            Self::Thinking => 2,
        }
    }
}

/// One control the UI should show: a label and the values it offers. Index 0
/// is always "Default" — meaning *don't say*, leave the choice to the agent's
/// own configuration, which is not the same as picking its default value.
#[derive(Clone, Debug)]
pub struct Knob {
    pub id: KnobId,
    pub label: &'static str,
    /// (what the user sees, what gets sent). The sent value is empty for
    /// "Default".
    pub options: Vec<(String, String)>,
}

impl Knob {
    fn new(id: KnobId, label: &'static str, options: &[(&str, &str)]) -> Self {
        let mut list = vec![("Default".to_string(), String::new())];
        list.extend(options.iter().map(|(d, v)| (d.to_string(), v.to_string())));
        Self { id, label, options: list }
    }

    /// The index of `value` among the options, or 0 (Default) if it isn't one
    /// of them — which is what keeps a model chosen in another build from
    /// being silently erased when this build doesn't list it.
    pub fn index_of(&self, value: Option<&str>) -> usize {
        value
            .and_then(|v| self.options.iter().position(|(_, opt)| opt == v))
            .unwrap_or(0)
    }

    pub fn value_at(&self, index: usize) -> Option<String> {
        self.options.get(index).map(|(_, v)| v.clone()).filter(|v| !v.is_empty())
    }
}

/// The agent actually in play, which decides what can be configured. Mirrors
/// the decision `start_backend` makes — keep the two in step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    /// `claude-code-acp` — a Claude subscription, driven by env vars the
    /// Claude Code CLI reads: `ANTHROPIC_MODEL`, `CLAUDE_CODE_EFFORT_LEVEL`,
    /// `MAX_THINKING_TOKENS`.
    ClaudeCode,
    /// `octos acp --provider <p>`. Takes `--model`; nothing else is reachable
    /// from here today (no reasoning-effort flag), whatever the underlying
    /// provider's API itself supports.
    Octos { provider: String },
    /// A provider whose endpoint speaks the Anthropic API, driven through the
    /// `claude-code-acp` adapter because octos isn't installed. No knobs: the
    /// adapter's env vars name Claude models and Claude effort levels, which
    /// mean nothing to the provider actually serving the request.
    Bridged { provider: String },
    /// Some other ACP agent via `ROBRIX_AGENT_CMD` — no knobs, because
    /// we have no idea what it reads.
    Custom,
}

impl Backend {
    /// What `start_backend` will actually launch, worked out the same way.
    pub fn detect() -> Self {
        if let Ok(cmd) = std::env::var("ROBRIX_AGENT_CMD") {
            return if cmd.contains("claude-code-acp") { Self::ClaudeCode } else { Self::Custom };
        }
        // A pick made in the Providers page this session, before the saved
        // config: it is what `start_backend` will pass on the command line,
        // so it is what the UI must name and take its knobs from.
        if let Some(provider) = super::providers::session_provider() {
            return Self::Octos { provider };
        }
        // With octos absent, a key whose endpoint speaks the Anthropic API is
        // run THROUGH the Claude Code adapter. Reporting that as `Octos` would
        // have named a program that isn't installed and offered octos's knobs,
        // which the adapter doesn't read — see `anthropic_compatible_bridge`.
        if let Some(provider) = super::bridged_provider() {
            return Self::Bridged { provider };
        }
        // The CONFIG first: it's what octos obeys, and the setup modal writes
        // one without touching the environment. Env next (an exported key with
        // no config is the other supported setup), then the historical default.
        let provider = super::provider_from_octos_config()
            .or_else(|| super::provider_from_env().map(str::to_string))
            .unwrap_or_else(|| "anthropic".to_string());
        Self::Octos { provider }
    }

    /// Human-readable name for the UI's hint line.
    pub fn display_name(&self) -> String {
        match self {
            Self::ClaudeCode => "Claude Code".to_string(),
            // The PROVIDER is what the user cares about — "octos ·
            // moonshot-coding" told them the plumbing and not the model, which
            // reads as though no key of theirs is in play at all.
            Self::Octos { provider } => {
                format!("{} via octos", super::providers::label_for(provider))
            }
            // Names BOTH halves, because both matter and each alone misleads:
            // the model answering is the provider's, but the program running
            // is Claude Code — which is why its completion sound plays.
            Self::Bridged { provider } => {
                format!("{} via Claude Code", super::providers::label_for(provider))
            }
            Self::Custom => "custom agent".to_string(),
        }
    }

    /// The controls this backend can actually honour.
    pub fn knobs(&self) -> Vec<Knob> {
        match self {
            // All three are env vars the CLI reads itself — verified against
            // the bundled @anthropic-ai/claude-agent-sdk.
            Self::ClaudeCode => vec![
                Knob::new(KnobId::Model, "Model", CLAUDE_MODELS),
                Knob::new(KnobId::Effort, "Effort", claude_efforts()),
                Knob::new(KnobId::Thinking, "Thinking", &[("On", "on"), ("Off", "off")]),
            ],
            // Effort for every provider: octos applies `gateway.reasoning_effort`
            // to each turn and maps it per provider (OpenAI/Grok get
            // `reasoning_effort`, Gemini a thinking budget, Anthropic a thinking
            // block); models without a reasoning style ignore it. Model only
            // where we can name models honestly — inventing ids for a provider
            // we can't enumerate would just error at generation time, and
            // Ollama already auto-picks the best installed model.
            Self::Octos { provider } => match provider.as_str() {
                // Kimi Coding Plan. Effort IS delivered (k3 takes
                // low|high|max, thinking always on) but has no medium rung.
                "moonshot-coding" | "kimi-coding" => {
                    vec![Knob::new(KnobId::Effort, "Effort", KIMI_EFFORTS)]
                }
                // Moonshot platform (kimi-k2.x). octos resolves these to
                // `ReasoningStyle::None` and emits NOTHING for effort, so
                // there is no honest control to show — the model's own
                // default is the whole story.
                "moonshot" | "kimi" => Vec::new(),
                "anthropic" => vec![
                    Knob::new(KnobId::Model, "Model", CLAUDE_MODELS),
                    Knob::new(KnobId::Effort, "Effort", OCTOS_EFFORTS),
                ],
                _ => vec![Knob::new(KnobId::Effort, "Effort", OCTOS_EFFORTS)],
            },
            // Nothing honest to offer: the adapter's knobs are Claude model
            // ids and Claude effort levels, and the endpoint serving the
            // request is not Claude.
            Self::Bridged { .. } => Vec::new(),
            Self::Custom => Vec::new(),
        }
    }

    /// The top rung of this backend's effort ladder, or `None` when it has no
    /// effort knob. Used by the create UI's Retry: after a generation fails,
    /// the useful second try is the hardest one available, not the next rung
    /// up — and "next rung" isn't even well defined from Default, which means
    /// *the agent's own setting* rather than a position on the ladder.
    pub fn top_effort(&self) -> Option<String> {
        let knob = self.knobs().into_iter().find(|k| k.id == KnobId::Effort)?;
        knob.options
            .last()
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    }

    /// Environment to layer onto the agent process.
    pub fn env(&self, prefs: &AgentPrefs) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if !matches!(self, Self::ClaudeCode) {
            return env;
        }
        if let Some(model) = &prefs.model {
            env.push(("ANTHROPIC_MODEL".to_string(), model.clone()));
        }
        if let Some(effort) = &prefs.effort {
            env.push(("CLAUDE_CODE_EFFORT_LEVEL".to_string(), effort.clone()));
        }
        if let Some(thinking) = &prefs.thinking {
            // A budget of 0 is how the CLI spells "thinking off".
            let budget = if thinking == "on" { THINKING_TOKENS } else { 0 };
            env.push(("MAX_THINKING_TOKENS".to_string(), budget.to_string()));
        }
        env
    }

    /// Extra CLI arguments for the agent command (octos takes its model here
    /// rather than through the environment).
    pub fn args(&self, prefs: &AgentPrefs) -> Vec<String> {
        match self.model_override(prefs) {
            Some(model) => vec!["--model".to_string(), model],
            None => Vec::new(),
        }
    }

    /// The saved model pick, but only when THIS backend actually offers a
    /// control for it.
    ///
    /// The pick is persisted globally, so a model chosen while one provider
    /// was configured is still in the file when another one is. Passing it on
    /// regardless is a baffling failure, not a useful passthrough: sending
    /// `claude-opus-5` to the Kimi coding plan makes octos stop recognising
    /// the model as k3, so it stops suppressing `temperature` for it, and the
    /// endpoint 400s with "invalid temperature: only 1 is allowed for this
    /// model" — a message about a parameter nobody set, naming neither the
    /// model nor the provider.
    ///
    /// Shared by the child process (which sends `--model`) and the embedded
    /// agent (which sets `AcpCommand.model`). They had separate copies of this
    /// rule for exactly one commit, which is how the bug above shipped.
    pub fn model_override(&self, prefs: &AgentPrefs) -> Option<String> {
        let model = prefs.model.as_ref()?;
        if !matches!(self, Self::Octos { .. }) {
            return None;
        }
        // A backend with no Model knob runs its provider's default.
        if !self.knobs().iter().any(|k| k.id == KnobId::Model) {
            return None;
        }
        Some(model.clone())
    }
}

/// Writes `gateway.reasoning_effort` into octos's own config, because that is
/// the only way in: `octos acp` reads the value from there (its `--effort` flag
/// exists on `octos chat`, not `acp`), and octos itself treats it as a
/// persistent setting applied to every turn rather than a per-request
/// parameter. So this control genuinely edits that setting.
///
/// Only the one field is touched — everything else in the file is preserved —
/// and nothing is written when the value already matches or when there's no
/// config to edit and nothing to say.
pub fn apply_octos_effort(effort: Option<&str>) -> Result<(), String> {
    let path = super::octos_config_candidates()
        .into_iter()
        .find(|p| p.exists())
        .or_else(|| {
            // Nothing to patch yet: only create a config if there's a value
            // worth recording.
            effort.is_some().then(|| super::octos_config_candidates().remove(0))
        });
    let Some(path) = path else {
        return Ok(());
    };

    let mut config = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err(format!("{} isn't a JSON object", path.display()));
    }

    let current = config
        .get("gateway")
        .and_then(|g| g.get("reasoning_effort"))
        .and_then(|e| e.as_str());
    if current == effort {
        return Ok(());
    }

    let gateway = config
        .as_object_mut()
        .expect("checked above")
        .entry("gateway")
        .or_insert_with(|| serde_json::json!({}));
    if !gateway.is_object() {
        return Err(format!("{} has a non-object `gateway`", path.display()));
    }
    let gateway = gateway.as_object_mut().expect("checked above");
    match effort {
        Some(value) => {
            gateway.insert("reasoning_effort".to_string(), serde_json::json!(value));
        }
        None => {
            gateway.remove("reasoning_effort");
        }
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    // The file may hold a key; keep octos's own owner-only mode.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// The user's picks. Stored as the raw values that get sent, so a value this
/// build doesn't offer (a newer model, say) round-trips instead of being reset
/// to Default the first time it's loaded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentPrefs {
    pub model: Option<String>,
    pub effort: Option<String>,
    /// `"on"` / `"off"`, or `None` to leave the agent's own setting alone.
    pub thinking: Option<String>,
}

impl AgentPrefs {
    /// Seeds from the host's own environment, so exporting these before
    /// launch pre-selects them in the UI rather than being silently
    /// overridden by it.
    pub fn from_env() -> Self {
        Self {
            model: std::env::var("ANTHROPIC_MODEL").ok().filter(|s| !s.is_empty()),
            effort: std::env::var("CLAUDE_CODE_EFFORT_LEVEL")
                .ok()
                .filter(|s| CLAUDE_EFFORTS.iter().any(|(_, v)| *v == s)),
            thinking: std::env::var("MAX_THINKING_TOKENS")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .map(|budget| if budget > 0 { "on" } else { "off" }.to_string()),
        }
    }

    pub fn get(&self, id: KnobId) -> Option<&str> {
        match id {
            KnobId::Model => self.model.as_deref(),
            KnobId::Effort => self.effort.as_deref(),
            KnobId::Thinking => self.thinking.as_deref(),
        }
    }

    pub fn set(&mut self, id: KnobId, value: Option<String>) {
        match id {
            KnobId::Model => self.model = value,
            KnobId::Effort => self.effort = value,
            KnobId::Thinking => self.thinking = value,
        }
    }
}

fn agent_prefs_path() -> std::path::PathBuf {
    a2app_core::data_root().join("agent_prefs.json")
}

/// The picks saved from a previous launch, or defaults when there are none.
pub fn load_agent_prefs() -> AgentPrefs {
    std::fs::read_to_string(agent_prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persists the picks under the a2app data root.
pub fn save_agent_prefs(prefs: &AgentPrefs) -> Result<(), String> {
    let body = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    a2app_core::persistence::atomic_write(&agent_prefs_path(), body.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_never_sent_to_a_backend_that_cannot_offer_it() {
        // The saved pick is global; the provider is not. Sending a Claude model
        // id to Kimi (or Ollama, or Groq) fails in a way that looks like the
        // host app is broken.
        let prefs = AgentPrefs {
            model: Some("claude-haiku-4-5".to_string()),
            ..Default::default()
        };
        let moonshot = Backend::Octos { provider: "moonshot".to_string() };
        assert!(moonshot.args(&prefs).is_empty(), "moonshot has no Model knob");
        let anthropic = Backend::Octos { provider: "anthropic".to_string() };
        assert_eq!(
            anthropic.args(&prefs),
            vec!["--model".to_string(), "claude-haiku-4-5".to_string()],
        );
    }

    #[test]
    fn only_chosen_fields_are_sent() {
        let backend = Backend::ClaudeCode;
        assert!(backend.env(&AgentPrefs::default()).is_empty());
        let prefs = AgentPrefs { effort: Some("max".into()), ..Default::default() };
        assert_eq!(
            backend.env(&prefs),
            vec![("CLAUDE_CODE_EFFORT_LEVEL".to_string(), "max".to_string())]
        );
    }

    #[test]
    fn thinking_off_is_a_zero_budget() {
        let prefs = AgentPrefs { thinking: Some("off".into()), ..Default::default() };
        assert_eq!(
            Backend::ClaudeCode.env(&prefs),
            vec![("MAX_THINKING_TOKENS".to_string(), "0".to_string())]
        );
    }

    /// The knobs are per-backend, not global: octos has no way to take an
    /// effort level, so it must not offer one — and its model goes through a
    /// CLI flag rather than the environment.
    #[test]
    fn backends_expose_only_what_they_can_deliver() {
        let claude = Backend::ClaudeCode.knobs();
        assert_eq!(claude.len(), 3);

        let octos = Backend::Octos { provider: "anthropic".into() };
        let ids: Vec<KnobId> = octos.knobs().iter().map(|k| k.id).collect();
        assert_eq!(ids, vec![KnobId::Model, KnobId::Effort]);

        let prefs = AgentPrefs {
            model: Some("claude-opus-5".into()),
            effort: Some("max".into()),
            ..Default::default()
        };
        // Model as an argument; the effort goes into octos's config instead of
        // an environment nothing reads (see apply_octos_effort).
        assert_eq!(octos.args(&prefs), vec!["--model", "claude-opus-5"]);
        assert!(octos.env(&prefs).is_empty());
        assert!(Backend::ClaudeCode.args(&prefs).is_empty());
    }

    /// A provider we can't name models for still gets the knob we CAN deliver
    /// (effort), just not a model picker full of guesses.
    #[test]
    fn a_provider_gets_what_it_can_deliver_and_no_more() {
        let openai = Backend::Octos { provider: "openai".into() }.knobs();
        let ids: Vec<KnobId> = openai.iter().map(|k| k.id).collect();
        // Effort is deliverable; a model picker would be guesswork.
        assert_eq!(ids, vec![KnobId::Effort]);
        // A foreign ACP binary is a black box — nothing at all.
        assert!(Backend::Custom.knobs().is_empty());
    }

    /// The create UI's segmented controls carry these labels literally — a
    /// segmented control takes its labels from the DSL, so adding an option
    /// here means adding a segment there. This pins the counts so the two
    /// can't drift silently.
    #[test]
    fn segment_counts_match_the_dsl() {
        let knobs = Backend::ClaudeCode.knobs();
        assert_eq!(knobs[0].options.len(), 4, "Model: Default + 3");
        // Effort has TWO controls declared — the probe picks one.
        assert_eq!(knobs[1].options.len(), claude_efforts().len() + 1);
        assert!(matches!(knobs[1].options.len(), 5 | 6));
        assert_eq!(knobs[2].options.len(), 3, "Thinking: Default + 2");

        // Kimi's Coding Plan ladder is its own control because it has no
        // medium rung.
        let kimi = Backend::Octos { provider: "moonshot-coding".into() }.knobs();
        assert_eq!(kimi.len(), 1, "effort only");
        assert_eq!(kimi[0].options.len(), 4, "Default + low/high/max");
        assert!(!kimi[0].options.iter().any(|(_, v)| v == "medium"));

        // The Moonshot platform models take no effort at all, so they get no
        // control rather than one that quietly does nothing.
        assert!(Backend::Octos { provider: "moonshot".into() }.knobs().is_empty());
    }

    /// Rows are addressed by knob, not by position — a backend offering effort
    /// but no model must still land in the effort row, or the control would
    /// render with the model's labels.
    #[test]
    fn every_knob_owns_a_fixed_row() {
        let openai = Backend::Octos { provider: "openai".into() }.knobs();
        assert_eq!(openai[0].id.row(), 1, "effort is row 1 even when it's listed first");

        let claude = Backend::ClaudeCode.knobs();
        let rows: Vec<usize> = claude.iter().map(|k| k.id.row()).collect();
        assert_eq!(rows, vec![0, 1, 2]);
        // No two knobs may share a row.
        let mut sorted = rows.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), rows.len());
    }

    /// The probed ladder must stay a superset of the safe one, in order.
    #[test]
    fn the_effort_ladder_is_probed_not_assumed() {
        let ladder = claude_efforts();
        assert!(ladder.len() == CLAUDE_EFFORTS.len() || ladder.len() == CLAUDE_EFFORTS.len() + 1);
        assert_eq!(ladder[0], CLAUDE_EFFORTS[0]);
        assert_eq!(ladder[ladder.len() - 1], ("Max", "max"));
        if ladder.len() > CLAUDE_EFFORTS.len() {
            assert_eq!(ladder[3], CLAUDE_EFFORT_XHIGH, "xhigh sits between high and max");
        }
    }

    /// Patching octos's config must be surgical: it's the user's file, and it
    /// holds their API key.
    #[test]
    fn octos_effort_patch_preserves_the_rest_of_the_config() {
        let _guard = super::super::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("octos-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"version":1,"provider":"openai","env_vars":{"OPENAI_API_KEY":"sk-secret"},
                "gateway":{"session_timeout_secs":600}}"#,
        )
        .unwrap();
        // Scope the lookup to our temp dir rather than the real ~/.octos.
        // SAFETY: single-threaded test (the suite runs these serially).
        unsafe { std::env::set_var("OCTOS_CONFIG_DIR", &dir) };

        apply_octos_effort(Some("high")).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["gateway"]["reasoning_effort"], "high");
        // Everything else survived, including the key and the sibling setting.
        assert_eq!(after["env_vars"]["OPENAI_API_KEY"], "sk-secret");
        assert_eq!(after["gateway"]["session_timeout_secs"], 600);
        assert_eq!(after["provider"], "openai");

        // Back to Default removes the field rather than writing a value octos
        // would then apply to every turn.
        apply_octos_effort(None).unwrap();
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(after["gateway"].get("reasoning_effort").is_none());
        assert_eq!(after["gateway"]["session_timeout_secs"], 600);

        unsafe { std::env::remove_var("OCTOS_CONFIG_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indices_round_trip_through_a_knob() {
        let knob = Knob::new(KnobId::Effort, "Effort", CLAUDE_EFFORTS);
        assert_eq!(knob.index_of(None), 0);
        assert_eq!(knob.value_at(0), None);
        for (i, (_, value)) in CLAUDE_EFFORTS.iter().enumerate() {
            assert_eq!(knob.index_of(Some(value)), i + 1);
            assert_eq!(knob.value_at(i + 1).as_deref(), Some(*value));
        }
    }

    /// A value from a build that knew about it must survive one that doesn't:
    /// the picker shows Default, but the stored value is still sent.
    #[test]
    fn an_unknown_value_is_preserved_not_erased() {
        let knob = Knob::new(KnobId::Model, "Model", CLAUDE_MODELS);
        assert_eq!(knob.index_of(Some("claude-something-new")), 0);

        let prefs = AgentPrefs {
            model: Some("claude-something-new".into()),
            ..Default::default()
        };
        assert_eq!(
            Backend::ClaudeCode.env(&prefs),
            vec![("ANTHROPIC_MODEL".to_string(), "claude-something-new".to_string())]
        );
    }
}
