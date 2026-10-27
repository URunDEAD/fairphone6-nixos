// SPDX-License-Identifier: GPL-3.0-only
//! Contact name for a number, read-only from the one contacts store every
//! Lomiri app uses on the FP6 (qtcontacts-sqlite,
//! ~/.local/share/system/privileged/Contacts/qtcontacts-sqlite/contacts.db;
//! see results/feature-push-20261024/contacts/README.md).

use crate::calls::same_number;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub fn default_db() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/system/privileged/Contacts/qtcontacts-sqlite/contacts.db")
}

/// Best display name from the stored parts.
pub fn display_name(label: Option<String>, first: Option<String>, last: Option<String>, nick: Option<String>) -> Option<String> {
    let clean = |s: Option<String>| s.map(|x| x.trim().to_owned()).filter(|x| !x.is_empty());
    if let Some(l) = clean(label) {
        return Some(l);
    }
    let name = [clean(first), clean(last)].into_iter().flatten().collect::<Vec<_>>().join(" ");
    if !name.is_empty() {
        return Some(name);
    }
    clean(nick)
}

pub fn lookup_in(db: &Path, number: &str) -> Result<Option<String>, String> {
    if crate::calls::digits(number).is_empty() || !db.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{}: {e}", db.display()))?;
    conn.busy_timeout(std::time::Duration::from_millis(500)).ok();
    let mut st = conn
        .prepare(
            "SELECT p.phoneNumber, p.normalizedNumber, d.displayLabel, n.firstName, n.lastName, \
             (SELECT k.nickname FROM Nicknames k WHERE k.contactId = c.contactId LIMIT 1) \
             FROM PhoneNumbers p JOIN Contacts c ON c.contactId = p.contactId \
             LEFT JOIN DisplayLabels d ON d.contactId = c.contactId \
             LEFT JOIN Names n ON n.contactId = c.contactId \
             WHERE c.deleted IS NULL",
        )
        .map_err(|e| format!("contacts query: {e}"))?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| format!("contacts query: {e}"))?;
    for row in rows.flatten() {
        let (raw, norm, label, first, last, nick) = row;
        let hit = [raw, norm].into_iter().flatten().any(|n| same_number(&n, number));
        if hit {
            if let Some(name) = display_name(label, first, last, nick) {
                return Ok(Some(name));
            }
        }
    }
    Ok(None)
}

/// Name for `number`, or None (no match, no database, unreadable database).
pub fn lookup(number: &str) -> Option<String> {
    let db = std::env::var_os("FP6_CALLREC_CONTACTS_DB").map(PathBuf::from).unwrap_or_else(default_db);
    match lookup_in(&db, number) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("contact lookup: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(display_name(Some("Label".into()), Some("A".into()), None, None).as_deref(), Some("Label"));
        assert_eq!(display_name(Some(" ".into()), Some("Test".into()), Some("Person".into()), None).as_deref(), Some("Test Person"));
        assert_eq!(display_name(None, None, None, Some("Nick".into())).as_deref(), Some("Nick"));
        assert_eq!(display_name(None, None, None, None), None);
    }

    #[test]
    fn sqlite_lookup() {
        let dir = std::env::temp_dir().join(format!("fp6-callrec-contacts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("contacts.db");
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(
                "CREATE TABLE Contacts (contactId INTEGER PRIMARY KEY, deleted DATETIME);
                 CREATE TABLE PhoneNumbers (detailId INTEGER PRIMARY KEY, contactId INTEGER, phoneNumber TEXT, subTypes TEXT, normalizedNumber TEXT);
                 CREATE TABLE DisplayLabels (detailId INTEGER PRIMARY KEY, contactId INTEGER UNIQUE, displayLabel TEXT, displayLabelGroup TEXT, displayLabelGroupSortOrder INTEGER);
                 CREATE TABLE Names (detailId INTEGER PRIMARY KEY, contactId INTEGER UNIQUE, firstName TEXT, lastName TEXT);
                 CREATE TABLE Nicknames (detailId INTEGER PRIMARY KEY, contactId INTEGER, nickname TEXT);
                 INSERT INTO Contacts VALUES (1, NULL), (2, '2026-01-01');
                 INSERT INTO PhoneNumbers VALUES (10, 1, '+999 1234 5678', '', '+99912345678'), (11, 2, '+99900000000', '', '+99900000000');
                 INSERT INTO Names VALUES (20, 1, 'Test', 'Person'), (21, 2, 'Gone', 'Contact');",
            )
            .unwrap();
        }
        assert_eq!(lookup_in(&db, "0012345678").unwrap().as_deref(), Some("Test Person"));
        assert_eq!(lookup_in(&db, "+99900000000").unwrap(), None); // deleted
        assert_eq!(lookup_in(&db, "+99977777777").unwrap(), None);
        assert_eq!(lookup_in(&db, "withheld").unwrap(), None);
        assert_eq!(lookup_in(&dir.join("missing.db"), "+999").unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
