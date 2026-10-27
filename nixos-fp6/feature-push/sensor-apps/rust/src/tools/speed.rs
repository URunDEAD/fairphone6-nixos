//! Speedometer: trip computer over GNSS fixes. The UI passes each fix from
//! QtPositioning (GeoClue, authorized by the location agent only while
//! Location is on); this feed reports the location switch state and the
//! trip values. No position is stored or sent anywhere.

use super::Tool;
use crate::trip::{Fix, Trip};
use crate::util::{parse_floats, Json};

pub struct Speed {
    trip: Trip,
    started: f64,
}

impl Speed {
    pub fn new() -> Speed {
        Speed { trip: Trip::new(), started: f64::NAN }
    }
}

impl Default for Speed {
    fn default() -> Self {
        Speed::new()
    }
}

impl Tool for Speed {
    fn period(&self) -> f64 {
        0.5
    }
    fn start(&mut self) {
        if self.started.is_nan() {
            self.started = crate::util::mono();
        }
    }
    fn tick(&mut self, _now: f64) -> Option<String> {
        let mut j = Json::new();
        let t = &self.trip;
        let (_, hv) = t.history.downsample(120);
        j.str("location", super::gps::location_state())
            .num("speed", t.speed)
            .num("distance", t.distance)
            .num("movingTime", t.moving_time)
            .num("max", t.max_speed)
            .num("average", t.average())
            .boolean("paused", t.paused)
            .nums("history", &hv);
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        match name {
            // "t_ms,lat,lon,speed_mps,acc_m" (NaN for unknown)
            "fix" => {
                let v = parse_floats(arg);
                if v.len() == 5 {
                    self.trip.add(Fix { t: v[0] / 1000.0, lat: v[1], lon: v[2], speed: v[3], acc: v[4] });
                }
            }
            "reset" => self.trip.reset(),
            "pause" => self.trip.paused = arg.trim() == "true",
            // laptop preview only: a synthetic NMEA file for QtPositioning
            "demoNmea" => {
                return Json::new().str("file", &std::env::var("FP6_NMEA_DEMO").unwrap_or_default()).finish();
            }
            _ => {}
        }
        Json::new().boolean("ok", true).finish()
    }
}
