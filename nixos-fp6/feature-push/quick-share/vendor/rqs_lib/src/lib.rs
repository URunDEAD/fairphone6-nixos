#![forbid(unsafe_code)]
// Pinned rqs_lib protocol engine; lifecycle, mDNS and policy live in quickshare-core.
#[macro_use]
extern crate log;
pub mod channel;
mod errors;
mod hdl;
pub mod storage;
pub mod utils;
pub use hdl::info::TransferMetadata;
pub use hdl::{InboundRequest, OutboundPayload, OutboundRequest, State, TextPayloadType};
pub use utils::DeviceType;
pub mod sharing_nearby {
    include!(concat!(env!("OUT_DIR"), "/sharing.nearby.rs"));
}
pub mod securemessage {
    include!(concat!(env!("OUT_DIR"), "/securemessage.rs"));
}
pub mod securegcm {
    include!(concat!(env!("OUT_DIR"), "/securegcm.rs"));
}
pub mod location_nearby_connections {
    include!(concat!(env!("OUT_DIR"), "/location.nearby.connections.rs"));
}
