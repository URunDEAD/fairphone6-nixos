// SPDX-License-Identifier: MIT
//! Sunrise / sunset (NOAA solar position algorithm, ~1 minute accuracy).

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SunDay {
    /// Unix times of sunrise and sunset on that UTC day.
    Normal { rise: i64, set: i64 },
    /// The sun stays above the horizon all day.
    PolarDay,
    /// The sun stays below the horizon all day.
    PolarNight,
}

fn rad(d: f64) -> f64 {
    d * PI / 180.0
}
fn deg(r: f64) -> f64 {
    r * 180.0 / PI
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
#[cfg_attr(not(test), allow(dead_code))]
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Sun events for the UTC day `day` (days since epoch) at lat/lon (degrees,
/// east positive). Standard altitude -0.833 degrees (refraction + disc).
pub fn sun_day(day: i64, lat: f64, lon: f64) -> SunDay {
    // Julian century at local noon (approximate by longitude).
    let jd = day as f64 + 2440587.5 + 0.5 - lon / 360.0;
    let t = (jd - 2451545.0) / 36525.0;
    let l0 = (280.46646 + t * (36000.76983 + t * 0.0003032)).rem_euclid(360.0);
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let c = rad(m).sin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + rad(2.0 * m).sin() * (0.019993 - 0.000101 * t)
        + rad(3.0 * m).sin() * 0.000289;
    let true_long = l0 + c;
    let omega = 125.04 - 1934.136 * t;
    let app_long = true_long - 0.00569 - 0.00478 * rad(omega).sin();
    let eps0 = 23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let eps = eps0 + 0.00256 * rad(omega).cos();
    let decl = deg((rad(eps).sin() * rad(app_long).sin()).asin());
    let y = (rad(eps / 2.0)).tan().powi(2);
    let eq_time = 4.0
        * deg(
            y * (2.0 * rad(l0)).sin() - 2.0 * e * rad(m).sin()
                + 4.0 * e * y * rad(m).sin() * (2.0 * rad(l0)).cos()
                - 0.5 * y * y * (4.0 * rad(l0)).sin()
                - 1.25 * e * e * (2.0 * rad(m)).sin(),
        );
    let cos_ha = (rad(90.833).cos() - rad(lat).sin() * rad(decl).sin()) / (rad(lat).cos() * rad(decl).cos());
    if cos_ha > 1.0 {
        return SunDay::PolarNight;
    }
    if cos_ha < -1.0 {
        return SunDay::PolarDay;
    }
    let ha = deg(cos_ha.acos());
    let noon_min = 720.0 - 4.0 * lon - eq_time;
    let base = day * 86400;
    SunDay::Normal {
        rise: base + ((noon_min - 4.0 * ha) * 60.0).round() as i64,
        set: base + ((noon_min + 4.0 * ha) * 60.0).round() as i64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(t: i64, day: i64, tz_h: i64) -> (i64, i64) {
        let s = t - day * 86400 + tz_h * 3600;
        (s / 3600, (s % 3600) / 60)
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(days_from_civil(2026, 10, 26), 20752);
    }

    fn check(y: i32, m: u32, d: u32, lat: f64, lon: f64, tz: i64, rise: (i64, i64), set: (i64, i64)) {
        let day = days_from_civil(y, m, d);
        match sun_day(day, lat, lon) {
            SunDay::Normal { rise: r, set: s } => {
                let (rh, rm) = hm(r, day, tz);
                let (sh, sm) = hm(s, day, tz);
                assert!((rh * 60 + rm - (rise.0 * 60 + rise.1)).abs() <= 3, "sunrise {rh}:{rm}");
                assert!((sh * 60 + sm - (set.0 * 60 + set.1)).abs() <= 3, "sunset {sh}:{sm}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn london_midsummer() {
        // Published: London 21 June sunrise 04:43, sunset 21:21 (BST, UTC+1).
        check(2026, 6, 21, 51.5074, -0.1278, 1, (4, 43), (21, 21));
    }

    #[test]
    fn bucharest_2026_10_26() {
        // Solar noon ~11:59 EET, half-day arc ~5 h 14 min at declination -12.6.
        check(2026, 10, 26, 44.43, 26.10, 2, (6, 45), (17, 13));
    }

    #[test]
    fn polar() {
        let june = days_from_civil(2026, 6, 21);
        let dec = days_from_civil(2026, 12, 21);
        assert_eq!(sun_day(june, 78.2, 15.6), SunDay::PolarDay);
        assert_eq!(sun_day(dec, 78.2, 15.6), SunDay::PolarNight);
    }
}
