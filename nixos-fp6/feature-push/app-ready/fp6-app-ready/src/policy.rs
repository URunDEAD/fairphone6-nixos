// SPDX-License-Identifier: MIT
//! OOM priorities and the low-memory kill order (Android lmkd-like).
//!
//! oom_score_adj (higher = killed first by the kernel / earlyoom):
//!   shell and session services     200 (user units; we never touch them)
//!   foreground (focused) app       300
//!   kept-ready app in background   600
//!   other background app           800 (running) / 900 (frozen, as lomiri-app-launch sets)
//! The foreground app sits above the shell on purpose: if it alone exhausts
//! RAM + zram, the kernel kills it, not Lomiri.
//!
//! Our own killer acts first, on PSI memory pressure: it kills whole
//! background apps (their cgroup), unpinned before kept-ready, least
//! recently used first, never the focused app and never anything that is
//! not a lomiri-app-launch app.

use crate::procs::MemInfo;

pub const ADJ_FOREGROUND: i32 = 300;
pub const ADJ_PINNED: i32 = 600;
pub const ADJ_BACKGROUND: i32 = 800;
pub const ADJ_FROZEN: i32 = 900;

pub fn oom_adj(focused: bool, pinned: bool, frozen: bool) -> i32 {
    if focused {
        ADJ_FOREGROUND
    } else if pinned {
        ADJ_PINNED
    } else if frozen {
        ADJ_FROZEN
    } else {
        ADJ_BACKGROUND
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
pub fn pressure(m: &MemInfo, psi_full_avg10: f64, saver: bool) -> Option<Pressure> {
    if m.total == 0 {
        return None;
    }
    let avail = m.available as f64 / m.total as f64;
    let swap_free = if m.swap_total > 0 { m.swap_free as f64 / m.swap_total as f64 } else { 0.0 };
    let (low_avail, low_swap) = if saver { (0.15, 0.50) } else { (0.10, 0.20) };
    if avail < 0.04 {
        Some(Pressure::Critical)
    } else if avail < low_avail && swap_free < low_swap {
        Some(Pressure::Low)
    } else if psi_full_avg10 > 15.0 && avail < 0.20 {
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
            // unpinned first, then least recently used, then the largest
            (!a.pinned)
                .cmp(&!b.pinned)
                .then(a.idle_s.cmp(&b.idle_s))
                .then(a.memory.cmp(&b.memory))
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, focused: bool, pinned: bool, idle: u64, mem: u64) -> Candidate {
        Candidate { app_id: id.into(), focused, pinned, idle_s: idle, memory: mem }
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
