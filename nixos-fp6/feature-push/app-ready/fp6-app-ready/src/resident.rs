// SPDX-License-Identifier: MIT
//! Keep the user's "always keep ready" apps (keep-ready and prelaunch lists)
//! resident: start them hidden after login, and start them hidden again a
//! while after they went away (closed by the user in the app switcher,
//! closed by the low-memory killer, or crashed), as long as memory and CPU
//! allow. Before 2026-10-28 only the "prelaunch" list was started, once, at
//! login: keep-ready apps were only kept frozen if the user happened to
//! open them, and were gone for good after a swipe-close.
//!
//! The profile decides: Conservative never starts apps on its own, Balanced
//! starts a closed app again after 5 minutes, Aggressive after 1 minute.
//! An app the low-memory killer closed waits 10 minutes and needs 10 points
//! more free memory than the profile's prelaunch floor (no kill / restart
//! loop). An app that dies within 30 s of our hidden start 3 times in a
//! row is left alone until the user opens it or changes the list.
//!
//! Pure logic here (unit tested); engine.rs feeds it the live state.

use std::time::Duration;

use crate::profile::Profile;

/// Not before this long after the daemon started (login prelaunch, indicators).
pub const LOGIN_SETTLE: Duration = Duration::from_secs(60);
/// One hidden start at a time; the next one after the previous has drawn and frozen.
pub const LAUNCH_SPACING: Duration = Duration::from_secs(10);
/// After the low-memory killer closed the app.
pub const KILLED_QUIET: Duration = Duration::from_secs(10 * 60);
/// Extra free memory (points of RAM) needed to restart an app the killer closed.
pub const KILLED_EXTRA_AVAIL_PCT: u32 = 10;
/// Dying this soon after our hidden start counts as a failed start ...
pub const QUICK_EXIT: Duration = Duration::from_secs(30);
/// ... and this many in a row stop the restarts.
pub const MAX_FAILURES: u32 = 3;
/// Too little memory, PSI or CPU busy: look again after this.
pub const RETRY: Duration = Duration::from_secs(60);
/// The user is switching apps: wait for a quiet moment.
pub const USER_ACTIVE_QUIET: Duration = Duration::from_secs(8);
pub const MAX_LOAD: f64 = 3.0;
/// PSI memory "some" avg10 (%) above which nothing new is started.
pub const MAX_PSI_SOME: f64 = 1.0;

/// How long after the user closed a kept-ready app it is started hidden
/// again; None: the profile does not start apps on its own.
pub fn relaunch_after(profile: Profile) -> Option<Duration> {
    match profile {
        Profile::Conservative => None,
        Profile::Balanced => Some(Duration::from_secs(5 * 60)),
        Profile::Aggressive => Some(Duration::from_secs(60)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gone {
    /// Closed by the user (app switcher), crashed, or the session ended it.
    Closed,
    /// Closed by our low-memory killer.
    Killed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppState {
    pub app: String,
    pub running: bool,
    /// Why and how long ago it last went away (None: not seen go away).
    pub gone: Option<(Gone, Duration)>,
    /// Failed hidden starts in a row.
    pub failures: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    /// The profile's relaunch delay (relaunch_after); None = off.
    pub relaunch_after: Option<Duration>,
    pub saver: bool,
    pub since_start: Duration,
    pub avail_pct: u32,
    /// The profile's prelaunch_min_avail_pct.
    pub min_avail_pct: u32,
    pub psi_some: f64,
    pub load: f64,
    pub since_focus_change: Option<Duration>,
    pub since_last_launch: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Next {
    /// Start this app hidden now.
    Launch(String),
    /// Look again after this long.
    Wait(Duration),
    /// Nothing to do until something changes (an app goes away, settings, saver off).
    Idle,
}

/// When `a` may be started again (zero = now); None = not wanted.
fn due_in(a: &AppState, relaunch: Duration) -> Option<Duration> {
    if a.running || a.failures >= MAX_FAILURES {
        return None;
    }
    let wait = match a.gone {
        None => Duration::ZERO,
        Some((Gone::Closed, ago)) => relaunch.saturating_sub(ago),
        Some((Gone::Killed, ago)) => KILLED_QUIET.max(relaunch).saturating_sub(ago),
    };
    Some(wait)
}

pub fn next(apps: &[AppState], env: &Env) -> Next {
    let Some(relaunch) = env.relaunch_after else { return Next::Idle };
    if env.saver {
        return Next::Idle;
    }
    let mut first_due: Option<&AppState> = None;
    let mut soonest: Option<Duration> = None;
    for a in apps {
        match due_in(a, relaunch) {
            Some(d) if d.is_zero() => {
                first_due.get_or_insert(a);
            }
            Some(d) => soonest = Some(soonest.map_or(d, |s| s.min(d))),
            None => {}
        }
    }
    let Some(app) = first_due else {
        return soonest.map_or(Next::Idle, Next::Wait);
    };
    if env.since_start < LOGIN_SETTLE {
        return Next::Wait(LOGIN_SETTLE - env.since_start);
    }
    if let Some(ago) = env.since_last_launch.filter(|d| *d < LAUNCH_SPACING) {
        return Next::Wait(LAUNCH_SPACING - ago);
    }
    if let Some(ago) = env.since_focus_change.filter(|d| *d < USER_ACTIVE_QUIET) {
        return Next::Wait(USER_ACTIVE_QUIET - ago + Duration::from_secs(2));
    }
    let floor = env.min_avail_pct
        + if matches!(app.gone, Some((Gone::Killed, _))) { KILLED_EXTRA_AVAIL_PCT } else { 0 };
    if env.avail_pct < floor || env.psi_some > MAX_PSI_SOME || env.load > MAX_LOAD {
        return Next::Wait(RETRY);
    }
    Next::Launch(app.app.clone())
}

/// The app went away `ran` after it appeared: a failed hidden start if we
/// started it and it died quickly; a normal run resets the count.
pub fn failures_after_exit(failures: u32, started_by_us: bool, ran: Duration) -> u32 {
    if started_by_us && ran < QUICK_EXIT {
        failures + 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            relaunch_after: relaunch_after(Profile::Aggressive),
            saver: false,
            since_start: Duration::from_secs(600),
            avail_pct: 60,
            min_avail_pct: Profile::Aggressive.tuning().prelaunch_min_avail_pct,
            psi_some: 0.0,
            load: 0.5,
            since_focus_change: Some(Duration::from_secs(120)),
            since_last_launch: None,
        }
    }

    fn app(id: &str, running: bool, gone: Option<(Gone, u64)>) -> AppState {
        AppState { app: id.into(), running, gone: gone.map(|(g, s)| (g, Duration::from_secs(s))), failures: 0 }
    }

    #[test]
    fn keep_ready_apps_not_running_after_login_are_started_in_list_order() {
        let apps = [app("camera", true, None), app("morph", false, None), app("dialer", false, None)];
        assert_eq!(next(&apps, &env()), Next::Launch("morph".into()));
    }

    #[test]
    fn waits_for_the_session_to_settle_after_login() {
        let e = Env { since_start: Duration::from_secs(20), ..env() };
        assert_eq!(next(&[app("morph", false, None)], &e), Next::Wait(Duration::from_secs(40)));
    }

    #[test]
    fn all_running_is_idle() {
        assert_eq!(next(&[app("a", true, None), app("b", true, None)], &env()), Next::Idle);
        assert_eq!(next(&[], &env()), Next::Idle);
    }

    #[test]
    fn user_closed_app_is_started_again_after_the_profile_delay() {
        let apps = [app("morph", false, Some((Gone::Closed, 20)))];
        assert_eq!(next(&apps, &env()), Next::Wait(Duration::from_secs(40)));
        let apps = [app("morph", false, Some((Gone::Closed, 61)))];
        assert_eq!(next(&apps, &env()), Next::Launch("morph".into()));
        let balanced = Env { relaunch_after: relaunch_after(Profile::Balanced), ..env() };
        assert_eq!(next(&apps, &balanced), Next::Wait(Duration::from_secs(239)));
    }

    #[test]
    fn conservative_profile_and_battery_saver_start_nothing() {
        let apps = [app("morph", false, None)];
        let c = Env { relaunch_after: relaunch_after(Profile::Conservative), ..env() };
        assert_eq!(next(&apps, &c), Next::Idle);
        assert_eq!(next(&apps, &Env { saver: true, ..env() }), Next::Idle);
    }

    #[test]
    fn killed_app_waits_longer_and_needs_more_free_memory() {
        let apps = [app("morph", false, Some((Gone::Killed, 120)))];
        assert_eq!(next(&apps, &env()), Next::Wait(Duration::from_secs(480)));
        let apps = [app("morph", false, Some((Gone::Killed, 700)))];
        let tight = Env { avail_pct: env().min_avail_pct + 5, ..env() };
        assert_eq!(next(&apps, &tight), Next::Wait(RETRY));
        let roomy = Env { avail_pct: env().min_avail_pct + KILLED_EXTRA_AVAIL_PCT, ..env() };
        assert_eq!(next(&apps, &roomy), Next::Launch("morph".into()));
    }

    #[test]
    fn memory_pressure_or_busy_cpu_defers() {
        let apps = [app("morph", false, None)];
        assert_eq!(next(&apps, &Env { avail_pct: 10, ..env() }), Next::Wait(RETRY));
        assert_eq!(next(&apps, &Env { psi_some: 5.0, ..env() }), Next::Wait(RETRY));
        assert_eq!(next(&apps, &Env { load: 6.0, ..env() }), Next::Wait(RETRY));
    }

    #[test]
    fn spacing_and_user_activity_defer() {
        let apps = [app("morph", false, None)];
        let e = Env { since_last_launch: Some(Duration::from_secs(4)), ..env() };
        assert_eq!(next(&apps, &e), Next::Wait(Duration::from_secs(6)));
        let e = Env { since_focus_change: Some(Duration::from_secs(3)), ..env() };
        assert_eq!(next(&apps, &e), Next::Wait(Duration::from_secs(7)));
    }

    #[test]
    fn soonest_pending_app_sets_the_wait() {
        let apps = [app("a", false, Some((Gone::Closed, 10))), app("b", false, Some((Gone::Closed, 50)))];
        assert_eq!(next(&apps, &env()), Next::Wait(Duration::from_secs(10)));
    }

    #[test]
    fn repeated_quick_deaths_stop_the_restarts() {
        let mut f = 0;
        for _ in 0..MAX_FAILURES {
            f = failures_after_exit(f, true, Duration::from_secs(3));
        }
        assert_eq!(f, MAX_FAILURES);
        let a = AppState { failures: f, ..app("broken", false, None) };
        assert_eq!(next(&[a.clone()], &env()), Next::Idle);
        // A real run (or one the user started) resets the count.
        assert_eq!(failures_after_exit(f, true, Duration::from_secs(300)), 0);
        assert_eq!(failures_after_exit(f, false, Duration::from_secs(3)), 0);
        // Others still get started.
        assert_eq!(next(&[a, app("ok", false, None)], &env()), Next::Launch("ok".into()));
    }
}
