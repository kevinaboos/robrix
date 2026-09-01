//! The `// name:` / `// icon:` / `// tint:` header comments at the top of a
//! Splash script. Importing a bare `.splash` file has to derive a manifest the
//! same way a generation does, so both read the same header rather than
//! growing two subtly different parsers.

use std::collections::BTreeMap;

/// The header comments, all optional.
#[derive(Default)]
pub struct Header {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub tint: Option<u32>,
    /// Capabilities the app says it needs (`// permissions: network, location`).
    /// Declaring is not granting: runtime tiers still prompt, and every one of
    /// these shows up where the user can block it.
    pub permissions: Vec<String>,
    /// Per-capability reasons (`// why-location: shows local forecasts`),
    /// shown on the prompt in the app's own voice.
    pub permission_reasons: BTreeMap<String, String>,
    /// Individual capability ids listed on the same line, which narrow
    /// their group to just those (`// permissions: matrix.room.members.read`).
    pub capabilities: Vec<String>,
}

/// Parses the header comments off the top of the script. They are left in the
/// source (this dialect allows `//` comments), so the installed app keeps its
/// provenance visible.
pub fn parse_app_header(source: &str) -> Header {
    let mut header = Header::default();
    // Enough lines for name/icon/tint plus a permissions line and a reason
    // per capability; the header is comments, so over-reading costs nothing.
    for line in source.lines().take(20) {
        let Some(rest) = line.trim().strip_prefix("//") else {
            continue;
        };
        let rest = rest.trim();
        if let Some(v) = rest.strip_prefix("name:") {
            let v = v.trim();
            if !v.is_empty() {
                header.name = Some(v.chars().take(18).collect::<String>().trim().to_string());
            }
        } else if let Some(v) = rest.strip_prefix("icon:") {
            // Take the first whitespace-separated token, capped — this keeps
            // multi-codepoint emoji (ZWJ sequences like 👨‍👩‍👧, flags, keycaps)
            // intact instead of truncating them to a broken first scalar.
            if let Some(tok) = v.split_whitespace().next() {
                header.icon = Some(tok.chars().take(12).collect());
            }
        } else if let Some(v) = rest.strip_prefix("tint:") {
            header.tint = parse_hex_color(v.trim());
        } else if let Some(v) = rest.strip_prefix("permissions:") {
            // Unknown ids are dropped rather than carried: an id this build
            // can't grant would sit in the app's info promising something fake.
            for id in v.split(',') {
                let id = id.trim();
                if crate::permissions::Permission::from_str(id).is_some() {
                    if !header.permissions.iter().any(|p| p == id) {
                        header.permissions.push(id.to_string());
                    }
                } else if let Some(cap) = crate::capabilities::by_id(id) {
                    // A capability declares its group too, narrowed to it.
                    if let Some(group) = cap.group
                        && !header.permissions.iter().any(|p| p == group.as_str())
                    {
                        header.permissions.push(group.as_str().to_string());
                    }
                    if !header.capabilities.iter().any(|c| c == id) {
                        header.capabilities.push(id.to_string());
                    }
                }
            }
        } else if let Some(v) = rest.strip_prefix("why-") {
            // `why-<perm>: reason`. The reason is the app's own words shown in
            // host chrome, so it is clamped like an imported bundle's.
            if let Some((id, reason)) = v.split_once(':') {
                let id = id.trim();
                let reason: String = reason.trim().chars().take(120).collect();
                if crate::permissions::Permission::from_str(id).is_some() && !reason.is_empty() {
                    header.permission_reasons.insert(id.to_string(), reason);
                }
            }
        }
    }
    header
}

/// `#4A90D9`, `0x4A90D9`, or `4A90D9` → 0xRRGGBB.
pub fn parse_hex_color(v: &str) -> Option<u32> {
    let v = v
        .trim_start_matches('#')
        .trim_start_matches("0x")
        .trim_start_matches('x');
    if v.len() != 6 {
        return None;
    }
    u32::from_str_radix(v, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A generated app declares its capabilities in the header, or it can
    /// never be granted them — an undeclared capability is refused outright.
    #[test]
    fn header_carries_permissions_and_reasons() {
        let src = "// name: Sunrise\n\
                   // icon: 🌅\n\
                   // permissions: network, location, made-up-cap, network\n\
                   // why-location: Uses your city.\n\
                   // why-made-up-cap: ignored\n\
                   View{}";
        let h = parse_app_header(src);
        // Unknown ids and repeats are dropped: neither can ever be granted,
        // and both would sit in the app's info promising something fake.
        assert_eq!(h.permissions, vec!["network".to_string(), "location".to_string()]);
        assert_eq!(h.permission_reasons.get("location").unwrap(), "Uses your city.");
        assert!(!h.permission_reasons.contains_key("made-up-cap"));
    }

    /// No header line means no capabilities, which is the safe default for
    /// every app that never asks for anything.
    #[test]
    fn header_without_permissions_declares_nothing() {
        let h = parse_app_header("// name: Tip\nView{}");
        assert!(h.permissions.is_empty());
        assert!(h.permission_reasons.is_empty());
    }

    #[test]
    fn parses_name_icon_and_tint() {
        let h = parse_app_header("// name: Tip Calc\n// icon: 💰\n// tint: #4A90D9\nView{}");
        assert_eq!(h.name.as_deref(), Some("Tip Calc"));
        assert_eq!(h.icon.as_deref(), Some("💰"));
        assert_eq!(h.tint, Some(0x4A90D9));
    }

    #[test]
    fn header_variants_parse() {
        assert_eq!(parse_hex_color("#aabbcc"), Some(0xaabbcc));
        assert_eq!(parse_hex_color("0xAABBCC"), Some(0xAABBCC));
        assert_eq!(parse_hex_color("nope"), None);
    }

    #[test]
    fn icon_header_keeps_multi_codepoint_emoji() {
        let h = parse_app_header("// icon: 👨‍👩‍👧\nView{}");
        assert_eq!(h.icon.as_deref(), Some("👨‍👩‍👧"));
    }
}
