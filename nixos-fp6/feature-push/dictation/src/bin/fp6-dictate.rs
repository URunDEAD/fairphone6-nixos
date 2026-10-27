#![forbid(unsafe_code)]
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use fp6_dictation::{data_dir, model, models, Settings, BUS, PATH};
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};
#[derive(Parser)]
#[command(
    version,
    about = "Offline dictation; downloads happen only on explicit request"
)]
struct Args {
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=29))]
    seconds: u64,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    Models,
    Download {
        model: String,
    },
    /// Transcribe a synthetic/test WAV directly, without opening the microphone.
    Transcribe {
        file: std::path::PathBuf,
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long, default_value = "base-q5_1")]
        model: String,
    },
}
fn download(id: &str) -> Result<()> {
    let m = model(id)?;
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(&m.filename);
    let part = dir.join(format!("{}.{}.part", m.filename, std::process::id()));
    let result = (|| -> Result<()> {
        let status = std::process::Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--retry",
                "2",
                "--max-time",
                "900",
                "--max-filesize",
                &m.bytes.to_string(),
                "--output",
            ])
            .arg(&part)
            .arg(format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
                m.filename
            ))
            .status()?;
        if !status.success() {
            bail!("Download failed");
        }
        let mut file = std::fs::File::open(&part)?;
        let mut hash = Sha256::new();
        let mut chunk = [0; 65536];
        let mut count = 0;
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            hash.update(&chunk[..n]);
            count += n as u64;
        }
        if count != m.bytes || format!("{:x}", hash.finalize()) != m.sha256 {
            bail!("Model checksum or size mismatch");
        }
        std::fs::rename(&part, &target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result?;
    println!("Installed {}", m.id);
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Some(Action::Models) => {
            for m in models() {
                println!(
                    "{}\t{} MiB\t{}",
                    m.id,
                    m.bytes / 1024 / 1024,
                    if data_dir().join(m.filename).exists() {
                        "installed"
                    } else {
                        "not installed"
                    }
                );
            }
        }
        Some(Action::Download { model }) => download(&model)?,
        Some(Action::Transcribe {
            file,
            language,
            model,
        }) => {
            let settings = Settings {
                language,
                model,
                ..Default::default()
            };
            settings.validate()?;
            let mut wav = hound::WavReader::open(file)?;
            let spec = wav.spec();
            if spec.channels != 1
                || spec.sample_rate != 16000
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
            {
                bail!("Expected mono 16 kHz signed 16-bit WAV");
            }
            if wav.duration() > 480000 {
                bail!("WAV exceeds 30 seconds");
            }
            let samples = wav
                .samples::<i16>()
                .map(|v| v.map(|x| x as f32 / 32768.0))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            println!(
                "{}",
                fp6_dictation::transcribe(
                    &samples,
                    &settings.model_path()?,
                    &settings.language,
                    true
                )?
            );
        }
        None => {
            let conn = zbus::connection::Builder::session()?
                .method_timeout(Duration::from_secs(100))
                .build()
                .await
                .context("No user session bus")?;
            let proxy = zbus::Proxy::new(&conn, BUS, PATH, BUS).await?;
            let id: u64 = proxy.call("Start", &()).await?;
            let operation = async {
                tokio::time::sleep(Duration::from_secs(args.seconds)).await;
                let text: String = proxy.call("Stop", &(id,)).await?;
                Ok::<_, anyhow::Error>(text)
            };
            tokio::select! {
                result=operation => println!("{}",result?),
                _=tokio::signal::ctrl_c() => { let _:()=proxy.call("Cancel",&(id,)).await?; }
            }
        }
    }
    Ok(())
}
