//! dconf dumps: the user's keys without the backup page's own settings
//! (restoring an old backup must not change where backups go).

/// Section path prefixes left out of a dump and skipped on restore.
pub const EXCLUDE: &[&str] = &["com/fairphone6/backup"];

/// Removes `[section]` blocks whose path is under one of EXCLUDE.
pub fn filter_dump(dump: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for line in dump.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let sec = t[1..t.len() - 1].trim_matches('/');
            skip = EXCLUDE.iter().any(|e| sec == *e || sec.starts_with(&format!("{e}/")));
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Number of keys in a dump (for the preview).
pub fn count_keys(dump: &str) -> usize {
    dump.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('[') && !l.starts_with('#') && l.contains('=')).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_own_section() {
        let d = "[com/fairphone6/backup]\nfolder='/media/user/SD'\n\n[com/lomiri/shell]\nfoo=1\n\n[com/fairphone6/backup/x]\na=1\n\n[com/fairphone6/backupfoo]\nb=2\n";
        let f = filter_dump(d);
        assert!(!f.contains("folder"));
        assert!(!f.contains("a=1"));
        assert!(f.contains("[com/lomiri/shell]\nfoo=1"));
        assert!(f.contains("b=2"));
        assert_eq!(count_keys(&f), 2);
    }
}
