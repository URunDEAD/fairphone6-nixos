//! FP6 USB connection preferences ("USB modes"): which USB functions the
//! phone offers to the computer on the cable (USB network, file transfer,
//! ADB, USB drive) and who powers whom (USB-C power role).
pub mod configfs;
pub mod model;
pub mod typec;

pub const BUS_NAME: &str = "com.fairphone6.UsbModes";
pub const PATH: &str = "/com/fairphone6/UsbModes";
pub const IFACE: &str = "com.fairphone6.UsbModes1";
pub const STATUS_FILE: &str = "/run/fp6-usb-modes/status.json";
