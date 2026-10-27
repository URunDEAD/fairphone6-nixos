use crate::config::Settings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub signal: u8,
    pub loss_percent: f64,
    pub gateway_ms: Option<f64>,
    pub public_ms: Option<f64>,
    pub public_ok: Option<bool>,
    pub captive: bool,
}
impl Metrics {
    pub fn bad(&self, s: &Settings) -> bool {
        self.captive
            || self.public_ok == Some(false)
            || (self.public_ok == Some(true)
                && (self.public_ms.is_some_and(|n| n >= s.thresholds.latency_ms)
                    || ((self.loss_percent >= s.thresholds.loss_percent
                        || self.signal < s.thresholds.weak_signal)
                        && self
                            .gateway_ms
                            .is_some_and(|n| n >= s.thresholds.latency_ms))))
    }
    pub fn recovered(&self, s: &Settings) -> bool {
        self.public_ok == Some(true)
            && !self.captive
            && self.loss_percent < s.thresholds.loss_percent / 2.0
            && self
                .gateway_ms
                .is_some_and(|n| n < s.thresholds.latency_ms / 2.0)
            && self
                .public_ms
                .is_some_and(|n| n < s.thresholds.latency_ms / 2.0)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Route {
    Wifi,
    Mobile,
}
#[derive(Debug, Default)]
pub struct Switcher {
    pub mobile_owned: bool,
    bad: u32,
    good: u32,
    last_change: Option<u64>,
}
impl Switcher {
    /// Returns a proposal. Only commit after the privileged adapter confirms success.
    pub fn observe(
        &mut self,
        now: u64,
        settings: &Settings,
        metrics: Option<Metrics>,
        mobile_available: bool,
    ) -> Option<Route> {
        if !settings.smart_switching
            || (settings.capped_plan && !settings.allow_capped)
            || !mobile_available
        {
            self.bad = 0;
            self.good = 0;
            return self.mobile_owned.then_some(Route::Wifi);
        }
        let Some(m) = metrics else {
            self.bad = 0;
            self.good = 0;
            return None;
        };
        self.bad = if m.bad(settings) {
            self.bad.saturating_add(1)
        } else {
            0
        };
        self.good = if m.recovered(settings) {
            self.good.saturating_add(1)
        } else {
            0
        };
        if self
            .last_change
            .is_some_and(|n| now.saturating_sub(n) < settings.thresholds.cooldown_secs)
        {
            return None;
        }
        if !self.mobile_owned && self.bad >= settings.thresholds.bad_samples {
            Some(Route::Mobile)
        } else if self.mobile_owned && self.good >= settings.thresholds.good_samples {
            Some(Route::Wifi)
        } else {
            None
        }
    }
    pub fn commit(&mut self, now: u64, route: Route) {
        self.mobile_owned = route == Route::Mobile;
        self.last_change = Some(now);
        self.bad = 0;
        self.good = 0;
    }
}
/// Foreground is exempt from both saver and background quota; explicit allow-list
/// exempts saver only. Quotas are background limits on chargeable networks.
pub fn app_allowed(
    settings: &Settings,
    costly: bool,
    app: &str,
    foreground: Option<&str>,
    bytes: u64,
) -> bool {
    if !costly || foreground == Some(app) {
        return true;
    }
    if settings.limit_bytes > 0 && bytes >= settings.limit_bytes {
        return false;
    }
    !settings.data_saver || settings.allow_list.contains(app)
}
/// NM UNKNOWN is conservatively treated as metered; YES and GUESS_YES are 1/3.
pub fn metered(value: u32) -> bool {
    !matches!(value, 2 | 4)
}
