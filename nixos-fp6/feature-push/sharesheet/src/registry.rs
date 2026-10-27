use crate::model::Target;
use anyhow::Result;
use zbus::{
    zvariant::{OwnedValue, Structure},
    Connection,
};
pub const BUS: &str = "com.lomiri.content.dbus.Service";
pub const PATH: &str = "/";

// Peer wire format is (ssaysb), wrapped in a variant; peer.cpp defines it.
pub fn decode_peer(value: OwnedValue) -> Result<Option<Target>> {
    let fields = Structure::try_from(value)?;
    let (id, name, _icon_data, _icon_name, _default): (String, String, Vec<u8>, String, bool) =
        fields.try_into()?;
    if id == "com.fairphone6.Share" || id.is_empty() {
        return Ok(None);
    }
    Ok(Some(Target::new(
        "content",
        &id,
        &name,
        "App · Content Hub",
        false,
    )))
}
pub async fn peers(bus: &Connection, kind: &str) -> Result<Vec<Target>> {
    let proxy = zbus::Proxy::new(bus, BUS, PATH, BUS).await?;
    let values: Vec<OwnedValue> = proxy.call("KnownSharesForType", &(kind,)).await?;
    Ok(values
        .into_iter()
        .filter_map(|v| decode_peer(v).ok().flatten())
        .collect())
}
