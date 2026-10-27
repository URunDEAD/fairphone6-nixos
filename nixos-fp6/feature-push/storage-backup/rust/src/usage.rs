//! Usage of the user's home by category and per app (~/.cache,
//! ~/.local/share, ~/.config entries).

use crate::util::{du, fs_space};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub cache: u64,
    pub data: u64,
    pub config: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HomeUsage {
    pub fs_total: u64,
    pub fs_free: u64,
    pub home: u64,
    pub pictures: u64,
    pub videos: u64,
    pub music: u64,
    pub documents: u64,
    pub downloads: u64,
    pub app_cache: u64,
    pub app_data: u64,
    pub waydroid_user: u64,
    pub backups: u64,
    pub apps: Vec<AppUsage>,
    pub time: i64,
}

/// Directories under ~/.local/share that are not app data shown per app.
const NOT_APPS: &[&str] = &["waydroid", "fp6-backup", "Trash"];

pub fn scan_home(home: &Path, data_dirs: &[PathBuf]) -> HomeUsage {
    let sz = |rel: &str| du(&home.join(rel)).bytes;
    let mut u = HomeUsage {
        home: sz(""),
        pictures: sz("Pictures"),
        videos: sz("Videos"),
        music: sz("Music"),
        documents: sz("Documents"),
        downloads: sz("Downloads"),
        app_cache: sz(".cache"),
        waydroid_user: sz(".local/share/waydroid"),
        backups: sz("Backups"),
        time: crate::util::now(),
        ..Default::default()
    };
    if let Some((t, f, _)) = fs_space(home) {
        u.fs_total = t;
        u.fs_free = f;
    }
    let mut apps: BTreeMap<String, AppUsage> = BTreeMap::new();
    for (dir, slot) in [(".cache", 0), (".local/share", 1), (".config", 2)] {
        let Ok(rd) = fs::read_dir(home.join(dir)) else { continue };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if slot == 1 && NOT_APPS.contains(&id.as_str()) {
                continue;
            }
            let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
            if !md.is_dir() {
                continue;
            }
            let b = du(&e.path()).bytes;
            let a = apps.entry(id.clone()).or_insert_with(|| AppUsage {
                id: id.clone(), name: id.clone(), icon: String::new(), cache: 0, data: 0, config: 0,
            });
            match slot {
                0 => a.cache += b,
                1 => a.data += b,
                _ => a.config += b,
            }
        }
    }
    u.app_data = apps.values().map(|a| a.data + a.config).sum();
    let names = desktop_names(data_dirs);
    let mut v: Vec<AppUsage> = apps.into_values().map(|mut a| {
        if let Some((n, i)) = lookup(&names, &a.id) {
            a.name = n;
            a.icon = i;
        }
        a
    }).collect();
    v.sort_by(|a, b| (b.cache + b.data + b.config).cmp(&(a.cache + a.data + a.config)).then(a.id.cmp(&b.id)));
    v.retain(|a| a.cache + a.data + a.config > 0);
    u.apps = v;
    u
}

/// desktop basename (lowercase) -> (Name, Icon)
pub fn desktop_names(data_dirs: &[PathBuf]) -> BTreeMap<String, (String, String)> {
    let mut m = BTreeMap::new();
    for d in data_dirs {
        let Ok(rd) = fs::read_dir(d.join("applications")) else { continue };
        for e in rd.flatten() {
            let f = e.file_name().to_string_lossy().into_owned();
            let Some(base) = f.strip_suffix(".desktop") else { continue };
            if m.contains_key(&base.to_lowercase()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else { continue };
            let (mut name, mut icon, mut in_entry) = (String::new(), String::new(), false);
            for l in text.lines() {
                let l = l.trim();
                if l.starts_with('[') {
                    in_entry = l == "[Desktop Entry]";
                } else if in_entry {
                    if let Some(v) = l.strip_prefix("Name=") {
                        name = v.to_string();
                    } else if let Some(v) = l.strip_prefix("Icon=") {
                        icon = v.to_string();
                    }
                }
            }
            if !name.is_empty() {
                m.insert(base.to_lowercase(), (name, icon));
            }
        }
    }
    m
}

/// Cache/data dir name -> desktop entry: exact basename, then the
/// reverse-DNS / click-style first or last label ("camera.ubports",
/// "org.gnome.Maps" -> maps).
fn lookup(names: &BTreeMap<String, (String, String)>, id: &str) -> Option<(String, String)> {
    let l = id.to_lowercase();
    if let Some(v) = names.get(&l) {
        return Some(v.clone());
    }
    for (k, v) in names {
        if k.ends_with(&format!(".{l}")) {
            return Some(v.clone());
        }
    }
    let parts: Vec<&str> = l.split('.').collect();
    if parts.len() > 1 {
        for cand in [parts[0], parts[parts.len() - 1]] {
            for (k, v) in names {
                if k == cand || k == &format!("lomiri-{cand}-app") || k.ends_with(&format!(".{cand}")) {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::tempdir;

    #[test]
    fn categories_and_apps() {
        let t = tempdir("usage");
        let home = t.join("home");
        let w = |p: &str, n: usize| {
            let p = home.join(p);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, vec![7u8; n]).unwrap();
        };
        w("Pictures/camera/IMG_0001.jpg", 300_000);
        w("Videos/clip.mp4", 900_000);
        w("Documents/notes.txt", 5000);
        w(".cache/camera.ubports/thumbs/a", 40_000);
        w(".cache/morph-browser/http/b", 120_000);
        w(".local/share/morph-browser/history.sqlite", 60_000);
        w(".local/share/waydroid/data/x", 10_000);
        w(".config/morph-browser/settings.ini", 100);
        let apps = t.join("share/applications");
        fs::create_dir_all(&apps).unwrap();
        fs::write(apps.join("morph-browser.desktop"), "[Desktop Entry]\nName=Morph Browser\nIcon=morph-browser\n").unwrap();
        fs::write(apps.join("lomiri-camera-app.desktop"), "[Desktop Entry]\nName=Camera\nIcon=camera-app\n[Desktop Action x]\nName=No\n").unwrap();
        let u = scan_home(&home, &[t.join("share")]);
        assert!(u.pictures >= 300_000 && u.videos >= 900_000);
        assert!(u.waydroid_user >= 10_000);
        assert_eq!(u.apps[0].id, "morph-browser");
        assert_eq!(u.apps[0].name, "Morph Browser");
        assert!(u.apps[0].cache >= 120_000 && u.apps[0].data >= 60_000 && u.apps[0].config > 0);
        let cam = u.apps.iter().find(|a| a.id == "camera.ubports").unwrap();
        assert_eq!(cam.name, "Camera");
        assert!(!u.apps.iter().any(|a| a.id == "waydroid"));
        assert!(u.fs_total > 0);
    }
}
