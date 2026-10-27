//! fp6-kbd-ngram build|predict|eval|check
#![forbid(unsafe_code)]

use fp6_kbd_ngram::{context_of, evaluate, normalize, parity_queries, predict, BuildOptions, Builder, Model};
use std::process::ExitCode;

const USAGE: &str = "usage:
  fp6-kbd-ngram build --lang LANG --out FILE [--min-bigram N] [--min-trigram N]
                [--meta KEY=VALUE]... [--weight W] --corpus FILE [[--weight W] --corpus FILE]...
  fp6-kbd-ngram predict MODEL 'left context' PREFIX [LIMIT]
  fp6-kbd-ngram eval MODEL TEXTFILE [K]       keystroke savings with top-K candidates
  fp6-kbd-ngram check MODEL                    parse and print statistics
  fp6-kbd-ngram parity MODEL TEXTFILE          queries from TEXTFILE with this
                                               predictor's answers (C++ parity test)";

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn load(path: &str) -> Result<Model, String> {
    Model::from_tsv(&read(path)?).map_err(|e| format!("{path}: {e}"))
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut it = args.into_iter();
    match it.next().as_deref() {
        Some("build") => {
            let (mut lang, mut out, mut weight) = (None, None, 1u64);
            let mut opts = BuildOptions::default();
            let mut meta = Vec::new();
            let mut corpora: Vec<(String, u64)> = Vec::new();
            while let Some(a) = it.next() {
                let mut val = || it.next().ok_or_else(|| format!("{a} needs a value"));
                match a.as_str() {
                    "--lang" => lang = Some(val()?),
                    "--out" => out = Some(val()?),
                    "--weight" => weight = val()?.parse().map_err(|_| "bad --weight".to_string())?,
                    "--min-bigram" => opts.min_bigram = val()?.parse().map_err(|_| "bad --min-bigram".to_string())?,
                    "--min-trigram" => opts.min_trigram = val()?.parse().map_err(|_| "bad --min-trigram".to_string())?,
                    "--meta" => {
                        let v = val()?;
                        let (k, v) = v.split_once('=').ok_or("--meta wants KEY=VALUE")?;
                        meta.push((k.to_string(), v.to_string()));
                    }
                    "--corpus" => corpora.push((val()?, weight)),
                    other => return Err(format!("unknown option {other}\n{USAGE}")),
                }
            }
            let lang = lang.ok_or("--lang is required")?;
            let out = out.ok_or("--out is required")?;
            if corpora.is_empty() {
                return Err("at least one --corpus is required".into());
            }
            let mut b = Builder::new(&lang);
            for (path, w) in &corpora {
                b.add_text(&read(path)?, *w);
            }
            let m = b.finish(opts, meta);
            std::fs::write(&out, m.to_tsv()).map_err(|e| format!("{out}: {e}"))?;
            eprintln!(
                "{out}: {} words, {} bigram contexts, {} trigram contexts, {} tokens",
                m.vocabulary(),
                m.bi.len(),
                m.tri.len(),
                m.total
            );
            Ok(())
        }
        Some("predict") => {
            let model = it.next().ok_or(USAGE)?;
            let left = it.next().ok_or(USAGE)?;
            let prefix = it.next().ok_or(USAGE)?;
            let limit = it.next().map(|s| s.parse().unwrap_or(5)).unwrap_or(5);
            let m = load(&model)?;
            let ctx = context_of(&m.lang, &left);
            for p in predict(&m, None, &ctx, &prefix, limit) {
                println!("{p}");
            }
            Ok(())
        }
        Some("eval") => {
            let model = it.next().ok_or(USAGE)?;
            let text = it.next().ok_or(USAGE)?;
            let k = it.next().map(|s| s.parse().unwrap_or(3)).unwrap_or(3);
            let m = load(&model)?;
            let r = evaluate(&m, &m.lang, &read(&text)?, k);
            println!(
                "words {}  letters {}  keystrokes {}  savings {:.1}%  top{k}@1 {:.1}%  top{k}@<=2 {:.1}%",
                r.words,
                r.letters,
                r.keystrokes,
                100.0 * r.savings(),
                100.0 * r.top_k_after_1 as f64 / r.words.max(1) as f64,
                100.0 * r.top_k_after_2 as f64 / r.words.max(1) as f64
            );
            Ok(())
        }
        Some("parity") => {
            let model = it.next().ok_or(USAGE)?;
            let text = it.next().ok_or(USAGE)?;
            let m = load(&model)?;
            for (left, prefix) in parity_queries(&m.lang, &read(&text)?) {
                let ctx = context_of(&m.lang, &left);
                let p = predict(&m, None, &ctx, &normalize(&m.lang, &prefix), 5);
                println!("{left}\t{prefix}\t{}", p.join("|"));
            }
            Ok(())
        }
        Some("check") => {
            let model = it.next().ok_or(USAGE)?;
            let m = load(&model)?;
            println!(
                "lang {}  words {}  bigram contexts {}  trigram contexts {}  tokens {}",
                m.lang,
                m.vocabulary(),
                m.bi.len(),
                m.tri.len(),
                m.total
            );
            for (k, v) in &m.meta {
                println!("{k}: {v}");
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fp6-kbd-ngram: {e}");
            ExitCode::FAILURE
        }
    }
}
