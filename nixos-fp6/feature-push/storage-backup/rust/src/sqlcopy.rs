//! SQLite copies through the online backup API: consistent snapshots of
//! databases other processes have open (contacts, history-service), and
//! restores into the live database (other connections see the new content
//! once the backup step commits).

use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

fn check(c: &Connection) -> Result<(), String> {
    let r: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if r != "ok" {
        return Err(format!("integrity check: {r}"));
    }
    Ok(())
}

/// Snapshot of `src` (opened read-only) into a new file `dst`, checked.
pub fn snapshot(src: &Path, dst: &Path) -> Result<(), String> {
    let s = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{}: {e}", src.display()))?;
    s.busy_timeout(Duration::from_secs(5)).ok();
    let _ = std::fs::remove_file(dst);
    let mut d = Connection::open(dst).map_err(|e| e.to_string())?;
    {
        let b = Backup::new(&s, &mut d).map_err(|e| e.to_string())?;
        b.run_to_completion(256, Duration::from_millis(20), None).map_err(|e| format!("backup of {}: {e}", src.display()))?;
    }
    // A self-contained file (no -wal next to it).
    d.pragma_update(None, "journal_mode", "DELETE").map_err(|e| e.to_string())?;
    check(&d)
}

/// Copies the checked database file `from` into the database at `live`
/// (created if missing), page by page through the backup API.
pub fn restore_into(from: &Path, live: &Path) -> Result<(), String> {
    let s = Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    check(&s)?;
    if let Some(dir) = live.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut d = Connection::open(live).map_err(|e| format!("{}: {e}", live.display()))?;
    d.busy_timeout(Duration::from_secs(10)).ok();
    {
        let b = Backup::new(&s, &mut d).map_err(|e| e.to_string())?;
        b.run_to_completion(256, Duration::from_millis(20), None).map_err(|e| format!("restore into {}: {e}", live.display()))?;
    }
    check(&d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::tempdir;

    #[test]
    fn snapshot_while_open_and_restore() {
        let t = tempdir("sql");
        let live = t.join("history.sqlite");
        let w = Connection::open(&live).unwrap();
        w.pragma_update(None, "journal_mode", "WAL").unwrap();
        w.execute_batch("CREATE TABLE threads (id TEXT, n INT); INSERT INTO threads VALUES ('t1', 1), ('t2', 2);").unwrap();
        // writer keeps a transaction open with uncommitted data
        w.execute_batch("BEGIN; INSERT INTO threads VALUES ('t3', 3);").unwrap();
        let snap = t.join("snap.sqlite");
        snapshot(&live, &snap).unwrap();
        w.execute_batch("COMMIT;").unwrap();
        let c = Connection::open(&snap).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM threads", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        assert!(!t.join("snap.sqlite-wal").exists() || std::fs::metadata(t.join("snap.sqlite-wal")).unwrap().len() == 0);
        // restore the 2-row snapshot into the live db while `w` stays open
        restore_into(&snap, &live).unwrap();
        let n: i64 = w.query_row("SELECT count(*) FROM threads", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        // a missing target gets created
        restore_into(&snap, &t.join("new/dir/contacts.db")).unwrap();
        // garbage is refused
        std::fs::write(t.join("bad.sqlite"), b"not sqlite at all, definitely not").unwrap();
        assert!(restore_into(&t.join("bad.sqlite"), &t.join("x.db")).is_err());
    }
}
