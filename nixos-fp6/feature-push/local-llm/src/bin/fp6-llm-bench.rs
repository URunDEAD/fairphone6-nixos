use anyhow::{bail, Context, Result};
use clap::Parser;
use fp6_llm::{download, Model};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{fs, io::AsyncReadExt, process::Command};

#[derive(Parser)]
#[command(
    about = "Phone-only llama-bench runner: verified installed models, prompt/generation speed, sampled RSS and thermals"
)]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "llama-bench")]
    bench: PathBuf,
    #[arg(long, required = true)]
    output: PathBuf,
    #[arg(long)]
    model: Vec<String>,
    #[arg(long, default_value_t = 3)]
    repetitions: u32,
    #[arg(long, default_value_t = 512)]
    prompt_tokens: u32,
    #[arg(long, default_value_t = 128)]
    generation_tokens: u32,
    #[arg(long)]
    gpu: bool,
    #[arg(
        long,
        default_value = "Not supplied; record ambient temperature, case, charging and throttling observations."
    )]
    thermal_note: String,
}
async fn thermals() -> Vec<Value> {
    let mut result = Vec::new();
    if let Ok(mut dirs) = fs::read_dir("/sys/class/thermal").await {
        while let Ok(Some(entry)) = dirs.next_entry().await {
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
            {
                continue;
            }
            let kind = fs::read_to_string(entry.path().join("type"))
                .await
                .unwrap_or_default();
            let temp = fs::read_to_string(entry.path().join("temp"))
                .await
                .unwrap_or_default();
            result.push(json!({"zone":entry.file_name().to_string_lossy(),"type":kind.trim(),"millidegrees_celsius":temp.trim().parse::<i64>().ok()}));
        }
    }
    result
}
fn rss_kib(text: &str) -> u64 {
    text.lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}
async fn run(args: &Args, model: &Model, root: &Path) -> Result<Value> {
    download::verify(&model.path(root), model)
        .await
        .context("benchmark model failed SHA-256 check")?;
    let before = thermals().await;
    let mut command = Command::new(&args.bench);
    // llama-bench never downloads. Reuse the same context-independent CPU batch/thread settings.
    command
        .args(["-m"])
        .arg(model.path(root))
        .args([
            "-p",
            &args.prompt_tokens.to_string(),
            "-n",
            &args.generation_tokens.to_string(),
            "-r",
            &args.repetitions.to_string(),
            "-t",
            "4",
            "-b",
            "256",
            "-ub",
            "128",
            "-ngl",
            if args.gpu { "99" } else { "0" },
            "-o",
            "json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .context("start llama-bench (select the CPU or Vulkan package's binary)")?;
    let pid = child.id().context("benchmark pid")?;
    let mut stdout = child.stdout.take().unwrap();
    let output = tokio::spawn(async move {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.map(|_| buf)
    });
    let mut peak = 0;
    let status = loop {
        peak = peak.max(rss_kib(
            &fs::read_to_string(format!("/proc/{pid}/status"))
                .await
                .unwrap_or_default(),
        ));
        if let Some(status) = child.try_wait()? {
            break status;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let raw = output.await??;
    if !status.success() {
        bail!("llama-bench failed for {}: {status}", model.id);
    }
    let measurements: Value = serde_json::from_slice(&raw).context("parse llama-bench JSON")?;
    Ok(
        json!({"id":model.id,"quantization":model.quantization,"sha256":model.sha256,"backend":if args.gpu {"vulkan"} else {"cpu"},"peak_sampled_rss_kib":peak,"ram_note":"100 ms RSS samples; may miss short peaks. Includes mapped resident weights; shared GPU allocations may be undercounted.","thermal_before":before,"thermal_after":thermals().await,"thermal_note":args.thermal_note,"measurements":measurements}),
    )
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.repetitions == 0 || args.prompt_tokens == 0 || args.generation_tokens == 0 {
        bail!("repetitions and token counts must be positive");
    }
    let root = args
        .data_dir
        .clone()
        .unwrap_or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".local/share")
                })
                .join("fp6-llm")
        })
        .join("models");
    let mut results = Vec::new();
    for model in fp6_llm::catalogue() {
        if (!args.model.is_empty() && !args.model.contains(&model.id))
            || !model.path(&root).exists()
        {
            continue;
        }
        eprintln!(
            "Benchmarking {} ({}); observe the phone's thermals",
            model.id, model.quantization
        );
        let result = match run(&args, &model, &root).await {
            Ok(v) => v,
            Err(e) => json!({"id":model.id,"error":format!("{e:#}")}),
        };
        results.push(result);
        fs::write(
            &args.output,
            serde_json::to_vec_pretty(&json!({"results":results}))?,
        )
        .await?;
    }
    if results.is_empty() {
        bail!("no matching installed models; download models explicitly first");
    }
    if results.iter().any(|r| r.get("error").is_some()) {
        bail!("some benchmarks failed; see {}", args.output.display());
    }
    Ok(())
}
