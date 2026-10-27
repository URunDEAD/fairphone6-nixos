use serde::Deserialize;
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub id: String,
    #[serde(default = "session")]
    pub bus: String,
    pub name: String,
    pub path: String,
    pub interface: String,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub property: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unverified: bool,
}
fn session() -> String {
    "session".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub probe: Vec<Probe>,
}
impl Config {
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        let cfg: Self = toml::from_str(text).map_err(|_| "Invalid probe configuration")?;
        let mut ids = std::collections::HashSet::new();
        if cfg.probe.len() > 64 {
            return Err("Too many probes");
        }
        for p in &cfg.probe {
            if p.id.is_empty()
                || p.id.len() > 48
                || !p
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || !ids.insert(&p.id)
            {
                return Err("Invalid or duplicate probe ID");
            }
            if !matches!(p.bus.as_str(), "system" | "session")
                || zbus::names::WellKnownName::try_from(p.name.as_str()).is_err()
                || zbus::zvariant::ObjectPath::try_from(p.path.as_str()).is_err()
                || zbus::names::InterfaceName::try_from(p.interface.as_str()).is_err()
            {
                return Err("Invalid D-Bus endpoint");
            }
            if p.unit.as_ref().is_some_and(|u| {
                !u.ends_with(".service")
                    || u.len() > 128
                    || !u.bytes().all(|b| {
                        b.is_ascii_alphanumeric()
                            || b == b'-'
                            || b == b'.'
                            || b == b'@'
                            || b == b'_'
                    })
            }) {
                return Err("Invalid installed unit hint");
            }
            if p.method.is_some() && p.property.is_some() {
                return Err("Use either a method or property");
            }
            // Deliberate narrow method allowlist. Extending it requires a source
            // audit; a config file cannot turn this into a state-changing client.
            if p.method.as_ref().is_some_and(|m| {
                !matches!(
                    m.as_str(),
                    "Status"
                        | "GetStatus"
                        | "GetSettings"
                        | "Devices"
                        | "Calls"
                        | "Snapshot"
                        | "Version"
                        | "GetVersion"
                        | "GetServerInformation"
                )
            }) {
                return Err("Method has not been audited as read-only");
            }
            if p.property
                .as_ref()
                .is_some_and(|p| zbus::names::MemberName::try_from(p.as_str()).is_err())
            {
                return Err("Invalid property");
            }
            if (p.method.is_some() || p.property.is_some())
                && !matches!(
                    p.shape.as_deref(),
                    Some(
                        "string"
                            | "bool"
                            | "u32"
                            | "json-object"
                            | "json-array"
                            | "server-information"
                    )
                )
            {
                return Err("Missing or invalid reply shape");
            }
            if p.fields.values().any(|s| {
                !matches!(
                    s.as_str(),
                    "string" | "bool" | "number" | "array" | "object"
                )
            }) {
                return Err("Invalid JSON field shape");
            }
        }
        Ok(cfg)
    }
}
