use crate::{
    config::Probe,
    report::{Check, Status},
};
use std::collections::HashMap;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedValue,
};
pub type Props = HashMap<String, OwnedValue>;
pub type Objects = Vec<(zbus::zvariant::OwnedObjectPath, Props)>;

pub fn connect(bus: &str) -> zbus::Result<Connection> {
    let address = if bus == "system" {
        std::env::var("DBUS_SYSTEM_BUS_ADDRESS")
            .unwrap_or_else(|_| "unix:path=/run/dbus/system_bus_socket".into())
    } else {
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap_or_else(|_| format!("unix:path=/run/user/{}/bus", nix::unistd::getuid()))
    };
    if !local_address(&address) {
        return Err(zbus::Error::Failure(
            "Non-local D-Bus transport refused".into(),
        ));
    }
    zbus::blocking::connection::Builder::address(address.as_str())?.build()
}
pub fn local_address(address: &str) -> bool {
    !address.is_empty()
        && address
            .split(';')
            .all(|a| a.starts_with("unix:path=") || a.starts_with("unix:abstract="))
}
/// Resolve ONCE to a unique owner. No call is sent to an activatable service
/// name, including introspection/properties, so owner races cannot activate it.
pub fn owner(c: &Connection, name: &str) -> Result<Option<String>, zbus::Error> {
    let bus = Proxy::new(
        c,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let owned: bool = bus.call("NameHasOwner", &(name,))?;
    if !owned {
        return Ok(None);
    }
    bus.call("GetNameOwner", &(name,)).map(Some)
}
pub fn proxy<'a>(
    c: &'a Connection,
    owner: &'a str,
    path: &'a str,
    interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
    zbus::blocking::proxy::Builder::new(c)
        .destination(owner)?
        .path(path)?
        .interface(interface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
}
pub fn properties(c: &Connection, owner: &str, path: &str, interface: &str) -> zbus::Result<Props> {
    proxy(c, owner, path, interface)?.call("GetProperties", &())
}
pub fn bool_prop(p: &Props, name: &str) -> Option<bool> {
    p.get(name).and_then(|v| bool::try_from(v).ok())
}
pub fn str_prop<'a>(p: &'a Props, name: &str) -> Option<&'a str> {
    p.get(name).and_then(|v| <&str>::try_from(v).ok())
}

/// Status of an installed unit whose bus name is not owned. `state` is
/// `systemctl show` output (LoadState, ActiveState, SubState, WantedBy,
/// RequiredBy, UpheldBy). A stopped unit that nothing starts at login
/// (on-demand, or D-Bus activated) is normal and passes; a stopped unit that a
/// target wants, or one that keeps restarting, is a warning.
pub fn stopped_unit_status(state: &str, activatable: bool) -> (Status, &'static str) {
    let prop = |name: &str| {
        state
            .lines()
            .find_map(|l| l.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
            .unwrap_or("")
            .trim()
    };
    let active = prop("ActiveState");
    if matches!(active, "failed" | "active") {
        return (
            Status::Fail,
            "Installed service has failed or is active without its expected bus name",
        );
    }
    if prop("SubState") == "auto-restart" || matches!(active, "activating" | "reloading") {
        return (
            Status::Warn,
            "Installed service is restarting or starting without its bus name yet",
        );
    }
    let wanted = ["WantedBy", "RequiredBy", "UpheldBy"]
        .iter()
        .any(|k| !prop(k).is_empty());
    if activatable {
        (
            Status::Pass,
            "Installed and D-Bus activatable; stopped until needed, activation intentionally avoided",
        )
    } else if !wanted {
        (
            Status::Pass,
            "Installed on-demand service is stopped; no activation requested",
        )
    } else {
        (
            Status::Warn,
            "Installed service is stopped although a target wants it; no activation requested",
        )
    }
}

pub fn check(p: &Probe) -> Check {
    let finish = |s, d| Check::new(&format!("feature-{}", p.id), s, d);
    let c = match connect(&p.bus) {
        Ok(c) => c,
        Err(_) => return finish(Status::Warn, "Bus unavailable; probe could not run"),
    };
    check_on(&c, p)
}
pub fn check_on(c: &Connection, p: &Probe) -> Check {
    let finish = |s, d| Check::new(&format!("feature-{}", p.id), s, d);
    let result = (|| -> zbus::Result<Check> {
        let owner = match owner(c, &p.name)? {
            Some(owner) => owner,
            None => {
                let bus = proxy(
                    c,
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                )?;
                let names: Vec<String> = bus.call("ListActivatableNames", &())?;
                let activatable = names.contains(&p.name);
                if let Some(unit) = &p.unit {
                    let mut args = vec![
                        "show",
                        "--property=LoadState",
                        "--property=ActiveState",
                        "--property=SubState",
                        "--property=WantedBy",
                        "--property=RequiredBy",
                        "--property=UpheldBy",
                        unit.as_str(),
                    ];
                    if p.bus == "session" {
                        args.push("--user");
                    }
                    match crate::process::capture("systemctl", &args) {
                        Ok(state) if state.lines().any(|l| l == "LoadState=not-found") => {}
                        Ok(state) => {
                            let (status, detail) = stopped_unit_status(&state, activatable);
                            return Ok(finish(status, detail));
                        }
                        Err(_) if activatable => {}
                        Err(_) => {
                            return Ok(finish(
                                Status::Warn,
                                "Bus name absent and installed-unit status unreadable",
                            ))
                        }
                    }
                }
                if activatable {
                    return Ok(finish(
                        Status::Pass,
                        "Installed and D-Bus activatable; stopped until needed, activation intentionally avoided",
                    ));
                }
                return Ok(if p.required {
                    finish(Status::Fail, "Required service is not installed or owned")
                } else {
                    finish(
                        Status::Skip,
                        "Feature is not installed or advertised on this bus",
                    )
                });
            }
        };
        let xml: String = proxy(c, &owner, &p.path, "org.freedesktop.DBus.Introspectable")?
            .call("Introspect", &())?;
        if xml.len() > 262144 {
            return Ok(finish(Status::Fail, "Introspection exceeds size limit"));
        }
        let doc = match roxmltree::Document::parse_with_options(
            &xml,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                nodes_limit: 20000,
                ..Default::default()
            },
        ) {
            Ok(d) => d,
            Err(_) => return Ok(finish(Status::Fail, "Invalid introspection XML")),
        };
        let interface = doc.descendants().find(|n| {
            n.has_tag_name("interface") && n.attribute("name") == Some(p.interface.as_str())
        });
        let Some(interface) = interface else {
            return Ok(finish(Status::Fail, "Expected interface is missing"));
        };
        if p.unverified || (p.method.is_none() && p.property.is_none()) {
            return Ok(finish(
                Status::Warn,
                "Interface reachable; no audited read-only status probe is available",
            ));
        }
        let api = proxy(c, &owner, &p.path, &p.interface)?;
        let valid = if let Some(property) = &p.property {
            if !interface.children().any(|n| {
                n.has_tag_name("property")
                    && n.attribute("name") == Some(property.as_str())
                    && matches!(n.attribute("access"), Some("read" | "readwrite"))
            }) {
                return Ok(finish(
                    Status::Fail,
                    "Expected readable property is missing",
                ));
            }
            let value: OwnedValue = api.get_property(property.as_str())?;
            match p.shape.as_deref() {
                Some("string" | "json-object" | "json-array") => {
                    <&str>::try_from(&value).is_ok_and(|s| validate_string(p, s))
                }
                Some("bool") => bool::try_from(&value).is_ok(),
                Some("u32") => u32::try_from(&value).is_ok(),
                _ => false,
            }
        } else {
            let method = p.method.as_ref().unwrap();
            let method_node = interface
                .children()
                .find(|n| n.has_tag_name("method") && n.attribute("name") == Some(method.as_str()));
            let Some(method_node) = method_node else {
                return Ok(finish(Status::Fail, "Expected read-only method is missing"));
            };
            if method_node
                .children()
                .any(|n| n.has_tag_name("arg") && n.attribute("direction") != Some("out"))
            {
                return Ok(finish(
                    Status::Fail,
                    "Probe method unexpectedly requires arguments",
                ));
            }
            let reply = api.call_method(method.as_str(), &())?;
            let body = reply.body();
            match p.shape.as_deref() {
                Some("string" | "json-object" | "json-array") => body
                    .deserialize::<String>()
                    .is_ok_and(|s| validate_string(p, &s)),
                Some("bool") => body.deserialize::<bool>().is_ok(),
                Some("u32") => body.deserialize::<u32>().is_ok(),
                Some("server-information") => body
                    .deserialize::<(String, String, String, String)>()
                    .is_ok(),
                _ => false,
            }
        };
        Ok(if valid {
            finish(
                Status::Pass,
                "Owned, introspected; read-only reply shape validated",
            )
        } else {
            finish(
                Status::Fail,
                "Read-only reply has an unexpected shape or value",
            )
        })
    })();
    result.unwrap_or_else(|e| {
        let denied = matches!(&e, zbus::Error::MethodError(name, _, _) if name.as_str().ends_with("AccessDenied"));
        finish(if denied { Status::Warn } else { Status::Fail }, if denied { "Permission denied; probe could not complete" } else { "D-Bus read-only probe failed; raw error suppressed" })
    })
}
pub fn validate_string(p: &Probe, s: &str) -> bool {
    if s.len() > 1024 * 1024 {
        return false;
    }
    if !p.values.is_empty() && !p.values.iter().any(|v| v == s) {
        return false;
    }
    match p.shape.as_deref() {
        Some("string") => !s.is_empty() && s.len() <= 4096,
        Some("json-array") => {
            serde_json::from_str::<serde_json::Value>(s).is_ok_and(|v| v.is_array())
        }
        Some("json-object") => serde_json::from_str::<serde_json::Value>(s).is_ok_and(|v| {
            v.is_object()
                && p.fields.iter().all(|(key, ty)| {
                    v.get(key).is_some_and(|x| match ty.as_str() {
                        "string" => x.is_string(),
                        "bool" => x.is_boolean(),
                        "number" => x.is_number(),
                        "array" => x.is_array(),
                        "object" => x.is_object(),
                        _ => false,
                    })
                })
        }),
        _ => false,
    }
}
