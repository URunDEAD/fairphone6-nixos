// SPDX-License-Identifier: GPL-3.0-or-later
//! fp6-permctl: command line for the permission broker.
//!
//!   fp6-permctl request PERM           ask for a permission (may prompt)
//!   fp6-permctl check PERM             current answer without prompting
//!   fp6-permctl open-device NODE [-- CMD ARGS...]
//!        open a USB device through the broker; with CMD, run it with the
//!        descriptor as fd 3 (FP6_DEVICE_FD=3), e.g. a libusb tool using
//!        libusb_wrap_sys_device
//!   fp6-permctl state                  JSON state (from the session agent)
//!   fp6-permctl set APP PERM ask|allow|while_using|deny
//!   fp6-permctl net APP wifi|mobile|vpn|local|all on|off
//!   fp6-permctl reset APP
//! PERM: usb nfc bluetooth contacts calendar messages files sensors

use fp6_permissions::{BUS_NAME, PATH, SESSION_NAME, SESSION_PATH};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use zbus::zvariant::{OwnedValue, Value};

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("fp6-permctl: {msg}");
    std::process::exit(1)
}

fn system() -> zbus::blocking::Connection {
    zbus::blocking::Connection::system().unwrap_or_else(|e| die(e))
}

fn call_str(method: &str, args: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> String {
    let c = system();
    let m = c
        .call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), method, args)
        .unwrap_or_else(|e| die(e));
    m.body().deserialize::<String>().unwrap_or_else(|e| die(e))
}

fn activate(action: &str, param: String) {
    let c = zbus::blocking::Connection::session().unwrap_or_else(|e| die(e));
    let params: Vec<Value> = vec![Value::from(param)];
    let platform: HashMap<&str, Value> = HashMap::new();
    c.call_method(Some(SESSION_NAME), SESSION_PATH, Some("org.gtk.Actions"), "Activate", &(action, params, platform))
        .unwrap_or_else(|e| die(e));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["request", p] => println!("{}", call_str("Request", &(p,))),
        ["check", p] => println!("{}", call_str("Check", &(p,))),
        ["open-device", node, rest @ ..] => {
            let c = system();
            let m = c
                .call_method(Some(BUS_NAME), PATH, Some(BUS_NAME), "OpenDevice", &(node, "rw"))
                .unwrap_or_else(|e| die(e));
            let fd: zbus::zvariant::OwnedFd = m.body().deserialize().unwrap_or_else(|e| die(e));
            let fd: std::os::fd::OwnedFd = fd.into();
            match rest {
                [] => println!("opened {node} (fd {})", fd.as_raw_fd()),
                ["--", cmd, cargs @ ..] => {
                    let raw = fd.as_raw_fd();
                    let mut command = std::process::Command::new(cmd);
                    command.args(cargs).env("FP6_DEVICE_FD", "3");
                    // SAFETY: only async-signal-safe dup2 between fork and exec.
                    unsafe {
                        command.pre_exec(move || {
                            if libc::dup2(raw, 3) < 0 {
                                return Err(std::io::Error::last_os_error());
                            }
                            Ok(())
                        });
                    }
                    die(command.exec());
                }
                _ => die("usage: open-device NODE [-- CMD ARGS...]"),
            }
        }
        ["state"] => {
            let c = zbus::blocking::Connection::session().unwrap_or_else(|e| die(e));
            let m = c
                .call_method(Some(SESSION_NAME), SESSION_PATH, Some("org.gtk.Actions"), "Describe", &("state",))
                .unwrap_or_else(|e| die(e));
            let (_enabled, _sig, st): (bool, zbus::zvariant::Signature, Vec<OwnedValue>) =
                m.body().deserialize().unwrap_or_else(|e| die(e));
            let s: String = st.first().and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default();
            println!("{s}");
        }
        ["set", app, p, d] => activate("set", format!("{app}\t{p}\t{d}")),
        ["net", app, class, on] => activate("net", format!("{app}\t{class}\t{}", if *on == "on" { 1 } else { 0 })),
        ["reset", app] => activate("reset", app.to_string()),
        _ => {
            eprintln!("usage: fp6-permctl request|check PERM | open-device NODE [-- CMD...] | state | set APP PERM DECISION | net APP CLASS on|off | reset APP");
            std::process::exit(2);
        }
    }
}
