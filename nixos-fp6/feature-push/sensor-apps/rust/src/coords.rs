//! Coordinate formats: decimal degrees, DMS, degrees + decimal minutes, UTM, MGRS.

const A: f64 = 6378137.0;
const F: f64 = 1.0 / 298.257223563;
const K0: f64 = 0.9996;

pub fn dd(lat: f64, lon: f64) -> String {
    format!("{:.6}, {:.6}", lat, lon)
}

fn hemi(v: f64, pos: char, neg: char) -> char {
    if v < 0.0 {
        neg
    } else {
        pos
    }
}

pub fn dms_one(v: f64, pos: char, neg: char) -> String {
    let a = v.abs();
    let mut d = a.floor();
    let mut m = ((a - d) * 60.0).floor();
    let mut s = ((a - d) * 60.0 - m) * 60.0;
    if s >= 59.95 {
        s = 0.0;
        m += 1.0;
    }
    if m >= 60.0 {
        m = 0.0;
        d += 1.0;
    }
    format!("{}°{:02}′{:04.1}″{}", d as i64, m as i64, s, hemi(v, pos, neg))
}

pub fn dms(lat: f64, lon: f64) -> String {
    format!("{} {}", dms_one(lat, 'N', 'S'), dms_one(lon, 'E', 'W'))
}

pub fn ddm_one(v: f64, pos: char, neg: char) -> String {
    let a = v.abs();
    let mut d = a.floor();
    let mut m = (a - d) * 60.0;
    if m >= 59.9995 {
        m = 0.0;
        d += 1.0;
    }
    format!("{}°{:06.3}′{}", d as i64, m, hemi(v, pos, neg))
}

pub fn ddm(lat: f64, lon: f64) -> String {
    format!("{} {}", ddm_one(lat, 'N', 'S'), ddm_one(lon, 'E', 'W'))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Utm {
    pub zone: u32,
    pub band: char,
    pub easting: f64,
    pub northing: f64,
    pub north: bool,
}

fn band_letter(lat: f64) -> char {
    const B: &[u8] = b"CDEFGHJKLMNPQRSTUVWX";
    let i = (((lat + 80.0) / 8.0).floor() as i64).clamp(0, 19) as usize;
    B[i] as char
}

/// UTM zone with the Norway/Svalbard exceptions.
pub fn utm_zone(lat: f64, lon: f64) -> u32 {
    let mut z = (((lon + 180.0) / 6.0).floor() as i64 % 60 + 1) as u32;
    if (56.0..64.0).contains(&lat) && (3.0..12.0).contains(&lon) {
        z = 32;
    }
    if (72.0..84.0).contains(&lat) {
        z = match lon {
            l if (0.0..9.0).contains(&l) => 31,
            l if (9.0..21.0).contains(&l) => 33,
            l if (21.0..33.0).contains(&l) => 35,
            l if (33.0..42.0).contains(&l) => 37,
            _ => z,
        };
    }
    z
}

/// Transverse Mercator (Krueger series, 6th order; sub-mm inside a zone).
pub fn utm(lat: f64, lon: f64) -> Option<Utm> {
    if !(-80.0..=84.0).contains(&lat) {
        return None; // polar areas use UPS
    }
    let zone = utm_zone(lat, lon);
    let lon0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
    let n = F / (2.0 - F);
    let a_ = A / (1.0 + n) * (1.0 + n * n / 4.0 + n.powi(4) / 64.0 + n.powi(6) / 256.0);
    let alpha = [
        n / 2.0 - 2.0 * n * n / 3.0 + 5.0 * n.powi(3) / 16.0 + 41.0 * n.powi(4) / 180.0 - 127.0 * n.powi(5) / 288.0
            + 7891.0 * n.powi(6) / 37800.0,
        13.0 * n * n / 48.0 - 3.0 * n.powi(3) / 5.0 + 557.0 * n.powi(4) / 1440.0 + 281.0 * n.powi(5) / 630.0
            - 1983433.0 * n.powi(6) / 1935360.0,
        61.0 * n.powi(3) / 240.0 - 103.0 * n.powi(4) / 140.0 + 15061.0 * n.powi(5) / 26880.0
            + 167603.0 * n.powi(6) / 181440.0,
        49561.0 * n.powi(4) / 161280.0 - 179.0 * n.powi(5) / 168.0 + 6601661.0 * n.powi(6) / 7257600.0,
        34729.0 * n.powi(5) / 80640.0 - 3418889.0 * n.powi(6) / 1995840.0,
        212378941.0 * n.powi(6) / 319334400.0,
    ];
    let phi = lat.to_radians();
    let lam = (lon - lon0).to_radians();
    let e = (F * (2.0 - F)).sqrt();
    let t = (phi.tan().asinh() - e * (e * phi.sin()).atanh()).sinh();
    let xi_p = t.atan2(lam.cos());
    let eta_p = (lam.sin() / (1.0 + t * t).sqrt()).atanh();
    let mut xi = xi_p;
    let mut eta = eta_p;
    for (j, a) in alpha.iter().enumerate() {
        let k = 2.0 * (j as f64 + 1.0);
        xi += a * (k * xi_p).sin() * (k * eta_p).cosh();
        eta += a * (k * xi_p).cos() * (k * eta_p).sinh();
    }
    let easting = 500000.0 + K0 * a_ * eta;
    let mut northing = K0 * a_ * xi;
    let north = lat >= 0.0;
    if !north {
        northing += 10_000_000.0;
    }
    Some(Utm { zone, band: band_letter(lat), easting, northing, north })
}

pub fn utm_string(lat: f64, lon: f64) -> String {
    match utm(lat, lon) {
        Some(u) => format!("{}{} {:.0} E {:.0} N", u.zone, u.band, u.easting.floor(), u.northing.floor()),
        None => "outside UTM (polar)".into(),
    }
}

/// MGRS with 1 m precision (5+5 digits).
pub fn mgrs(lat: f64, lon: f64) -> Option<String> {
    let u = utm(lat, lon)?;
    let set = (u.zone - 1) % 6; // 0..5
    const COLS: [&[u8]; 3] = [b"ABCDEFGH", b"JKLMNPQR", b"STUVWXYZ"];
    let col_letters = COLS[(set % 3) as usize];
    let e100k = (u.easting / 100000.0).floor() as i64; // 1..8
    let col = *col_letters.get((e100k - 1).clamp(0, 7) as usize)? as char;
    const ROWS: &[u8] = b"ABCDEFGHJKLMNPQRSTUV"; // 20 letters
    let n100k = (u.northing / 100000.0).floor() as i64;
    let offset = if set % 2 == 1 { 5 } else { 0 }; // even-numbered zones start at F
    let row = ROWS[((n100k + offset).rem_euclid(20)) as usize] as char;
    let e = (u.easting.rem_euclid(100000.0)).floor() as i64;
    let n = (u.northing.rem_euclid(100000.0)).floor() as i64;
    Some(format!("{}{} {}{} {:05} {:05}", u.zone, u.band, col, row, e, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(dms(45.758333, -21.229167), "45°45′30.0″N 21°13′45.0″W");
        assert_eq!(ddm(-45.5, 21.25), "45°30.000′S 21°15.000′E");
        assert_eq!(dd(1.0, 2.0), "1.000000, 2.000000");
    }

    #[test]
    fn utm_known_points() {
        // Equator on a central meridian: E = 500000, N = 0.
        let u = utm(0.0, 3.0).unwrap();
        assert_eq!(u.zone, 31);
        assert!((u.easting - 500000.0).abs() < 1e-6 && u.northing.abs() < 1e-6);
        // 45 N on a central meridian: N = k0 * meridian arc (4 984 944.378 m).
        let u = utm(45.0, 21.0).unwrap();
        assert_eq!((u.zone, u.band), (34, 'T'));
        assert!((u.northing - 0.9996 * 4984944.378).abs() < 0.01, "{}", u.northing);
        // Southern hemisphere false northing
        let u = utm(-45.0, 21.0).unwrap();
        assert!((u.northing - (10_000_000.0 - 0.9996 * 4984944.378)).abs() < 0.01);
        // Eiffel Tower (48.85825 N, 2.2945 E; published ~31U 448252 5411933,
        // the reference coordinates are only given to ~10 m)
        let u = utm(48.85825, 2.2945).unwrap();
        assert!((u.easting - 448252.0).abs() < 10.0 && (u.northing - 5411933.0).abs() < 10.0, "{u:?}");
        let m = mgrs(48.85825, 2.2945).unwrap();
        assert!(m.starts_with("31U DQ 4825"), "{m}");
        // Norway exception
        assert_eq!(utm_zone(60.0, 5.0), 32);
        assert!(utm(85.0, 0.0).is_none());
    }

    #[test]
    fn mgrs_even_zone_rows() {
        // Washington Monument: 18S UJ 23487 06483
        let m = mgrs(38.8895, -77.0353).unwrap();
        assert!(m.starts_with("18S UJ 234"), "{m}");
    }
}
