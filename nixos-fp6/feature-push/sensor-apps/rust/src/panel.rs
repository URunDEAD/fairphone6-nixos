//! Physical panel geometry for the on-screen ruler.
//!
//! FP6: DSI panel "boe,bj631jhm-t71-d900" (driver panel-novatek-nt37705),
//! mode 1116 x 2484. The driver's width_mm/height_mm are 66 x 146 (rounded
//! to whole millimetres); Fairphone specifies a 6.31" diagonal. From the
//! diagonal: sqrt(1116^2 + 2484^2) = 2723.2 px over 160.27 mm ->
//! 16.991 px/mm (431.6 ppi), i.e. 65.68 x 146.19 mm, consistent with the
//! driver's rounded values (66/1116 -> 16.91, 146/2484 -> 17.01 px/mm).

pub struct Panel {
    pub name: &'static str,
    pub width_px: u32,
    pub height_px: u32,
    pub px_per_mm: f64,
    pub source: &'static str,
}

pub const FP6: Panel = Panel {
    name: "boe,bj631jhm-t71-d900",
    width_px: 1116,
    height_px: 2484,
    px_per_mm: 16.991,
    source: "6.31\" diagonal, 1116x2484 (panel-novatek-nt37705: 66x146 mm)",
};

pub fn diag_px_per_mm(w: f64, h: f64, inches: f64) -> f64 {
    (w * w + h * h).sqrt() / (inches * 25.4)
}

/// Looks for the panel's compatible string in the device tree.
pub fn detect() -> Option<&'static Panel> {
    let base = std::path::Path::new("/sys/firmware/devicetree/base/soc@0");
    let disp = std::fs::read_dir(base).ok()?.filter_map(|e| e.ok()).find(|e| {
        e.file_name().to_string_lossy().starts_with("display-subsystem")
    })?;
    for dsi in std::fs::read_dir(disp.path()).ok()?.filter_map(|e| e.ok()) {
        if !dsi.file_name().to_string_lossy().starts_with("dsi@") {
            continue;
        }
        for p in std::fs::read_dir(dsi.path()).ok()?.filter_map(|e| e.ok()) {
            if p.file_name().to_string_lossy().starts_with("panel") {
                let c = std::fs::read(p.path().join("compatible")).unwrap_or_default();
                if c.split(|b| *b == 0).any(|s| s == FP6.name.as_bytes()) {
                    return Some(&FP6);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp6_density() {
        let d = diag_px_per_mm(1116.0, 2484.0, 6.31);
        assert!((d - FP6.px_per_mm).abs() < 0.001, "{d}");
        assert!((1116.0 / d - 65.68).abs() < 0.01);
        assert!((2484.0 / d - 146.19).abs() < 0.01);
        // within 1 % of the driver's rounded mm
        assert!(((1116.0 / 66.0) / d - 1.0).abs() < 0.01);
        assert!(((2484.0 / 146.0) / d - 1.0).abs() < 0.01);
    }
}
