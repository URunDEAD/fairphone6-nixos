#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
pub const BUS: &str = "com.fairphone6.QuickShare1";
pub const PATH: &str = "/com/fairphone6/QuickShare1";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Transfer {
    pub id: String,
    pub direction: String,
    pub sender: String,
    pub files: Vec<String>,
    pub pin: String,
    pub bytes: u64,
    pub total: u64,
    pub status: String,
    pub directory: String,
    pub text: String,
    pub is_url: bool,
}

#[zbus::proxy(
    interface = "com.fairphone6.QuickShare1",
    default_service = "com.fairphone6.QuickShare1",
    default_path = "/com/fairphone6/QuickShare1"
)]
pub trait QuickShare {
    #[zbus(property)]
    fn visibility(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_visibility(&self, value: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn device_name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_device_name(&self, value: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;
    fn start_discovery(&self) -> zbus::Result<()>;
    fn stop_discovery(&self) -> zbus::Result<()>;
    fn list_devices(&self) -> zbus::Result<String>;
    fn list_transfers(&self) -> zbus::Result<String>;
    fn send(&self, target: &str, items: &[String]) -> zbus::Result<String>;
    fn accept(&self, transfer_id: &str) -> zbus::Result<()>;
    fn reject(&self, transfer_id: &str) -> zbus::Result<()>;
    fn cancel(&self, transfer_id: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn device_found(&self, id: &str, name: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn device_lost(&self, id: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn incoming_request(
        &self,
        transfer_id: &str,
        sender: &str,
        files: Vec<String>,
        pin: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    fn progress(&self, transfer_id: &str, bytes: u64, total: u64) -> zbus::Result<()>;
    #[zbus(signal)]
    fn finished(&self, transfer_id: &str, status: &str) -> zbus::Result<()>;
}
