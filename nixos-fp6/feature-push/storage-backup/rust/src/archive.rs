//! Backup archive: an age file (X25519 recipient) holding a tar stream.
//!
//!   fp6-backup/index.json     first: what is inside (read by the preview)
//!   items/<category>/...      the files
//!   fp6-backup/manifest.json  last: SHA-256 and size of every item
//!
//! Verification decrypts the whole file (age authenticates every 64 KiB
//! chunk and the end of the stream), recomputes every hash and checks the
//! entries against index and manifest. Extraction writes selected items to
//! a staging directory and reports them only if the whole archive verified.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "fp6-backup";
pub const VERSION: u32 = 1;
pub const INDEX: &str = "fp6-backup/index.json";
pub const MANIFEST: &str = "fp6-backup/manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// Path inside the tar (items/...).
    pub path: String,
    pub category: String,
    /// Where it goes back: "home:<relative path>", "dconf", "sqlite:contacts",
    /// "sqlite:history", "wifi:<file name>".
    pub target: String,
    pub size: u64,
    pub mode: u32,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CategorySummary {
    pub files: u64,
    pub bytes: u64,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Index {
    pub format: String,
    pub version: u32,
    pub created: i64,
    pub categories: BTreeMap<String, CategorySummary>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hashed {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// The source changed size while it was read (padded/cut to the planned size).
    #[serde(default)]
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Manifest {
    pub items: Vec<Hashed>,
}

pub enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads exactly `left` bytes: zero padding if the file shrank, cut if it grew.
struct Exact<R> {
    inner: Option<R>,
    left: u64,
    hasher: Sha256,
    changed: bool,
    progress: u64,
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            if let Some(r) = self.inner.as_mut() {
                let mut one = [0u8; 1];
                if r.read(&mut one)? > 0 {
                    self.changed = true;
                }
                self.inner = None;
            }
            return Ok(0);
        }
        let want = buf.len().min(self.left as usize);
        let n = match self.inner.as_mut() {
            Some(r) => r.read(&mut buf[..want])?,
            None => 0,
        };
        let n = if n == 0 {
            self.changed = true;
            self.inner = None;
            buf[..want].fill(0);
            want
        } else {
            n
        };
        self.hasher.update(&buf[..n]);
        self.left -= n as u64;
        self.progress += n as u64;
        Ok(n)
    }
}

fn append<W: Write, R: Read>(b: &mut tar::Builder<W>, path: &str, size: u64, mode: u32, mtime: i64, r: R) -> io::Result<(String, bool)> {
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Regular);
    h.set_size(size);
    h.set_mode(mode & 0o7777);
    h.set_mtime(mtime.max(0) as u64);
    let mut ex = Exact { inner: Some(r), left: size, hasher: Sha256::new(), changed: false, progress: 0 };
    b.append_data(&mut h, path, &mut ex)?;
    Ok((hex(&ex.hasher.finalize()), ex.changed))
}

/// Writes the archive. `sources[i]` provides `index.items[i]`.
pub fn write<W: Write>(out: W, recipient: &age::x25519::Recipient, index: &Index, sources: &[Source],
                       progress: &mut dyn FnMut(u64)) -> Result<(W, Manifest), String> {
    if sources.len() != index.items.len() {
        return Err("internal: sources and index differ".into());
    }
    let enc = age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient)).map_err(|e| e.to_string())?;
    let stream = enc.wrap_output(out).map_err(|e| e.to_string())?;
    let mut b = tar::Builder::new(stream);
    b.mode(tar::HeaderMode::Deterministic);
    let ij = serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?;
    let now = crate::util::now();
    append(&mut b, INDEX, ij.len() as u64, 0o600, now, &ij[..]).map_err(|e| e.to_string())?;
    let mut man = Manifest::default();
    let mut done = 0u64;
    for (it, src) in index.items.iter().zip(sources) {
        let (sha, changed) = match src {
            Source::Bytes(v) => append(&mut b, &it.path, it.size, it.mode, it.mtime, &v[..]),
            Source::File(p) => {
                let f = File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
                append(&mut b, &it.path, it.size, it.mode, it.mtime, BufReader::with_capacity(256 * 1024, f))
            }
        }.map_err(|e| format!("{}: {e}", it.path))?;
        done += it.size;
        progress(done);
        man.items.push(Hashed { path: it.path.clone(), sha256: sha, size: it.size, changed });
    }
    let mj = serde_json::to_vec_pretty(&man).map_err(|e| e.to_string())?;
    append(&mut b, MANIFEST, mj.len() as u64, 0o600, now, &mj[..]).map_err(|e| e.to_string())?;
    let stream = b.into_inner().map_err(|e| e.to_string())?;
    let out = stream.finish().map_err(|e| e.to_string())?;
    Ok((out, man))
}

fn decrypt(path: &Path, id: &dyn age::Identity) -> Result<impl Read, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let d = age::Decryptor::new_buffered(BufReader::with_capacity(256 * 1024, f)).map_err(|e| format!("not a backup archive: {e}"))?;
    d.decrypt(std::iter::once(id)).map_err(|e| match e {
        age::DecryptError::NoMatchingKeys => "this archive was made with another key".to_string(),
        e => e.to_string(),
    })
}

/// Only the index (first entry): fast preview.
pub fn read_index(path: &Path, id: &dyn age::Identity) -> Result<Index, String> {
    let r = decrypt(path, id)?;
    let mut a = tar::Archive::new(r);
    let mut entries = a.entries().map_err(|e| e.to_string())?;
    let mut e = entries.next().ok_or("empty archive")?.map_err(|e| e.to_string())?;
    if e.path().map_err(|e| e.to_string())?.to_string_lossy() != INDEX {
        return Err("archive does not start with an index".into());
    }
    let mut s = Vec::new();
    e.read_to_end(&mut s).map_err(|e| e.to_string())?;
    let idx: Index = serde_json::from_slice(&s).map_err(|e| format!("index: {e}"))?;
    if idx.format != FORMAT || idx.version > VERSION {
        return Err(format!("unsupported archive {} v{}", idx.format, idx.version));
    }
    Ok(idx)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Verified {
    pub index: Index,
    pub files: u64,
    pub bytes: u64,
    pub changed: Vec<String>,
}

/// Decrypts and checks everything; selected items are written to
/// `staging/<n>` and returned with their staged path. The caller must only
/// use staged files when this returns Ok.
pub fn process(path: &Path, id: &dyn age::Identity, select: &dyn Fn(&Item) -> bool, staging: Option<&Path>,
               progress: &mut dyn FnMut(u64)) -> Result<(Verified, Vec<(Item, PathBuf)>), String> {
    let r = decrypt(path, id)?;
    let mut a = tar::Archive::new(r);
    let mut index: Option<Index> = None;
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut seen: HashMap<String, (String, u64)> = HashMap::new();
    let mut manifest: Option<Manifest> = None;
    let mut staged = Vec::new();
    let mut done = 0u64;
    for (n, e) in a.entries().map_err(|e| e.to_string())?.enumerate() {
        let mut e = e.map_err(|e| format!("archive damaged: {e}"))?;
        let p = e.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        if manifest.is_some() {
            return Err(format!("unexpected entry after the manifest: {p}"));
        }
        if n == 0 {
            if p != INDEX {
                return Err("archive does not start with an index".into());
            }
            let mut s = Vec::new();
            e.read_to_end(&mut s).map_err(|e| e.to_string())?;
            let i: Index = serde_json::from_slice(&s).map_err(|e| format!("index: {e}"))?;
            if i.format != FORMAT || i.version > VERSION {
                return Err(format!("unsupported archive {} v{}", i.format, i.version));
            }
            by_path = i.items.iter().enumerate().map(|(k, it)| (it.path.clone(), k)).collect();
            index = Some(i);
            continue;
        }
        if p == MANIFEST {
            let mut s = Vec::new();
            e.read_to_end(&mut s).map_err(|e| e.to_string())?;
            manifest = Some(serde_json::from_slice(&s).map_err(|e| format!("manifest: {e}"))?);
            continue;
        }
        let idx = index.as_ref().unwrap();
        let k = *by_path.get(&p).ok_or_else(|| format!("entry not in the index: {p}"))?;
        let item = &idx.items[k];
        if seen.contains_key(&p) {
            return Err(format!("duplicate entry {p}"));
        }
        let mut sink: Box<dyn Write> = Box::new(io::sink());
        let mut dst = None;
        if let (Some(dir), true) = (staging, select(item)) {
            let sp = dir.join(format!("{k:06}"));
            let f = File::create(&sp).map_err(|e| format!("{}: {e}", sp.display()))?;
            sink = Box::new(io::BufWriter::new(f));
            dst = Some(sp);
        }
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 128 * 1024];
        let mut size = 0u64;
        loop {
            let m = e.read(&mut buf).map_err(|e| format!("archive damaged in {p}: {e}"))?;
            if m == 0 {
                break;
            }
            h.update(&buf[..m]);
            sink.write_all(&buf[..m]).map_err(|e| e.to_string())?;
            size += m as u64;
            done += m as u64;
            progress(done);
        }
        sink.flush().map_err(|e| e.to_string())?;
        drop(sink);
        seen.insert(p.clone(), (hex(&h.finalize()), size));
        if let Some(sp) = dst {
            staged.push((item.clone(), sp));
        }
    }
    let index = index.ok_or("empty archive")?;
    let manifest = manifest.ok_or("archive is incomplete (no manifest)")?;
    let mh: HashMap<&str, &Hashed> = manifest.items.iter().map(|h| (h.path.as_str(), h)).collect();
    let mut changed = Vec::new();
    for it in &index.items {
        let m = mh.get(it.path.as_str()).ok_or_else(|| format!("{} missing from the manifest", it.path))?;
        let (sha, size) = seen.get(&it.path).ok_or_else(|| format!("{} missing from the archive", it.path))?;
        if *sha != m.sha256 || *size != m.size || *size != it.size {
            return Err(format!("checksum mismatch: {}", it.path));
        }
        if m.changed {
            changed.push(it.path.clone());
        }
    }
    if mh.len() != index.items.len() {
        return Err("manifest and index differ".into());
    }
    let bytes = index.items.iter().map(|i| i.size).sum();
    Ok((Verified { files: index.items.len() as u64, bytes, changed, index }, staged))
}

pub fn verify(path: &Path, id: &dyn age::Identity) -> Result<Verified, String> {
    process(path, id, &|_| false, None, &mut |_| {}).map(|r| r.0)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::tempdir;

    pub fn sample_index(items: Vec<(&str, &str, &str, u64)>) -> Index {
        Index {
            format: FORMAT.into(), version: VERSION, created: 1_800_000_000, categories: BTreeMap::new(),
            items: items.into_iter().map(|(p, c, t, s)| Item { path: p.into(), category: c.into(), target: t.into(), size: s, mode: 0o644, mtime: 1 }).collect(),
        }
    }

    #[test]
    fn roundtrip_verify_extract_and_tamper() {
        let t = tempdir("arch");
        let id = age::x25519::Identity::generate();
        let photo = t.join("p.jpg");
        std::fs::write(&photo, vec![9u8; 200_000]).unwrap();
        let long = format!("items/photos/Pictures/{}/x.jpg", "deep".repeat(40));
        let idx = sample_index(vec![
            ("items/settings/dconf.ini", "settings", "dconf", 12),
            (&long, "photos", "home:Pictures/x.jpg", 200_000),
        ]);
        let out = File::create(t.join("a.tar.age")).unwrap();
        let srcs = vec![Source::Bytes(b"[a]\nb=true\n\n".to_vec()), Source::File(photo.clone())];
        let (_, man) = write(out, &id.to_public(), &idx, &srcs, &mut |_| {}).unwrap();
        assert_eq!(man.items.len(), 2);
        let a = t.join("a.tar.age");
        assert_eq!(read_index(&a, &id).unwrap(), idx);
        let v = verify(&a, &id).unwrap();
        assert_eq!(v.files, 2);
        assert!(v.changed.is_empty());
        let st = t.join("stage");
        std::fs::create_dir(&st).unwrap();
        let (_, staged) = process(&a, &id, &|i| i.category == "photos", Some(&st), &mut |_| {}).unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(std::fs::read(&staged[0].1).unwrap(), vec![9u8; 200_000]);
        // another key
        let other = age::x25519::Identity::generate();
        assert!(verify(&a, &other).unwrap_err().contains("another key"));
        // flip one byte in the ciphertext body
        let mut bytes = std::fs::read(&a).unwrap();
        let k = bytes.len() / 2;
        bytes[k] ^= 1;
        std::fs::write(t.join("bad.tar.age"), &bytes).unwrap();
        assert!(verify(&t.join("bad.tar.age"), &id).is_err());
        // truncated
        std::fs::write(t.join("short.tar.age"), &std::fs::read(&a).unwrap()[..k]).unwrap();
        assert!(verify(&t.join("short.tar.age"), &id).is_err());
    }

    #[test]
    fn source_that_changes_size_keeps_the_archive_valid() {
        let t = tempdir("arch2");
        let id = age::x25519::Identity::generate();
        let f = t.join("grow");
        std::fs::write(&f, vec![1u8; 100]).unwrap();
        // planned 50 bytes (it "grew"), planned 300 bytes (it "shrank")
        let idx = sample_index(vec![("items/documents/a", "documents", "home:Documents/a", 50), ("items/documents/b", "documents", "home:Documents/b", 300)]);
        let out = File::create(t.join("a.tar.age")).unwrap();
        write(out, &id.to_public(), &idx, &[Source::File(f.clone()), Source::File(f)], &mut |_| {}).unwrap();
        let v = verify(&t.join("a.tar.age"), &id).unwrap();
        assert_eq!(v.changed.len(), 2);
    }
}
