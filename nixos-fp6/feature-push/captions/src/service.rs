use crate::{
    models,
    streaming::{Chunk, Merger, Segmenter, FRAME, RATE},
    BUS, PATH,
};
use anyhow::{bail, Context, Result};
use futures_lite::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{watch, Mutex},
};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub language: String,
    pub model: String,
    pub source: String,
    pub threads: u8,
    pub silence_minutes: u8,
    pub allow_calls: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            language: "en".into(),
            model: "tiny-q5_1".into(),
            source: "monitor".into(),
            threads: 2,
            silence_minutes: 3,
            allow_calls: false,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        models::model(&self.model)?;
        if !["en", "ro"].contains(&self.language.as_str())
            || !["monitor", "microphone"].contains(&self.source.as_str())
            || !(1..=8).contains(&self.threads)
            || !(1..=30).contains(&self.silence_minutes)
        {
            bail!("Invalid language/source/CPU/silence options");
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub state: String,
    pub capture_active: bool,
    pub allow_calls: bool,
    pub source: String,
    pub text: String,
    pub message: String,
    pub dropped: u64,
    pub decode_ms: u64,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            capture_active: false,
            allow_calls: false,
            source: String::new(),
            text: String::new(),
            message: "Captions are off".into(),
            dropped: 0,
            decode_ms: 0,
        }
    }
}
#[derive(Default)]
struct Inner {
    snapshot: Snapshot,
    cancel: Option<watch::Sender<bool>>,
    owner: String,
}
#[derive(Clone, Default)]
pub struct Service {
    inner: Arc<Mutex<Inner>>,
}
fn failure(e: impl ToString) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
/// Unknown/unavailable telephony is deliberately blocked unless explicitly opted in.
pub async fn calls_clear() -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        let c = zbus::Connection::system().await.ok()?;
        let manager = zbus::Proxy::new(&c, "org.ofono", "/", "org.ofono.Manager")
            .await
            .ok()?;
        type Objects = Vec<(
            zbus::zvariant::OwnedObjectPath,
            HashMap<String, zbus::zvariant::OwnedValue>,
        )>;
        let modems: Objects = manager.call("GetModems", &()).await.ok()?;
        if modems.is_empty() {
            return None;
        }
        for (path, _) in modems {
            let p = zbus::Proxy::new(&c, "org.ofono", path, "org.ofono.VoiceCallManager")
                .await
                .ok()?;
            let calls: Objects = p.call("GetCalls", &()).await.ok()?;
            if !calls.is_empty() {
                return Some(false);
            }
        }
        Some(true)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}
/// Subscribe before checking GetCalls so a new call cannot be lost between lookup and listen.
pub async fn call_events() -> Result<zbus::MessageStream> {
    let c = zbus::Connection::system().await?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.ofono")?
        .interface("org.ofono.VoiceCallManager")?
        .member("CallAdded")?
        .build();
    Ok(zbus::MessageStream::for_match_rule(rule, &c, Some(8)).await?)
}
impl Service {
    async fn publish(&self, c: &zbus::Connection) {
        let json = serde_json::to_string(&self.inner.lock().await.snapshot).unwrap();
        let _ = c
            .emit_signal(None::<&str>, PATH, BUS, "Updated", &(json,))
            .await;
    }
    async fn state(&self, c: &zbus::Connection, state: &str, active: bool, message: &str) {
        {
            let mut i = self.inner.lock().await;
            i.snapshot.state = state.into();
            i.snapshot.capture_active = active;
            i.snapshot.message = message.into();
        }
        self.publish(c).await;
    }
    async fn run(
        &self,
        c: &zbus::Connection,
        o: Options,
        owner: String,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        // Cancellation also interrupts model verification/loading and call lookup.
        tokio::select! {
            r=self.session(c,o.clone())=>r,
            r=self.owner_guard(c,owner)=>r,
            _=cancel.changed()=>Ok(())
        }
    }
    async fn owner_guard(&self, c: &zbus::Connection, owner: String) -> Result<()> {
        let dbus = zbus::fdo::DBusProxy::new(c).await?;
        let name = zbus::names::BusName::try_from(owner.as_str())?;
        loop {
            if !dbus.name_has_owner(name.clone()).await.unwrap_or(false) {
                bail!("Controller disconnected");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn session(&self, c: &zbus::Connection, o: Options) -> Result<()> {
        let mut events = if o.allow_calls {
            None
        } else {
            Some(
                tokio::time::timeout(Duration::from_secs(2), call_events())
                    .await
                    .context("Call status subscription timed out")?
                    .context("Call status unavailable")?,
            )
        };
        if !o.allow_calls && !calls_clear().await {
            bail!("Call status active or unknown; captions blocked");
        }
        let m = models::model(&o.model)?;
        let path = models::directory().join(&m.filename);
        let verify_path = path.clone();
        tokio::task::spawn_blocking(move || models::verify(&verify_path, &m)).await??;
        let mut worker = Command::new(std::env::current_exe()?)
            .arg("worker")
            .arg(&path)
            .arg(&o.language)
            .arg(o.threads.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut input = worker.stdin.take().context("worker stdin")?;
        let mut output = BufReader::new(worker.stdout.take().context("worker stdout")?);
        let mut ready = String::new();
        tokio::time::timeout(Duration::from_secs(60), output.read_line(&mut ready)).await??;
        if ready.trim() != "ready" {
            bail!("Model failed to load");
        }
        if !o.allow_calls && !calls_clear().await {
            bail!("Call status active or unknown; captions blocked");
        }
        let mut capture = Command::new("pw-record");
        capture.args([
            "--raw",
            "--rate",
            "16000",
            "--channels",
            "1",
            "--format",
            "s16",
            "--latency",
            "40ms",
            "--media-role",
            "Accessibility",
            "--properties",
        ]);
        // stream.capture.sink selects a sink monitor, never a microphone fallback.
        let props = if o.source == "monitor" {
            "{ application.name = FP6-Captions node.name = fp6-captions media.name = Live-Captions stream.capture.sink = true node.dont-fallback = true }"
        } else {
            "{ application.name = FP6-Captions node.name = fp6-captions media.name = Live-Captions }"
        };
        let mut capture = capture
            .arg(props)
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut audio = capture.stdout.take().context("capture stdout")?;
        self.state(c, "listening", true, "Audio in use · local processing")
            .await;
        let (tx, mut rx) = watch::channel::<Option<(u64, Chunk)>>(None);
        let silent_samples = AtomicU64::new(0);
        let capture_loop = async {
            let mut segmenter = Segmenter::default();
            let mut bytes = [0; FRAME * 2];
            let mut seq = 0;
            let mut last_clear = Instant::now();
            loop {
                tokio::time::timeout(Duration::from_secs(5), audio.read_exact(&mut bytes))
                    .await
                    .context("PipeWire stalled")??;
                let samples: Vec<f32> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|v| i16::from_le_bytes([v[0], v[1]]) as f32 / 32768.)
                    .collect();
                if let Some(chunk) = segmenter.push(&samples) {
                    seq += 1;
                    tx.send_replace(Some((seq, chunk)));
                }
                silent_samples.store(segmenter.silent_samples(), Ordering::Relaxed);
                if segmenter.silent_samples() >= o.silence_minutes as u64 * 60 * RATE as u64 {
                    return Ok::<(), anyhow::Error>(());
                }
                if segmenter.silent_samples() >= 8 * RATE as u64
                    && last_clear.elapsed() > Duration::from_secs(1)
                {
                    self.inner.lock().await.snapshot.text.clear();
                    self.publish(c).await;
                    last_clear = Instant::now();
                }
            }
        };
        let decode_loop = async {
            let mut previous = 0;
            let mut merger = Merger::default();
            loop {
                rx.changed().await?;
                let (seq, chunk) = rx.borrow_and_update().clone().context("Missing chunk")?;
                let dropped = seq.saturating_sub(previous + 1);
                previous = seq;
                // UI expiry or dropped windows end text continuity.
                if dropped > 0 || self.inner.lock().await.snapshot.text.is_empty() {
                    merger.clear();
                }
                let start = Instant::now();
                input
                    .write_all(&(chunk.samples.len() as u32).to_le_bytes())
                    .await?;
                let bytes: Vec<u8> = chunk
                    .samples
                    .iter()
                    .flat_map(|s| ((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())
                    .collect();
                input.write_all(&bytes).await?;
                input.flush().await?;
                let mut line = String::new();
                let n = tokio::time::timeout(Duration::from_secs(15), output.read_line(&mut line))
                    .await
                    .context("Decoder too slow (15 s)")??;
                if n == 0 || line.len() > 16384 {
                    bail!("Decoder failed");
                }
                let text: String = serde_json::from_str(&line)?;
                if silent_samples.load(Ordering::Relaxed) >= 8 * RATE as u64 {
                    merger.clear();
                    continue;
                }
                {
                    let mut i = self.inner.lock().await;
                    i.snapshot.text = merger.append(&text, chunk.overlaps && dropped == 0);
                    i.snapshot.dropped += dropped;
                    i.snapshot.decode_ms = start.elapsed().as_millis() as u64;
                }
                self.publish(c).await;
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        };
        let guard = async {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if !o.allow_calls && !calls_clear().await {
                    bail!("Call status active or unknown; captions stopped");
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        };
        let call_added = async {
            if let Some(stream) = &mut events {
                let _ = stream.next().await;
            } else {
                std::future::pending::<()>().await;
            }
            Err::<(), anyhow::Error>(anyhow::anyhow!(
                "Call started or event stream lost; captions stopped"
            ))
        };
        let result =
            tokio::select! { r=capture_loop=>r,r=decode_loop=>r,r=guard=>r,r=call_added=>r };
        let _ = capture.kill().await;
        let _ = capture.wait().await;
        let _ = worker.kill().await;
        let _ = worker.wait().await;
        result
    }
    pub async fn shutdown(&self) {
        if let Some(tx) = &self.inner.lock().await.cancel {
            let _ = tx.send(true);
        }
    }
}
#[zbus::interface(name = "com.fairphone6.Captions1")]
impl Service {
    async fn start(
        &self,
        options: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] c: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let o: Options = serde_json::from_str(options).map_err(failure)?;
        o.validate().map_err(failure)?;
        let owner = header
            .sender()
            .context("Missing sender")
            .map_err(failure)?
            .to_string();
        let mut i = self.inner.lock().await;
        if i.cancel.is_some() {
            return Err(failure("Already running"));
        }
        let (tx, rx) = watch::channel(false);
        i.cancel = Some(tx);
        i.owner = owner.clone();
        i.snapshot = Snapshot {
            state: "starting".into(),
            source: o.source.clone(),
            allow_calls: o.allow_calls,
            ..Default::default()
        };
        let service = self.clone();
        let c = c.clone();
        tokio::spawn(async move {
            let result = service.run(&c, o, owner, rx).await;
            // kill_on_drop has closed both child pipes before publishing inactive.
            {
                let mut i = service.inner.lock().await;
                i.cancel = None;
                i.owner.clear();
                i.snapshot.text.clear();
                i.snapshot.capture_active = false;
                i.snapshot.state = "idle".into();
                i.snapshot.message = match result {
                    Ok(()) => "Stopped; audio and text discarded".into(),
                    Err(e) => e.to_string(),
                };
            }
            service.publish(&c).await;
        });
        Ok(())
    }
    /// Any local user-session controller can stop for privacy; only the owner can keep a session alive.
    async fn stop(&self) -> zbus::fdo::Result<()> {
        self.shutdown().await;
        Ok(())
    }
    async fn snapshot(&self) -> String {
        serde_json::to_string(&self.inner.lock().await.snapshot).unwrap()
    }
    #[zbus(signal)]
    async fn updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        snapshot: &str,
    ) -> zbus::Result<()>;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_are_bounded_and_calls_default_blocked() {
        let mut o = Options::default();
        o.validate().unwrap();
        assert!(!o.allow_calls);
        o.threads = 9;
        assert!(o.validate().is_err());
        o.threads = 2;
        o.silence_minutes = 0;
        assert!(o.validate().is_err());
    }
}
