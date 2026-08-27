//! Version history for modified mini-apps.
//!
//! Every time an app's script is rewritten (an AI modification, or restoring
//! an older version), the *previous* state is snapshotted first, so the user
//! can always get back to what they had. Snapshots live beside the app's code
//! in its own directory:
//!
//! ```text
//! apps/<id>/versions/20260724-153204.splash   the source as it was
//! apps/<id>/versions/20260724-153204.json     when, why, and its name/icon/tint
//! ```
//!
//! The stamp is local wall-clock time, so a version is identifiable straight
//! from the filename (the user asked for date/timestamped versions). The
//! `.splash` is written first and the `.json` last — listing keys off the
//! `.json`, so a crash mid-snapshot leaves an ignored orphan rather than a
//! version that claims to exist but has no code (same ordering rule as
//! `save_user_app`).

use serde::{Deserialize, Serialize};

use crate::manifest::MiniAppManifest;

/// How many snapshots to keep per app; older ones are pruned oldest-first so
/// a heavily-iterated app can't grow without bound.
pub const MAX_VERSIONS: usize = 20;

/// One entry in an app's history, newest first when listed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppVersion {
    /// Filename key, e.g. `20260724-153204` (local time, sortable).
    pub stamp: String,
    /// Seconds since the unix epoch, for stable ordering across DST/tz moves.
    pub at_unix: u64,
    /// Why this version was superseded — the request that changed it, or a
    /// marker like "Original" / "Before restore".
    pub note: String,
    /// The manifest fields at that point, so restoring brings back the app's
    /// identity (a modification may have renamed or restyled it), not just code.
    pub name: String,
    pub icon: String,
    pub tint: u32,
}

/// Y-M-D H:M:S in the local zone, from a unix timestamp + the local offset.
/// (`chrono` is only an optional dep here, and this is all the calendar math
/// we need: Howard Hinnant's civil-from-days, valid for any modern date.)
pub fn civil_from_unix(at_unix: u64, offset_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let local = at_unix as i64 + offset_secs;
    let days = local.div_euclid(86_400);
    let secs_of_day = local.rem_euclid(86_400);

    // Shift the era so March is month 1 (leap day lands at the end of a year).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    (
        y,
        m,
        d,
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
        (secs_of_day % 60) as u32,
    )
}

/// The filename key for a moment: `YYYYMMDD-HHMMSS` in local time.
pub fn stamp_for(at_unix: u64, offset_secs: i64) -> String {
    let (y, mo, d, h, mi, s) = civil_from_unix(at_unix, offset_secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// A short human label for a version row, e.g. `Jul 24, 15:32`.
pub fn label_for(at_unix: u64, offset_secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (_y, mo, d, h, mi, _s) = civil_from_unix(at_unix, offset_secs);
    let month = MONTHS.get(mo.saturating_sub(1) as usize).copied().unwrap_or("???");
    format!("{month} {d}, {h:02}:{mi:02}")
}

/// Seconds since the unix epoch, now.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Builds the version record for `manifest` as it stands right now.
pub fn version_of(manifest: &MiniAppManifest, note: &str, at_unix: u64, offset_secs: i64) -> AppVersion {
    AppVersion {
        stamp: stamp_for(at_unix, offset_secs),
        at_unix,
        note: note.trim().to_string(),
        name: manifest.name.clone(),
        icon: manifest.icon.clone(),
        tint: manifest.tint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_conversion_matches_known_dates() {
        // 2026-07-24 15:32:04 UTC = 1784907124.
        assert_eq!(civil_from_unix(1_784_907_124, 0), (2026, 7, 24, 15, 32, 4));
        // The unix epoch itself.
        assert_eq!(civil_from_unix(0, 0), (1970, 1, 1, 0, 0, 0));
        // A leap day.
        assert_eq!(civil_from_unix(1_709_164_800, 0), (2024, 2, 29, 0, 0, 0));
        // Negative offsets roll the date back across midnight.
        assert_eq!(civil_from_unix(1_709_164_800, -8 * 3600), (2024, 2, 28, 16, 0, 0));
    }

    #[test]
    fn stamps_are_sortable_and_labels_readable() {
        let earlier = stamp_for(1_784_907_124, 0);
        let later = stamp_for(1_784_907_124 + 3600, 0);
        assert_eq!(earlier, "20260724-153204");
        assert!(later > earlier, "stamps must sort chronologically as strings");
        assert_eq!(label_for(1_784_907_124, 0), "Jul 24, 15:32");
    }
}
