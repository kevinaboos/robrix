//! First-run provider setup: the paste-a-key modal's backend.
//!
//! Writes a minimal octos config (`provider` + the key in `env_vars`, the
//! same shape `octos init` produces) so both the spawned `octos acp` and the
//! in-process backend pick it up. The provider is inferred from the key's
//! prefix. Deliberately conservative about clobbering: a config this module
//! didn't write is never touched — only our own marker-stamped file may be
//! overwritten (re-running setup, tests).

use std::path::{Path, PathBuf};

/// Marker field stamped into configs we write, licensing later overwrites.
const MARKER: &str = "robrix_a2app_setup";

/// Key-prefix → provider. Longest/most-specific prefixes first; `sk-` (plain
/// OpenAI) must come last of the `sk` family.
const KEY_PREFIXES: &[(&str, &str)] = &[
    ("sk-ant-", "anthropic"),
    ("sk-or-", "openrouter"),
    // Kimi Coding Plan. A plain Moonshot *platform* key is just `sk-…`, which
    // is indistinguishable from OpenAI's, so it can't be detected here — that
    // one has to be configured directly.
    ("sk-kimi-", "moonshot-coding"),
    ("gsk_", "groq"),
    ("AIza", "gemini"),
    ("sk-", "openai"),
];

/// The provider a pasted key most likely belongs to.
pub fn provider_for_key(key: &str) -> Option<&'static str> {
    let key = key.trim();
    KEY_PREFIXES
        .iter()
        .find(|(prefix, _)| key.starts_with(prefix))
        .map(|(_, provider)| *provider)
}

/// The env-var name octos expects a provider's key under.
pub fn key_env_for_provider(provider: &str) -> String {
    super::key_env_for(provider)
}

/// Where a written config goes: `$OCTOS_CONFIG_DIR`, else `~/.octos` (the
/// legacy location octos always reads, and the one users know from docs).
pub fn config_write_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OCTOS_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(".octos")
}

/// Writes `dir/config.json` for `provider` with the key inline (0600), the
/// exact shape `octos init` produces. Refuses to overwrite a config that
/// wasn't written by this setup flow.
pub fn write_min_config(dir: &Path, provider: &str, key: &str) -> Result<(), String> {
    let path = dir.join("config.json");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let ours = serde_json::from_str::<serde_json::Value>(&existing)
            .ok()
            .and_then(|v| v.get(MARKER).and_then(|m| m.as_bool()))
            .unwrap_or(false);
        if !ours {
            return Err("octos is already configured (~/.octos/config.json exists)".to_string());
        }
    }
    let key_env = key_env_for_provider(provider);
    let config = serde_json::json!({
        "version": 1,
        MARKER: true,
        "provider": provider,
        "env_vars": { key_env: key.trim() },
    });
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let body = serde_json::to_string_pretty(&config).expect("static shape");
    std::fs::write(&path, body).map_err(|e| format!("couldn't write config: {e}"))?;
    // The file holds a secret: owner-only, like octos's own stores.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_provider_from_key_prefix() {
        assert_eq!(provider_for_key("sk-ant-api03-xyz"), Some("anthropic"));
        assert_eq!(provider_for_key("  sk-or-v1-xyz "), Some("openrouter"));
        assert_eq!(provider_for_key("gsk_xyz"), Some("groq"));
        assert_eq!(provider_for_key("AIzaSyXyz"), Some("gemini"));
        assert_eq!(provider_for_key("sk-proj-xyz"), Some("openai"));
        assert_eq!(provider_for_key("whatever"), None);
        assert_eq!(provider_for_key(""), None);
    }

    #[test]
    fn key_env_names_match_octos() {
        assert_eq!(key_env_for_provider("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(key_env_for_provider("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(key_env_for_provider("somethingelse"), "SOMETHINGELSE_API_KEY");
    }

    #[test]
    fn writes_and_guards_config() {
        let dir = std::env::temp_dir().join("hl_setup_test");
        let _ = std::fs::remove_dir_all(&dir);

        write_min_config(&dir, "anthropic", " sk-ant-k ").unwrap();
        let text = std::fs::read_to_string(dir.join("config.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["provider"], "anthropic");
        assert_eq!(v["env_vars"]["ANTHROPIC_API_KEY"], "sk-ant-k");

        // Our own file may be overwritten (rerun of setup)...
        write_min_config(&dir, "openai", "sk-2").unwrap();
        // ...but a user's config (no marker) is refused.
        std::fs::write(dir.join("config.json"), r#"{"provider":"gemini"}"#).unwrap();
        assert!(write_min_config(&dir, "openai", "sk-3").is_err());
    }
}
