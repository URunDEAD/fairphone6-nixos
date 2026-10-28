// SPDX-License-Identifier: MIT
//! Small ini-style file edits (UITK theme.ini, GTK settings.ini, the session
//! scale file read by lomiri-session at login). Writes are atomic (temp file
//! + rename) and skipped when nothing changes.

use std::fs;
use std::io::Write;
use std::path::Path;

pub const THEME_LIGHT: &str = "Lomiri.Components.Themes.Ambiance";
pub const THEME_DARK: &str = "Lomiri.Components.Themes.SuruDark";

/// Sets (Some) or removes (None) `key` in `[section]` of an ini text, keeping
/// everything else. Adds the section if missing.
pub fn ini_set(text: &str, section: &str, key: &str, value: Option<&str>) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut seen_section = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if in_section && !done {
                if let Some(v) = value {
                    out.push(format!("{key}={v}"));
                }
                done = true;
            }
            in_section = t == header;
            seen_section |= in_section;
            out.push(line.to_string());
            continue;
        }
        if in_section {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim() == key {
                    if !done {
                        if let Some(v) = value {
                            out.push(format!("{key}={v}"));
                        }
                        done = true;
                    }
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        if let Some(v) = value {
            if !seen_section {
                if !out.is_empty() && !out.last().unwrap().is_empty() {
                    out.push(String::new());
                }
                out.push(header);
            }
            out.push(format!("{key}={v}"));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let header = format!("[{section}]");
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = t == header;
        } else if in_section {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Content of ~/.config/fp6-display-comfort/session.env.
pub fn session_env(grid_unit_px: i32, font_percent: i32, bold: bool) -> String {
    format!(
        "# Written by fp6-display-comfort (System Settings > Display Comfort).\n\
         # Read by lomiri-session at the next login.\n\
         FP6_GRID_UNIT_PX={}\nFP6_FONT_SCALE_PERCENT={}\nFP6_BOLD_TEXT={}\n",
        grid_unit_px.clamp(18, 32),
        font_percent.clamp(80, 150),
        u8::from(bold)
    )
}

/// Content of ~/.config/lomiri-ui-toolkit/font-scale: the live UITK text size
/// in percent (uitk-font-scale.patch accepts 50..200; Settings offers 80..150).
pub fn uitk_font_scale(font_percent: i32) -> String {
    format!("{}\n", font_percent.clamp(80, 150))
}

/// (grid unit, font percent) from a session.env text, as lomiri-session reads it.
pub fn parse_session_env(text: &str) -> (i32, i32, bool) {
    let (mut gu, mut pct, mut bold) = (24, 100, false);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match (k, v.parse::<i32>()) {
                ("FP6_GRID_UNIT_PX", Ok(n)) if (18..=32).contains(&n) => gu = n,
                ("FP6_FONT_SCALE_PERCENT", Ok(n)) if (80..=150).contains(&n) => pct = n,
                ("FP6_BOLD_TEXT", Ok(n)) if (0..=1).contains(&n) => bold = n == 1,
                _ => {}
            }
        }
    }
    (gu, pct, bold)
}

/// Writes `content` atomically if it differs. Returns true if written.
pub fn write_if_changed(path: &Path, content: &str) -> std::io::Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("fp6-tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(true)
}

/// Applies a key edit to an ini file (missing file = empty).
pub fn edit_ini(path: &Path, section: &str, key: &str, value: Option<&str>) -> std::io::Result<bool> {
    let old = fs::read_to_string(path).unwrap_or_default();
    if old.is_empty() && value.is_none() {
        return Ok(false);
    }
    let new = ini_set(&old, section, key, value);
    write_if_changed(path, &new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_replace_remove() {
        let t = "[General]\ntheme=Lomiri.Components.Themes.Ambiance\n";
        let d = ini_set(t, "General", "theme", Some(THEME_DARK));
        assert_eq!(d, "[General]\ntheme=Lomiri.Components.Themes.SuruDark\n");
        assert_eq!(ini_get(&d, "General", "theme").as_deref(), Some(THEME_DARK));
        let g = "[Settings]\ngtk-decoration-layout=:\n";
        let g2 = ini_set(g, "Settings", "gtk-application-prefer-dark-theme", Some("true"));
        assert_eq!(g2, "[Settings]\ngtk-decoration-layout=:\ngtk-application-prefer-dark-theme=true\n");
        let g3 = ini_set(&g2, "Settings", "gtk-application-prefer-dark-theme", None);
        assert_eq!(g3, g);
        assert_eq!(ini_set("", "General", "theme", Some("x")), "[General]\ntheme=x\n");
        // Key appended at the end of its section, not after a later one.
        let two = "[A]\na=1\n[B]\nb=2\n";
        assert_eq!(ini_set(two, "A", "k", Some("v")), "[A]\na=1\nk=v\n[B]\nb=2\n");
    }

    #[test]
    fn session_file() {
        let s = session_env(21, 115, false);
        assert!(s.contains("\nFP6_GRID_UNIT_PX=21\n") && s.ends_with("FP6_FONT_SCALE_PERCENT=115\nFP6_BOLD_TEXT=0\n"));
        assert_eq!(parse_session_env(&s), (21, 115, false));
        assert_eq!(parse_session_env(&session_env(24, 100, true)), (24, 100, true));
        assert_eq!(parse_session_env("FP6_GRID_UNIT_PX=40\nFP6_FONT_SCALE_PERCENT=1;x\nFP6_BOLD_TEXT=2\n"), (24, 100, false));
        // Files from before bold text: off.
        assert_eq!(parse_session_env("FP6_GRID_UNIT_PX=21\nFP6_FONT_SCALE_PERCENT=115\n"), (21, 115, false));
        assert_eq!(uitk_font_scale(115), "115\n");
        assert_eq!(uitk_font_scale(400), "150\n");
        assert!(session_env(99, 10, true).contains("FP6_GRID_UNIT_PX=32\nFP6_FONT_SCALE_PERCENT=80\nFP6_BOLD_TEXT=1\n"));
    }
}
