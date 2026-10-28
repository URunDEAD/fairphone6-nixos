use crate::{
    process::capture,
    report::{Check, Status},
};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(p.into());
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    dirs.extend(std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/run/current-system/sw/share:/usr/local/share:/usr/share".into()),
    ));
    // Qt wrappers can set XDG_DATA_DIRS to only their own runtime assets.
    // Always include the installed Nix profiles when auditing phone packages.
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".nix-profile/share"));
    }
    if let Ok(profiles) = std::env::var("NIX_PROFILES") {
        dirs.extend(
            profiles
                .split_whitespace()
                .map(|p| PathBuf::from(p).join("share")),
        );
    }
    dirs
}
/// Executable search path for desktop Exec= entries. A systemd user unit
/// gets a minimal PATH (coreutils, systemd, ...), while the launcher resolves
/// Exec in the session PATH, so the session/NixOS profile directories are
/// always appended after the inherited PATH.
pub fn exec_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let user = std::env::var("USER").ok().filter(|u| !u.is_empty()).or_else(|| {
        nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|u| u.name)
    });
    if let Some(home) = &home {
        dirs.push(home.join(".local/bin"));
    }
    dirs.push("/run/wrappers/bin".into());
    if let Some(home) = &home {
        dirs.push(home.join(".nix-profile/bin"));
        dirs.push(home.join(".local/state/nix/profile/bin"));
    }
    if let Some(user) = &user {
        dirs.push(PathBuf::from("/etc/profiles/per-user").join(user).join("bin"));
    }
    dirs.push("/nix/var/nix/profiles/default/bin".into());
    dirs.push("/run/current-system/sw/bin".into());
    let mut seen = HashSet::new();
    dirs.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    dirs
}
pub fn schema_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Some(p) = std::env::var_os("GSETTINGS_SCHEMA_DIR") {
        dirs.push(PathBuf::from(p));
    }
    for data in data_dirs() {
        dirs.push(data.join("glib-2.0/schemas"));
        if let Ok(entries) = fs::read_dir(data.join("gsettings-schemas")) {
            for entry in entries.flatten().take(4096) {
                dirs.push(entry.path().join("glib-2.0/schemas"));
            }
        }
    }
    dirs
}
fn read_small(path: &Path) -> Result<String, ()> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|_| ())?;
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| ())?;
    if text.len() > 1024 * 1024 {
        Err(())
    } else {
        Ok(text)
    }
}
/// Returns target schema count, rejecting the types that crash gsettings-qt.
pub fn inspect_schema(xml: &str) -> Result<usize, &'static str> {
    inspect_schema_set(&[xml.to_owned()])
}
fn inspect_schema_set(xmls: &[String]) -> Result<usize, &'static str> {
    let mut schemas = BTreeMap::<String, (Option<String>, bool)>::new();
    for xml in xmls {
        let doc = roxmltree::Document::parse(xml).map_err(|_| "Malformed schema XML")?;
        for schema in doc.descendants().filter(|n| n.has_tag_name("schema")) {
            let id = schema.attribute("id").ok_or("Schema is missing an ID")?;
            let unsafe_type = schema
                .descendants()
                .any(|n| n.has_tag_name("key") && matches!(n.attribute("type"), Some("ai" | "ax")));
            if schemas.get(id).is_some_and(|(_, bad)| *bad) || unsafe_type {
                // A duplicate unsafe definition must never be hidden by order.
                schemas.insert(
                    id.into(),
                    (schema.attribute("extends").map(String::from), true),
                );
            } else {
                schemas.insert(
                    id.into(),
                    (schema.attribute("extends").map(String::from), false),
                );
            }
        }
    }
    let mut count = 0;
    for id in schemas
        .keys()
        .filter(|id| id.starts_with("com.fairphone6."))
    {
        count += 1;
        let mut current = Some(id.as_str());
        let mut seen = HashSet::new();
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err("Schema inheritance is cyclic");
            }
            let (parent, unsafe_type) = schemas
                .get(id)
                .ok_or("Inherited schema source is unavailable; safety cannot be validated")?;
            if *unsafe_type {
                return Err(
                    "Schema contains or inherits an ai/ax key that is unsafe for gsettings-qt",
                );
            }
            current = parent.as_deref();
        }
    }
    Ok(count)
}
pub fn schemas(dirs: &[PathBuf]) -> Check {
    let mut seen = HashSet::new();
    let mut xmls = vec![];
    let mut compile_dirs = vec![];
    let mut unreadable = false;
    for dir in dirs {
        let Ok(canonical) = fs::canonicalize(dir) else {
            continue;
        };
        if !seen.insert(canonical) {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            unreadable = true;
            continue;
        };
        let mut relevant = false;
        for (i, entry) in entries.enumerate() {
            if i >= 8192 {
                return Check::new(
                    "schemas",
                    Status::Warn,
                    "Schema directory exceeds inspection limit",
                );
            }
            let Ok(entry) = entry else {
                unreadable = true;
                continue;
            };
            if !entry
                .file_name()
                .to_string_lossy()
                .ends_with(".gschema.xml")
            {
                continue;
            }
            let Ok(text) = read_small(&entry.path()) else {
                unreadable = true;
                continue;
            };
            relevant |= text.contains("com.fairphone6.");
            xmls.push(text);
        }
        if relevant {
            compile_dirs.push(dir);
        }
    }
    let n = match inspect_schema_set(&xmls) {
        Ok(n) => n,
        Err(reason) => return Check::new("schemas", Status::Fail, reason),
    };
    for dir in compile_dirs {
        if capture(
            "glib-compile-schemas",
            &["--strict", "--dry-run", dir.to_str().unwrap_or("")],
        )
        .is_err()
        {
            return Check::new(
                "schemas",
                Status::Fail,
                "Schema directory fails strict dry-run compilation or compiler is unavailable",
            );
        }
    }
    Check::new(
        "schemas",
        if unreadable {
            Status::Warn
        } else if n == 0 {
            Status::Skip
        } else {
            Status::Pass
        },
        &format!(
            "{n} Fairphone schemas inspected for compilation and ai/ax types{}",
            if unreadable {
                "; some files were unreadable"
            } else {
                ""
            }
        ),
    )
}
fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn resolve_exec(name: &str, search: &[PathBuf]) -> bool {
    let p = Path::new(name);
    if p.is_absolute() {
        executable(p)
    } else if name.contains('/') {
        false
    } else {
        search.iter().any(|dir| executable(&dir.join(name)))
    }
}
pub fn inspect_desktop(
    text: &str,
    search: &[PathBuf],
    icons: &HashSet<String>,
) -> Result<bool, &'static str> {
    let mut inside = false;
    let mut fields = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if !inside || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim(), value.trim());
        }
    }
    if fields.get("X-Lomiri-Touch") != Some(&"true") || fields.get("Hidden") == Some(&"true") {
        return Ok(false);
    }
    // Entries Lomiri never lists (NoDisplay, OnlyShowIn/NotShowIn) can still be
    // launched as handlers, so Exec is checked, but they need no icon.
    let listed_in = |key: &str| {
        fields
            .get(key)
            .is_some_and(|v| v.split(';').any(|d| d.trim() == "Lomiri"))
    };
    let shown = fields.get("NoDisplay") != Some(&"true")
        && (!fields.contains_key("OnlyShowIn") || listed_in("OnlyShowIn"))
        && !listed_in("NotShowIn");
    let args = shell_words::split(
        fields
            .get("Exec")
            .ok_or("Touch desktop entry has no Exec")?,
    )
    .map_err(|_| "Desktop Exec is malformed")?;
    let first = args.first().ok_or("Desktop Exec is empty")?;
    if !resolve_exec(first, search) {
        return Err("Desktop executable is missing or not executable");
    }
    if Path::new(first).file_name().is_some_and(|v| v == "env") {
        let mut i = 1;
        while i < args.len() {
            if args[i] == "--" {
                i += 1;
                break;
            }
            if matches!(args[i].as_str(), "-u" | "--unset") {
                i += 2;
                continue;
            }
            if args[i] == "-i" || args[i].starts_with("--unset=") || args[i].contains('=') {
                i += 1;
                continue;
            }
            break;
        }
        if args.get(i).is_none_or(|s| !resolve_exec(s, search)) {
            return Err("Desktop env target is missing or unsupported");
        }
    }
    if let Some(exec) = fields.get("TryExec") {
        if !resolve_exec(exec, search) {
            return Err("Desktop TryExec is missing");
        }
    }
    if !shown {
        return Ok(true);
    }
    let icon = fields
        .get("Icon")
        .filter(|v| !v.is_empty())
        .ok_or("Touch desktop entry has no Icon")?;
    if Path::new(icon).is_absolute() {
        if !Path::new(icon).is_file() {
            return Err("Desktop icon file is missing");
        }
    } else if !icons.contains(*icon) {
        return Err("Desktop themed icon is missing");
    }
    Ok(true)
}
fn collect(
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    budget: &mut usize,
) -> Result<(), ()> {
    if !dir.exists() {
        return Ok(());
    }
    let canon = fs::canonicalize(dir).map_err(|_| ())?;
    if !seen.insert(canon) {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|_| ())? {
        if *budget == 0 {
            return Err(());
        }
        *budget -= 1;
        let path = entry.map_err(|_| ())?.path();
        if path.is_dir() {
            if depth == 0 {
                return Err(());
            }
            collect(&path, depth - 1, out, seen, budget)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}
pub fn desktops(dirs: &[PathBuf], search: &[PathBuf]) -> Check {
    let mut icon_files = vec![];
    let mut entries = vec![];
    let mut seen = HashSet::new();
    let mut budget = 200000;
    let mut complete = true;
    for dir in dirs {
        complete &= collect(
            &dir.join("icons"),
            10,
            &mut icon_files,
            &mut seen,
            &mut budget,
        )
        .is_ok();
        complete &= collect(
            &dir.join("pixmaps"),
            2,
            &mut icon_files,
            &mut seen,
            &mut budget,
        )
        .is_ok();
        complete &= collect(
            &dir.join("applications"),
            5,
            &mut entries,
            &mut seen,
            &mut budget,
        )
        .is_ok();
    }
    if let Some(home) = std::env::var_os("HOME") {
        complete &= collect(
            &PathBuf::from(home).join(".icons"),
            10,
            &mut icon_files,
            &mut seen,
            &mut budget,
        )
        .is_ok();
    }
    let icons: HashSet<_> = icon_files
        .iter()
        .filter(|p| {
            matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("png" | "svg" | "xpm" | "svgz")
            )
        })
        .flat_map(|p| {
            [p.file_stem(), p.file_name()]
                .into_iter()
                .flatten()
                .filter_map(|s| s.to_str().map(String::from))
        })
        .collect();
    let mut count = 0;
    let mut failing: Vec<(String, &'static str)> = vec![];
    let mut ids = HashSet::new();
    for path in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|v| v == "desktop"))
    {
        // The first data dir that has an id wins (XDG precedence), as in the launcher.
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !ids.insert(id.clone()) {
            continue;
        }
        let Ok(text) = read_small(path) else {
            complete = false;
            continue;
        };
        match inspect_desktop(&text, search, &icons) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(reason) => {
                count += 1;
                failing.push((id, reason));
            }
        }
    }
    let bad = failing.len();
    failing.sort();
    const LISTED: usize = 10;
    let mut list = failing
        .iter()
        .take(LISTED)
        .map(|(id, reason)| format!("{id} ({reason})"))
        .collect::<Vec<_>>()
        .join(", ");
    if bad > LISTED {
        list.push_str(&format!(", +{} more", bad - LISTED));
    }
    Check::new(
        "desktops",
        if bad > 0 {
            Status::Fail
        } else if !complete {
            Status::Warn
        } else if count == 0 {
            Status::Skip
        } else {
            Status::Pass
        },
        &format!(
            "{count} touch desktop entries checked; {bad} have invalid Exec or Icon{}{}",
            if bad > 0 {
                format!(": {list}")
            } else {
                String::new()
            },
            if complete { "" } else { "; search incomplete" }
        ),
    )
}
