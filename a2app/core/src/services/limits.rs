//! What stops a mini-app that does NOT play nicely.
//!
//! The permission model answers "may this app do X". It says nothing about
//! "may it do X four thousand times a second", and a sandbox that contains a
//! hostile app but lets it wedge the host is only half a sandbox. An app
//! cannot escape its isolate — that is enforced elsewhere and is not in
//! question here — but without this module it could:
//!
//! - loop `files.pick` and stack native file dialogs on the user,
//! - loop `clipboard.read` and spawn a process per call,
//! - loop `url.open` and fill the screen with browser tabs,
//! - loop `location.get` and turn the host into a request amplifier,
//! - hammer another app's isolate through `ipc.send`.
//!
//! So every brokered request spends from a per-app token bucket priced by
//! what the service actually costs, the services that put OS UI on screen are
//! additionally one-at-a-time and foreground-only, and an app that keeps
//! slamming into the limit earns strikes. Enough strikes and the host stops
//! it and tells the user, which is the only honest end state: a program that
//! will not stop asking has to be made to stop.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::manifest::MiniAppId;

/// Tokens an app may hold. Sized for real bursts — an app waking up refreshes
/// several things at once — while a runaway loop drains it in well under a
/// second.
const BUCKET_CAPACITY: f64 = 30.0;
/// Tokens returned per second. Sustained traffic above this is not a mini-app
/// doing its job.
const REFILL_PER_SEC: f64 = 6.0;
/// How long an app is refused outright after draining its bucket. Long enough
/// to break a loop's rhythm, short enough that a merely-enthusiastic app
/// recovers unnoticed.
const COOLDOWN: Duration = Duration::from_secs(3);
/// Cooldowns before the host stops the app instead of just refusing it.
const STRIKES_BEFORE_STOP: u32 = 4;
/// Strikes decay after this long without trouble, so an app is not condemned
/// for something it did an hour ago.
const STRIKE_DECAY: Duration = Duration::from_secs(120);

/// What a request costs. Cheap calls are ones the host answers from memory;
/// expensive ones spawn processes, hit the network, or put OS UI on screen.
fn cost_of(service: &str) -> f64 {
    match service {
        // Answered from state the host already holds — cheap, but not free:
        // each one still crosses the bridge, builds a JSON answer and re-enters
        // the isolate, so a script polling these in a loop is still a script
        // the host has to keep up with.
        "env" | "permissions.query" => 1.0,
        // Touch host state or another isolate.
        "notify.post" | "notify.clear" | "clipboard.write" | "ipc.send" => 1.0,
        "matrix.room_info" | "matrix.profile" => 1.0,
        // Leave the process: network, a child process, the OS.
        "location.get" | "clipboard.read" | "permissions.request" => 5.0,
        // Round-trip through the matrix worker, and sending speaks as the user.
        "matrix.read_messages" | "matrix.send_message" | "matrix.room_members"
        | "matrix.pinned_events" | "matrix.room_threads" => 5.0,
        // Put something on screen the user has to deal with.
        "files.pick" | "files.save" | "auth.check" | "share" | "url.open" => 8.0,
        // Unknown services are refused upstream; price them like the worst.
        _ => 8.0,
    }
}

/// Services that put OS UI on screen or hand the user to another app. These
/// are foreground-only: an app whose pane isn't the one on screen has no
/// business raising a file picker or launching a browser.
pub fn is_ui_service(service: &str) -> bool {
    matches!(
        service,
        "files.pick" | "files.save" | "auth.check" | "share" | "url.open"
    )
}

/// The subset that stays on screen until the user answers. These are also
/// one-at-a-time per app: a second file picker stacked on the first is never
/// something a well-behaved app does, and it is a good way to trap a user in
/// dialogs they cannot dismiss.
pub fn is_modal_service(service: &str) -> bool {
    matches!(service, "files.pick" | "files.save" | "auth.check")
}

/// Why a request was refused, so the caller can answer the script honestly
/// and the user can be told something true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Over budget: the app is asking far too often.
    RateLimited,
    /// A dialog-opening service while one is already pending.
    AlreadyPending,
    /// A dialog-opening service from a background surface.
    NotForeground,
}

impl Refusal {
    /// What the script is told. Deliberately plain: a hostile app learns
    /// nothing useful, and an honest one learns to back off.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::RateLimited => "too many requests, slow down",
            Refusal::AlreadyPending => "a dialog is already open for this app",
            Refusal::NotForeground => "this needs the app to be on screen",
        }
    }
}

#[derive(Default)]
struct AppLimits {
    tokens: f64,
    last_refill: Option<Instant>,
    cooldown_until: Option<Instant>,
    strikes: u32,
    last_strike: Option<Instant>,
    /// Outstanding OS-dialog request, if any.
    dialog_pending: bool,
    /// Refusals since the app started, for the app's info page.
    refusals: u64,
}

/// Per-app rate limiting and strike-keeping for the broker.
#[derive(Default)]
pub struct AbuseLimiter {
    apps: HashMap<MiniAppId, AppLimits>,
}

/// The limiter's verdict on one request.
pub enum Verdict {
    Allow,
    Refuse(Refusal),
    /// Refuse AND stop the app: it has ignored the limit too many times.
    Stop(Refusal),
}

impl AbuseLimiter {
    /// Charges a request against the app's budget.
    ///
    /// `foreground` is whether this app's pane is the one the user is looking
    /// at, which is what makes "no file pickers from a backgrounded app"
    /// enforceable rather than a convention.
    pub fn check(&mut self, app_id: &str, service: &str, foreground: bool) -> Verdict {
        let now = Instant::now();
        let app = self.apps.entry(app_id.to_string()).or_insert_with(|| AppLimits {
            tokens: BUCKET_CAPACITY,
            last_refill: Some(Instant::now()),
            ..Default::default()
        });

        // Strikes fade, so an app is judged on what it is doing now.
        if let Some(last) = app.last_strike {
            if now.duration_since(last) > STRIKE_DECAY {
                app.strikes = 0;
                app.last_strike = None;
            }
        }

        if let Some(until) = app.cooldown_until {
            if now < until {
                app.refusals += 1;
                return Verdict::Refuse(Refusal::RateLimited);
            }
            app.cooldown_until = None;
            // Come back with a partial bucket, not a full one: a loop that
            // waited out the cooldown should hit the wall again quickly.
            // `last_refill` moves with it, or the refill below would credit
            // the whole cooldown back and hand over a full bucket anyway.
            app.tokens = BUCKET_CAPACITY / 3.0;
            app.last_refill = Some(now);
        }

        // Refill for elapsed time.
        let elapsed = app
            .last_refill
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        app.last_refill = Some(now);
        app.tokens = (app.tokens + elapsed * REFILL_PER_SEC).min(BUCKET_CAPACITY);

        if is_ui_service(service) && !foreground {
            app.refusals += 1;
            return Verdict::Refuse(Refusal::NotForeground);
        }
        if is_modal_service(service) && app.dialog_pending {
            app.refusals += 1;
            return Verdict::Refuse(Refusal::AlreadyPending);
        }

        let cost = cost_of(service);
        if app.tokens < cost {
            app.refusals += 1;
            app.cooldown_until = Some(now + COOLDOWN);
            app.strikes += 1;
            app.last_strike = Some(now);
            if app.strikes >= STRIKES_BEFORE_STOP {
                return Verdict::Stop(Refusal::RateLimited);
            }
            return Verdict::Refuse(Refusal::RateLimited);
        }
        app.tokens -= cost;
        Verdict::Allow
    }

    /// Marks an app's OS dialog as on screen. Called where the dialog is
    /// actually raised, NOT in [`Self::check`] — a request parked behind a
    /// permission prompt has not opened anything yet, and flagging it there
    /// would make the post-grant retry refuse itself.
    pub fn dialog_started(&mut self, app_id: &str) {
        if let Some(app) = self.apps.get_mut(app_id) {
            app.dialog_pending = true;
        }
    }

    /// Marks an app's OS dialog as finished (answered, cancelled or failed).
    pub fn dialog_finished(&mut self, app_id: &str) {
        if let Some(app) = self.apps.get_mut(app_id) {
            app.dialog_pending = false;
        }
    }

    /// How many requests this app has had refused.
    pub fn refusals(&self, app_id: &str) -> u64 {
        self.apps.get(app_id).map(|a| a.refusals).unwrap_or(0)
    }

    /// Wipes an app's record — after a force stop, an uninstall, or the user
    /// deliberately letting it run again.
    pub fn forget(&mut self, app_id: &str) {
        self.apps.remove(app_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(limiter: &mut AbuseLimiter, service: &str, n: usize) -> Vec<Verdict> {
        (0..n).map(|_| limiter.check("t", service, true)).collect()
    }

    /// A burst is fine; a runaway loop is not.
    #[test]
    fn a_loop_runs_out_of_budget() {
        let mut l = AbuseLimiter::default();
        let verdicts = drain(&mut l, "notify.post", 60);
        assert!(
            matches!(verdicts[0], Verdict::Allow),
            "a normal call must go through"
        );
        assert!(
            verdicts.iter().any(|v| matches!(v, Verdict::Refuse(_))),
            "a 60-call loop must be refused somewhere"
        );
        assert!(l.refusals("t") > 0);
    }

    /// Keep slamming into the limit and the host stops asking nicely.
    #[test]
    fn repeated_abuse_escalates_to_a_stop() {
        let mut l = AbuseLimiter::default();
        let mut stopped = false;
        for _ in 0..STRIKES_BEFORE_STOP {
            // Drain, then force the cooldown to lapse so the next burst earns
            // its own strike rather than being refused by the cooldown.
            for _ in 0..80 {
                if let Verdict::Stop(_) = l.check("t", "notify.post", true) {
                    stopped = true;
                }
            }
            if let Some(app) = l.apps.get_mut("t") {
                app.cooldown_until = None;
                app.tokens = BUCKET_CAPACITY;
            }
        }
        assert!(stopped, "four cooldowns in a row must end the app's run");
    }

    /// A backgrounded app must never be able to put OS UI on screen.
    #[test]
    fn dialogs_are_foreground_only_and_one_at_a_time() {
        let mut l = AbuseLimiter::default();
        assert!(matches!(
            l.check("t", "files.pick", false),
            Verdict::Refuse(Refusal::NotForeground)
        ));
        assert!(matches!(l.check("t", "files.pick", true), Verdict::Allow));
        l.dialog_started("t");
        assert!(
            matches!(
                l.check("t", "files.pick", true),
                Verdict::Refuse(Refusal::AlreadyPending)
            ),
            "a second picker while one is open is never legitimate"
        );
        l.dialog_finished("t");
        assert!(matches!(l.check("t", "files.pick", true), Verdict::Allow));
    }

    /// Waiting out a cooldown buys a partial bucket, not a fresh one — the
    /// elapsed-time refill must not quietly credit the cooldown back.
    #[test]
    fn a_lapsed_cooldown_does_not_restore_a_full_bucket() {
        let mut l = AbuseLimiter::default();
        drain(&mut l, "notify.post", 60);
        // Pretend the cooldown elapsed a while ago, as a patient loop would.
        let past = Instant::now() - Duration::from_secs(30);
        if let Some(app) = l.apps.get_mut("t") {
            app.cooldown_until = Some(past);
            app.last_refill = Some(past);
        }
        assert!(matches!(l.check("t", "notify.post", true), Verdict::Allow));
        let tokens = l.apps.get("t").map(|a| a.tokens).unwrap_or_default();
        assert!(
            tokens < BUCKET_CAPACITY / 2.0,
            "came back with {tokens} tokens; a lapsed cooldown must not refill the bucket"
        );
    }

    /// Cheap, ordinary traffic is never punished.
    #[test]
    fn ordinary_use_is_not_rate_limited() {
        let mut l = AbuseLimiter::default();
        for _ in 0..20 {
            assert!(matches!(l.check("t", "env", true), Verdict::Allow));
        }
        assert_eq!(l.refusals("t"), 0);
    }

    /// Forgetting an app gives it a clean slate (force stop, uninstall, or the
    /// user choosing to trust it again).
    #[test]
    fn forget_clears_the_record() {
        let mut l = AbuseLimiter::default();
        drain(&mut l, "files.pick", 40);
        assert!(l.refusals("t") > 0);
        l.forget("t");
        assert_eq!(l.refusals("t"), 0);
        assert!(matches!(l.check("t", "files.pick", true), Verdict::Allow));
    }
}
