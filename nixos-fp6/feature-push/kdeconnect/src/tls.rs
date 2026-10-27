use anyhow::{ensure, Result};
use openssl::{
    asn1::Asn1Time,
    bn::BigNum,
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    rsa::Rsa,
    ssl::{Ssl, SslContext, SslContextBuilder, SslMethod, SslVerifyMode, SslVersion},
    x509::{X509NameBuilder, X509},
};
use std::{path::Path, pin::Pin};
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

#[derive(Clone)]
pub struct Credentials {
    pub cert: X509,
    key: PKey<Private>,
}
impl Credentials {
    pub fn load(directory: &Path, id: &str) -> Result<Self> {
        let cert = directory.join("certificate.pem");
        let key = directory.join("private.pem");
        if cert.exists() || key.exists() {
            use std::os::unix::fs::PermissionsExt;
            for path in [&cert, &key] {
                ensure!(
                    std::fs::symlink_metadata(path)?.is_file(),
                    "identity files must be regular files"
                );
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
            let c = Self {
                cert: X509::from_pem(&std::fs::read(cert)?)?,
                key: PKey::private_key_from_pem(&std::fs::read(key)?)?,
            };
            ensure!(
                c.cert.public_key()?.public_eq(&c.key),
                "certificate and key differ"
            );
            ensure!(common_name(&c.cert)? == id, "certificate identity mismatch");
            return Ok(c);
        }
        let c = Self::generate(id)?;
        crate::store::atomic_write(&key, &c.key.private_key_to_pem_pkcs8()?)?;
        crate::store::atomic_write(&cert, &c.cert.to_pem()?)?;
        Ok(c)
    }
    pub fn generate(id: &str) -> Result<Self> {
        let key = PKey::from_rsa(Rsa::generate(2048)?)?;
        let mut name = X509NameBuilder::new()?;
        name.append_entry_by_nid(Nid::COMMONNAME, id)?;
        name.append_entry_by_nid(Nid::ORGANIZATIONNAME, "KDE Connect")?;
        let name = name.build();
        let mut b = X509::builder()?;
        b.set_version(2)?;
        let mut serial = BigNum::new()?;
        serial.rand(128, openssl::bn::MsbOption::MAYBE_ZERO, false)?;
        b.set_serial_number(serial.to_asn1_integer()?.as_ref())?;
        b.set_subject_name(&name)?;
        b.set_issuer_name(&name)?;
        b.set_pubkey(&key)?;
        b.set_not_before(Asn1Time::days_from_now(0)?.as_ref())?;
        b.set_not_after(Asn1Time::days_from_now(3650)?.as_ref())?;
        b.sign(&key, MessageDigest::sha256())?;
        Ok(Self {
            cert: b.build(),
            key,
        })
    }
    fn context(&self) -> Result<SslContext> {
        let mut b = SslContextBuilder::new(SslMethod::tls())?;
        b.set_min_proto_version(Some(SslVersion::TLS1_2))?;
        b.set_certificate(&self.cert)?;
        b.set_private_key(&self.key)?;
        // KDE Connect uses self-signed certificates, not the Web PKI. The TLS
        // proof of possession is verified by OpenSSL. CN, dates and the saved
        // certificate pin are checked BEFORE exposing the connection to plugins.
        b.set_verify_callback(
            SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
            |_, _| true,
        );
        Ok(b.build())
    }
    pub async fn handshake(&self, tcp: TcpStream, server: bool) -> Result<SslStream<TcpStream>> {
        let mut stream = SslStream::new(Ssl::new(self.context()?.as_ref())?, tcp)?;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            if server {
                Pin::new(&mut stream).accept().await
            } else {
                Pin::new(&mut stream).connect().await
            }
        })
        .await??;
        let cert = stream
            .ssl()
            .peer_certificate()
            .ok_or_else(|| anyhow::anyhow!("missing peer certificate"))?;
        let now = Asn1Time::days_from_now(0)?;
        ensure!(
            cert.not_before() <= now && cert.not_after() >= now,
            "peer certificate is outside validity dates"
        );
        Ok(stream)
    }
}
pub fn common_name(cert: &X509) -> Result<String> {
    Ok(cert
        .subject_name()
        .entries_by_nid(Nid::COMMONNAME)
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing certificate common name"))?
        .data()
        .to_string()?)
}
pub fn fingerprint(cert: &X509) -> Result<String> {
    Ok(cert
        .digest(MessageDigest::sha256())?
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":"))
}
pub fn verification_code(a: &X509, b: &X509, timestamp: i64) -> Result<String> {
    // KDE Qt and Android both use SubjectPublicKeyInfo DER for this hash.
    let mut a = a.public_key()?.public_key_to_der()?;
    let mut b = b.public_key()?.public_key_to_der()?;
    if a < b {
        std::mem::swap(&mut a, &mut b);
    }
    a.extend(b);
    a.extend(timestamp.to_string().bytes());
    Ok(openssl::sha::sha256(&a)[..4]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v8_verification_matches_independent_spki_vector() {
        let a = X509::from_pem(include_bytes!("../tests/fixtures/synthetic-a.pem")).unwrap();
        let b = X509::from_pem(include_bytes!("../tests/fixtures/synthetic-b.pem")).unwrap();
        assert_eq!(verification_code(&a, &b, 1700000000).unwrap(), "BED2B05B");
        assert_eq!(verification_code(&b, &a, 1700000000).unwrap(), "BED2B05B");
    }
}
