//! Nix store accounting from the store database (read-only): the closure
//! of the running system, what old generations hold, and the garbage
//! (paths `nix-store --gc --print-dead` reports).

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct StoreDb {
    paths: Vec<(String, u64)>,
    idx: HashMap<String, usize>,
    refs: Vec<Vec<usize>>,
}

impl StoreDb {
    pub fn load(db: &Path) -> Result<StoreDb, String> {
        let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| format!("{}: {e}", db.display()))?;
        c.busy_timeout(std::time::Duration::from_secs(10)).ok();
        let mut ids: HashMap<i64, usize> = HashMap::new();
        let mut paths = Vec::new();
        {
            let mut st = c.prepare("SELECT id, path, COALESCE(narSize, 0) FROM ValidPaths").map_err(|e| e.to_string())?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
                .map_err(|e| e.to_string())?;
            for r in rows {
                let (id, p, sz) = r.map_err(|e| e.to_string())?;
                ids.insert(id, paths.len());
                paths.push((p, sz.max(0) as u64));
            }
        }
        let mut refs = vec![Vec::new(); paths.len()];
        {
            let mut st = c.prepare("SELECT referrer, reference FROM Refs").map_err(|e| e.to_string())?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?;
            for r in rows {
                let (a, b) = r.map_err(|e| e.to_string())?;
                if let (Some(&a), Some(&b)) = (ids.get(&a), ids.get(&b)) {
                    if a != b {
                        refs[a].push(b);
                    }
                }
            }
        }
        let idx = paths.iter().enumerate().map(|(i, (p, _))| (p.clone(), i)).collect();
        Ok(StoreDb { paths, idx, refs })
    }

    pub fn total(&self) -> u64 {
        self.paths.iter().map(|p| p.1).sum()
    }

    pub fn closure<'a>(&self, roots: impl IntoIterator<Item = &'a str>) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut stack: Vec<usize> = roots.into_iter().filter_map(|r| self.idx.get(r).copied()).collect();
        while let Some(i) = stack.pop() {
            if seen.insert(i) {
                stack.extend(self.refs[i].iter().copied().filter(|j| !seen.contains(j)));
            }
        }
        seen
    }

    pub fn size(&self, set: &HashSet<usize>) -> u64 {
        set.iter().map(|&i| self.paths[i].1).sum()
    }

    pub fn size_of_paths<'a>(&self, paths: impl IntoIterator<Item = &'a str>) -> (u64, usize) {
        let mut n = 0;
        let mut s = 0;
        for p in paths {
            if let Some(&i) = self.idx.get(p) {
                s += self.paths[i].1;
                n += 1;
            }
        }
        (s, n)
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoreUsage {
    pub total: u64,
    pub current_system: u64,
    /// Held only by the deletable old generations (an upper bound of what
    /// deleting them and collecting garbage frees).
    pub old_generations: u64,
    pub garbage: u64,
    pub garbage_paths: usize,
    pub garbage_known: bool,
    pub other: u64,
}

/// `current` = toplevels kept (current, booted, pins); `old` = toplevels of
/// deletable generations; `dead` = output of `nix-store --gc --print-dead`
/// (None if it was not run).
pub fn usage(db: &StoreDb, current: &[String], old: &[String], dead: Option<&[String]>) -> StoreUsage {
    let total = db.total();
    let cur = db.closure(current.iter().map(|s| s.as_str()));
    let current_system = db.size(&cur);
    let oldc = db.closure(old.iter().map(|s| s.as_str()));
    let only_old: HashSet<usize> = oldc.difference(&cur).copied().collect();
    let old_generations = db.size(&only_old);
    let (garbage, garbage_paths, garbage_known) = match dead {
        Some(d) => {
            let (s, n) = db.size_of_paths(d.iter().map(|s| s.as_str()));
            (s, n, true)
        }
        None => (0, 0, false),
    };
    let other = total.saturating_sub(current_system + old_generations + garbage);
    StoreUsage { total, current_system, old_generations, garbage, garbage_paths, garbage_known, other }
}

pub fn parse_dead(out: &str) -> Vec<String> {
    out.lines().map(str::trim).filter(|l| l.starts_with("/nix/store/")).map(String::from).collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::tempdir;

    /// A synthetic store database with Nix's ValidPaths/Refs schema.
    pub fn make_db(path: &Path, items: &[(&str, i64, &[&str])]) {
        let c = Connection::open(path).unwrap();
        c.execute_batch("CREATE TABLE ValidPaths (id integer primary key autoincrement not null, path text unique not null, hash text not null, registrationTime integer not null, deriver text, narSize integer, ultimate integer, sigs text, ca text);
                         CREATE TABLE Refs (referrer integer not null, reference integer not null, primary key (referrer, reference));").unwrap();
        for (p, sz, _) in items {
            c.execute("INSERT INTO ValidPaths (path, hash, registrationTime, narSize) VALUES (?1, 'sha256:0', 0, ?2)", rusqlite::params![p, sz]).unwrap();
        }
        for (p, _, rs) in items {
            for r in rs.iter() {
                c.execute("INSERT INTO Refs SELECT a.id, b.id FROM ValidPaths a, ValidPaths b WHERE a.path = ?1 AND b.path = ?2", rusqlite::params![p, r]).unwrap();
            }
        }
    }

    #[test]
    fn closure_and_usage() {
        let t = tempdir("store");
        let db = t.join("db.sqlite");
        let s = |c: char, n: &str| format!("/nix/store/{}-{n}", c.to_string().repeat(32));
        let (sys1, sys2, glibc, lomiri_old, lomiri_new, junk) =
            (s('a', "sys1"), s('b', "sys2"), s('c', "glibc"), s('d', "lomiri-old"), s('e', "lomiri-new"), s('f', "junk"));
        make_db(&db, &[
            (&sys1, 10, &[&lomiri_old, &glibc]),
            (&sys2, 10, &[&lomiri_new, &glibc, &sys2]),
            (&glibc, 1000, &[]),
            (&lomiri_old, 300, &[&glibc]),
            (&lomiri_new, 400, &[&glibc]),
            (&junk, 77, &[]),
        ]);
        let d = StoreDb::load(&db).unwrap();
        assert_eq!(d.total(), 1797);
        let u = usage(&d, std::slice::from_ref(&sys2), std::slice::from_ref(&sys1), Some(&parse_dead(&format!("finding garbage collector roots...\n{junk}\n"))));
        assert_eq!(u.current_system, 1410);
        assert_eq!(u.old_generations, 310);
        assert_eq!(u.garbage, 77);
        assert_eq!(u.garbage_paths, 1);
        assert_eq!(u.other, 0);
        let u = usage(&d, &[sys2], &[], None);
        assert!(!u.garbage_known);
        assert_eq!(u.other, 387);
    }
}
