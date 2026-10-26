//! World Magnetic Model (WMM2025, NOAA NCEI / BGS, public domain), degree 12.
//!
//! Coefficients: data/WMM2025.COF, unchanged from NCEI's WMM2025COF.zip
//! (valid 2025.0-2030.0). Algorithm from the WMM2025 technical report:
//! geodetic -> geocentric spherical, Schmidt semi-normalised associated
//! Legendre functions, secular variation applied linearly, field rotated back
//! to the geodetic frame.

const COF: &str = include_str!("../data/WMM2025.COF");
const N: usize = 12;
const A_REF: f64 = 6371.2; // km, geomagnetic reference radius
const WGS84_A: f64 = 6378.137;
const WGS84_F: f64 = 1.0 / 298.257223563;

pub struct Model {
    pub epoch: f64,
    g: [[f64; N + 1]; N + 1],
    h: [[f64; N + 1]; N + 1],
    gd: [[f64; N + 1]; N + 1],
    hd: [[f64; N + 1]; N + 1],
}

#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// North, east, down components (nT).
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub h: f64,
    pub f: f64,
    /// Declination (deg, east positive), inclination (deg, down positive).
    pub decl: f64,
    pub incl: f64,
}

impl Model {
    pub fn wmm2025() -> Model {
        Model::parse(COF).expect("embedded WMM2025.COF")
    }

    pub fn parse(text: &str) -> Option<Model> {
        let mut lines = text.lines();
        let epoch: f64 = lines.next()?.split_whitespace().next()?.parse().ok()?;
        let z = [[0.0; N + 1]; N + 1];
        let mut m = Model { epoch, g: z, h: z, gd: z, hd: z };
        for l in lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 6 {
                continue;
            }
            let (Ok(n), Ok(mm)) = (f[0].parse::<usize>(), f[1].parse::<usize>()) else { continue };
            if n == 0 || n > N || mm > n {
                continue;
            }
            m.g[n][mm] = f[2].parse().ok()?;
            m.h[n][mm] = f[3].parse().ok()?;
            m.gd[n][mm] = f[4].parse().ok()?;
            m.hd[n][mm] = f[5].parse().ok()?;
        }
        Some(m)
    }

    /// lat/lon in degrees (geodetic, WGS84), height in km above the ellipsoid,
    /// decimal year.
    pub fn field(&self, lat: f64, lon: f64, height_km: f64, year: f64) -> Field {
        let phi = lat.to_radians();
        let lam = lon.to_radians();
        let e2 = WGS84_F * (2.0 - WGS84_F);
        let rc = WGS84_A / (1.0 - e2 * phi.sin().powi(2)).sqrt();
        let p = (rc + height_km) * phi.cos();
        let zz = (rc * (1.0 - e2) + height_km) * phi.sin();
        let r = (p * p + zz * zz).sqrt();
        let phic = (zz / r).asin();

        let dt = year - self.epoch;
        // Legendre functions of x = sin(phi') (= cos(theta)).
        let x = phic.sin();
        let s = phic.cos().max(1e-12);
        let mut pn = [[0.0f64; N + 1]; N + 1];
        let mut dp = [[0.0f64; N + 1]; N + 1]; // d/dtheta
        pn[0][0] = 1.0;
        for n in 1..=N {
            for m in 0..=n {
                if n == m {
                    pn[n][m] = if n == 1 {
                        s
                    } else {
                        ((2 * n - 1) as f64 / (2 * n) as f64).sqrt() * s * pn[n - 1][n - 1]
                    };
                } else {
                    let a = (2 * n - 1) as f64 * x * pn[n - 1][m];
                    let b = if n >= 2 && m <= n - 2 {
                        (((n - 1) * (n - 1) - m * m) as f64).sqrt() * pn[n - 2][m]
                    } else {
                        0.0
                    };
                    pn[n][m] = (a - b) / ((n * n - m * m) as f64).sqrt();
                }
            }
            for m in 0..=n {
                let prev = if m < n { pn[n - 1][m] } else { 0.0 };
                dp[n][m] = (n as f64 * x * pn[n][m] - ((n * n - m * m) as f64).sqrt() * prev) / s;
            }
        }

        let (mut bx, mut by, mut bz) = (0.0, 0.0, 0.0);
        let ar = A_REF / r;
        let mut arn = ar * ar; // (a/r)^(n+2) starts at n=0 -> ^2
        for n in 1..=N {
            arn *= ar;
            for m in 0..=n {
                let g = self.g[n][m] + dt * self.gd[n][m];
                let h = self.h[n][m] + dt * self.hd[n][m];
                let (sm, cm) = ((m as f64) * lam).sin_cos();
                let gc = g * cm + h * sm;
                bx += arn * gc * dp[n][m];
                by += arn * (m as f64) * (g * sm - h * cm) * pn[n][m];
                bz -= arn * (n as f64 + 1.0) * gc * pn[n][m];
            }
        }
        by /= s;
        // geocentric -> geodetic
        let psi = phic - phi;
        let xg = bx * psi.cos() - bz * psi.sin();
        let zg = bx * psi.sin() + bz * psi.cos();
        let h = (xg * xg + by * by).sqrt();
        Field {
            x: xg,
            y: by,
            z: zg,
            h,
            f: (h * h + zg * zg).sqrt(),
            decl: by.atan2(xg).to_degrees(),
            incl: zg.atan2(h).to_degrees(),
        }
    }

    pub fn valid_until(&self) -> f64 {
        self.epoch + 5.0
    }
}

/// Days since 1970-01-01 -> (year, month, day). (H. Hinnant's algorithm.)
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Unix time -> decimal year.
pub fn decimal_year(unix: f64) -> f64 {
    let days = (unix / 86400.0).floor() as i64;
    let (y, _, _) = civil_from_days(days);
    // days from epoch to Jan 1 of y
    let mut d0 = 0i64;
    if y >= 1970 {
        for yy in 1970..y {
            d0 += if is_leap(yy) { 366 } else { 365 };
        }
    }
    let len = if is_leap(y) { 366.0 } else { 365.0 };
    y as f64 + (unix / 86400.0 - d0 as f64) / len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_test_values() {
        let m = Model::wmm2025();
        let tv = include_str!("../data/WMM2025_TestValues.txt");
        let mut n = 0;
        for l in tv.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<f64> = l.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let r = m.field(f[2], f[3], f[1], f[0]);
            assert!((r.decl - f[4]).abs() < 0.01, "D {l}: {}", r.decl);
            assert!((r.incl - f[5]).abs() < 0.01, "I {l}: {}", r.incl);
            assert!((r.h - f[6]).abs() < 1.0, "H {l}: {}", r.h);
            assert!((r.x - f[7]).abs() < 1.0, "X {l}: {}", r.x);
            assert!((r.y - f[8]).abs() < 1.0, "Y {l}: {}", r.y);
            assert!((r.z - f[9]).abs() < 1.0, "Z {l}: {}", r.z);
            n += 1;
        }
        assert!(n >= 20, "{n}");
    }

    #[test]
    fn timisoara_declination() {
        // Timisoara (45.75 N, 21.23 E): about +6 deg east in 2026.
        let m = Model::wmm2025();
        let r = m.field(45.75, 21.23, 0.1, 2026.8);
        assert!(r.decl > 5.0 && r.decl < 7.5, "{}", r.decl);
        assert!(r.incl > 60.0 && r.incl < 66.0, "{}", r.incl);
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20454), (2026, 1, 1));
        let y = decimal_year(1767225600.0); // 2026-01-01T00:00Z
        assert!((y - 2026.0).abs() < 1e-9, "{y}");
        let y = decimal_year(1767225600.0 + 0.5 * 365.0 * 86400.0);
        assert!((y - 2026.5).abs() < 1e-9, "{y}");
    }
}
