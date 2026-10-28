use crate::{
    config::Settings,
    helper::{Snapshot, NAME as HELPER, PATH as HELPER_PATH},
    policy::{Route, Switcher},
    runtime::{notify, read_json, save_json, user_path},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const NAME: &str = "org.fairphone.NetPolicy1";
pub const PATH: &str = "/org/fairphone/NetPolicy1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub settings: Settings,
    pub snapshot: Snapshot,
    pub foreground: String,
    pub route: String,
    pub message: String,
    /// Why the policy cannot work on this system (empty: it can), e.g.
    /// "kernel lacks nft socket matching (CONFIG_NFT_SOCKET)".
    #[serde(default)]
    pub unavailable: String,
    pub metrics: Option<crate::policy::Metrics>,
}
struct State {
    status: Status,
    foreground_seen: Option<Instant>,
    config_path: PathBuf,
}
struct Api {
    state: Arc<Mutex<State>>,
}
fn failed(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
#[zbus::interface(name = "org.fairphone.NetPolicy1")]
impl Api {
    fn get_status(&self) -> zbus::fdo::Result<String> {
        serde_json::to_string(&self.state.lock().map_err(failed)?.status).map_err(failed)
    }
    fn set_settings(&self, json: &str) -> zbus::fdo::Result<()> {
        if json.len() > 65536 {
            return Err(failed("settings too large"));
        }
        let settings: Settings = serde_json::from_str(json).map_err(failed)?;
        settings.validate().map_err(failed)?;
        let mut state = self.state.lock().map_err(failed)?;
        save_json(&state.config_path, &settings).map_err(failed)?;
        state.status.settings = settings;
        Ok(())
    }
    fn set_data_saver(&self, enabled: bool) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().map_err(failed)?;
        let mut settings = state.status.settings.clone();
        settings.data_saver = enabled;
        save_json(&state.config_path, &settings).map_err(failed)?;
        state.status.settings = settings;
        Ok(())
    }
    /// Lomiri coordinator sends the app id on focus changes and every five seconds.
    /// Empty string means no foreground application. Session bus is a same-UID trust boundary.
    fn set_foreground(&self, app: &str) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().map_err(failed)?;
        if !app.is_empty() && !state.status.snapshot.apps.iter().any(|a| a == app) {
            return Err(failed("app is not configured"));
        }
        state.status.foreground = app.into();
        state.foreground_seen = Some(Instant::now());
        Ok(())
    }
}
pub fn serve() -> Result<()> {
    let config_path = user_path("XDG_CONFIG_HOME", ".config", "settings.json")?;
    let settings: Settings = if config_path.exists() {
        read_json(&config_path)?
    } else {
        Settings::default()
    };
    settings.validate()?;
    let state = Arc::new(Mutex::new(State {
        status: Status {
            settings,
            message: "Waiting for system helper".into(),
            ..Status::default()
        },
        foreground_seen: None,
        config_path,
    }));
    let session = zbus::blocking::connection::Builder::session()?
        .name(NAME)?
        .serve_at(
            PATH,
            Api {
                state: state.clone(),
            },
        )?
        .build()?;
    let bus = crate::runtime::system()?;
    let mut switcher = Switcher::default();
    let start = Instant::now();
    let mut last_warning_month = String::new();
    let mut warned = std::collections::BTreeSet::new();
    let mut last_error = String::new();
    let mut last_sample = 0;
    loop {
        let mut unavailable = String::new();
        let (settings, foreground) = {
            let mut state = state
                .lock()
                .map_err(|_| anyhow::anyhow!("daemon mutex poisoned"))?;
            if state
                .foreground_seen
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(15))
            {
                state.status.foreground.clear();
            }
            (
                state.status.settings.clone(),
                state.status.foreground.clone(),
            )
        };
        let result = (|| -> Result<(Snapshot, Option<crate::policy::Metrics>)> {
            let helper = zbus::blocking::Proxy::new(&bus, HELPER, HELPER_PATH, HELPER)?;
            let raw: String = helper.call(
                "Sync",
                &(serde_json::to_string(&settings)?, foreground.clone()),
            )?;
            let mut snapshot: Snapshot = serde_json::from_str(&raw)?;
            unavailable = snapshot.unavailable.clone();
            anyhow::ensure!(unavailable.is_empty(), "unavailable: {unavailable}");
            anyhow::ensure!(snapshot.error.is_empty(), "{}", snapshot.error);
            switcher.mobile_owned = snapshot.mobile.owned;
            let metrics = snapshot.metrics;
            let now = start.elapsed().as_secs();
            let proposal = if snapshot.sample_id != last_sample
                || !settings.smart_switching
                || (settings.capped_plan && !settings.allow_capped)
                || !snapshot.mobile.available
            {
                last_sample = snapshot.sample_id;
                switcher.observe(now, &settings, metrics, snapshot.mobile.available)
            } else {
                None
            };
            if let Some(route) = proposal {
                helper.call::<_, _, ()>("SetMobilePreferred", &(route == Route::Mobile))?;
                switcher.commit(now, route);
                snapshot.mobile.owned = route == Route::Mobile;
                notify(
                    &session,
                    "Network preference changed",
                    if route == Route::Mobile {
                        "Wi-Fi is connected but unusable. Using mobile data; background data restrictions still apply."
                    } else {
                        "Wi-Fi recovered or mobile fallback was disabled. Restored your previous mobile preference."
                    },
                );
            }
            Ok((snapshot, metrics))
        })();
        let mut state = state
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon mutex poisoned"))?;
        match result {
            Ok((snapshot, metrics)) => {
                if last_warning_month != snapshot.usage.month {
                    warned.clear();
                    last_warning_month = snapshot.usage.month.clone();
                }
                for (app, bytes) in &snapshot.usage.totals {
                    if settings.warning_bytes > 0
                        && *bytes >= settings.warning_bytes
                        && warned.insert(app.clone())
                    {
                        notify(&session, "Monthly data warning", &format!("{app} has used at least {} MB on metered/mobile links this UTC month.", bytes / 1_000_000));
                    }
                }
                state.status.route = if snapshot.mobile.owned {
                    "Mobile preferred"
                } else {
                    "Normal routing"
                }
                .into();
                state.status.message =
                    if settings.smart_switching && settings.capped_plan && !settings.allow_capped {
                        "Automatic fallback blocked: capped plan"
                    } else {
                        "Policy active"
                    }
                    .into();
                state.status.snapshot = snapshot;
                state.status.metrics = metrics;
                state.status.unavailable.clear();
                last_error.clear();
            }
            Err(error) => {
                let error = format!("{error:#}");
                state.status.message = if unavailable.is_empty() {
                    format!("Policy unavailable: {error}")
                } else {
                    format!("Unavailable: {unavailable}")
                };
                state.status.unavailable = unavailable;
                if error != last_error {
                    eprintln!("netpolicyd: {error}");
                    notify(&session, "Network policy needs attention", &error);
                    last_error = error;
                }
            }
        }
        drop(state);
        std::thread::sleep(Duration::from_secs(5));
    }
}
pub fn call(method: &str, value: Option<&str>) -> Result<String> {
    let bus = crate::runtime::session()?;
    let proxy = zbus::blocking::Proxy::new(&bus, NAME, PATH, NAME)?;
    match method {
        "status" => Ok(proxy.call("GetStatus", &())?),
        "settings" => {
            proxy.call::<_, _, ()>("SetSettings", &(value.context("settings requires JSON")?))?;
            Ok("Saved".into())
        }
        "foreground" => {
            proxy.call::<_, _, ()>("SetForeground", &(value.unwrap_or("")))?;
            Ok("Updated".into())
        }
        "saver" => {
            let v = value.context("saver requires on/off")?;
            anyhow::ensure!(v == "on" || v == "off", "saver requires on/off");
            proxy.call::<_, _, ()>("SetDataSaver", &(v == "on"))?;
            Ok("Updated".into())
        }
        _ => anyhow::bail!(
            "usage: netpolicyctl status | settings JSON | foreground APP_ID | saver on|off"
        ),
    }
}
