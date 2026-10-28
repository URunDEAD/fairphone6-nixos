// SPDX-License-Identifier: MIT
//! OOM priorities and the low-memory kill order (Android lmkd-like).
//!
//! oom_score_adj (higher = killed first by the kernel / earlyoom):
//!   shell and session services     200 (user units; we never touch them)
//!   foreground (focused) app       300
//!   kept-ready app in background   600 (the user's "always keep")
//!   predicted app in background    650..700 (more likely = lower)
//!   other background app           800 (running) / 900 (frozen, as lomiri-app-launch sets)
//!   "never keep" app in background 950
//! The foreground app sits above the shell on purpose: if it alone exhausts
//! RAM + zram, the kernel kills it, not Lomiri.
//!
//! Our own killer acts first, on PSI memory pressure: it kills whole
//! background apps (their cgroup) in class order - "never keep", plain
//! background, predicted, always keep - and inside a class the app least
//! likely to be used next first (prediction), then the least recently used,
//! then the largest; never the focused app and never anything that is not a
//! lomiri-app-launch app.

use crate::procs::MemInfo;

pub const ADJ_FOREGROUND: i32 = 300;
pub const ADJ_PINNED: i32 = 600;
pub const ADJ_PREDICTED: i32 = 650;
pub const ADJ_BACKGROUND: i32 = 800;
pub const ADJ_FROZEN: i32 = 900;
pub const ADJ_NEVER: i32 = 950;

/// Kill order class: lower is closed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Never = 0,
    Background = 1,
    Predicted = 2,
    Pinned = 3,
}

#[cfg(test)]
pub fn oom_adj(focused: bool, pinned: bool, frozen: bool) -> i32 {
    oom_adj_class(focused, if pinned { Class::Pinned } else { Class::Background }, frozen, 0.0)
}

/// `likelihood`: predicted probability (0..1) of a predicted app.
pub fn oom_adj_class(focused: bool, class: Class, frozen: bool, likelihood: f64) -> i32 {
    if focused {
        return ADJ_FOREGROUND;
    }
    match class {
        Class::Pinned => ADJ_PINNED,
        Class::Predicted => ADJ_PREDICTED + (50.0 * (1.0 - likelihood.clamp(0.0, 1.0))).round() as i32,
        Class::Never => ADJ_NEVER,
        Class::Background if frozen => ADJ_FROZEN,
        Class::Background => ADJ_BACKGROUND,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressure {
    /// Below 4 % available: kill even with free swap.
    Critical,
    /// Below 10 % available and swap (zram) almost full.
    Low,
    /// Heavy stall (PSI full avg10 > 15 %) with less than 20 % available.
    Thrashing,
}

/// `saver`: battery saver is on: close background apps earlier (less
/// swapping in and out = less CPU) - below 15 % available with zram half full.
#[cfg(test)]
pub fn pressure(m: &MemInfo, psi_full_avg10: f64, saver: bool) -> Option<Pressure> {
    pressure_with(m, psi_full_avg10, saver, &Limits::default())
}

/// The profile's thresholds (profile.rs); the 4 % critical line is fixed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub low_avail: f64,
    pub low_swap: f64,
    pub thrash_full: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { low_avail: 0.10, low_swap: 0.20, thrash_full: 15.0 }
    }
}

pub fn pressure_with(m: &MemInfo, psi_full_avg10: f64, saver: bool, lim: &Limits) -> Option<Pressure> {
    if m.total == 0 {
        return None;
    }
    let avail = m.available as f64 / m.total as f64;
    let swap_free = if m.swap_total > 0 { m.swap_free as f64 / m.swap_total as f64 } else { 0.0 };
    let (low_avail, low_swap, thrash) = if saver { (0.15, 0.50, 15.0) } else { (lim.low_avail.max(0.05), lim.low_swap, lim.thrash_full) };
    if avail < 0.04 {
        Some(Pressure::Critical)
    } else if avail < low_avail && swap_free < low_swap {
        Some(Pressure::Low)
    } else if psi_full_avg10 > thrash && avail < 0.20 {
        Some(Pressure::Thrashing)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub app_id: String,
    pub focused: bool,
    pub pinned: bool,
    pub class: Class,
    /// Predicted probability of being used next (0 = unknown / unlikely).
    pub likelihood: f64,
    /// Seconds since the app last had focus (u64::MAX = never).
    pub idle_s: u64,
    pub memory: u64,
}

/// Index of the app to kill next, if any.
pub fn victim(c: &[Candidate]) -> Option<usize> {
    c.iter()
        .enumerate()
        .filter(|(_, a)| !a.focused)
        .max_by(|(_, a), (_, b)| {
            // lowest class first, then the least likely, then least recently used, then the largest
            b.class
                .cmp(&a.class)
                .then(b.likelihood.total_cmp(&a.likelihood))
                .then(a.idle_s.cmp(&b.idle_s))
                .then(a.memory.cmp(&b.memory))
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, focused: bool, pinned: bool, idle: u64, mem: u64) -> Candidate {
        let class = if pinned { Class::Pinned } else { Class::Background };
        Candidate { app_id: id.into(), focused, pinned, class, likelihood: 0.0, idle_s: idle, memory: mem }
    }

    fn pcand(id: &str, class: Class, likelihood: f64, idle: u64) -> Candidate {
        Candidate { app_id: id.into(), focused: false, pinned: class == Class::Pinned, class, likelihood, idle_s: idle, memory: 100 }
    }

    #[test]
    fn kill_order_by_prediction() {
        let apps = vec![
            pcand("pinned", Class::Pinned, 0.0, 9000),
            pcand("likely", Class::Predicted, 0.4, 3000),
            pcand("unlikely", Class::Predicted, 0.05, 10),
            pcand("plain", Class::Background, 0.2, 5),
            pcand("never", Class::Never, 0.9, 1),
        ];
        let mut left = apps.clone();
        let mut order = Vec::new();
        while let Some(i) = victim(&left) {
            order.push(left.remove(i).app_id);
        }
        assert_eq!(order, ["never", "plain", "unlikely", "likely", "pinned"]);
        // Same class and likelihood: least recently used first.
        let apps = vec![pcand("new", Class::Background, 0.1, 5), pcand("old", Class::Background, 0.1, 500)];
        assert_eq!(apps[victim(&apps).unwrap()].app_id, "old");
        // In a class, likelihood beats recency.
        let apps = vec![pcand("old-likely", Class::Background, 0.5, 5000), pcand("new-unlikely", Class::Background, 0.01, 1)];
        assert_eq!(apps[victim(&apps).unwrap()].app_id, "new-unlikely");
    }

    #[test]
    fn adj_classes() {
        assert_eq!(oom_adj_class(true, Class::Never, true, 0.0), ADJ_FOREGROUND);
        assert!(oom_adj_class(false, Class::Pinned, true, 0.0) < oom_adj_class(false, Class::Predicted, true, 0.9));
        assert!(oom_adj_class(false, Class::Predicted, true, 0.9) < oom_adj_class(false, Class::Predicted, true, 0.1));
        assert!(oom_adj_class(false, Class::Predicted, true, 0.0) < oom_adj_class(false, Class::Background, false, 0.0));
        assert!(oom_adj_class(false, Class::Background, true, 0.0) < oom_adj_class(false, Class::Never, false, 0.0));
        assert!(oom_adj_class(false, Class::Never, false, 0.0) <= 1000);
    }

    #[test]
    fn profile_limits() {
        let gib = 1u64 << 30;
        let m = |avail: f64, swap_free: f64| MemInfo {
            total: 7 * gib,
            available: (7.0 * gib as f64 * avail) as u64,
            swap_total: 5 * gib,
            swap_free: (5.0 * gib as f64 * swap_free) as u64,
        };
        let aggressive = Limits { low_avail: 0.07, low_swap: 0.15, thrash_full: 20.0 };
        assert_eq!(pressure_with(&m(0.08, 0.1), 0.0, false, &aggressive), None);
        assert_eq!(pressure_with(&m(0.06, 0.1), 0.0, false, &aggressive), Some(Pressure::Low));
        assert_eq!(pressure_with(&m(0.15, 0.9), 18.0, false, &aggressive), None);
        assert_eq!(pressure_with(&m(0.15, 0.9), 25.0, false, &aggressive), Some(Pressure::Thrashing));
        // critical stays at 4 % whatever the profile says
        let silly = Limits { low_avail: 0.0, low_swap: 0.0, thrash_full: 100.0 };
        assert_eq!(pressure_with(&m(0.03, 1.0), 0.0, false, &silly), Some(Pressure::Critical));
    }

    #[test]
    fn adj_order() {
        assert!(oom_adj(true, true, false) < oom_adj(false, true, true));
        assert!(oom_adj(false, true, true) < oom_adj(false, false, false));
        assert!(oom_adj(false, false, false) < oom_adj(false, false, true));
        assert!(ADJ_FOREGROUND > 200);
    }

    #[test]
    fn kill_order() {
        let apps = vec![
            cand("fg", true, false, 0, 900),
            cand("pinned-old", false, true, 5000, 500),
            cand("bg-new", false, false, 10, 100),
            cand("bg-old", false, false, 600, 50),
        ];
        assert_eq!(apps[victim(&apps).unwrap()].app_id, "bg-old");
        let apps = vec![cand("fg", true, false, 0, 900), cand("pinned", false, true, 1, 1)];
        assert_eq!(apps[victim(&apps).unwrap()].app_id, "pinned");
        assert_eq!(victim(&[cand("fg", true, false, 0, 1)]), None);
        assert_eq!(victim(&[]), None);
    }

    #[test]
    fn pressure_levels() {
        let gib = 1u64 << 30;
        let m = |avail: f64, swap_free: f64| MemInfo {
            total: 7 * gib,
            available: (7.0 * gib as f64 * avail) as u64,
            swap_total: 3 * gib,
            swap_free: (3.0 * gib as f64 * swap_free) as u64,
        };
        assert_eq!(pressure(&m(0.5, 1.0), 0.0, false), None);
        assert_eq!(pressure(&m(0.08, 0.5), 0.0, false), None);
        assert_eq!(pressure(&m(0.08, 0.1), 0.0, false), Some(Pressure::Low));
        assert_eq!(pressure(&m(0.03, 0.9), 0.0, false), Some(Pressure::Critical));
        assert_eq!(pressure(&m(0.15, 0.9), 30.0, false), Some(Pressure::Thrashing));
        let no_swap = MemInfo { swap_total: 0, swap_free: 0, ..m(0.09, 0.0) };
        assert_eq!(pressure(&no_swap, 0.0, false), Some(Pressure::Low));
        // battery saver: earlier
        assert_eq!(pressure(&m(0.12, 0.4), 0.0, false), None);
        assert_eq!(pressure(&m(0.12, 0.4), 0.0, true), Some(Pressure::Low));
    }
}
