//! The set of AI providers the generator can use, and which one is active.
//!
//! octos has exactly one active provider, but a person has several keys. Both
//! facts live in octos's own `config.json`: every key sits in the `env_vars`
//! map under its provider's variable name, and `provider` names the one in
//! use. octos resolves a key by looking up *that provider's* variable
//! (`Config::get_api_key`), so keys for the others are simply never read —
//! switching provider is a one-field edit, and no second key store has to
//! exist.
//!
//! Writes are surgical for the same reason `apply_octos_effort` is: it's the
//! user's file, it holds their secrets, and it may hold settings this app
//! knows nothing about.

use std::path::PathBuf;

/// A provider that can be set up from a pasted key.
pub struct ProviderSpec {
    /// octos's provider id — what goes in `config.json`'s `provider` field.
    pub id: &'static str,
    /// What the user sees.
    pub label: &'static str,
    /// Shape of the key, shown while adding one.
    pub hint: &'static str,
}

/// Offered in the Add-provider picker, most likely first. Ids and key
/// variables must match octos's registry (`crates/octos-llm/src/registry/`);
/// the variable itself comes from [`super::PROVIDER_KEY_ENVS`] so there's one
/// source of truth.
pub const CATALOG: &[ProviderSpec] = &[
    ProviderSpec { id: "anthropic", label: "Anthropic (Claude)", hint: "sk-ant-…" },
    ProviderSpec { id: "openai", label: "OpenAI", hint: "sk-…" },
    ProviderSpec { id: "moonshot-coding", label: "Kimi (Coding Plan)", hint: "sk-kimi-…" },
    ProviderSpec { id: "moonshot", label: "Kimi / Moonshot", hint: "sk-…" },
    ProviderSpec { id: "gemini", label: "Google Gemini", hint: "AIza…" },
    ProviderSpec { id: "groq", label: "Groq", hint: "gsk_…" },
    ProviderSpec { id: "openrouter", label: "OpenRouter", hint: "sk-or-…" },
    ProviderSpec { id: "deepseek", label: "DeepSeek", hint: "sk-…" },
];

pub fn spec(id: &str) -> Option<&'static ProviderSpec> {
    CATALOG.iter().find(|p| p.id == id)
}

/// A provider's display name, falling back to the raw id so a provider
/// configured by hand (octos knows more than we list) still reads sensibly.
pub fn label_for(id: &str) -> String {
    spec(id).map(|p| p.label.to_string()).unwrap_or_else(|| id.to_string())
}

/// Where a provider's credential comes from — which decides whether this app
/// may edit or remove it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// In octos's config, under this app's control.
    Config,
    /// An exported environment variable. Ours to read, not to change.
    Environment,
    /// `octos auth login` put it in octos's auth store.
    AuthStore,
    /// Needs no key at all (a local Ollama).
    None,
    /// An ACP agent chosen by `ROBRIX_AGENT_CMD` — a Claude Code
    /// subscription, or any other agent binary. It overrides octos entirely,
    /// so it isn't a credential this page can manage: it's decided by how the
    /// app was started.
    AgentCommand,
}

/// One provider that could be switched to right now.
#[derive(Clone, Debug)]
pub struct ConfiguredProvider {
    pub id: String,
    pub label: String,
    pub source: KeySource,
    /// What a generation started right now would use.
    pub active: bool,
    /// What a fresh launch would use — the choice saved in octos's config.
    pub is_default: bool,
}

impl ConfiguredProvider {
    /// Whether this app can rewrite or delete this credential. It can only
    /// touch what it wrote: an exported variable belongs to the shell and the
    /// auth store belongs to octos.
    pub fn editable(&self) -> bool {
        self.source == KeySource::Config
    }

    /// Whether this entry is configured outside the app and can't be acted on
    /// here — only explained.
    pub fn external(&self) -> bool {
        self.source == KeySource::AgentCommand
    }

    /// The one-line explanation under the name. Leads with "In use" for the
    /// active provider, because which one is selected is the first thing this
    /// page has to answer.
    pub fn detail(&self) -> String {
        let where_from = match self.source {
            KeySource::Config => format!("Key saved · {}", super::key_env_for(&self.id)),
            KeySource::Environment => {
                format!("From your environment · {}", super::key_env_for(&self.id))
            }
            KeySource::AuthStore => "Signed in with `octos auth login`".to_string(),
            KeySource::None => "Runs locally — no key needed".to_string(),
            KeySource::AgentCommand => {
                format!("ROBRIX_AGENT_CMD={}", agent_command().unwrap_or_default())
            }
        };
        // Both facts, because they can disagree: a provider picked for this
        // session is in use WITHOUT being what the next launch will pick, and
        // saying only one of the two makes the other look broken.
        match (self.active, self.is_default) {
            (true, true) => format!("In use · Default · {where_from}"),
            (true, false) => format!("In use now · {where_from}"),
            (false, true) => format!("Default · {where_from}"),
            (false, false) => where_from,
        }
    }
}

/// The ACP agent command the app was started with, if any.
pub fn agent_command() -> Option<String> {
    std::env::var("ROBRIX_AGENT_CMD").ok().filter(|c| !c.trim().is_empty())
}

/// Display name for that agent. A Claude Code subscription via
/// `claude-code-acp` is the common case and deserves to be recognised by name
/// rather than shown as a path.
fn agent_command_label(cmd: &str) -> String {
    if cmd.contains("claude-code-acp") {
        "Claude Code (your subscription)".to_string()
    } else {
        "Custom ACP agent".to_string()
    }
}

/// The config file octos will actually read (its first existing candidate), or
/// where we would create one.
fn config_path() -> Option<PathBuf> {
    super::octos_config_candidates()
        .into_iter()
        .find(|p| p.exists())
        .or_else(|| super::octos_config_candidates().into_iter().next())
}

fn read_config() -> serde_json::Value {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Rewrites the config after `edit` has mutated it, preserving everything the
/// edit didn't touch and keeping the file owner-only.
fn write_config(edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "no writable octos config location".to_string())?;
    let mut config = read_config();
    let object = config
        .as_object_mut()
        .ok_or_else(|| format!("{} isn't a JSON object", path.display()))?;
    object.entry("version").or_insert_with(|| serde_json::json!(1));
    edit(object);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| format!("couldn't write config: {e}"))?;
    // It holds API keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Keys this app has saved into the config, by provider id.
fn keys_in_config() -> Vec<String> {
    let config = read_config();
    let Some(vars) = config.get("env_vars").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    super::PROVIDER_KEY_ENVS
        .iter()
        .filter(|(var, _)| vars.get(*var).and_then(|v| v.as_str()).is_some_and(|v| !v.trim().is_empty()))
        .map(|(_, provider)| provider.to_string())
        .collect()
}

/// The key we hold for a provider, from our config or from the environment.
///
/// Only for handing to an agent we spawn — never for display (the page shows a
/// mask). The config is checked first for the same reason [`octos_providers_marked`]
/// ranks it first: a key this app saved is the more specific answer.
pub fn key_for(id: &str) -> Option<String> {
    let var = super::key_env_for(id);
    read_config()
        .get("env_vars")
        .and_then(|vars| vars.get(&var))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| std::env::var(&var).ok())
        .filter(|k| !k.trim().is_empty())
}

/// The pseudo-id for the row representing `ROBRIX_AGENT_CMD`.
pub const AGENT_COMMAND_ID: &str = "agent-command";

/// What a generation started RIGHT NOW would actually use.
///
/// Mirrors `start_backend`'s selection order exactly, and is the single source
/// of truth for which row the page marks "In use". Anything that overrides the
/// saved choice at runtime — an agent command, an exported key, the Claude Code
/// bridge, a pick made this session — has to appear here, or the page shows a
/// tick next to something that isn't running.
pub fn in_use_id() -> Option<String> {
    if agent_command().is_some() {
        return Some(AGENT_COMMAND_ID.to_string());
    }
    if let Some(id) = session_provider() {
        return Some(id);
    }
    if let Some(id) = super::bridged_provider() {
        return Some(id);
    }
    super::provider_from_octos_config()
        .or_else(|| super::provider_from_env().map(str::to_string))
        .or_else(super::provider_from_auth_store)
        .or_else(|| super::ollama_model().map(|_| "ollama".to_string()))
}

/// Every provider that could be used right now, the one in use first-class.
///
/// Merged from all the places a credential can live, so the list matches what
/// a generation would actually find rather than only what this app wrote — and
/// runtime overrides get a row of their own rather than silently winning
/// behind a page that claims something else is selected.
pub fn list() -> Vec<ConfiguredProvider> {
    let in_use = in_use_id();
    let saved_default = super::provider_from_octos_config();
    let mut found: Vec<ConfiguredProvider> = Vec::new();
    // An explicit agent command wins over everything: octos is not consulted
    // at all in that case (see `start_backend`), so it is THE provider, and
    // any keys below it are dormant until the app runs without it. It
    // still gets listed ALONGSIDE them, so the saved default stays visible as
    // "what you'd get without this override" instead of vanishing.
    if let Some(cmd) = agent_command() {
        found.push(ConfiguredProvider {
            id: AGENT_COMMAND_ID.to_string(),
            label: agent_command_label(&cmd),
            source: KeySource::AgentCommand,
            active: true,
            is_default: false,
        });
    }
    found.extend(octos_providers_marked(in_use, saved_default));
    found
}

/// The providers octos itself could use, with `in_use` and `saved_default`
/// decided by the caller so every row's badges come from one calculation.
fn octos_providers_marked(
    in_use: Option<String>,
    saved_default: Option<String>,
) -> Vec<ConfiguredProvider> {
    let active = in_use;
    // Gathered in precedence order, first mention winning: a key we saved is
    // more specific than the same provider showing up via the environment.
    let mut candidates: Vec<(String, KeySource)> = Vec::new();
    candidates.extend(keys_in_config().into_iter().map(|id| (id, KeySource::Config)));
    if let Some(id) = super::provider_from_env() {
        candidates.push((id.to_string(), KeySource::Environment));
    }
    if let Some(id) = super::provider_from_auth_store() {
        candidates.push((id, KeySource::AuthStore));
    }
    if super::ollama_model().is_some() {
        candidates.push(("ollama".to_string(), KeySource::None));
    }
    // A provider named in the config whose key lives somewhere we can't see
    // (a keychain marker, say) is still selected — list it rather than
    // showing an empty picker next to a working setup.
    if let Some(id) = saved_default.clone() {
        candidates.push((id, KeySource::AuthStore));
    }

    let mut found: Vec<ConfiguredProvider> = Vec::new();
    for (id, source) in candidates {
        if found.iter().any(|p| p.id == id) {
            continue;
        }
        found.push(ConfiguredProvider {
            label: label_for(&id),
            active: active.as_deref() == Some(id.as_str()),
            is_default: saved_default.as_deref() == Some(id.as_str()),
            id,
            source,
        });
    }
    found
}

/// The config file's path for the "where do keys go" line, with `$HOME`
/// abbreviated — the full path is noise in a footnote.
pub fn config_display_path() -> String {
    let Some(path) = config_path() else {
        return String::new();
    };
    let shown = path.display().to_string();
    match std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned()) {
        Some(home) if shown.starts_with(&home) => shown.replacen(&home, "~", 1),
        _ => shown,
    }
}

/// Whether anything at all is set up. The create UI asks this before letting
/// someone type a request that could only fail — so it MUST count the
/// `ROBRIX_AGENT_CMD` path, which needs no key and no octos config and
/// is how a Claude subscription is used. Nagging that setup for a provider
/// would be nagging a working app.
pub fn any_configured() -> bool {
    agent_command().is_some() || !list().is_empty()
}

/// The provider picked for THIS SESSION only, overriding the saved default.
///
/// Deliberately not persisted: "use this one for now" and "use this one from
/// now on" are different intentions, and collapsing them means you can't try a
/// provider without also committing to it. Cleared by restarting the app.
static SESSION_PROVIDER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Picks the provider for the rest of this run of the app.
pub fn set_session(id: &str) {
    *SESSION_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
}

/// Drops the session pick, falling back to the saved default.
pub fn clear_session() {
    *SESSION_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The session pick, if one was made.
pub fn session_provider() -> Option<String> {
    SESSION_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The provider saved in octos's config — what a fresh launch starts with.
pub fn default_provider() -> Option<String> {
    super::provider_from_octos_config()
}

/// What a generation started right now would actually use.
pub fn effective_provider() -> Option<String> {
    session_provider().or_else(default_provider)
}

/// Makes `id` the saved default. Only the `provider` field moves; the keys and
/// every other setting stay exactly as they were.
pub fn set_active(id: &str) -> Result<(), String> {
    write_config(|config| {
        config.insert("provider".to_string(), serde_json::json!(id));
        // A model pinned for the previous provider would be sent to this one.
        config.remove("model");
    })
}

/// Saves a key for `id` and makes it active — the "add a provider" path.
pub fn save_key(id: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste a key first".to_string());
    }
    // Some prefixes name their provider unambiguously (`sk-ant-`, `gsk_`,
    // `AIza`, `sk-or-`, `sk-kimi-`). If one of those lands on the wrong row
    // it's a slip, and saving it would fail later as an auth error with no
    // hint about the real cause. A plain `sk-…` is genuinely shared by
    // OpenAI/Moonshot/DeepSeek, so it is never second-guessed.
    if let Some(detected) = super::setup::provider_for_key(key) {
        if detected != id && detected != "openai" {
            return Err(format!(
                "That looks like a {} key — pick that row instead.",
                label_for(detected)
            ));
        }
    }
    let var = super::key_env_for(id);
    write_config(|config| {
        let vars = config
            .entry("env_vars")
            .or_insert_with(|| serde_json::json!({}));
        if !vars.is_object() {
            *vars = serde_json::json!({});
        }
        if let Some(map) = vars.as_object_mut() {
            map.insert(var, serde_json::json!(key));
        }
        config.insert("provider".to_string(), serde_json::json!(id));
        config.remove("model");
    })
}

/// Forgets a saved key. Only removes what this app can own — an exported
/// variable or an auth-store login is left alone (the UI doesn't offer it).
pub fn forget(id: &str) -> Result<(), String> {
    let var = super::key_env_for(id);
    write_config(|config| {
        if let Some(map) = config.get_mut("env_vars").and_then(|v| v.as_object_mut()) {
            map.remove(&var);
        }
        if config.get("provider").and_then(|p| p.as_str()) == Some(id) {
            config.remove("provider");
            config.remove("model");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test gets its own config dir; `OCTOS_CONFIG_DIR` is the first
    /// candidate, so it wins over anything on the developer's machine.
    fn with_temp_config<T>(body: impl FnOnce() -> T) -> T {
        // Held for the whole body: OCTOS_CONFIG_DIR is process-global.
        let _guard = super::super::CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "hl_providers_test_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("OCTOS_CONFIG_DIR", &dir) };
        // Belt and braces: never run a test body that would write outside the
        // fixture. This assertion is here because it once wasn't, and the
        // tests wrote to the developer's real ~/.octos/config.json.
        let resolved = config_path().expect("a config path");
        assert!(
            resolved.starts_with(&dir),
            "test config path escaped the fixture: {}",
            resolved.display()
        );
        let out = body();
        unsafe { std::env::remove_var("OCTOS_CONFIG_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    fn config_json() -> serde_json::Value {
        read_config()
    }

    /// "Use for now" and "make default" are separate on purpose: trying a
    /// provider must not rewrite what tomorrow's launch starts with.
    #[test]
    fn a_session_pick_does_not_touch_the_saved_default() {
        with_temp_config(|| {
            clear_session();
            save_key("anthropic", "sk-ant-one").unwrap();
            save_key("openai", "sk-two").unwrap();
            set_active("anthropic").unwrap();
            assert_eq!(default_provider().as_deref(), Some("anthropic"));

            set_session("openai");
            // In use changes...
            assert_eq!(effective_provider().as_deref(), Some("openai"));
            // ...the saved default does not.
            assert_eq!(default_provider().as_deref(), Some("anthropic"));

            // And the page reflects BOTH facts on the right rows.
            let rows = list();
            let openai = rows.iter().find(|p| p.id == "openai").expect("openai row");
            let anthropic = rows.iter().find(|p| p.id == "anthropic").expect("anthropic row");
            assert!(openai.active && !openai.is_default, "{}", openai.detail());
            assert!(anthropic.is_default && !anthropic.active, "{}", anthropic.detail());
            assert!(openai.detail().contains("In use now"));
            assert!(anthropic.detail().contains("Default"));

            clear_session();
            assert_eq!(effective_provider().as_deref(), Some("anthropic"));
        });
    }

    /// Exactly one row may claim to be in use, and it must be the one that
    /// would actually run — otherwise the page ticks something inert.
    #[test]
    fn exactly_one_row_is_in_use() {
        with_temp_config(|| {
            clear_session();
            save_key("anthropic", "sk-ant-one").unwrap();
            save_key("openai", "sk-two").unwrap();
            set_active("openai").unwrap();
            let in_use: Vec<_> = list().into_iter().filter(|p| p.active).collect();
            assert_eq!(in_use.len(), 1, "rows in use: {in_use:?}");
            assert_eq!(in_use[0].id, "openai");
            assert_eq!(in_use[0].id, in_use_id().unwrap());
        });
    }

    /// The bridge that lets a Kimi key run without octos hands the key to the
    /// agent process, so `key_for` has to return the real thing — and has to
    /// find it under the provider's own variable, not a guessed one.
    #[test]
    fn a_saved_key_can_be_read_back_for_the_agent() {
        with_temp_config(|| {
            assert_eq!(key_for("moonshot-coding"), None);
            save_key("moonshot-coding", "sk-kimi-secret").unwrap();
            assert_eq!(key_for("moonshot-coding").as_deref(), Some("sk-kimi-secret"));
            // Kimi's two products are separate providers with separate hosts;
            // one must never answer for the other.
            assert_eq!(key_for("moonshot"), None);
        });
    }

    #[test]
    fn keys_accumulate_and_switching_is_one_field() {
        with_temp_config(|| {
            save_key("anthropic", "sk-ant-one").unwrap();
            save_key("moonshot-coding", "sk-kimi-two").unwrap();

            // Both keys survive; the second add is the active one.
            let config = config_json();
            let vars = config.get("env_vars").unwrap();
            assert_eq!(vars.get("ANTHROPIC_API_KEY").unwrap(), "sk-ant-one");
            assert_eq!(vars.get("KIMI_CODING_API_KEY").unwrap(), "sk-kimi-two");
            assert_eq!(config.get("provider").unwrap(), "moonshot-coding");

            // Switching back touches nothing but `provider`.
            set_active("anthropic").unwrap();
            let config = config_json();
            assert_eq!(config.get("provider").unwrap(), "anthropic");
            let vars = config.get("env_vars").unwrap();
            assert_eq!(vars.get("KIMI_CODING_API_KEY").unwrap(), "sk-kimi-two");
        });
    }

    #[test]
    fn a_pinned_model_never_follows_a_provider_switch() {
        with_temp_config(|| {
            save_key("anthropic", "sk-ant-one").unwrap();
            write_config(|c| {
                c.insert("model".to_string(), serde_json::json!("claude-opus-5"));
            })
            .unwrap();
            set_active("moonshot-coding").unwrap();
            // A Claude model id sent to Kimi is a baffling failure.
            assert!(config_json().get("model").is_none());
        });
    }

    #[test]
    fn unrelated_settings_are_preserved() {
        with_temp_config(|| {
            write_config(|c| {
                c.insert("gateway".to_string(), serde_json::json!({"reasoning_effort": "high"}));
                c.insert("mcp_servers".to_string(), serde_json::json!({"x": 1}));
            })
            .unwrap();
            save_key("groq", "gsk_abc").unwrap();
            let config = config_json();
            assert_eq!(config.pointer("/gateway/reasoning_effort").unwrap(), "high");
            assert!(config.get("mcp_servers").is_some());
        });
    }

    #[test]
    fn an_unmistakable_key_on_the_wrong_row_is_refused() {
        with_temp_config(|| {
            let err = save_key("openai", "sk-ant-api03-xyz").unwrap_err();
            assert!(err.contains("Anthropic"), "{err}");
            // Nothing was written.
            assert!(config_json().get("env_vars").is_none());
            // A shared `sk-` prefix is not second-guessed.
            assert!(save_key("moonshot", "sk-plain-platform-key").is_ok());
        });
    }

    #[test]
    fn forgetting_a_key_drops_it_and_deactivates() {
        with_temp_config(|| {
            save_key("openai", "sk-openai").unwrap();
            forget("openai").unwrap();
            let config = config_json();
            assert!(config.pointer("/env_vars/OPENAI_API_KEY").is_none());
            assert!(config.get("provider").is_none());
        });
    }

    /// An app started with an ACP agent command is fully set up: no key,
    /// no octos config. Reporting otherwise made the create UI offer provider
    /// setup on every click for exactly the Claude-subscription setup.
    #[test]
    fn an_agent_command_counts_as_configured() {
        with_temp_config(|| {
            assert!(!any_configured(), "empty config, no agent command");
            unsafe { std::env::set_var("ROBRIX_AGENT_CMD", "claude-code-acp") };
            assert!(any_configured());
            let list = list();
            assert!(list[0].external(), "the agent command is the provider in use");
            assert!(list[0].active);
            assert!(!list[0].editable(), "it's chosen by how the app was started");
            assert!(list[0].label.contains("Claude Code"));
            unsafe { std::env::remove_var("ROBRIX_AGENT_CMD") };
        });
    }

    #[test]
    fn the_list_reports_what_is_saved_and_which_is_active() {
        with_temp_config(|| {
            save_key("anthropic", "sk-ant-one").unwrap();
            save_key("groq", "gsk_two").unwrap();
            let list = list();
            let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
            assert!(ids.contains(&"anthropic") && ids.contains(&"groq"));
            let active: Vec<&str> =
                list.iter().filter(|p| p.active).map(|p| p.id.as_str()).collect();
            assert_eq!(active, vec!["groq"]);
            assert!(list.iter().find(|p| p.id == "groq").unwrap().editable());
        });
    }
}
