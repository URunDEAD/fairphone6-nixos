//! Fetching manifests and Nix store operations (nix copy, path-info,
//! profile, GC roots), through the nix CLI and curl.

use crate::config::Config;
use crate::util;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Fetches a URL (file://, http(s):// through curl) with a size cap.
pub fn fetch(url: &str, max: u64) -> Result<Vec<u8>, String> {
    if let Some(p) = url.strip_prefix("file://") {
        let md = fs::metadata(p).map_err(|e| format!("{p}: {e}"))?;
        if md.len() > max {
            return Err(format!("{url}: larger than {max} bytes"));
        }
        return fs::read(p).map_err(|e| format!("{p}: {e}"));
    }
    let out = Command::new("curl")
        .args(["-fsSL", "--proto", "=https,http", "--max-time", "120", "--connect-timeout", "20", "--max-filesize", &max.to_string(), url])
        .output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(format!("could not fetch {url}: {}", e.trim()));
    }
    if out.stdout.len() as u64 > max {
        return Err(format!("{url}: larger than {max} bytes"));
    }
    Ok(out.stdout)
}

fn nix_args(_cfg: &Config) -> Vec<String> {
    vec!["--extra-experimental-features".into(), "nix-command".into()]
}

fn store_args(cfg: &Config) -> Vec<String> {
    cfg.nix_store.as_ref().map(|s| vec!["--store".into(), s.clone()]).unwrap_or_default()
}

/// Where a store path's files are visible on this machine.
pub fn local_path(cfg: &Config, store_path: &str) -> PathBuf {
    cfg.store_dir.join(store_path.trim_start_matches("/nix/store/"))
}

pub fn present(cfg: &Config, store_path: &str) -> bool {
    fs::symlink_metadata(local_path(cfg, store_path)).is_ok()
}

/// `nix copy --from <cache> paths...` into the local store (or --to the dry
/// run's store), with signatures checked against `trusted-public-keys`
/// (only the release key when one is configured). The child is returned
/// so the caller can watch progress and cancel.
pub fn copy_from(cfg: &Config, cache: &str, paths: &[String]) -> Result<Child, String> {
    let mut a = nix_args(cfg);
    a.push("copy".into());
    a.extend(["--from".into(), cache.into()]);
    if let Some(s) = &cfg.nix_store {
        a.extend(["--to".into(), s.clone()]);
    }
    if !cfg.nix_public_key.is_empty() {
        a.extend(["--option".into(), "trusted-public-keys".into(), cfg.nix_public_key.clone()]);
    }
    a.extend(["--option".into(), "narinfo-cache-negative-ttl".into(), "0".into()]);
    a.extend(paths.iter().cloned());
    util::log(format!("fp6-updated: {} {}", cfg.nix, a.join(" ")));
    Command::new(&cfg.nix).args(&a).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .env("NIX_REMOTE", if cfg.nix_store.is_some() { "" } else { "daemon" })
        .spawn().map_err(|e| format!("{}: {e}", cfg.nix))
}

/// All of `paths` are valid in the local store.
pub fn valid(cfg: &Config, paths: &[String]) -> Result<(), String> {
    let mut a = nix_args(cfg);
    a.push("path-info".into());
    a.extend(store_args(cfg));
    a.extend(paths.iter().cloned());
    util::run(&cfg.nix, &a).map(|_| ())
}

pub fn set_profile(cfg: &Config, toplevel: &str) -> Result<(), String> {
    let mut a = vec!["-p".to_string(), cfg.profiles.join(&cfg.profile).to_string_lossy().into_owned()];
    a.extend(store_args(cfg));
    a.extend(["--set".into(), toplevel.into()]);
    util::log(format!("fp6-updated: {} {}", cfg.nix_env, a.join(" ")));
    util::run(&cfg.nix_env, &a).map(|_| ())
}

pub fn profile_target(cfg: &Config) -> Option<String> {
    fs::canonicalize(cfg.profiles.join(&cfg.profile)).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Indirect GC root `<gcroots>/<name>` -> store path (replacing ours only).
pub fn gcroot(cfg: &Config, name: &str, target: &str) -> Result<(), String> {
    fs::create_dir_all(&cfg.gcroots).map_err(|e| format!("{}: {e}", cfg.gcroots.display()))?;
    let l = cfg.gcroots.join(name);
    let _ = fs::remove_file(&l);
    std::os::unix::fs::symlink(target, &l).map_err(|e| format!("{}: {e}", l.display()))
}

pub fn drop_gcroot(cfg: &Config, name: &str) {
    let _ = fs::remove_file(cfg.gcroots.join(name));
}

pub fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}
