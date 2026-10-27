//! fp6-release: the release side (laptop or the Mac mini builder).
//!
//!   fp6-release keygen --dir KEYS [--nix-key-name fp6-ota-1]
//!       manifest.key (0600, NEVER commit) + manifest.pub (minisign format),
//!       nix-cache.secret (0600) + nix-cache.pub (nix key generate-secret).
//!   fp6-release publish --root ROOT --channel CH --toplevel P --boot-image P --keys KEYS
//!       [--changelog TEXT | --changelog-file F] [--kernel-notes TEXT] [--min-release N]
//!       [--store SRC] [--store-dir DIR] [--cache-to URL] [--cache-ref URL] [--expires-days N]
//!       Copies the closure into ROOT/cache (file:// binary cache, every path
//!       signed with nix-cache.secret; or --cache-to s3://...), checks every
//!       path is signed by that key, writes ROOT/channels/CH/closure-rN.json,
//!       manifest.json and manifest.json.minisig (and a copy under releases/N).
//!   fp6-release verify --root ROOT --channel CH --pubkey KEYS/manifest.pub
//!   fp6-release serve --root ROOT --listen 0.0.0.0:8742
//!       Static GET/HEAD server for ROOT (the phone fetches over NetBird).

use fp6_ota::bootimg;
use fp6_ota::manifest::{self, BootImage, Closure, ClosureEntry, ClosureRef, Kernel, Manifest, ReleaseInfo};
use fp6_ota::minisig::{PublicKey, SecretKey};
use fp6_ota::util::{self, is_store_path, now, sha256_bytes, write_atomic};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn die(m: impl AsRef<str>) -> ! {
    eprintln!("fp6-release: {}", m.as_ref());
    std::process::exit(1);
}

struct Args(HashMap<String, String>);
impl Args {
    fn parse(v: &[String]) -> Args {
        let mut m = HashMap::new();
        let mut i = 0;
        while i < v.len() {
            let k = v[i].strip_prefix("--").unwrap_or_else(|| die(format!("unexpected argument {}", v[i]))).to_string();
            let val = v.get(i + 1).cloned().unwrap_or_else(|| die(format!("--{k} needs a value")));
            m.insert(k, val);
            i += 2;
        }
        Args(m)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }
    fn need(&self, k: &str) -> &str {
        self.get(k).unwrap_or_else(|| die(format!("--{k} is required")))
    }
}

fn check_secret_mode(p: &Path) {
    let md = fs::metadata(p).unwrap_or_else(|e| die(format!("{}: {e}", p.display())));
    if md.permissions().mode() & 0o077 != 0 {
        die(format!("{} is readable by others (chmod 600 it)", p.display()));
    }
}

fn keygen(a: &Args) {
    let dir = PathBuf::from(a.need("dir"));
    fs::create_dir_all(&dir).unwrap_or_else(|e| die(e.to_string()));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).ok();
    for f in ["manifest.key", "manifest.pub", "nix-cache.secret", "nix-cache.pub"] {
        if dir.join(f).exists() {
            die(format!("{} exists: not overwriting keys", dir.join(f).display()));
        }
    }
    let k = SecretKey::generate().unwrap_or_else(|e| die(e));
    write_atomic(&dir.join("manifest.key"), k.to_file().as_bytes(), 0o600).unwrap_or_else(|e| die(e));
    write_atomic(&dir.join("manifest.pub"), k.public().to_file().as_bytes(), 0o644).unwrap_or_else(|e| die(e));
    let name = a.get("nix-key-name").unwrap_or("fp6-ota-1");
    let sec = util::run("nix", &["--extra-experimental-features".into(), "nix-command".into(), "key".into(), "generate-secret".into(), "--key-name".into(), name.into()])
        .unwrap_or_else(|e| die(e));
    write_atomic(&dir.join("nix-cache.secret"), sec.trim().as_bytes(), 0o600).unwrap_or_else(|e| die(e));
    let mut c = std::process::Command::new("nix").args(["--extra-experimental-features", "nix-command", "key", "convert-secret-to-public"])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap_or_else(|e| die(e.to_string()));
    c.stdin.take().unwrap().write_all(sec.trim().as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    write_atomic(&dir.join("nix-cache.pub"), &out.stdout, 0o644).unwrap_or_else(|e| die(e));
    println!("manifest key {} -> {}", fp6_ota::minisig::key_id_hex(&k.key_id), dir.join("manifest.pub").display());
    println!("  manifestPublicKey = \"{}\";", k.public().to_base64());
    println!("  nixPublicKey = \"{}\";", String::from_utf8_lossy(&out.stdout).trim());
    println!("The secret files (manifest.key, nix-cache.secret) never leave this machine and are never committed.");
}

/// Resolves a logical /nix/store path inside `store_dir`, following
/// symlinks into the store (works for chroot stores like /tmp/x/nix/store).
fn store_resolve(store_dir: &Path, logical: &str) -> Result<PathBuf, String> {
    let rest = logical.strip_prefix("/nix/store/").ok_or_else(|| format!("{logical}: not in the store"))?;
    let mut comps: Vec<String> = rest.split('/').filter(|c| !c.is_empty() && *c != ".").map(String::from).collect();
    let mut i = 0;
    let mut hops = 0;
    while i < comps.len() {
        let cur = store_dir.join(comps[..=i].join("/"));
        if let Ok(t) = fs::read_link(&cur) {
            hops += 1;
            if hops > 40 {
                return Err(format!("{logical}: too many symlinks"));
            }
            let t = t.to_string_lossy().into_owned();
            let mut base: Vec<String> = if let Some(r) = t.strip_prefix("/nix/store/") {
                r.split('/').map(String::from).collect()
            } else if t.starts_with('/') {
                return Err(format!("{logical}: symlink out of the store ({t})"));
            } else {
                let mut b = comps[..i].to_vec();
                b.extend(t.split('/').map(String::from));
                b
            };
            base.extend(comps[i + 1..].iter().cloned());
            let mut norm: Vec<String> = Vec::new();
            for c in base {
                match c.as_str() {
                    "" | "." => {}
                    ".." => {
                        norm.pop();
                    }
                    _ => norm.push(c),
                }
            }
            comps = norm;
            i = 0;
            continue;
        }
        i += 1;
    }
    Ok(store_dir.join(comps.join("/")))
}

fn nix(args: &[&str]) -> Result<String, String> {
    let mut a: Vec<String> = vec!["--extra-experimental-features".into(), "nix-command".into()];
    a.extend(args.iter().map(|s| s.to_string()));
    util::log(format!("fp6-release: nix {}", args.join(" ")));
    util::run("nix", &a)
}

/// `nix path-info --json` (both the old array and the new object form).
fn path_info(store: Option<&str>, paths: &[&str]) -> Result<Vec<(String, u64)>, String> {
    let mut a = vec!["path-info", "--json", "-r"];
    if let Some(s) = store {
        a.extend(["--store", s]);
    }
    a.extend(paths);
    let v: Value = serde_json::from_str(&nix(&a)?).map_err(|e| format!("path-info: {e}"))?;
    let mut out = vec![];
    match v {
        Value::Array(xs) => {
            for x in xs {
                out.push((x["path"].as_str().unwrap_or_default().to_string(), x["narSize"].as_u64().unwrap_or(0)));
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                out.push((k, x["narSize"].as_u64().unwrap_or(0)));
            }
        }
        _ => return Err("path-info: unexpected output".into()),
    }
    out.sort();
    Ok(out)
}

fn publish(a: &Args) {
    let root = PathBuf::from(a.need("root"));
    let channel = a.need("channel").to_string();
    let toplevel = a.need("toplevel").to_string();
    let bootimg_path = a.need("boot-image").to_string();
    let keys = PathBuf::from(a.need("keys"));
    let store = a.get("store");
    let store_dir = a.get("store-dir").map(PathBuf::from).unwrap_or_else(|| match store {
        Some(s) if s.starts_with('/') => Path::new(s).join("nix/store"),
        _ => PathBuf::from("/nix/store"),
    });
    for p in [&toplevel, &bootimg_path] {
        if !is_store_path(p) {
            die(format!("{p} is not a store path"));
        }
    }
    check_secret_mode(&keys.join("manifest.key"));
    check_secret_mode(&keys.join("nix-cache.secret"));
    let mkey = SecretKey::from_file(&fs::read_to_string(keys.join("manifest.key")).unwrap_or_else(|e| die(e.to_string()))).unwrap_or_else(|e| die(e));
    let nix_pub = fs::read_to_string(keys.join("nix-cache.pub")).unwrap_or_else(|e| die(e.to_string())).trim().to_string();

    // The system's own release file decides version/release/channel.
    let rel_file = store_resolve(&store_dir, &format!("{toplevel}/etc/fp6-release")).unwrap_or_else(|e| die(e));
    let info: ReleaseInfo = serde_json::from_slice(&fs::read(&rel_file).unwrap_or_else(|e| die(format!("{}: {e} (is the ota module enabled in this system?)", rel_file.display()))))
        .unwrap_or_else(|e| die(format!("fp6-release: {e}")));
    if info.channel != channel {
        die(format!("the system is built for channel {:?}, not {channel:?}", info.channel));
    }
    let chan_dir = root.join("channels").join(&channel);
    if let Ok(b) = fs::read(chan_dir.join("manifest.json")) {
        let prev: Manifest = serde_json::from_slice(&b).unwrap_or_else(|e| die(format!("previous manifest: {e}")));
        if info.release <= prev.release {
            die(format!("release {} is not newer than the published release {} (bump hardware.fairphone6.ota.release)", info.release, prev.release));
        }
    }

    // Boot image: consistent with the toplevel, within the partition limit.
    let img_file = store_resolve(&store_dir, &bootimg_path).unwrap_or_else(|e| die(e));
    let img = fs::read(&img_file).unwrap_or_else(|e| die(format!("{}: {e}", img_file.display())));
    let h = bootimg::parse_header(&img[..img.len().min(4096)]).unwrap_or_else(|e| die(format!("boot image: {e}")));
    if bootimg::generation_from_cmdline(&h.cmdline).as_deref() != Some(toplevel.as_str()) {
        die(format!("the boot image boots {:?}, not {toplevel}", bootimg::generation_from_cmdline(&h.cmdline)));
    }
    if img.len() as u64 >= manifest::MAX_BOOT_IMAGE {
        die("boot image is 96 MiB or larger");
    }

    // Binary cache: copy + sign, then check every path carries our signature.
    let secret = fs::canonicalize(keys.join("nix-cache.secret")).unwrap();
    let cache_dir = root.join("cache");
    let to = match a.get("cache-to") {
        Some(u) => format!("{u}{}secret-key={}", if u.contains('?') { "&" } else { "?" }, secret.display()),
        None => {
            fs::create_dir_all(&cache_dir).unwrap_or_else(|e| die(e.to_string()));
            format!("file://{}?secret-key={}&compression=zstd", fs::canonicalize(&cache_dir).unwrap().display(), secret.display())
        }
    };
    let mut copy = vec!["copy", "--to", &to];
    if let Some(s) = store {
        copy.extend(["--from", s]);
    }
    copy.extend([toplevel.as_str(), bootimg_path.as_str()]);
    nix(&copy).unwrap_or_else(|e| die(e));
    let cache_url = a.get("cache-to").map(str::to_string)
        .unwrap_or_else(|| format!("file://{}", fs::canonicalize(&cache_dir).unwrap().display()));
    nix(&["store", "verify", "--store", &cache_url, "--no-contents", "-r", "--option", "trusted-public-keys", &nix_pub, &toplevel, &bootimg_path])
        .unwrap_or_else(|e| die(format!("not every path in the cache carries the release signature: {e}")));

    let infos = path_info(store, &[&toplevel, &bootimg_path]).unwrap_or_else(|e| die(e));
    let paths: Vec<ClosureEntry> = infos.into_iter().map(|(p, nar)| {
        let hash = p.trim_start_matches("/nix/store/").split('-').next().unwrap_or("").to_string();
        let file_size = fs::read_to_string(cache_dir.join(format!("{hash}.narinfo"))).ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("FileSize: ").and_then(|v| v.trim().parse().ok())))
            .unwrap_or(0);
        ClosureEntry { path: p, nar_size: nar, file_size }
    }).collect();
    let closure = Closure { paths };
    let cbytes = serde_json::to_vec(&closure).unwrap();
    let changelog = match (a.get("changelog"), a.get("changelog-file")) {
        (_, Some(f)) => fs::read_to_string(f).unwrap_or_else(|e| die(format!("{f}: {e}"))),
        (Some(t), None) => t.to_string(),
        _ => String::new(),
    };
    let t = now();
    let cfile = format!("closure-r{}.json", info.release);
    let m = Manifest {
        format: manifest::FORMAT,
        device: info.device.clone(),
        channel: channel.clone(),
        release: info.release,
        version: info.version.clone(),
        label: info.label.clone(),
        date: util::rfc3339(t),
        timestamp: t,
        toplevel: toplevel.clone(),
        boot_image: BootImage { path: bootimg_path.clone(), sha256: sha256_bytes(&img), size: img.len() as u64 },
        kernel: Kernel { release: info.kernel.clone(), notes: a.get("kernel-notes").unwrap_or("").to_string() },
        min_release: a.get("min-release").map(|v| v.parse().unwrap_or_else(|_| die("--min-release"))).unwrap_or(0),
        changelog: changelog.trim().to_string(),
        closure: ClosureRef {
            file: cfile.clone(),
            sha256: sha256_bytes(&cbytes),
            paths: closure.paths.len() as u64,
            nar_size: closure.paths.iter().map(|p| p.nar_size).sum(),
            file_size: closure.paths.iter().map(|p| p.file_size).sum(),
        },
        cache: Some(a.get("cache-ref").map(str::to_string).unwrap_or_else(|| "../../cache".into())),
        expires: a.get("expires-days").map(|d| t + d.parse::<i64>().unwrap_or_else(|_| die("--expires-days")) * 86400),
    };
    m.validate().unwrap_or_else(|e| die(format!("manifest: {e}")));
    let mbytes = serde_json::to_vec_pretty(&m).unwrap();
    let sig = mkey.sign(&mbytes, &m.trusted_comment(&mbytes)).unwrap_or_else(|e| die(e));
    // Self-check with the public key before publishing.
    manifest::verify(&mbytes, &sig, &mkey.public(), &m.device, &channel, t).unwrap_or_else(|e| die(format!("self-check: {e}")));
    let rel_dir = chan_dir.join("releases").join(m.release.to_string());
    for d in [&rel_dir, &chan_dir] {
        write_atomic(&d.join(&cfile), &cbytes, 0o644).unwrap_or_else(|e| die(e));
    }
    write_atomic(&rel_dir.join("manifest.json"), &mbytes, 0o644).unwrap_or_else(|e| die(e));
    write_atomic(&rel_dir.join("manifest.json.minisig"), sig.as_bytes(), 0o644).unwrap_or_else(|e| die(e));
    write_atomic(&chan_dir.join("manifest.json.minisig"), sig.as_bytes(), 0o644).unwrap_or_else(|e| die(e));
    write_atomic(&chan_dir.join("manifest.json"), &mbytes, 0o644).unwrap_or_else(|e| die(e));
    println!("published release {} ({}) on channel {channel}", m.release, m.version);
    println!("  toplevel   {}", m.toplevel);
    println!("  boot image {} ({} B, sha256 {})", m.boot_image.path, m.boot_image.size, m.boot_image.sha256);
    println!("  closure    {} paths, {} B unpacked, {} B compressed", m.closure.paths, m.closure.nar_size, m.closure.file_size);
    println!("  manifest   {}", chan_dir.join("manifest.json").display());
}

fn verify(a: &Args) {
    let chan = PathBuf::from(a.need("root")).join("channels").join(a.need("channel"));
    let key = PublicKey::parse(&fs::read_to_string(a.need("pubkey")).unwrap_or_else(|e| die(e.to_string()))).unwrap_or_else(|e| die(e));
    let b = fs::read(chan.join("manifest.json")).unwrap_or_else(|e| die(e.to_string()));
    let s = fs::read_to_string(chan.join("manifest.json.minisig")).unwrap_or_else(|e| die(e.to_string()));
    let pre: Manifest = serde_json::from_slice(&b).unwrap_or_else(|e| die(e.to_string()));
    let m = manifest::verify(&b, &s, &key, &pre.device, a.need("channel"), now()).unwrap_or_else(|e| die(format!("NOT VALID: {e}")));
    manifest::verify_closure(&fs::read(chan.join(&m.closure.file)).unwrap_or_else(|e| die(e.to_string())), &m).unwrap_or_else(|e| die(e));
    println!("valid: release {} ({}), {} paths", m.release, m.version, m.closure.paths);
}

fn serve(a: &Args) {
    let root = fs::canonicalize(a.need("root")).unwrap_or_else(|e| die(e.to_string()));
    let listen = a.get("listen").unwrap_or("127.0.0.1:8742");
    let l = std::net::TcpListener::bind(listen).unwrap_or_else(|e| die(format!("{listen}: {e}")));
    eprintln!("fp6-release: serving {} on http://{listen}/", root.display());
    for s in l.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || {
            let _ = handle(s, &root);
        });
    }
}

fn handle(mut s: std::net::TcpStream, root: &Path) -> std::io::Result<()> {
    s.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    let (method, target) = (parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or("/"));
    let path = target.split('?').next().unwrap_or("/");
    let ok_path = path.starts_with('/') && !path.split('/').any(|c| c == ".." || c.starts_with('.') && !c.is_empty())
        && path.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.+".contains(c));
    let file = root.join(path.trim_start_matches('/'));
    let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let reply = |s: &mut std::net::TcpStream, code: &str| s.write_all(format!("HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
    if method != "GET" && method != "HEAD" {
        return reply(&mut s, "405 Method Not Allowed");
    }
    let md = match fs::metadata(&file) {
        Ok(m) if ok_path && m.is_file() => m,
        _ => {
            eprintln!("{peer} {method} {path} 404");
            return reply(&mut s, "404 Not Found");
        }
    };
    let ct = if path.ends_with(".narinfo") { "text/x-nix-narinfo" } else if path.ends_with(".json") { "application/json" } else { "application/octet-stream" };
    s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", md.len()).as_bytes())?;
    if method == "GET" {
        let mut f = fs::File::open(&file)?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            s.write_all(&buf[..n])?;
        }
    }
    eprintln!("{peer} {method} {path} 200 {}", md.len());
    Ok(())
}

fn main() {
    let v: Vec<String> = std::env::args().skip(1).collect();
    let a = Args::parse(v.get(1..).unwrap_or(&[]));
    match v.first().map(String::as_str) {
        Some("keygen") => keygen(&a),
        Some("publish") => publish(&a),
        Some("verify") => verify(&a),
        Some("serve") => serve(&a),
        _ => die("usage: fp6-release keygen|publish|verify|serve (see the source header)"),
    }
}
