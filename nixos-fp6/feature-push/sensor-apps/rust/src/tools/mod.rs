//! Feeds: one worker thread per active app that samples its sensors at the
//! app's rate and keeps the latest JSON snapshot for the UI to take.
//!
//! A feed only runs while the UI says it is active (the app is visible and
//! focused); `set_active(false)` stops the thread, closes the devices and
//! ends any capture process, so nothing samples while Lomiri has the app
//! in the background.

pub mod altimeter;
pub mod compass;
pub mod dashboard;
pub mod flashlight;
pub mod gps;
pub mod level;
pub mod light;
pub mod measure;
pub mod metal;
pub mod sound;
pub mod speed;
pub mod steps;
pub mod tone;
pub mod tuner;
pub mod vibration;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub trait Tool: Send {
    /// Sampling period in seconds.
    fn period(&self) -> f64;
    /// Opens devices / processes. Called on the worker thread.
    fn start(&mut self) {}
    /// One sample; returns the snapshot to publish, if any.
    fn tick(&mut self, now: f64) -> Option<String>;
    /// Closes everything. Called on the worker thread.
    fn stop(&mut self) {}
    /// UI commands (also allowed while stopped).
    fn command(&mut self, _name: &str, _arg: &str) -> String {
        "{}".into()
    }
}

pub fn make(kind: &str) -> Option<Box<dyn Tool>> {
    Some(match kind {
        "compass" => Box::new(compass::Compass::new()),
        "level" => Box::new(level::Level::new()),
        "measure" => Box::new(measure::Measure::new()),
        "altimeter" => Box::new(altimeter::Altimeter::new()),
        "sound" => Box::new(sound::Sound::new()),
        "light" => Box::new(light::LightMeter::new()),
        "gps" => Box::new(gps::Gps::new()),
        "dashboard" => Box::new(dashboard::Dashboard::new()),
        // batch 2
        "speed" => Box::new(speed::Speed::new()),
        "metal" => Box::new(metal::Metal::new()),
        "vibration" => Box::new(vibration::Vibration::new()),
        "steps" => Box::new(steps::Steps::new()),
        "torch" => Box::new(flashlight::Flashlight::new()),
        "tuner" => Box::new(tuner::Tuner::new()),
        "tone" => Box::new(tone::Tone::new()),
        _ => return None,
    })
}

struct Shared {
    tool: Mutex<Box<dyn Tool>>,
    latest: Mutex<Option<String>>,
    running: AtomicBool,
}

pub struct Feed {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Feed {
    pub fn new(kind: &str) -> Option<Feed> {
        Some(Feed {
            shared: Arc::new(Shared {
                tool: Mutex::new(make(kind)?),
                latest: Mutex::new(None),
                running: AtomicBool::new(false),
            }),
            thread: None,
        })
    }

    pub fn set_active(&mut self, on: bool) {
        if on {
            if self.thread.is_some() {
                return;
            }
            self.shared.running.store(true, Ordering::SeqCst);
            let sh = self.shared.clone();
            self.thread = std::thread::Builder::new()
                .name("fp6-feed".into())
                .spawn(move || run(sh))
                .ok();
        } else if let Some(t) = self.thread.take() {
            self.shared.running.store(false, Ordering::SeqCst);
            let _ = t.join();
        }
    }

    pub fn take(&self) -> Option<String> {
        self.shared.latest.lock().ok()?.take()
    }

    pub fn command(&self, name: &str, arg: &str) -> String {
        match self.shared.tool.lock() {
            Ok(mut t) => t.command(name, arg),
            Err(_) => "{}".into(),
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.set_active(false);
    }
}

fn run(sh: Arc<Shared>) {
    let mut period = {
        let mut t = sh.tool.lock().unwrap();
        t.start();
        t.period()
    };
    let mut next = crate::util::mono();
    while sh.running.load(Ordering::SeqCst) {
        let now = crate::util::mono();
        if now >= next {
            // the period may change with the tool's mode (flashlight strobe)
            let out = sh.tool.lock().ok().and_then(|mut t| {
                let o = t.tick(now);
                period = t.period();
                o
            });
            if let Some(o) = out {
                if let Ok(mut l) = sh.latest.lock() {
                    *l = Some(o);
                }
            }
            next += period;
            if next < now {
                next = now + period; // fell behind: do not burst
            }
        }
        let wait = (next - crate::util::mono()).clamp(0.001, 0.05).min(period.max(0.001));
        std::thread::sleep(Duration::from_secs_f64(wait));
    }
    if let Ok(mut t) = sh.tool.lock() {
        t.stop();
    }
}
