//! fp6-usb-drive: phone as a USB CD-ROM / disk for a connected PC.
//!
//!  * image, checksum, sha256, catalog: pure logic (unit-tested on x86).
//!  * gadget, safepath: the privileged helper's configfs and file checks.
//!  * ops, client, ffi: the app backend (runs as the user) and its C ABI.
pub mod catalog;
pub mod checksum;
pub mod client;
pub mod ffi;
pub mod gadget;
pub mod image;
pub mod ops;
pub mod safepath;
pub mod sha256;
