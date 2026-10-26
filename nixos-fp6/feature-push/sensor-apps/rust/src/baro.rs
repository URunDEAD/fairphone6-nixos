//! Barometric altitude (ICAO standard atmosphere, troposphere) and pressure trend.

/// 1 - (p/p0)^0.190263 scaled: ISA troposphere, T0 = 288.15 K, L = 0.0065 K/m.
pub const H_SCALE: f64 = 44330.77;
pub const EXPONENT: f64 = 0.190263;
pub const STD_QNH: f64 = 1013.25;

/// Altitude (m) for pressure `p` (hPa) with sea-level pressure `p0` (hPa).
pub fn altitude(p: f64, p0: f64) -> f64 {
    H_SCALE * (1.0 - (p / p0).powf(EXPONENT))
}

/// Sea-level pressure (QNH-style, hPa) that makes `p` read altitude `h` (m).
pub fn sea_level_pressure(p: f64, h: f64) -> f64 {
    p / (1.0 - h / H_SCALE).powf(1.0 / EXPONENT)
}

/// Least-squares slope of (t, v) in units of v per second.
pub fn slope(t: &[f64], v: &[f64]) -> Option<f64> {
    let n = t.len().min(v.len());
    if n < 3 {
        return None;
    }
    let mt = t[..n].iter().sum::<f64>() / n as f64;
    let mv = v[..n].iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        num += (t[i] - mt) * (v[i] - mv);
        den += (t[i] - mt).powi(2);
    }
    if den <= 0.0 {
        None
    } else {
        Some(num / den)
    }
}

/// Pressure tendency over 3 h (hPa) -> description (WMO-style bands).
pub fn tendency_label(d3h: f64) -> &'static str {
    let a = d3h.abs();
    let dir = if d3h > 0.0 { "rising" } else { "falling" };
    match a {
        a if a < 0.1 => "steady",
        a if a <= 1.5 => {
            if dir == "rising" {
                "rising slowly"
            } else {
                "falling slowly"
            }
        }
        a if a <= 3.5 => dir,
        a if a <= 6.0 => {
            if dir == "rising" {
                "rising quickly"
            } else {
                "falling quickly"
            }
        }
        _ => {
            if dir == "rising" {
                "rising very rapidly"
            } else {
                "falling very rapidly"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isa_table() {
        // ICAO standard atmosphere: 1000 m -> 898.75 hPa, 2000 m -> 794.95 hPa,
        // 5000 m -> 540.20 hPa, -300 m -> 1049.81 hPa.
        assert!(altitude(STD_QNH, STD_QNH).abs() < 1e-9);
        for (p, h) in [(898.75, 1000.0), (794.95, 2000.0), (540.20, 5000.0), (1049.81, -300.0)] {
            let a = altitude(p, STD_QNH);
            assert!((a - h).abs() < 0.5, "{p} hPa -> {a} m, want {h}");
        }
    }

    #[test]
    fn known_elevation_roundtrip() {
        // Phone reads 1009.98 hPa at a known 90 m: QNH ~ 1020.8 hPa
        let p0 = sea_level_pressure(1009.98, 90.0);
        assert!((p0 - 1020.84).abs() < 0.1, "{p0}");
        assert!((altitude(1009.98, p0) - 90.0).abs() < 1e-6);
        // 1 hPa near sea level ~ 8.3 m
        let d = altitude(1012.25, STD_QNH) - altitude(1013.25, STD_QNH);
        assert!((d - 8.33).abs() < 0.02, "{d}");
    }

    #[test]
    fn trend() {
        let t: Vec<f64> = (0..100).map(|i| i as f64 * 60.0).collect();
        let v: Vec<f64> = t.iter().map(|t| 1010.0 - t / 3600.0).collect(); // -1 hPa/h
        let s = slope(&t, &v).unwrap() * 3.0 * 3600.0;
        assert!((s + 3.0).abs() < 1e-9);
        assert_eq!(tendency_label(s), "falling");
        assert_eq!(tendency_label(0.05), "steady");
        assert_eq!(tendency_label(1.0), "rising slowly");
    }
}
