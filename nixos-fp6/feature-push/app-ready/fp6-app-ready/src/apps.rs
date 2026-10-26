// SPDX-License-Identifier: MIT
//! Installed apps as lomiri-app-launch sees "legacy" desktop apps: the app id
//! is the .desktop basename; Type=Application, a Name, not NoDisplay/Hidden,
//! and OnlyShowIn/NotShowIn compatible with XDG_CURRENT_DESKTOP (Lomiri).
//! The first data dir that has a given id wins (XDG precedence).
//! (Shared with fp6-gestures; this copy also keeps the Exec line.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub exec: String,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(home) if !home.is_empty() => dirs.push(PathBuf::from(home)),
        _ => {
            if let Some(home) = std::env::var_os("HOME") {
                dirs.push(PathBuf::from(home).join(".local/share"));
            }
        }
    }
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&list));
    }
    if let Ok(user) = std::env::var("USER") {
        dirs.push(PathBuf::from(format!("/etc/profiles/per-user/{user}/share")));
    }
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    let mut seen = Vec::new();
    dirs.retain(|d| {
        if seen.contains(d) {
            false
        } else {
            seen.push(d.clone());
            true
        }
    });
    dirs
}

fn desktop_list(value: &str) -> Vec<String> {
    value.split(';').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Parse one .desktop file; None if it is not a launchable, visible app.
pub fn parse_desktop(id: &str, text: &str, current_desktops: &[String]) -> Option<App> {
    let mut in_entry = false;
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let truthy = |k: &str| keys.get(k).is_some_and(|v| v == "true");
    if keys.get("Type").map(String::as_str) != Some("Application") || truthy("NoDisplay") || truthy("Hidden") {
        return None;
    }
    let name = keys.get("Name")?.clone();
    if !current_desktops.is_empty() {
        let only = keys.get("OnlyShowIn").map(|v| desktop_list(v)).unwrap_or_default();
        let not = keys.get("NotShowIn").map(|v| desktop_list(v)).unwrap_or_default();
        let any = |list: &[String]| list.iter().any(|d| current_desktops.contains(d));
        if (!only.is_empty() && !any(&only)) || any(&not) {
            return None;
        }
    }
    Some(App {
        id: id.to_string(),
        name,
        icon: keys.get("Icon").cloned().unwrap_or_default(),
        exec: keys.get("Exec").cloned().unwrap_or_default(),
    })
}

pub fn scan(dirs: &[PathBuf], current_desktops: &[String]) -> Vec<App> {
    let mut found: BTreeMap<String, Option<App>> = BTreeMap::new();
    for dir in dirs {
        let apps = dir.join("applications");
        let Ok(entries) = std::fs::read_dir(&apps) else { continue };
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for path in files {
            let Some(id) = desktop_id(&path) else { continue };
            if found.contains_key(&id) {
                continue;
            }
            let app = std::fs::read_to_string(&path).ok().and_then(|t| parse_desktop(&id, &t, current_desktops));
            // A hidden entry earlier in the path still shadows later ones.
            found.insert(id, app);
        }
    }
    let mut apps: Vec<App> = found.into_values().flatten().collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    apps
}

fn desktop_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_suffix(".desktop")?;
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some(id.to_string())
}

pub fn current_desktops() -> Vec<String> {
    let value = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Lomiri".into());
    value.split(':').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// GSettings "installed-apps" entries: "id\tname\ticon".
pub fn encode(apps: &[App]) -> Vec<String> {
    apps.iter()
        .map(|a| format!("{}\t{}\t{}", a.id, a.name.replace('\t', " "), a.icon.replace('\t', " ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_like_lomiri_app_launch() {
        let lomiri = vec!["Lomiri".to_string()];
        let ok = "[Desktop Entry]\nType=Application\nName=Camera\nName[de]=Kamera\nIcon=camera-app\nExec=x\n";
        assert_eq!(parse_desktop("cam", ok, &lomiri).unwrap().name, "Camera");
        assert!(parse_desktop("a", "[Desktop Entry]\nType=Application\nName=A\nNoDisplay=true\n", &lomiri).is_none());
        assert!(parse_desktop("a", "[Desktop Entry]\nType=Link\nName=A\n", &lomiri).is_none());
        assert!(parse_desktop("a", "[Desktop Entry]\nType=Application\nName=A\nOnlyShowIn=GNOME;\n", &lomiri).is_none());
        assert!(parse_desktop("a", "[Desktop Entry]\nType=Application\nName=A\nOnlyShowIn=GNOME;Lomiri;\n", &lomiri).is_some());
        assert!(parse_desktop("a", "[Desktop Entry]\nType=Application\nName=A\nNotShowIn=Lomiri\n", &lomiri).is_none());
        let action = "[Desktop Entry]\nType=Application\nName=A\n[Desktop Action x]\nName=B\nNoDisplay=true\n";
        assert_eq!(parse_desktop("a", action, &lomiri).unwrap().name, "A");
    }

    #[test]
    fn scan_precedence_and_sort() {
        let tmp = std::env::temp_dir().join(format!("fp6-app-ready-apps-{}", std::process::id()));
        let first = tmp.join("one/applications");
        let second = tmp.join("two/applications");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("b.desktop"), "[Desktop Entry]\nType=Application\nName=zeta\n").unwrap();
        std::fs::write(first.join("hidden.desktop"), "[Desktop Entry]\nType=Application\nName=H\nHidden=true\n").unwrap();
        std::fs::write(second.join("b.desktop"), "[Desktop Entry]\nType=Application\nName=Other\n").unwrap();
        std::fs::write(second.join("hidden.desktop"), "[Desktop Entry]\nType=Application\nName=H\n").unwrap();
        std::fs::write(second.join("a.desktop"), "[Desktop Entry]\nType=Application\nName=Alpha\n").unwrap();
        let apps = scan(&[tmp.join("one"), tmp.join("two")], &["Lomiri".into()]);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta"]);
        assert_eq!(encode(&apps)[0], "a\tAlpha\t");
        std::fs::remove_dir_all(tmp).unwrap();
    }
}

/// The app's desktop entry, wherever it is (also NoDisplay ones).
pub fn find(id: &str) -> Option<App> {
    for dir in data_dirs() {
        let path = dir.join("applications").join(format!("{id}.desktop"));
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_desktop(id, &text, &[]);
        }
    }
    None
}

/// First word of an Exec line, resolved through PATH and symlinks.
pub fn exec_binary(exec: &str) -> Option<PathBuf> {
    let first = exec.split_whitespace().next()?.trim_matches('"');
    if first.is_empty() || first.starts_with('%') {
        return None;
    }
    let candidate = if first.contains('/') {
        Some(PathBuf::from(first))
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .chain(["/run/current-system/sw/bin".into(), "/etc/profiles/per-user/user/bin".into()])
            .map(|d| d.join(first))
            .find(|p| p.is_file())
    }?;
    std::fs::canonicalize(candidate).ok()
}

#[cfg(test)]
mod exec_tests {
    use super::*;

    #[test]
    fn exec_first_word() {
        assert_eq!(exec_binary("/bin/sh -c x"), std::fs::canonicalize("/bin/sh").ok());
        assert_eq!(exec_binary("%u"), None);
        assert_eq!(exec_binary(""), None);
    }
}
