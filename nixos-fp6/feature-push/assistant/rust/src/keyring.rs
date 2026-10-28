//! API keys and e-mail passwords: the freedesktop Secret Service
//! (gnome-keyring) when it answers on the session bus, else a 0600 JSON
//! file in the assistant's private data directory (0700). Values never
//! leave this module except to the HTTP/IMAP/SMTP clients, are never
//! logged and are never sent over the assistant's own D-Bus interface
//! (it only has Set/Has/Delete).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use tokio::sync::Mutex;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const SS_BUS: &str = "org.freedesktop.secrets";
const SS_PATH: &str = "/org/freedesktop/secrets";
const SS_IFACE: &str = "org.freedesktop.Secret.Service";
const APP_ATTR: &str = "fp6-assistant";

type SecretStruct = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

pub struct Secrets {
    session: Option<zbus::Connection>,
    file: PathBuf,
    lock: Mutex<()>,
    /// Force the file backend (tests, or no keyring wanted).
    file_only: bool,
}

impl Secrets {
    pub fn new(session: Option<zbus::Connection>, file: PathBuf) -> Self {
        Secrets { session, file, lock: Mutex::new(()), file_only: false }
    }
    pub fn file_only(file: PathBuf) -> Self {
        Secrets { session: None, file, lock: Mutex::new(()), file_only: true }
    }

    /// "keyring" or "file" (shown in Settings).
    pub async fn backend_name(&self) -> &'static str {
        if self.keyring().await.is_some() {
            "keyring"
        } else {
            "file"
        }
    }

    async fn keyring(&self) -> Option<(zbus::Connection, OwnedObjectPath)> {
        if self.file_only {
            return None;
        }
        let conn = self.session.clone()?;
        let reply = conn
            .call_method(Some(SS_BUS), SS_PATH, Some(SS_IFACE), "OpenSession", &("plain", Value::from("")))
            .await
            .ok()?;
        let (_out, session): (OwnedValue, OwnedObjectPath) = reply.body().deserialize().ok()?;
        Some((conn, session))
    }

    fn attrs(key: &str) -> HashMap<&str, &str> {
        HashMap::from([("application", APP_ATTR), ("key", key)])
    }

    async fn ss_search(conn: &zbus::Connection, key: &str) -> Option<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>)> {
        let r = conn.call_method(Some(SS_BUS), SS_PATH, Some(SS_IFACE), "SearchItems", &(Self::attrs(key),)).await.ok()?;
        r.body().deserialize().ok()
    }

    /// Unlock items/collections; follows a prompt (keyring password dialog)
    /// for up to 60 s.
    async fn ss_unlock(conn: &zbus::Connection, objects: Vec<OwnedObjectPath>) -> bool {
        let Ok(r) = conn.call_method(Some(SS_BUS), SS_PATH, Some(SS_IFACE), "Unlock", &(objects,)).await else { return false };
        let Ok((_unlocked, prompt)): Result<(Vec<OwnedObjectPath>, OwnedObjectPath), _> = r.body().deserialize() else { return false };
        if prompt.as_str() == "/" {
            return true;
        }
        Self::follow_prompt(conn, prompt).await
    }

    async fn follow_prompt(conn: &zbus::Connection, prompt: OwnedObjectPath) -> bool {
        use futures_util::StreamExt;
        let Ok(proxy) = zbus::Proxy::new(conn, SS_BUS, prompt.clone(), "org.freedesktop.Secret.Prompt").await else { return false };
        let Ok(mut completed) = proxy.receive_signal("Completed").await else { return false };
        if proxy.call_method("Prompt", &("",)).await.is_err() {
            return false;
        }
        match tokio::time::timeout(std::time::Duration::from_secs(60), completed.next()).await {
            Ok(Some(msg)) => msg.body().deserialize::<(bool, OwnedValue)>().map(|(dismissed, _)| !dismissed).unwrap_or(false),
            _ => false,
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let _g = self.lock.lock().await;
        if let Some((conn, session)) = self.keyring().await {
            if let Some((unlocked, locked)) = Self::ss_search(&conn, key).await {
                let mut items = unlocked;
                if items.is_empty() && !locked.is_empty() && Self::ss_unlock(&conn, locked.clone()).await {
                    items = locked;
                }
                if let Some(item) = items.into_iter().next() {
                    let r = conn
                        .call_method(Some(SS_BUS), SS_PATH, Some(SS_IFACE), "GetSecrets", &(vec![item.clone()], session.clone()))
                        .await
                        .ok()?;
                    let map: HashMap<OwnedObjectPath, SecretStruct> = r.body().deserialize().ok()?;
                    return map.into_values().next().and_then(|s| String::from_utf8(s.2).ok());
                }
                return None;
            }
        }
        let map: BTreeMap<String, String> = crate::settings::load_json(&self.file);
        map.get(key).cloned()
    }

    pub async fn has(&self, key: &str) -> bool {
        self.get(key).await.map(|v| !v.is_empty()).unwrap_or(false)
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let _g = self.lock.lock().await;
        if let Some((conn, session)) = self.keyring().await {
            let r = conn.call_method(Some(SS_BUS), SS_PATH, Some(SS_IFACE), "ReadAlias", &("default",)).await.map_err(|e| e.to_string())?;
            let collection: OwnedObjectPath = r.body().deserialize().map_err(|e| e.to_string())?;
            if collection.as_str() != "/" {
                let _ = Self::ss_unlock(&conn, vec![collection.clone()]).await;
                let mut props: HashMap<&str, Value> = HashMap::new();
                props.insert("org.freedesktop.Secret.Item.Label", Value::from(format!("Assistant: {key}")));
                props.insert("org.freedesktop.Secret.Item.Attributes", Value::from(Self::attrs(key)));
                let secret: SecretStruct = (session, Vec::new(), value.as_bytes().to_vec(), "text/plain".into());
                let r = conn
                    .call_method(Some(SS_BUS), collection.as_str(), Some("org.freedesktop.Secret.Collection"), "CreateItem", &(props, secret, true))
                    .await
                    .map_err(|e| format!("keyring: {e}"))?;
                let (_item, prompt): (OwnedObjectPath, OwnedObjectPath) = r.body().deserialize().map_err(|e| e.to_string())?;
                if prompt.as_str() != "/" && !Self::follow_prompt(&conn, prompt).await {
                    return Err("the keyring did not store the key (dismissed)".into());
                }
                // A copy in the file would defeat the keyring: remove any.
                self.file_remove(key);
                return Ok(());
            }
        }
        let mut map: BTreeMap<String, String> = crate::settings::load_json(&self.file);
        map.insert(key.to_string(), value.to_string());
        crate::settings::save_json(&self.file, &map).map_err(|e| e.to_string())
    }

    fn file_remove(&self, key: &str) {
        let mut map: BTreeMap<String, String> = crate::settings::load_json(&self.file);
        if map.remove(key).is_some() {
            let _ = crate::settings::save_json(&self.file, &map);
        }
    }

    pub async fn delete(&self, key: &str) {
        let _g = self.lock.lock().await;
        if let Some((conn, _)) = self.keyring().await {
            if let Some((unlocked, locked)) = Self::ss_search(&conn, key).await {
                for item in unlocked.into_iter().chain(locked) {
                    let _ = conn.call_method(Some(SS_BUS), item.as_str(), Some("org.freedesktop.Secret.Item"), "Delete", &()).await;
                }
            }
        }
        self.file_remove(key);
    }
}

#[allow(dead_code)]
fn _path_type_check(_: ObjectPath<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn file_backend_roundtrip_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("data/secrets.json");
        let s = Secrets::file_only(f.clone());
        assert!(!s.has("backend:anthropic").await);
        s.set("backend:anthropic", "test-key-not-real").await.unwrap();
        assert_eq!(s.get("backend:anthropic").await.as_deref(), Some("test-key-not-real"));
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        s.delete("backend:anthropic").await;
        assert!(!s.has("backend:anthropic").await);
    }
}
