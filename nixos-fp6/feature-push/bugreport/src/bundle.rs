use crate::{
    collect::{self, Entry},
    config::Options,
    redact::redact,
    runner::{RealRunner, Runner},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn private_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
fn private_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(path).map_err(|_| "Cannot inspect output directory")?;
            // Compare owner to a newly created private directory instead of invoking libc.
            let own = tempfile::tempdir().map_err(|_| "Cannot check output ownership")?;
            let uid = own
                .path()
                .metadata()
                .map_err(|_| "Cannot check output ownership")?
                .uid();
            if !meta.is_dir()
                || meta.file_type().is_symlink()
                || meta.uid() != uid
                || meta.permissions().mode() & 0o077 != 0
            {
                Err(
                    "Bug reports directory must be owned by you, mode 0700, and not a symlink"
                        .into(),
                )
            } else {
                Ok(())
            }
        }
        Err(_) => Err("Cannot create private output directory".into()),
    }
}
pub fn default_output_root() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .ok_or("HOME is not set")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME must be absolute".into());
    }
    let documents = home.join("Documents");
    if !documents.exists() {
        fs::create_dir(&documents).map_err(|_| "Cannot create Documents")?;
    }
    Ok(documents.join("Bug reports"))
}
fn append<W: Write>(archive: &mut tar::Builder<W>, name: &str, bytes: &[u8]) -> Result<(), String> {
    if !collect::safe_entry_name(name) {
        return Err("Unsafe archive member name".into());
    }
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .append_data(&mut header, name, bytes)
        .map_err(|_| "Cannot append archive data".into())
}
/// Text is redacted again at the last persistence boundary. No raw staging files.
pub fn write_bundle(
    root: &Path,
    options: &Options,
    entries: &[Entry],
    screenshot: Option<&[u8]>,
) -> Result<PathBuf, String> {
    options.validate()?;
    if screenshot.is_some() && !options.screenshot {
        return Err("Screenshot consent missing".into());
    }
    private_dir(root)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock before epoch")?
        .as_secs();
    let staging = tempfile::Builder::new()
        .prefix(&format!(".pending-{timestamp}-"))
        .tempdir_in(root)
        .map_err(|_| "Cannot create private staging directory")?;
    fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700))
        .map_err(|_| "Cannot secure staging directory")?;
    let summary = format!("FP6 diagnostics v1\nTime window: {}\nSections: {}\nRestricted data requested: {}\nScreenshot: {}\n\nAll text is conservatively redacted. Review before sharing. Images are NOT redacted.\nMissing data is not proof of a healthy system. No core files, uploads, or telemetry.\n\n{}",
        options.since, options.sections.join(", "), options.privileged,
        if screenshot.is_some() { "INCLUDED — UNREDACTED" } else { "not included" },
        entries.iter().map(|e| format!("{}: {}\n", e.name, e.status)).collect::<String>());
    let summary = redact(&summary);
    let archive_path = staging.path().join("report.tar.zst");
    let file = private_file(&archive_path).map_err(|_| "Cannot create archive")?;
    let encoder = zstd::Encoder::new(file, 3).map_err(|_| "Cannot initialize compression")?;
    let mut archive = tar::Builder::new(encoder);
    append(&mut archive, "summary.txt", summary.as_bytes())?;
    for entry in entries {
        append(&mut archive, &entry.name, redact(&entry.text).as_bytes())?;
    }
    if let Some(bytes) = screenshot {
        append(&mut archive, "capture/screenshot.png", bytes)?;
    }
    let encoder = archive.into_inner().map_err(|_| "Cannot finish tar")?;
    let file = encoder.finish().map_err(|_| "Cannot finish compression")?;
    file.sync_all().map_err(|_| "Cannot sync archive")?;
    let mut summary_file =
        private_file(&staging.path().join("summary.txt")).map_err(|_| "Cannot create summary")?;
    summary_file
        .write_all(summary.as_bytes())
        .and_then(|_| summary_file.sync_all())
        .map_err(|_| "Cannot write summary")?;
    let suffix = staging
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace(".pending-", "fp6-");
    let destination = root.join(suffix);
    fs::rename(staging.path(), &destination).map_err(|_| "Cannot publish completed report")?;
    Ok(destination.join("report.tar.zst"))
}
/// Assumes the GNOME-compatible session screenshot API, implemented by some shell/portal builds.
/// Failure is recorded, never substituted with a stale image or an automatic desktop grab.
pub fn capture_screenshot(s: &impl Runner) -> (Entry, Option<Vec<u8>>) {
    let failed = |why: &str| {
        (
            Entry {
                name: "capture/status.txt".into(),
                status: why.into(),
                text: why.into(),
            },
            None,
        )
    };
    let dir = match tempfile::Builder::new().prefix("fp6-capture-").tempdir() {
        Ok(d) => d,
        Err(_) => return failed("unavailable: cannot create private screenshot directory"),
    };
    let path = dir.path().join("screenshot.png");
    let Some(path_str) = path.to_str() else {
        return failed("unavailable: invalid screenshot path");
    };
    let out = s.run(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell.Screenshot",
            "--object-path",
            "/org/gnome/Shell/Screenshot",
            "--method",
            "org.gnome.Shell.Screenshot.Screenshot",
            "false",
            "false",
            path_str,
        ],
    );
    if out.status != "ok" || !out.text.trim_start().starts_with("(true,") {
        return failed("unavailable: screenshot API missing, denied, or failed");
    }
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 32 * 1024 * 1024 => m,
        _ => return failed("unavailable: invalid screenshot file"),
    };
    let _ = meta;
    let mut bytes = Vec::new();
    if File::open(&path)
        .and_then(|f| f.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .is_err()
        || bytes.len() > 32 * 1024 * 1024
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return failed("unavailable: invalid screenshot image");
    }
    (
        Entry {
            name: "capture/status.txt".into(),
            status: "ok: unredacted screenshot included".into(),
            text: "Explicitly requested unredacted image. Review before sharing.\n".into(),
        },
        Some(bytes),
    )
}
pub fn create(options: &Options) -> Result<PathBuf, String> {
    options.validate()?;
    let source = RealRunner;
    // Screenshot first, before slow collectors or a polkit prompt can obscure the transient bug.
    let capture = options.screenshot.then(|| capture_screenshot(&source));
    let mut entries = collect::collect(options, &source)?;
    if options.privileged {
        entries.extend(collect::collect_privileged(options, &source));
    }
    let image = if let Some((entry, bytes)) = capture {
        entries.push(entry);
        bytes
    } else {
        None
    };
    write_bundle(&default_output_root()?, options, &entries, image.as_deref())
}
