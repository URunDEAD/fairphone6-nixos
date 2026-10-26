// SPDX-License-Identifier: MIT
//! GSettings com.fairphone6.app-ready as a plain value (read on the GLib main
//! thread, handed to the engine thread).

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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            keep_ready: Vec::new(),
            prelaunch: Vec::new(),
            preload: Vec::new(),
            freeze_background: false,
            preload_budget_mb: 300,
            preload_interval_min: 120,
            mlock_mb: 0,
            zram_enabled: true,
            low_memory_killer: true,
        }
    }
}

impl Config {
    /// Kept ready: lower OOM priority, killed last.
    pub fn pinned(&self, app_id: &str) -> bool {
        self.keep_ready.iter().chain(self.prelaunch.iter()).any(|a| a == app_id)
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
}
