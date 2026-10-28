//! fp6-lock-sounds: prepares the FP6 lock/unlock sounds for Lomiri.
//!
//!   fp6-lock-sounds convert IN OUT [--peak-db D] [--max-ms N]
//!       decode IN (Ogg Vorbis, MP3, FLAC, WAV, AAC/M4A), trim, normalise,
//!       write a 16-bit PCM WAV (used at build time for the bundled sounds)
//!   fp6-lock-sounds sync [--cache DIR]
//!       prepare the custom sounds chosen in Settings > Sound (user service)
//!   fp6-lock-sounds info FILE
//!       print rate, channels, duration and peak

mod audio;
mod gvariant;
mod sync;
mod wav;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: fp6-lock-sounds convert IN OUT [--peak-db D] [--max-ms N]\n       fp6-lock-sounds sync [--cache DIR]\n       fp6-lock-sounds info FILE");
    ExitCode::from(2)
}

fn options(args: &[String]) -> Result<(audio::Options, Vec<String>), String> {
    let mut opt = audio::Options::default();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--peak-db" => opt.peak_db = it.next().and_then(|v| v.parse().ok()).ok_or("--peak-db needs a number")?,
            "--max-ms" => opt.max_ms = it.next().and_then(|v| v.parse().ok()).ok_or("--max-ms needs a number")?,
            "--cache" => rest.extend(["--cache".to_string(), it.next().ok_or("--cache needs a directory")?.clone()]),
            _ => rest.push(a.clone()),
        }
    }
    if !(-60.0..=0.0).contains(&opt.peak_db) || !(50..=10_000).contains(&opt.max_ms) {
        return Err("--peak-db must be in -60..0, --max-ms in 50..10000".into());
    }
    Ok((opt, rest))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some(cmd) = args.first() else { return Ok(usage()) };
    let (opt, rest) = options(&args[1..])?;
    match (cmd.as_str(), rest.as_slice()) {
        ("convert", [input, output]) => {
            let clip = audio::process(audio::decode(Path::new(input), opt.max_ms)?, opt)
                .map_err(|e| format!("{input}: {e}"))?;
            wav::write(Path::new(output), &clip)?;
            println!("{output}: {} Hz, {} ch, {} ms, peak {:.1} dBFS", clip.rate, clip.channels,
                     clip.duration_ms(), 20.0 * clip.peak().log10());
            Ok(ExitCode::SUCCESS)
        }
        ("info", [file]) => {
            let clip = audio::decode(Path::new(file), 60_000)?;
            println!("{file}: {} Hz, {} ch, {} ms, peak {:.1} dBFS", clip.rate, clip.channels,
                     clip.duration_ms(), 20.0 * clip.peak().max(1e-9).log10());
            Ok(ExitCode::SUCCESS)
        }
        ("sync", r) => {
            let cache = match r {
                [] => sync::default_cache_dir().ok_or("neither XDG_CACHE_HOME nor HOME is set")?,
                [flag, dir] if flag == "--cache" => PathBuf::from(dir),
                _ => return Ok(usage()),
            };
            let bin = std::env::var_os("FP6_GSETTINGS").map(PathBuf::from).unwrap_or_else(|| "gsettings".into());
            let report = sync::sync(&sync::GsettingsCli { bin }, &cache, opt)?;
            for c in &report.converted {
                println!("prepared {c}");
            }
            for f in &report.failed {
                eprintln!("could not prepare {f} (Lomiri plays the original file instead)");
            }
            if report.removed > 0 {
                println!("removed {} unused cache file(s)", report.removed);
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => Ok(usage()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("fp6-lock-sounds: {e}");
            ExitCode::FAILURE
        }
    }
}
