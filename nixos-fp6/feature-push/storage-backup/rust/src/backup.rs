//! Backup and restore: what goes into an archive, where it goes back.

use crate::archive::{self, Index, Item, Source};
use crate::keys::KeyStore;
use crate::util::{list_files, safe_name, safe_rel};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const CATEGORIES: &[&str] = &["settings", "contacts", "messages", "photos", "documents", "wifi"];
pub const PREFIX: &str = "fp6-backup-";
pub const SUFFIX: &str = ".tar.age";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Selection {
    pub settings: bool,
    pub config_dirs: Vec<String>,
    pub contacts: bool,
    pub messages: bool,
    pub photos: bool,
    pub documents: bool,
    pub wifi: bool,
}

pub struct Env {
    pub home: PathBuf,
    pub contacts_db: PathBuf,
    pub history_db: PathBuf,
    /// ~/.local/share/fp6-backup: keys, staging, pre-restore copies.
    pub work: PathBuf,
}

impl Env {
    pub fn for_home(home: &Path) -> Env {
        let share = home.join(".local/share");
        Env {
            home: home.to_path_buf(),
            contacts_db: share.join("system/privileged/Contacts/qtcontacts-sqlite/contacts.db"),
            history_db: share.join("history-service/history.sqlite"),
            work: share.join("fp6-backup"),
        }
    }
}

/// Things that need other processes or the root helper.
pub trait Hooks {
    fn dconf_dump(&self) -> Result<String, String>;
    fn dconf_load(&self, ini: &str) -> Result<(), String>;
    fn wifi_export(&self) -> Result<Vec<(String, String)>, String>;
    fn wifi_import(&self, files: Vec<(String, String)>) -> Result<String, String>;
    /// After a database was restored (e.g. restart lomiri-history-daemon).
    fn restored(&self, _what: &str) {}
}

/// Local time as YYYYmmdd-HHMMSS.
pub fn stamp(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    unsafe { libc::localtime_r(&tt, &mut tm) };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

struct Plan {
    index: Index,
    sources: Vec<Source>,
    warnings: Vec<String>,
}

fn add_tree(p: &mut Plan, root: &Path, category: &str, arch_prefix: &str, target_prefix: &str) {
    for rel in list_files(root) {
        let Some(r) = rel.to_str() else {
            p.warnings.push(format!("skipped a file with a non-UTF-8 name in {}", root.display()));
            continue;
        };
        let full = root.join(&rel);
        let Ok(md) = fs::symlink_metadata(&full) else { continue };
        p.index.items.push(Item {
            path: format!("{arch_prefix}/{r}"), category: category.into(), target: format!("{target_prefix}/{r}"),
            size: md.len(), mode: md.mode() & 0o777, mtime: md.mtime(),
        });
        p.sources.push(Source::File(full));
    }
}

fn sqlite_count(db: &Path, sql: &str) -> Option<i64> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    c.query_row(sql, [], |r| r.get(0)).ok()
}

fn plan(sel: &Selection, env: &Env, hooks: &dyn Hooks, staging: &Path) -> Result<Plan, String> {
    let now = crate::util::now();
    let mut p = Plan {
        index: Index { format: archive::FORMAT.into(), version: archive::VERSION, created: now, categories: BTreeMap::new(), items: vec![] },
        sources: vec![], warnings: vec![],
    };
    let mut details: BTreeMap<&str, String> = BTreeMap::new();
    if sel.settings {
        let dump = crate::dconf::filter_dump(&hooks.dconf_dump()?);
        let keys = crate::dconf::count_keys(&dump);
        p.index.items.push(Item { path: "items/settings/dconf.ini".into(), category: "settings".into(), target: "dconf".into(),
                                  size: dump.len() as u64, mode: 0o600, mtime: now });
        p.sources.push(Source::Bytes(dump.into_bytes()));
        let mut dirs = Vec::new();
        for d in &sel.config_dirs {
            if !safe_name(d) || d == "dconf" {
                p.warnings.push(format!("skipped config folder {d:?}"));
                continue;
            }
            let root = env.home.join(".config").join(d);
            if fs::symlink_metadata(&root).map(|m| m.is_dir()).unwrap_or(false) {
                add_tree(&mut p, &root, "settings", &format!("items/settings/config/{d}"), &format!("home:.config/{d}"));
                dirs.push(d.clone());
            }
        }
        details.insert("settings", format!("{keys} keys{}", if dirs.is_empty() { String::new() } else { format!("; {}", dirs.join(", ")) }));
    }
    for (on, cat, db, name, count_sql) in [
        (sel.contacts, "contacts", &env.contacts_db, "contacts.db", "SELECT count(*) FROM Contacts"),
        (sel.messages, "messages", &env.history_db, "history.sqlite", "SELECT count(*) FROM threads"),
    ] {
        if !on {
            continue;
        }
        if !db.exists() {
            p.warnings.push(format!("no {cat} database yet"));
            continue;
        }
        let snap = staging.join(name);
        crate::sqlcopy::snapshot(db, &snap)?;
        let md = fs::metadata(&snap).map_err(|e| e.to_string())?;
        if let Some(n) = sqlite_count(&snap, count_sql) {
            details.insert(cat, if cat == "contacts" { format!("{n} contacts") } else { format!("{n} conversations") });
        }
        p.index.items.push(Item { path: format!("items/{cat}/{name}"), category: cat.into(), target: format!("sqlite:{cat}"),
                                  size: md.len(), mode: 0o600, mtime: now });
        p.sources.push(Source::File(snap));
    }
    if sel.photos {
        for d in ["Pictures", "Videos"] {
            add_tree(&mut p, &env.home.join(d), "photos", &format!("items/photos/{d}"), &format!("home:{d}"));
        }
    }
    if sel.documents {
        add_tree(&mut p, &env.home.join("Documents"), "documents", "items/documents/Documents", "home:Documents");
    }
    if sel.wifi {
        let files = hooks.wifi_export()?;
        details.insert("wifi", format!("{} networks", files.len()));
        for (name, text) in files {
            let Some(fname) = crate::wifi::file_name(&name) else { continue };
            p.index.items.push(Item { path: format!("items/wifi/{fname}"), category: "wifi".into(), target: format!("wifi:{fname}"),
                                      size: text.len() as u64, mode: 0o600, mtime: now });
            p.sources.push(Source::Bytes(text.into_bytes()));
        }
    }
    for it in &p.index.items {
        let c = p.index.categories.entry(it.category.clone()).or_default();
        c.files += 1;
        c.bytes += it.size;
    }
    for (cat, d) in details {
        p.index.categories.entry(cat.into()).or_default().detail = d;
    }
    Ok(p)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub file: String,
    pub bytes: u64,
    pub files: u64,
    pub archive_bytes: u64,
    pub verified: bool,
    pub warnings: Vec<String>,
    pub pruned: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listed {
    pub file: String,
    pub name: String,
    pub size: u64,
    pub stamp: String,
}

pub fn list_backups(dir: &Path) -> Vec<Listed> {
    let mut v: Vec<Listed> = fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let st = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?.to_string();
        let md = fs::symlink_metadata(e.path()).ok()?;
        md.is_file().then(|| Listed { file: e.path().to_string_lossy().into_owned(), name: name.clone(), size: md.len(), stamp: st })
    }).collect();
    v.sort_by(|a, b| b.name.cmp(&a.name));
    v
}

pub fn prune(dir: &Path, keep: usize) -> Vec<String> {
    let keep = keep.max(1);
    let mut removed = Vec::new();
    for l in list_backups(dir).into_iter().skip(keep) {
        if fs::remove_file(&l.file).is_ok() {
            removed.push(l.name);
        }
    }
    removed
}

pub fn run_backup(sel: &Selection, env: &Env, hooks: &dyn Hooks, keys: &KeyStore, dest: &Path, keep: usize,
                  progress: &mut dyn FnMut(u64, u64)) -> Result<BackupResult, String> {
    let id = keys.identity().ok_or("set a backup passphrase first")?;
    if !keys.exists() {
        return Err("set a backup passphrase first".into());
    }
    let dmd = fs::metadata(dest).map_err(|e| format!("backup folder {}: {e}", dest.display()))?;
    if !dmd.is_dir() {
        return Err(format!("{} is not a folder", dest.display()));
    }
    let now = crate::util::now();
    let staging = env.work.join(format!("tmp-{now}-{}", std::process::id()));
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(&staging, fs::Permissions::from_mode(0o700));
    let r = (|| {
        let p = plan(sel, env, hooks, &staging)?;
        if p.index.items.is_empty() {
            return Err("nothing to back up".to_string());
        }
        let total: u64 = p.index.items.iter().map(|i| i.size).sum();
        let estimate = total + p.index.items.len() as u64 * 1536 + (1 << 20);
        if let Some((_, free, fstype)) = crate::util::fs_space(dest) {
            if free < estimate + (16 << 20) {
                return Err(format!("not enough space in {}: {} MB needed, {} MB free", dest.display(), estimate >> 20, free >> 20));
            }
            if fstype == crate::util::MSDOS_SUPER_MAGIC && estimate >= (4u64 << 30) - (1 << 20) {
                return Err("the archive would be 4 GB or more, which a FAT card/stick cannot store; leave photos out or use an exFAT/ext4 folder".into());
            }
        }
        let name = format!("{PREFIX}{}{SUFFIX}", stamp(now));
        let fin = dest.join(&name);
        let part = dest.join(format!(".{name}.partial"));
        let f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let w = std::io::BufWriter::with_capacity(1 << 20, f);
        let res = archive::write(w, &id.to_public(), &p.index, &p.sources, &mut |d| progress(d, total * 2));
        let res = res.and_then(|(w, _)| {
            let f = w.into_inner().map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        let v = archive::process(&part, &id, &|_| false, None, &mut |d| progress(total + d, total * 2));
        let v = match v {
            Ok(v) => v.0,
            Err(e) => {
                let _ = fs::remove_file(&part);
                return Err(format!("verification failed, archive removed: {e}"));
            }
        };
        fs::rename(&part, &fin).map_err(|e| e.to_string())?;
        if let Ok(d) = fs::File::open(dest) {
            let _ = d.sync_all();
        }
        keys.export_to(dest)?;
        let mut warnings = p.warnings;
        if !v.changed.is_empty() {
            warnings.push(format!("{} files changed while they were saved", v.changed.len()));
        }
        let pruned = prune(dest, keep);
        Ok(BackupResult {
            file: fin.to_string_lossy().into_owned(), bytes: v.bytes, files: v.files,
            archive_bytes: fs::metadata(&fin).map(|m| m.len()).unwrap_or(0), verified: true, warnings, pruned,
        })
    })();
    let _ = fs::remove_dir_all(&staging);
    r
}

/// Identity for an archive: this phone's key, else the passphrase-protected
/// key file in the archive's folder.
pub fn identity_for(archive: &Path, keys: &KeyStore, passphrase: Option<&str>) -> Result<(age::x25519::Identity, bool), String> {
    if let Some(id) = keys.identity() {
        if archive::read_index(archive, &id).is_ok() {
            return Ok((id, false));
        }
    }
    let pass = passphrase.filter(|p| !p.is_empty()).ok_or("passphrase needed")?;
    let kf = archive.parent().unwrap_or(Path::new(".")).join(crate::keys::KEY_FILE);
    let w = fs::read(&kf).map_err(|_| format!("{} is missing next to the backup", crate::keys::KEY_FILE))?;
    let id = crate::keys::unwrap(&w, pass)?;
    archive::read_index(archive, &id)?;
    Ok((id, true))
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: BTreeMap<String, u64>,
    pub kept_both: u64,
    pub unchanged: u64,
    pub warnings: Vec<String>,
}

fn home_target_ok(category: &str, rel: &str) -> bool {
    if !safe_rel(rel) {
        return false;
    }
    match category {
        "settings" => rel.starts_with(".config/") && !rel.starts_with(".config/dconf/"),
        "photos" => rel.starts_with("Pictures/") || rel.starts_with("Videos/"),
        "documents" => rel.starts_with("Documents/"),
        _ => false,
    }
}

/// No symlink between home and the target's parent.
fn no_symlink_parents(home: &Path, rel: &str) -> bool {
    let mut p = home.to_path_buf();
    let parts: Vec<&str> = rel.split('/').collect();
    for c in &parts[..parts.len() - 1] {
        p.push(c);
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return false,
            _ => {}
        }
    }
    match fs::symlink_metadata(home.join(rel)) {
        Ok(m) => !m.file_type().is_symlink(),
        Err(_) => true,
    }
}

fn sha_file(p: &Path) -> Option<Vec<u8>> {
    let mut f = fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut b = vec![0u8; 128 * 1024];
    loop {
        let n = f.read(&mut b).ok()?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Some(h.finalize().to_vec())
}

fn place(staged: &Path, dest: &Path, mode: u32, mtime: i64) -> Result<(), String> {
    if fs::rename(staged, dest).is_err() {
        fs::copy(staged, dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    let _ = fs::set_permissions(dest, fs::Permissions::from_mode(mode & 0o777));
    if let Ok(f) = fs::File::options().write(true).open(dest) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime.max(0) as u64));
    }
    Ok(())
}

fn kept_both_name(dest: &Path) -> PathBuf {
    let stem = dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = dest.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 1.. {
        let n = if i == 1 { format!("{stem} (restored){ext}") } else { format!("{stem} (restored {i}){ext}") };
        let c = dest.with_file_name(n);
        if fs::symlink_metadata(&c).is_err() {
            return c;
        }
    }
    unreachable!()
}

pub fn restore(archive_path: &Path, id: &age::x25519::Identity, categories: &[String], env: &Env, hooks: &dyn Hooks,
               progress: &mut dyn FnMut(u64)) -> Result<RestoreReport, String> {
    let now = crate::util::now();
    let staging = env.work.join(format!("restore-{now}-{}", std::process::id()));
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(&staging, fs::Permissions::from_mode(0o700));
    let r = (|| {
        let sel = |i: &Item| categories.iter().any(|c| c == &i.category);
        let (_, staged) = archive::process(archive_path, id, &sel, Some(&staging), progress)?;
        let mut rep = RestoreReport::default();
        let mut wifi = Vec::new();
        for (item, sp) in staged {
            let count = |rep: &mut RestoreReport| *rep.restored.entry(item.category.clone()).or_default() += 1;
            if item.target == "dconf" {
                let text = fs::read_to_string(&sp).map_err(|e| e.to_string())?;
                hooks.dconf_load(&crate::dconf::filter_dump(&text))?;
                count(&mut rep);
            } else if let Some(rel) = item.target.strip_prefix("home:") {
                if !home_target_ok(&item.category, rel) || !no_symlink_parents(&env.home, rel) {
                    rep.warnings.push(format!("skipped {rel}"));
                    continue;
                }
                let dest = env.home.join(rel);
                fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
                match fs::symlink_metadata(&dest) {
                    Ok(m) if m.is_file() && m.len() == item.size && sha_file(&dest) == sha_file(&sp) => {
                        rep.unchanged += 1;
                        continue;
                    }
                    Ok(m) if m.is_file() && item.category != "settings" => {
                        place(&sp, &kept_both_name(&dest), item.mode, item.mtime)?;
                        rep.kept_both += 1;
                    }
                    Ok(m) if !m.is_file() => {
                        rep.warnings.push(format!("skipped {rel}: not a file on the phone"));
                        continue;
                    }
                    _ => place(&sp, &dest, item.mode, item.mtime)?,
                }
                count(&mut rep);
            } else if let Some(which) = item.target.strip_prefix("sqlite:") {
                let live = match which {
                    "contacts" => &env.contacts_db,
                    "history" | "messages" => &env.history_db,
                    _ => {
                        rep.warnings.push(format!("unknown database {which}"));
                        continue;
                    }
                };
                if live.exists() {
                    let pre = env.work.join("pre-restore");
                    fs::create_dir_all(&pre).map_err(|e| e.to_string())?;
                    crate::sqlcopy::snapshot(live, &pre.join(format!("{which}-{}.sqlite", stamp(now))))?;
                }
                crate::sqlcopy::restore_into(&sp, live)?;
                hooks.restored(which);
                count(&mut rep);
            } else if let Some(name) = item.target.strip_prefix("wifi:") {
                let text = fs::read_to_string(&sp).map_err(|e| e.to_string())?;
                wifi.push((name.to_string(), text));
            } else {
                rep.warnings.push(format!("unknown target {}", item.target));
            }
        }
        if !wifi.is_empty() {
            let n = wifi.len() as u64;
            let msg = hooks.wifi_import(wifi)?;
            rep.restored.insert("wifi".into(), n);
            if !msg.is_empty() {
                rep.warnings.push(msg);
            }
        }
        Ok(rep)
    })();
    let _ = fs::remove_dir_all(&staging);
    r
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::tempdir;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        dconf: RefCell<String>,
        loaded: RefCell<Vec<String>>,
        wifi_in: RefCell<Vec<(String, String)>>,
        restored: RefCell<Vec<String>>,
    }
    impl Hooks for Fake {
        fn dconf_dump(&self) -> Result<String, String> { Ok(self.dconf.borrow().clone()) }
        fn dconf_load(&self, ini: &str) -> Result<(), String> { self.loaded.borrow_mut().push(ini.into()); Ok(()) }
        fn wifi_export(&self) -> Result<Vec<(String, String)>, String> {
            Ok(vec![("Test Net.nmconnection".into(), crate::wifi::tests::SAMPLE.into())])
        }
        fn wifi_import(&self, f: Vec<(String, String)>) -> Result<String, String> { self.wifi_in.borrow_mut().extend(f); Ok(String::new()) }
        fn restored(&self, w: &str) { self.restored.borrow_mut().push(w.into()) }
    }

    fn mk(home: &Path, rel: &str, data: &[u8]) {
        let p = home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    #[test]
    fn backup_restore_roundtrip() {
        let t = tempdir("bk");
        let home = t.join("home");
        let env = Env::for_home(&home);
        mk(&home, "Pictures/camera/IMG_0001.jpg", &[1u8; 70_000]);
        mk(&home, "Videos/v.mp4", &[2u8; 30_000]);
        mk(&home, "Documents/n.txt", b"synthetic note");
        mk(&home, ".config/morph-browser/settings.ini", b"[General]\nhome=about:blank\n");
        mk(&home, ".config/secret/x", b"not selected");
        fs::create_dir_all(env.history_db.parent().unwrap()).unwrap();
        let c = rusqlite::Connection::open(&env.history_db).unwrap();
        c.execute_batch("CREATE TABLE threads (id TEXT); INSERT INTO threads VALUES ('a'),('b');").unwrap();
        drop(c);
        let fake = Fake::default();
        *fake.dconf.borrow_mut() = "[com/lomiri/touch/system]\nfoo=true\n\n[com/fairphone6/backup]\nfolder='/x'\n".into();
        let keys = KeyStore { dir: env.work.clone(), work_factor: Some(10) };
        let dest = t.join("SD/Backups");
        fs::create_dir_all(&dest).unwrap();
        let sel = Selection { settings: true, config_dirs: vec!["morph-browser".into(), "../etc".into()], contacts: true,
                              messages: true, photos: true, documents: true, wifi: true };
        assert!(run_backup(&sel, &env, &fake, &keys, &dest, 3, &mut |_, _| {}).unwrap_err().contains("passphrase"));
        keys.set_passphrase(None, "synthetic passphrase").unwrap();
        let r = run_backup(&sel, &env, &fake, &keys, &dest, 3, &mut |_, _| {}).unwrap();
        assert!(r.verified);
        assert!(r.warnings.iter().any(|w| w.contains("contacts")), "{:?}", r.warnings);
        assert!(dest.join(crate::keys::KEY_FILE).exists());
        assert_eq!(list_backups(&dest).len(), 1);
        assert!(fs::read_dir(&env.work).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with("tmp-")));
        let arch = PathBuf::from(&r.file);
        let idx = archive::read_index(&arch, &keys.identity().unwrap()).unwrap();
        assert_eq!(idx.categories["photos"].files, 2);
        assert_eq!(idx.categories["settings"].detail, "1 keys; morph-browser");
        assert_eq!(idx.categories["messages"].detail, "2 conversations");
        assert!(!idx.items.iter().any(|i| i.target.contains("secret")));

        // new phone: no key, wrong then right passphrase
        let home2 = t.join("home2");
        let env2 = Env::for_home(&home2);
        let keys2 = KeyStore { dir: env2.work.clone(), work_factor: Some(10) };
        assert_eq!(identity_for(&arch, &keys2, None).err().as_deref(), Some("passphrase needed"));
        assert!(identity_for(&arch, &keys2, Some("nope nope nope")).is_err());
        let (id, from_file) = identity_for(&arch, &keys2, Some("synthetic passphrase")).unwrap();
        assert!(from_file);
        mk(&home2, "Pictures/camera/IMG_0001.jpg", b"a different photo with the same name");
        mk(&home2, "Videos/v.mp4", &[2u8; 30_000]);
        let fake2 = Fake::default();
        let rep = restore(&arch, &id, &["photos".into(), "settings".into(), "messages".into(), "wifi".into()], &env2, &fake2, &mut |_| {}).unwrap();
        assert_eq!(rep.unchanged, 1);
        assert_eq!(rep.kept_both, 1);
        assert!(home2.join("Pictures/camera/IMG_0001 (restored).jpg").exists());
        assert_eq!(fs::read(home2.join("Pictures/camera/IMG_0001.jpg")).unwrap(), b"a different photo with the same name");
        assert_eq!(fs::read(home2.join(".config/morph-browser/settings.ini")).unwrap(), b"[General]\nhome=about:blank\n");
        assert!(!home2.join("Documents/n.txt").exists(), "documents were not selected");
        assert_eq!(fake2.loaded.borrow().len(), 1);
        assert!(!fake2.loaded.borrow()[0].contains("fairphone6/backup"));
        assert_eq!(fake2.wifi_in.borrow().len(), 1);
        assert_eq!(fake2.restored.borrow().as_slice(), ["messages"]);
        let c = rusqlite::Connection::open(&env2.history_db).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM threads", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn restore_targets_are_confined() {
        assert!(home_target_ok("photos", "Pictures/a.jpg"));
        assert!(!home_target_ok("photos", ".bashrc"));
        assert!(!home_target_ok("photos", "Pictures/../.bashrc"));
        assert!(!home_target_ok("settings", ".config/dconf/user"));
        assert!(!home_target_ok("settings", ".local/share/x"));
        assert!(!home_target_ok("documents", "/etc/passwd"));
        assert!(!home_target_ok("wifi", "Documents/x"));
        let t = tempdir("sym");
        fs::create_dir_all(t.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(t.join("elsewhere"), t.join("Pictures")).unwrap();
        assert!(!no_symlink_parents(&t, "Pictures/a.jpg"));
        assert!(no_symlink_parents(&t, "Documents/a.txt"));
    }

    #[test]
    fn prune_keeps_newest_and_only_ours() {
        let t = tempdir("prune");
        for s in ["20261001-030000", "20261002-030000", "20261003-030000"] {
            fs::write(t.join(format!("{PREFIX}{s}{SUFFIX}")), "x").unwrap();
        }
        fs::write(t.join("holiday.tar.age"), "x").unwrap();
        assert_eq!(prune(&t, 2), vec![format!("{PREFIX}20261001-030000{SUFFIX}")]);
        assert!(t.join("holiday.tar.age").exists());
        assert_eq!(list_backups(&t)[0].stamp, "20261003-030000");
        assert_eq!(stamp(0).len(), 15);
    }
}
