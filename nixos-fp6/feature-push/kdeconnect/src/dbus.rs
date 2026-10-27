use crate::app::App;
use std::sync::Arc;

pub struct Api(pub Arc<App>);
fn error(e: anyhow::Error) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
#[zbus::interface(name = "com.fairphone6.KdeConnect1")]
impl Api {
    async fn devices(&self) -> zbus::fdo::Result<String> {
        self.0.snapshot().await.map_err(error)
    }
    async fn discover(&self) {
        self.0.discover.notify_one();
    }
    async fn request_pair(&self, id: &str, fingerprint: &str) -> zbus::fdo::Result<()> {
        self.0.request_pair(id, fingerprint).await.map_err(error)
    }
    async fn accept_pair(&self, id: &str, fingerprint: &str) -> zbus::fdo::Result<()> {
        self.0.accept_pair(id, fingerprint).await.map_err(error)
    }
    async fn unpair(&self, id: &str) -> zbus::fdo::Result<()> {
        self.0.unpair(id).await.map_err(error)
    }
    async fn set_plugin(&self, id: &str, plugin: &str, enabled: bool) -> zbus::fdo::Result<()> {
        self.0.set_plugin(id, plugin, enabled).await.map_err(error)
    }
    async fn action(&self, id: &str, action: &str, data: &str) -> zbus::fdo::Result<()> {
        self.0.action(id, action, data).await.map_err(error)
    }
    async fn clipboard_state(&self) -> String {
        self.0.clipboard_state().await.to_string()
    }
    async fn report_clipboard(&self, content: &str) -> zbus::fdo::Result<()> {
        self.0.local_clipboard(content).await.map_err(error)
    }
    async fn stop_ringing(&self) -> zbus::fdo::Result<()> {
        self.0.stop_ring().await.map_err(error)
    }
}
