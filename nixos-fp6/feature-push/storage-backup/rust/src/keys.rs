//! Backup encryption keys.
//!
//! Archives are age files encrypted to an X25519 recipient. The matching
//! identity is stored twice:
//!  * ~/.local/share/fp6-backup/identity.txt (0600): lets scheduled backups
//!    run and verify without asking for the passphrase. It sits on the phone
//!    next to the data it protects, so it adds no exposure; it is never put
//!    into a backup.
//!  * fp6-backup-key.age: the identity encrypted with the user's passphrase
//!    (age scrypt), kept locally and written next to the archives. With the
//!    passphrase it restores a backup on a reset or new phone, and plain
//!    `rage`/`age` can open everything too:
//!    age -d fp6-backup-key.age > key.txt
//!    age -d -i key.txt fp6-backup-....tar.age | tar t

use age::secrecy::{ExposeSecret, SecretString};
use age::x25519;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const KEY_FILE: &str = "fp6-backup-key.age";
pub const MIN_PASSPHRASE: usize = 8;

pub struct KeyStore {
    pub dir: PathBuf,
    /// scrypt log2(N); None = age's calibration (about 1 s on this device).
    pub work_factor: Option<u8>,
}

pub fn wrap(id: &x25519::Identity, passphrase: &str, work_factor: Option<u8>) -> Result<Vec<u8>, String> {
    let mut r = age::scrypt::Recipient::new(SecretString::from(passphrase.to_string()));
    if let Some(n) = work_factor {
        r.set_work_factor(n);
    }
    let enc = age::Encryptor::with_recipients(std::iter::once(&r as &dyn age::Recipient)).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut w = enc.wrap_output(&mut out).map_err(|e| e.to_string())?;
    w.write_all(id.to_string().expose_secret().as_bytes()).map_err(|e| e.to_string())?;
    w.write_all(b"\n").map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn unwrap(bytes: &[u8], passphrase: &str) -> Result<x25519::Identity, String> {
    let d = age::Decryptor::new_buffered(bytes).map_err(|e| format!("key file: {e}"))?;
    let id = age::scrypt::Identity::new(SecretString::from(passphrase.to_string()));
    let mut r = d.decrypt(std::iter::once(&id as &dyn age::Identity)).map_err(|e| match e {
        age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => "wrong passphrase".to_string(),
        e => e.to_string(),
    })?;
    let mut s = String::new();
    r.read_to_string(&mut s).map_err(|e| e.to_string())?;
    x25519::Identity::from_str(s.trim()).map_err(|e| format!("key file: {e}"))
}

impl KeyStore {
    pub fn new(dir: PathBuf) -> Self {
        let work_factor = std::env::var("FP6_BACKUP_SCRYPT_LOG_N").ok().and_then(|v| v.parse().ok());
        KeyStore { dir, work_factor }
    }
    fn id_path(&self) -> PathBuf {
        self.dir.join("identity.txt")
    }
    fn wrapped_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }
    pub fn exists(&self) -> bool {
        self.identity().is_some() && self.wrapped_path().exists()
    }
    pub fn identity(&self) -> Option<x25519::Identity> {
        let s = fs::read_to_string(self.id_path()).ok()?;
        x25519::Identity::from_str(s.trim()).ok()
    }
    pub fn recipient(&self) -> Option<x25519::Recipient> {
        self.identity().map(|i| i.to_public())
    }
    pub fn wrapped(&self) -> Option<Vec<u8>> {
        fs::read(self.wrapped_path()).ok()
    }

    fn store(&self, id: &x25519::Identity, wrapped: &[u8]) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700));
        crate::util::write_atomic(&self.wrapped_path(), wrapped, 0o600).map_err(|e| e.to_string())?;
        crate::util::write_atomic(&self.id_path(), format!("{}\n", id.to_string().expose_secret()).as_bytes(), 0o600)
            .map_err(|e| e.to_string())
    }

    /// First setup, or a new passphrase. With an existing key the old
    /// passphrase must match (the key itself stays, so older backups stay
    /// readable with this phone's key; their key files keep the old passphrase).
    pub fn set_passphrase(&self, old: Option<&str>, new: &str) -> Result<(), String> {
        if new.chars().count() < MIN_PASSPHRASE {
            return Err(format!("the passphrase needs at least {MIN_PASSPHRASE} characters"));
        }
        let id = match (self.identity(), self.wrapped()) {
            (Some(id), Some(w)) => {
                let old = old.ok_or("enter the current passphrase")?;
                let check = unwrap(&w, old)?;
                if check.to_public().to_string() != id.to_public().to_string() {
                    return Err("the stored key files do not match".into());
                }
                id
            }
            _ => x25519::Identity::generate(),
        };
        let w = wrap(&id, new, self.work_factor)?;
        self.store(&id, &w)
    }

    /// Restoring on a phone without a key (or with another one): use the key
    /// from the backup folder from now on.
    pub fn adopt(&self, id: &x25519::Identity, wrapped: &[u8]) -> Result<(), String> {
        self.store(id, wrapped)
    }

    /// Writes the passphrase-protected key next to the archives (if missing
    /// or different).
    pub fn export_to(&self, folder: &Path) -> Result<(), String> {
        let w = self.wrapped().ok_or("no backup key; set a passphrase first")?;
        let p = folder.join(KEY_FILE);
        if fs::read(&p).ok().as_deref() == Some(&w[..]) {
            return Ok(());
        }
        crate::util::write_atomic(&p, &w, 0o600).map_err(|e| format!("{}: {e}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::tempdir;

    #[test]
    fn setup_change_and_unwrap() {
        let t = tempdir("keys");
        let ks = KeyStore { dir: t.join("k"), work_factor: Some(10) };
        assert!(!ks.exists());
        assert!(ks.set_passphrase(None, "short").is_err());
        ks.set_passphrase(None, "correct horse").unwrap();
        assert!(ks.exists());
        let pub1 = ks.recipient().unwrap().to_string();
        assert!(ks.set_passphrase(Some("wrong one!"), "battery staple").is_err());
        assert!(ks.set_passphrase(None, "battery staple").is_err());
        ks.set_passphrase(Some("correct horse"), "battery staple").unwrap();
        assert_eq!(ks.recipient().unwrap().to_string(), pub1);
        let id = unwrap(&ks.wrapped().unwrap(), "battery staple").unwrap();
        assert_eq!(id.to_public().to_string(), pub1);
        assert_eq!(unwrap(&ks.wrapped().unwrap(), "correct horse").err().as_deref(), Some("wrong passphrase"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(t.join("k/identity.txt")).unwrap().permissions().mode() & 0o777, 0o600);
        ks.export_to(&t).unwrap();
        assert_eq!(fs::read(t.join(KEY_FILE)).unwrap(), ks.wrapped().unwrap());
    }
}
