#![forbid(unsafe_code)]
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use fp6_captions::{
    engine, models,
    service::{Options, Service},
    streaming::{Merger, Segmenter, FRAME},
    BUS, PATH,
};
use std::{path::PathBuf, time::Instant};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Daemon,
    Models,
    Download {
        model: String,
    },
    Stop,
    Status,
    /// Keep this process alive to own the session. Ctrl-C stops capture.
    Start {
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long, default_value = "tiny-q5_1")]
        model: String,
        #[arg(long, default_value = "monitor")]
        source: String,
        #[arg(long, default_value_t = 2)]
        threads: u8,
        #[arg(long, default_value_t = 3)]
        silence_minutes: u8,
        #[arg(long)]
        allow_calls: bool,
    },
    /// Synthetic WAV benchmark: never opens PipeWire. Text omitted from output.
    Benchmark {
        file: PathBuf,
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long, default_value = "tiny-q5_1")]
        model: String,
        #[arg(long, default_value_t = 2)]
        threads: u8,
        #[arg(long)]
        segmentation_only: bool,
    },
    #[command(hide = true)]
    Worker {
        path: PathBuf,
        language: String,
        threads: u8,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Action::Worker {
            path,
            language,
            threads,
        } => engine::worker(&path, &language, threads)?,
        Action::Models => println!("{}", serde_json::to_string_pretty(&models::catalog())?),
        Action::Download { model } => models::download(&model)?,
        Action::Daemon => {
            let s = Service::default();
            let _c = zbus::connection::Builder::session()?
                .name(BUS)?
                .serve_at(PATH, s.clone())?
                .build()
                .await?;
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            tokio::select! { _=tokio::signal::ctrl_c()=>{},_=term.recv()=>{} }
            s.shutdown().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Action::Stop | Action::Status | Action::Start { .. } => {
            let c = zbus::Connection::session().await?;
            let p = zbus::Proxy::new(&c, BUS, PATH, BUS).await?;
            match Args::parse().command {
                Action::Stop => {
                    let _: () = p.call("Stop", &()).await?;
                }
                Action::Status => {
                    let s: String = p.call("Snapshot", &()).await?;
                    println!("{s}");
                }
                Action::Start {
                    language,
                    model,
                    source,
                    threads,
                    silence_minutes,
                    allow_calls,
                } => {
                    let o = Options {
                        language,
                        model,
                        source,
                        threads,
                        silence_minutes,
                        allow_calls,
                    };
                    o.validate()?;
                    let _: () = p.call("Start", &(serde_json::to_string(&o)?,)).await?;
                    loop {
                        tokio::select! {
                            _=tokio::signal::ctrl_c()=> { let _:()=p.call("Stop",&()).await?;break; }
                            _=tokio::time::sleep(std::time::Duration::from_secs(1))=> { let s:String=p.call("Snapshot",&()).await?;let s:fp6_captions::service::Snapshot=serde_json::from_str(&s)?; if s.state=="idle" { eprintln!("{}",s.message);break; } }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        Action::Benchmark {
            file,
            language,
            model,
            threads,
            segmentation_only,
        } => {
            let mut wav = hound::WavReader::open(file)?;
            let spec = wav.spec();
            if spec.channels != 1
                || spec.sample_rate != 16000
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
                || wav.duration() > 16000 * 600
            {
                bail!("Expected mono PCM16 16 kHz synthetic WAV, at most 10 min");
            }
            let pcm: Vec<f32> = wav
                .samples::<i16>()
                .map(|v| v.map(|x| x as f32 / 32768.))
                .collect::<Result<_, _>>()?;
            let started = Instant::now();
            let mut decoder = if segmentation_only {
                None
            } else {
                let m = models::model(&model)?;
                let path = models::directory().join(&m.filename);
                models::verify(&path, &m)?;
                Some(engine::Decoder::new(&path, &language, threads)?)
            };
            let load_ms = started.elapsed().as_secs_f64() * 1000.;
            let mut segmenter = Segmenter::default();
            let mut merger = Merger::default();
            let mut rows = vec![];
            let mut ready_at: f64 = 0.;
            let mut chunks = vec![];
            for frame in pcm.chunks(FRAME) {
                let mut padded = [0.; FRAME];
                padded[..frame.len()].copy_from_slice(frame);
                if let Some(c) = segmenter.push(&padded) {
                    chunks.push(c);
                }
            }
            if let Some(c) = segmenter.finish() {
                chunks.push(c);
            }
            for c in chunks {
                let t = Instant::now();
                let text = if let Some(d) = &mut decoder {
                    d.decode(&c.samples)?
                } else {
                    String::new()
                };
                let decode_ms = t.elapsed().as_secs_f64() * 1000.;
                let end_ms = c.end_sample as f64 / 16.;
                ready_at = ready_at.max(end_ms) + decode_ms;
                merger.append(&text, c.overlaps);
                rows.push(serde_json::json!({"end_audio_ms":end_ms,"window_ms":c.samples.len() as f64/16.,"decode_ms":decode_ms,"simulated_ready_ms":ready_at,"word_count":text.split_whitespace().count()}));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"kind":if segmentation_only {"segmentation-only"}else{"offline-decoder"},"language":language,"model":model,"threads":threads,"audio_ms":pcm.len() as f64/16.,"load_ms":load_ms,"chunks":rows,"rolling_words":merger.text().split_whitespace().count(),"note":"Ready times simulate serial processing; live service drops stale pending windows. Excludes PipeWire and display. No recognition accuracy claim."})
                )?
            );
        }
    }
    Ok(())
}
