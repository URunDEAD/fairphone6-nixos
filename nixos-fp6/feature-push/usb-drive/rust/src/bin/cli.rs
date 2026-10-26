//! fp6-usb-drive-cli: scriptable front end (tests, ssh).
//!   detect PATH | sha256 PATH | sums PATH | status | host PATH MODE [exclusive] | stop
//!   catalog | resolve ID | command NAME [JSON]
use fp6_usbdrive::{catalog, checksum, client, image, ops};
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let r: Result<String, String> = match arg(0).as_str() {
        "detect" => image::detect(std::path::Path::new(&arg(1))).map(|i| i.to_json().to_string()).map_err(|e| e.to_string()),
        "sha256" => checksum::hash_file(std::path::Path::new(&arg(1)), &checksum::Progress::default()),
        "sums" => Ok(format!("{:?}", checksum::expected_for(std::path::Path::new(&arg(1))))),
        "status" => client::status(),
        "host" => client::host(&arg(1), &arg(2), arg(3) == "exclusive"),
        "stop" => client::stop(),
        "catalog" => Ok(ops::Backend::new().catalog().to_string()),
        "resolve" => {
            let path = ops::catalog_path();
            std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| catalog::parse(&t))
                .and_then(|v| v.into_iter().find(|e| e.id == arg(1)).ok_or("unknown id".into()))
                .and_then(|e| ops::resolve(&e)).map(|(n, h, u)| format!("{n}\n{h}\n{u}"))
        }
        "command" => {
            let b = Arc::new(ops::Backend::new());
            let v: serde_json::Value = serde_json::from_str(if a.len() > 2 { &a[2] } else { "{}" }).unwrap_or_default();
            let r = b.command(&arg(1), &v);
            // Wait for a started job.
            if let Some(id) = r.get("job").and_then(|j| j.as_u64()) {
                loop {
                    let j = b.jobs().as_array().unwrap().iter().find(|j| j["id"] == id).cloned().unwrap();
                    if j["status"] != "running" {
                        println!("{j}");
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
            Ok(r.to_string())
        }
        _ => Err("usage: fp6-usb-drive-cli detect|sha256|sums|status|host|stop|catalog|resolve|command ...".into()),
    };
    match r {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
