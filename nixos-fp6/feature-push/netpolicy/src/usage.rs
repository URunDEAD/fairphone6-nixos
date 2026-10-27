use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub boot_id: String,
    pub month: String,
    pub totals: BTreeMap<String, u64>,
    pub last: BTreeMap<String, u64>,
    pub warned: BTreeMap<String, bool>,
}
impl Usage {
    /// Raw cumulative counters persist across rule changes. A lower reading means
    /// a kernel reset; charge the new value once, never unsigned-underflow.
    pub fn sample(&mut self, month: &str, raw: &BTreeMap<String, u64>) {
        if self.month != month {
            self.month = month.to_owned();
            self.totals.clear();
            self.warned.clear();
            // Boundary interval belongs to the new UTC month; retain baselines.
        }
        for (app, bytes) in raw {
            let before = self.last.insert(app.clone(), *bytes).unwrap_or(0);
            let delta = bytes.checked_sub(before).unwrap_or(*bytes);
            let total = self.totals.entry(app.clone()).or_default();
            *total = total.saturating_add(delta);
        }
    }
    pub fn warnings(&mut self, threshold: u64) -> Vec<String> {
        if threshold == 0 {
            return vec![];
        }
        let mut result = vec![];
        for (app, total) in &self.totals {
            if *total >= threshold && !self.warned.get(app).copied().unwrap_or(false) {
                self.warned.insert(app.clone(), true);
                result.push(app.clone());
            }
        }
        result
    }
}
