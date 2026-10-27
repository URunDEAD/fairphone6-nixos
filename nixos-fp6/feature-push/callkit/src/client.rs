//! Small typed client. Keep the connection alive for the lifetime of all calls.
use crate::model::{Call, Command};
use zbus::proxy;
pub const BUS_NAME: &str = "com.fairphone6.Calls1";
pub const PATH: &str = "/com/fairphone6/Calls1";
#[proxy(
    interface = "com.fairphone6.Calls1",
    default_service = "com.fairphone6.Calls1",
    default_path = "/com/fairphone6/Calls1"
)]
pub trait Calls {
    fn report_incoming(&self, app: &str, display_name: &str) -> zbus::Result<u64>;
    fn report_outgoing(&self, app: &str, display_name: &str) -> zbus::Result<u64>;
    fn report_connected(&self, id: u64) -> zbus::Result<()>;
    fn report_ended(&self, id: u64) -> zbus::Result<()>;
    fn calls(&self) -> zbus::Result<String>;
    fn commands(&self, id: u64) -> zbus::Result<String>;
    fn complete(&self, id: u64, serial: u64, success: bool) -> zbus::Result<()>;
    fn snapshot(&self) -> zbus::Result<String>;
    fn control(&self, id: u64, action: &str) -> zbus::Result<u64>;
    fn history(&self) -> zbus::Result<String>;
    fn clear_history(&self) -> zbus::Result<()>;
    fn set_proximity(&self, near: bool) -> zbus::Result<()>;
    #[zbus(signal)]
    fn changed(&self, revision: u64) -> zbus::Result<()>;
}
pub async fn own_calls(proxy: &CallsProxy<'_>) -> Result<Vec<Call>, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&proxy.calls().await?)?)
}
pub async fn pending(
    proxy: &CallsProxy<'_>,
    id: u64,
) -> Result<Option<Command>, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&proxy.commands(id).await?)?)
}
