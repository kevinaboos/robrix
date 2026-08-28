//! Deciding whether a typed request means "make me a new app" or "change
//! an app I already have".
//!
//! The create UI is primarily for *creating*, so this is deliberately
//! high-precision rather than clever: a request only becomes a modification
//! when it BOTH names an installed app AND reads like an edit ("make the
//! weather app show animations", "add a reset button to the pomodoro"). Any
//! hint that the user wants something new ("create a weather app") wins, and
//! anything ambiguous falls back to creating — a surprise duplicate app is
//! obvious and harmless, whereas a surprise rewrite is neither. (Modifications
//! are also snapshotted into version history, so even a wrong guess here is
//! recoverable.)

use a2app_core::manifest::MiniAppId;

/// What a typed request turned out to mean.
#[derive(Debug, PartialEq)]
pub enum Intent {
    /// Generate a brand-new app.
    Create,
    /// Rewrite an installed app in place.
    Modify(MiniAppId),
}

/// Phrases that mean "something NEW", checked first so "create a weather app"
/// stays a creation even though it names an installed app.
const CREATE_PHRASES: &[&str] = &[
    "create ",
    "build ",
    "generate ",
    "make a ",
    "make an ",
    "make me a ",
    "make me an ",
    "write a ",
    "write an ",
    "new app",
    "another ",
    "i want a ",
    "i want an ",
    "i need a ",
    "i need an ",
];

/// Phrases that mean "change what's already there". Note `make the`/`make my`
/// versus the `make a`/`make an` above — that one word is what separates
/// "make the weather app show animations" from "make a weather app".
const MODIFY_PHRASES: &[&str] = &[
    "change ",
    "modify ",
    "update ",
    "edit ",
    "tweak ",
    "adjust ",
    "improve ",
    "refine ",
    "fix ",
    "rename ",
    "restyle ",
    "redesign ",
    "make the ",
    "make my ",
    "make it ",
    "give the ",
    "give my ",
    "set the ",
    "turn the ",
    "should ",
    "instead of",
    "no longer",
    "get rid of",
    "remove ",
    "delete ",
    "add ",
];

/// Whether `needle` appears in `haystack` as a reference to something that
/// already exists.
///
/// Two filters: word boundaries (so the app "Tip" doesn't match inside
/// "multiple"), and the article in front of it. "add **a** weather widget"
/// introduces a NEW thing, while "make **the** weather app show animations"
/// points at the one you have — so an indefinite mention doesn't count.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    // Walk CHAR boundaries: stepping bytes would panic on a multi-byte app
    // name ("Übersicht", "☕ Coffee"), and skipping a whole match length would
    // miss a self-overlapping one.
    haystack.char_indices().any(|(start, _)| {
        haystack[start..].starts_with(needle)
            && word_bounded(haystack, start, needle.len())
            && !is_indefinite(&haystack[..start])
    })
}

/// Whether the `len`-byte match at `start` is delimited by non-alphanumerics
/// on both sides (so "tip" doesn't match inside "multiple" or "tips").
fn word_bounded(haystack: &str, start: usize, len: usize) -> bool {
    let before_ok = haystack[..start]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric());
    let after_ok = haystack[start + len..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric());
    before_ok && after_ok
}

/// Whether a cue phrase occurs as its own word(s). Plain `contains` would let
/// "credit" satisfy "edit ", "a new appearance" satisfy "new app", and
/// "address" satisfy "add " — silently flipping the classification.
///
/// Phrases that already end in a space carry their own trailing delimiter, so
/// only the START needs a boundary; ones that don't ("new app", "instead of")
/// are bounded at both ends.
fn contains_phrase(text: &str, phrase: &str) -> bool {
    let needs_trailing = !phrase.ends_with(' ');
    text.char_indices()
        .filter(|(start, _)| text[*start..].starts_with(phrase))
        .map(|(start, _)| (start, phrase))
        .any(|(start, m)| {
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = !needs_trailing
            || text[start + m.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
        before_ok && after_ok
    })
}

/// Whether the text leading up to a mention introduces it as something new
/// ("a", "an", "another", "some") rather than referring to an existing app.
fn is_indefinite(before: &str) -> bool {
    let last = before.trim_end().rsplit(|c: char| c.is_whitespace()).next();
    matches!(last, Some("a") | Some("an") | Some("another") | Some("some"))
}

/// Classifies a request against the installed apps (`(id, display name)`).
/// The longest matching app name wins, so "weather widget" beats "weather".
pub fn classify(request: &str, apps: &[(MiniAppId, String)]) -> Intent {
    let text = request.to_lowercase();

    // Which installed app does this name, if any?
    let mut best: Option<(&MiniAppId, usize)> = None;
    for (id, name) in apps {
        let name_lc = name.to_lowercase();
        let id_lc = id.to_lowercase();
        // The display name is the natural way to refer to an app; the id is a
        // fallback for ids that differ from the name (e.g. "keep-notes-2").
        let matched_len = if contains_word(&text, &name_lc) {
            name_lc.len()
        } else if contains_word(&text, &id_lc) {
            id_lc.len()
        } else {
            continue;
        };
        if best.is_none_or(|(_, len)| matched_len > len) {
            best = Some((id, matched_len));
        }
    }
    let Some((app_id, _)) = best else {
        return Intent::Create;
    };

    // "create a weather app" names Weather but clearly wants a new one.
    if CREATE_PHRASES.iter().any(|p| contains_phrase(&text, p)) {
        return Intent::Create;
    }
    if MODIFY_PHRASES.iter().any(|p| contains_phrase(&text, p)) {
        return Intent::Modify(app_id.clone());
    }
    // Named an app but gave no edit cue ("weather", "a nicer weather app") —
    // treat as a creation, the default job.
    Intent::Create
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps() -> Vec<(MiniAppId, String)> {
        vec![
            ("weather".to_string(), "Weather".to_string()),
            ("notes".to_string(), "Notes".to_string()),
            ("tip".to_string(), "Tip".to_string()),
            ("pomodoro-2".to_string(), "Pomodoro".to_string()),
        ]
    }

    fn modify_of(request: &str) -> Option<String> {
        match classify(request, &apps()) {
            Intent::Modify(id) => Some(id),
            Intent::Create => None,
        }
    }

    #[test]
    fn recognizes_modifications_of_installed_apps() {
        // The motivating example.
        assert_eq!(modify_of("make the weather app show animations").as_deref(), Some("weather"));
        assert_eq!(modify_of("add a reset button to the pomodoro").as_deref(), Some("pomodoro-2"));
        assert_eq!(modify_of("change the notes app to use a dark theme").as_deref(), Some("notes"));
        assert_eq!(modify_of("the tip app should support splitting").as_deref(), Some("tip"));
        assert_eq!(modify_of("make my notes bigger").as_deref(), Some("notes"));
        // The id works too when it differs from the name.
        assert_eq!(modify_of("update pomodoro-2 to count down").as_deref(), Some("pomodoro-2"));
    }

    #[test]
    fn creation_requests_stay_creations() {
        // Names an installed app but explicitly wants a new one.
        assert_eq!(modify_of("create a weather app"), None);
        assert_eq!(modify_of("make a weather app with animations"), None);
        assert_eq!(modify_of("build me another notes app"), None);
        assert_eq!(modify_of("i want a tip calculator"), None);
        // No installed app named at all.
        assert_eq!(modify_of("make the chess app show a timer"), None);
        assert_eq!(modify_of("a pomodoro timer"), None);
        // Named, but no edit cue → default to creating.
        assert_eq!(modify_of("weather"), None);
    }

    #[test]
    fn app_names_match_on_word_boundaries() {
        // "Tip" must not match inside "multiple"/"tips".
        assert_eq!(modify_of("change the multiple choice quiz"), None);
        // ...but a real mention still matches with punctuation around it.
        assert_eq!(modify_of("update the \"tip\" app").as_deref(), Some("tip"));
        assert!(contains_word("make the weather app", "weather"));
        assert!(!contains_word("weatherproof gear", "weather"));
    }

    #[test]
    fn indefinite_mentions_are_not_references_to_installed_apps() {
        // "a weather widget" introduces a NEW thing inside a request for an
        // app the user doesn't have — modifying Weather would be wrong.
        assert_eq!(modify_of("add a weather widget to my address book"), None);
        assert_eq!(modify_of("add another notes section"), None);
        // ...while a definite mention is still a reference.
        assert_eq!(modify_of("add a dark mode to the weather app").as_deref(), Some("weather"));
        assert_eq!(modify_of("add a search box to my notes").as_deref(), Some("notes"));
    }

    #[test]
    fn phrases_match_as_words_not_substrings() {
        // "credit" contains "edit ", "address" contains "add ", "a new
        // appearance" contains "new app" — none of them are real cues.
        assert_eq!(modify_of("a credit tracker like the notes app"), None);
        assert_eq!(modify_of("weather with a new appearance"), None);
        // Exactly the cases the review reproduced against the real classifier.
        assert_eq!(modify_of("a credit card tip calculator"), None);
        assert_eq!(modify_of("a currency exchange calculator"), None);
        assert_eq!(modify_of("a prefix/suffix text tool for notes"), None);
        assert!(contains_phrase("please edit the app", "edit "));
        assert!(!contains_phrase("a credit tracker", "edit "));
        assert!(!contains_phrase("save my address", "add "));
    }

    #[test]
    fn non_ascii_app_names_do_not_panic() {
        // A multi-byte name must not make the scanner slice mid-character.
        let apps = vec![
            ("cafe".to_string(), "Café".to_string()),
            ("weather-jp".to_string(), "天気".to_string()),
            ("emoji".to_string(), "🌦 Forecast".to_string()),
        ];
        // Any of these panicking is the bug; the verdicts themselves are
        // secondary.
        let _ = classify("make the café app show hours", &apps);
        let _ = classify("update 天気 to add typhoons", &apps);
        let _ = classify("change the 🌦 forecast layout", &apps);
        assert_eq!(
            classify("make the café app show hours", &apps),
            Intent::Modify("cafe".to_string())
        );
    }

    #[test]
    fn longest_app_name_wins() {
        let apps = vec![
            ("weather".to_string(), "Weather".to_string()),
            ("weather-pro".to_string(), "Weather Pro".to_string()),
        ];
        assert_eq!(
            classify("change the weather pro forecast", &apps),
            Intent::Modify("weather-pro".to_string())
        );
    }
}
