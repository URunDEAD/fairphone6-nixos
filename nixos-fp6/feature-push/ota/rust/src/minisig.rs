//! Ed25519 release-manifest signatures in the minisign format.
//!
//! Public keys and `.minisig` files are minisign-compatible, so a manifest can
//! also be checked with the stock tool (`minisign -Vm manifest.json -P <key>`).
//! Signatures use the current "ED" algorithm (Ed25519 over the BLAKE2b-512 of
//! the file); the legacy "Ed" (over the raw file) is accepted when verifying.
//! The trusted comment is covered by the global signature.
//!
//! The secret key file is NOT minisign's (scrypt-encrypted) format: it is
//! `untrusted comment: ...` + base64("Fs" || key id || 32-byte seed), kept
//! mode 0600 on the release machine only (results/.../ota/keys, git-ignored).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use blake2::{Blake2b512, Digest};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use std::io::Read;

pub struct PublicKey {
    pub key_id: [u8; 8],
    pub key: VerifyingKey,
}

pub struct SecretKey {
    pub key_id: [u8; 8],
    pub key: SigningKey,
}

/// minisign prints the key id as the little-endian u64 in upper-case hex.
pub fn key_id_hex(id: &[u8; 8]) -> String {
    format!("{:016X}", u64::from_le_bytes(*id))
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open("/dev/urandom").map_err(|e| format!("/dev/urandom: {e}"))?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(b)
}

/// The base64 line of a key or signature file: the first line that is not a
/// comment (a bare base64 string is accepted too).
fn payload_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
}

impl SecretKey {
    pub fn generate() -> Result<SecretKey, String> {
        let b = random(40)?;
        let mut key_id = [0u8; 8];
        key_id.copy_from_slice(&b[..8]);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&b[8..40]);
        Ok(SecretKey { key_id, key: SigningKey::from_bytes(&seed) })
    }

    pub fn from_seed(key_id: [u8; 8], seed: [u8; 32]) -> SecretKey {
        SecretKey { key_id, key: SigningKey::from_bytes(&seed) }
    }

    pub fn public(&self) -> PublicKey {
        PublicKey { key_id: self.key_id, key: self.key.verifying_key() }
    }

    pub fn to_file(&self) -> String {
        let mut b = b"Fs".to_vec();
        b.extend_from_slice(&self.key_id);
        b.extend_from_slice(self.key.as_bytes());
        format!("untrusted comment: fp6-ota manifest SECRET key {} (never commit, never copy to the phone)\n{}\n",
                key_id_hex(&self.key_id), B64.encode(b))
    }

    pub fn from_file(s: &str) -> Result<SecretKey, String> {
        let b = B64.decode(payload_line(s).ok_or("empty secret key file")?).map_err(|e| format!("secret key: {e}"))?;
        if b.len() != 42 || &b[..2] != b"Fs" {
            return Err("not an fp6-ota secret key".into());
        }
        let mut key_id = [0u8; 8];
        key_id.copy_from_slice(&b[2..10]);
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&b[10..42]);
        Ok(SecretKey::from_seed(key_id, seed))
    }

    /// A `.minisig` file for `msg`. `trusted` must be a single line.
    pub fn sign(&self, msg: &[u8], trusted: &str) -> Result<String, String> {
        if trusted.contains('\n') || trusted.contains('\r') {
            return Err("trusted comment must be one line".into());
        }
        let sig = self.key.sign(&Blake2b512::digest(msg));
        let mut sb = b"ED".to_vec();
        sb.extend_from_slice(&self.key_id);
        sb.extend_from_slice(&sig.to_bytes());
        let mut g = sig.to_bytes().to_vec();
        g.extend_from_slice(trusted.as_bytes());
        let global = self.key.sign(&g);
        Ok(format!("untrusted comment: fp6-ota release manifest signature from key {}\n{}\ntrusted comment: {}\n{}\n",
                   key_id_hex(&self.key_id), B64.encode(sb), trusted, B64.encode(global.to_bytes())))
    }
}

impl PublicKey {
    pub fn to_base64(&self) -> String {
        let mut b = b"Ed".to_vec();
        b.extend_from_slice(&self.key_id);
        b.extend_from_slice(self.key.as_bytes());
        B64.encode(b)
    }

    pub fn to_file(&self) -> String {
        format!("untrusted comment: minisign public key {}\n{}\n", key_id_hex(&self.key_id), self.to_base64())
    }

    /// From a minisign public key file or its base64 line.
    pub fn parse(s: &str) -> Result<PublicKey, String> {
        let b = B64.decode(payload_line(s).ok_or("empty public key")?).map_err(|e| format!("public key: {e}"))?;
        if b.len() != 42 || &b[..2] != b"Ed" {
            return Err("not a minisign Ed25519 public key".into());
        }
        let mut key_id = [0u8; 8];
        key_id.copy_from_slice(&b[2..10]);
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&b[10..42]);
        let key = VerifyingKey::from_bytes(&pk).map_err(|e| format!("public key: {e}"))?;
        Ok(PublicKey { key_id, key })
    }

    /// Verifies a `.minisig` for `msg`; returns the trusted comment.
    pub fn verify(&self, msg: &[u8], minisig: &str) -> Result<String, String> {
        let lines: Vec<&str> = minisig.lines().collect();
        if lines.len() < 4 || !lines[0].starts_with("untrusted comment:") {
            return Err("malformed signature file".into());
        }
        let sb = B64.decode(lines[1].trim()).map_err(|e| format!("signature: {e}"))?;
        if sb.len() != 74 {
            return Err("malformed signature".into());
        }
        let alg = &sb[..2];
        if sb[2..10] != self.key_id {
            return Err(format!("signed by another key ({} instead of {})",
                               key_id_hex(sb[2..10].try_into().unwrap()), key_id_hex(&self.key_id)));
        }
        let sig = Signature::from_bytes(sb[10..74].try_into().unwrap());
        let ok = match alg {
            b"ED" => self.key.verify_strict(&Blake2b512::digest(msg), &sig),
            b"Ed" => self.key.verify_strict(msg, &sig),
            _ => return Err("unknown signature algorithm".into()),
        };
        ok.map_err(|_| "signature does not match the file".to_string())?;
        let trusted = lines[2].strip_prefix("trusted comment: ").ok_or("missing trusted comment")?;
        let gb = B64.decode(lines[3].trim()).map_err(|e| format!("global signature: {e}"))?;
        let gs = Signature::from_bytes(gb.as_slice().try_into().map_err(|_| "malformed global signature")?);
        let mut g = sb[10..74].to_vec();
        g.extend_from_slice(trusted.as_bytes());
        self.key.verify_strict(&g, &gs).map_err(|_| "trusted comment signature does not match".to_string())?;
        Ok(trusted.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SecretKey {
        SecretKey::from_seed(*b"\x01\x02\x03\x04\x05\x06\x07\x08", [7u8; 32])
    }

    #[test]
    fn sign_verify_roundtrip_and_tamper() {
        let k = key();
        let p = PublicKey::parse(&k.public().to_file()).unwrap();
        let msg = br#"{"release":3}"#;
        let s = k.sign(msg, "fp6-ota release=3 channel=kernel6").unwrap();
        assert_eq!(p.verify(msg, &s).unwrap(), "fp6-ota release=3 channel=kernel6");
        // a changed file
        assert!(p.verify(br#"{"release":4}"#, &s).unwrap_err().contains("does not match"));
        // a changed trusted comment
        let t = s.replace("release=3", "release=9");
        assert!(p.verify(msg, &t).unwrap_err().contains("trusted comment"));
        // another key
        let other = SecretKey::from_seed(*b"\x09\x09\x09\x09\x09\x09\x09\x09", [8u8; 32]);
        assert!(other.public().verify(msg, &s).unwrap_err().contains("another key"));
        // same key id, different key material
        let evil = SecretKey::from_seed(k.key_id, [9u8; 32]);
        let s2 = evil.sign(msg, "x").unwrap();
        assert!(p.verify(msg, &s2).is_err());
        // truncated
        assert!(p.verify(msg, &s.lines().take(2).collect::<Vec<_>>().join("\n")).is_err());
        assert!(k.sign(msg, "two\nlines").is_err());
    }

    #[test]
    fn key_files_roundtrip() {
        let k = key();
        let k2 = SecretKey::from_file(&k.to_file()).unwrap();
        assert_eq!(k2.key.as_bytes(), k.key.as_bytes());
        assert_eq!(k2.key_id, k.key_id);
        assert_eq!(key_id_hex(&k.key_id), "0807060504030201");
        assert!(PublicKey::parse(&k.to_file()).is_err(), "a secret key is not a public key");
        assert!(SecretKey::from_file(&k.public().to_file()).is_err());
        let p = PublicKey::parse(&k.public().to_base64()).unwrap();
        assert_eq!(p.key.as_bytes(), k.public().key.as_bytes());
    }

    #[test]
    fn legacy_ed_signature_accepted() {
        let k = key();
        let msg = b"legacy";
        let sig = k.key.sign(msg);
        let mut sb = b"Ed".to_vec();
        sb.extend_from_slice(&k.key_id);
        sb.extend_from_slice(&sig.to_bytes());
        let mut g = sig.to_bytes().to_vec();
        g.extend_from_slice(b"tc");
        let gs = k.key.sign(&g);
        let file = format!("untrusted comment: x\n{}\ntrusted comment: tc\n{}\n", B64.encode(sb), B64.encode(gs.to_bytes()));
        assert_eq!(k.public().verify(msg, &file).unwrap(), "tc");
    }
}
