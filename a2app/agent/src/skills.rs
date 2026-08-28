//! `persistent-guide`: persistent dialect-guide injection on the octos side.
//!
//! `octos acp` assembles each session's system prompt from, among other
//! things, bootstrap files in `<session-cwd>/.octos/` — `AGENTS.md` is read on
//! every `session/new` and APPENDED to the base prompt, uncapped (see
//! `load_bootstrap_files` in octos-cli and its call in `ConfigAgentFactory::
//! build`). Since we spawn one agent per generation with the workspace as
//! cwd, writing the guide there once makes every session carry it — and
//! per-turn prompts shrink from ~6KB of guide to one pointer line.
//!
//! Freshness is free: a new `session/new` happens every generation, and the
//! deploy below rewrites the file whenever the compiled-in guide changed.
//!
//! Note this mechanism is octos-specific (other ACP agents won't read it), so
//! the slim prompt is only used after a successful deploy — and a foreign
//! agent pointed at via `ROBRIX_AGENT_CMD` that ignores the file still
//! works, just with the guide inlined per prompt as usual... unless it ALSO
//! runs with this workspace cwd and honors AGENTS.md, in which case it wins
//! too. The offline `fake_acp` ignores prompts entirely, so tests pass either
//! way.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the guide file is known to be in place for this process.
static DEPLOYED: AtomicBool = AtomicBool::new(false);

/// Writes the dialect guide to `<workspace>/.octos/AGENTS.md` (only when its
/// contents changed), recording success so prompts can go slim.
pub fn deploy_guide(workspace: &Path) {
    let dir = workspace.join(".octos");
    let path = dir.join("AGENTS.md");
    let desired = format!(
        "<!-- deployed by robrix; edits are overwritten -->\n\n{}",
        super::SPLASH_GUIDE
    );
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current == desired {
        DEPLOYED.store(true, Ordering::Release);
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, desired).is_ok() {
        DEPLOYED.store(true, Ordering::Release);
    }
}

/// Whether prompts may rely on the agent-side guide instead of inlining it.
pub fn guide_is_deployed() -> bool {
    DEPLOYED.load(Ordering::Acquire)
}

/// Marks the guide as injected by other means — the in-process backend
/// appends it to the agent's system prompt directly instead of via a file.
#[cfg(feature = "embedded")]
pub fn mark_deployed() {
    DEPLOYED.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_guide_and_reports_it() {
        let ws = std::env::temp_dir().join("hl_skills_test_ws");
        let _ = std::fs::remove_dir_all(&ws);
        deploy_guide(&ws);
        let path = ws.join(".octos").join("AGENTS.md");
        let written = std::fs::read_to_string(&path).expect("guide written");
        assert!(written.contains("Splash mini-app guide"));
        assert!(guide_is_deployed());
        // Second deploy is a no-op rewrite (same contents).
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        deploy_guide(&ws);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }
}
