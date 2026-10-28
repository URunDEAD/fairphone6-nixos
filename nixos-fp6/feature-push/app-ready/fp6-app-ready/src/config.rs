// SPDX-License-Identifier: MIT
//! GSettings com.fairphone6.app-ready as a plain value (read on the GLib main
//! thread, handed to the engine thread).

use crate::profile::{Profile, Tuning};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Kept in memory, frozen in the background, lower OOM priority.
    pub keep_ready: Vec<String>,
    /// Started hidden at login (implies keep-ready).
    pub prelaunch: Vec<String>,
    /// Files warmed in the page cache at login / idle.
    pub preload: Vec<String>,
    /// Freeze every background app (not only the kept-ready ones).
    pub freeze_background: bool,
    pub preload_budget_mb: u32,
    pub preload_interval_min: u32,
    pub mlock_mb: u32,
    pub zram_enabled: bool,
    pub low_memory_killer: bool,
    /// Settings > Memory & performance profile.
    pub profile: Profile,
    /// Learn app usage and keep the likely next apps ready / warm.
    pub prediction: bool,
    /// Predicted apps kept ready; None = the profile's number.
    pub ready_apps: Option<usize>,
    /// Warm the shell's and the predicted apps' files once the session is idle after login.
    pub preload_at_boot: bool,
    /// zram size in % of RAM; None = the profile's size.
    pub zram_percent: Option<u32>,
    /// Never kept ready, prelaunched or warmed by prediction; closed first when memory is low.
    pub never_keep: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            keep_ready: Vec::new(),
            prelaunch: Vec::new(),
            preload: Vec::new(),
            freeze_background: false,
            preload_budget_mb: 400,
            preload_interval_min: 120,
            mlock_mb: 0,
            zram_enabled: true,
            low_memory_killer: true,
            profile: Profile::Aggressive,
            prediction: true,
            ready_apps: None,
            preload_at_boot: true,
            zram_percent: None,
            never_keep: Vec::new(),
        }
    }
}

impl Config {
    /// Kept ready by the user ("always keep"): lower OOM priority, killed last.
    pub fn pinned(&self, app_id: &str) -> bool {
        self.keep_ready.iter().chain(self.prelaunch.iter()).any(|a| a == app_id)
    }

    pub fn never(&self, app_id: &str) -> bool {
        self.never_keep.iter().any(|a| a == app_id)
    }

    pub fn tuning(&self) -> Tuning {
        self.profile.tuning()
    }

    /// Number of predicted apps kept ready (0 with prediction off).
    pub fn ready_apps(&self) -> usize {
        if !self.prediction {
            return 0;
        }
        self.ready_apps.unwrap_or(self.tuning().ready_apps).min(12)
    }

    pub fn zram_percent(&self) -> u32 {
        self.zram_percent.unwrap_or(self.tuning().zram_percent)
    }

    /// Lomiri suspends it in the background (Stage.qml fp6Frozen()).
    pub fn frozen(&self, app_id: &str) -> bool {
        self.freeze_background || self.pinned(app_id)
    }

    /// Apps whose files are preloaded: the preload list plus everything kept ready
    /// is NOT implied (preload is its own choice), deduplicated, in list order.
    pub fn preload_apps(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in &self.preload {
            if !out.contains(a) {
                out.push(a.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_and_frozen() {
        let mut c = Config { keep_ready: vec!["a".into()], prelaunch: vec!["b".into()], ..Config::default() };
        assert!(c.pinned("a") && c.pinned("b") && !c.pinned("c"));
        assert!(!c.frozen("c"));
        c.freeze_background = true;
        assert!(c.frozen("c"));
    }

    #[test]
    fn profile_defaults_and_overrides() {
        let mut c = Config::default();
        assert_eq!(c.profile, Profile::Aggressive);
        assert_eq!(c.ready_apps(), Profile::Aggressive.tuning().ready_apps);
        c.ready_apps = Some(2);
        assert_eq!(c.ready_apps(), 2);
        c.ready_apps = Some(99);
        assert_eq!(c.ready_apps(), 12);
        c.prediction = false;
        assert_eq!(c.ready_apps(), 0);
        assert_eq!(c.zram_percent(), 75);
        c.zram_percent = Some(25);
        assert_eq!(c.zram_percent(), 25);
        c.never_keep = vec!["x".into()];
        assert!(c.never("x") && !c.never("y"));
    }
}
