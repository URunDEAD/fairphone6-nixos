#![forbid(unsafe_code)]
use anyhow::{bail, Context, Result};
use fp6_dictation::{Settings, BUS, MAX_BYTES, MAX_SECONDS, PATH};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{oneshot, watch, Mutex},
};
use zbus::{message::Header, Connection};

type Reply = std::result::Result<String, String>;
struct Session {
    id: u64,
    owner: String,
    stop: Option<oneshot::Sender<()>>,
    cancel: watch::Sender<bool>,
    result: Option<oneshot::Receiver<Reply>>,
}
#[derive(Default)]
struct Inner {
    next: u64,
    session: Option<Session>,
    phase: String,
}
#[derive(Clone)]
struct Service {
    inner: Arc<Mutex<Inner>>,
    capture: String,
}
fn failure(message: impl ToString) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(message.to_string())
}
fn owner(header: &Header<'_>) -> zbus::fdo::Result<String> {
    header
        .sender()
        .map(ToString::to_string)
        .ok_or_else(|| failure("Missing sender"))
}
fn authorize(session: &Session, caller: &str, id: u64) -> zbus::fdo::Result<()> {
    if session.owner != caller || session.id != id {
        return Err(zbus::fdo::Error::AccessDenied(
            "Session belongs to another caller or is stale".into(),
        ));
    }
    Ok(())
}
impl Service {
    async fn phase(&self, conn: &Connection, phase: &str) {
        self.inner.lock().await.phase = phase.into();
        let _ = conn
            .emit_signal(
                None::<&str>,
                PATH,
                BUS,
                "StatusChanged",
                &(phase, phase == "recording"),
            )
            .await;
    }
    async fn run(
        &self,
        conn: Connection,
        caller: String,
        settings: Settings,
        stop: oneshot::Receiver<()>,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<String> {
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        let name = zbus::names::BusName::try_from(caller.as_str())?;
        let monitor = async {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if !dbus.name_has_owner(name.clone()).await.unwrap_or(false) {
                    break;
                }
            }
        };
        tokio::select! {
            result = self.capture_and_transcribe(&conn, settings, stop) => result,
            _ = cancel.changed() => { bail!("Cancelled"); }
            _ = monitor => { bail!("Caller disconnected"); }
        }
    }
    async fn capture_and_transcribe(
        &self,
        conn: &Connection,
        settings: Settings,
        mut stop: oneshot::Receiver<()>,
    ) -> Result<String> {
        let mut child = Command::new(&self.capture)
            .args([
                "--raw",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                "--media-role",
                "Communication",
                "--properties",
                "application.name=FP6-Dictation node.name=fp6-dictation media.name=Dictation",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Cannot start PipeWire capture")?;
        self.phase(conn, "recording").await;
        let mut stdout = child.stdout.take().context("Capture pipe missing")?;
        let mut audio = Vec::with_capacity(MAX_BYTES);
        let mut chunk = [0_u8; 4096];
        let deadline = tokio::time::sleep(Duration::from_secs(MAX_SECONDS));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut stop => break,
                _ = &mut deadline => { bail!("Recording limit reached; audio discarded"); }
                read = stdout.read(&mut chunk) => {
                    let n = read?;
                    if n == 0 { bail!("PipeWire capture ended unexpectedly"); }
                    if audio.len()+n > MAX_BYTES { bail!("Recording limit reached; audio discarded"); }
                    audio.extend_from_slice(&chunk[..n]);
                }
            }
        }
        child.kill().await?;
        child.wait().await?;
        drop(stdout);
        self.phase(conn, "transcribing").await;
        if !audio.len().is_multiple_of(2) {
            audio.pop();
        }
        let mut worker = Command::new(std::env::current_exe()?)
            .arg("--worker")
            .arg(settings.model_path()?)
            .arg(&settings.language)
            .arg(if settings.punctuation {
                "true"
            } else {
                "false"
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = worker.stdin.take().context("Worker pipe missing")?;
        let operation = async {
            stdin.write_all(&audio).await?;
            drop(stdin);
            drop(audio);
            let output = worker.wait_with_output().await?;
            if !output.status.success() {
                bail!("Offline transcription failed");
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_owned())
        };
        tokio::time::timeout(Duration::from_secs(90), operation)
            .await
            .context("Transcription timed out")?
    }
}
#[zbus::interface(name = "com.fairphone6.Dictation1")]
impl Service {
    async fn start(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> zbus::fdo::Result<u64> {
        let caller = owner(&header)?;
        let settings = Settings::load().map_err(failure)?;
        settings.model_path().map_err(failure)?;
        let mut inner = self.inner.lock().await;
        if inner.session.is_some() {
            return Err(failure("Busy"));
        }
        inner.next = inner
            .next
            .checked_add(1)
            .ok_or_else(|| failure("Session IDs exhausted"))?;
        let id = inner.next;
        let (stop_tx, stop_rx) = oneshot::channel();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (result_tx, result_rx) = oneshot::channel();
        inner.session = Some(Session {
            id,
            owner: caller.clone(),
            stop: Some(stop_tx),
            cancel: cancel_tx,
            result: Some(result_rx),
        });
        inner.phase = "starting".into();
        let service = self.clone();
        let conn = conn.clone();
        tokio::spawn(async move {
            let result = service
                .run(conn.clone(), caller, settings, stop_rx, cancel_rx)
                .await
                .map_err(|e| e.to_string());
            // Clear state only after child futures have dropped and closed their streams.
            service.phase(&conn, "idle").await;
            service.inner.lock().await.session = None;
            let _ = result_tx.send(result);
        });
        Ok(id)
    }
    async fn stop(&self, id: u64, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<String> {
        let result = {
            let mut inner = self.inner.lock().await;
            let session = inner
                .session
                .as_mut()
                .ok_or_else(|| failure("No session"))?;
            authorize(session, &owner(&header)?, id)?;
            let result = session
                .result
                .take()
                .ok_or_else(|| failure("Stop already pending"))?;
            if let Some(stop) = session.stop.take() {
                let _ = stop.send(());
            }
            result
        };
        result.await.map_err(failure)?.map_err(failure)
    }
    async fn cancel(&self, id: u64, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        let inner = self.inner.lock().await;
        let session = inner
            .session
            .as_ref()
            .ok_or_else(|| failure("No session"))?;
        authorize(session, &owner(&header)?, id)?;
        let _ = session.cancel.send(true);
        Ok(())
    }
    #[zbus(signal)]
    async fn status_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        state: &str,
        mic_active: bool,
    ) -> zbus::Result<()>;
    #[zbus(property(emits_changed_signal = "false"))]
    async fn state(&self) -> String {
        let inner = self.inner.lock().await;
        if inner.phase.is_empty() {
            "idle".into()
        } else {
            inner.phase.clone()
        }
    }
    #[zbus(property(emits_changed_signal = "false"))]
    async fn mic_active(&self) -> bool {
        self.inner.lock().await.phase == "recording"
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--worker") {
        use std::io::Read;
        if args.len() != 5 {
            bail!("Invalid worker arguments");
        }
        let mut bytes = Vec::new();
        std::io::stdin()
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let text = fp6_dictation::transcribe(
            &fp6_dictation::pcm(&bytes)?,
            std::path::Path::new(&args[2]),
            &args[3],
            args[4] == "true",
        )?;
        println!("{text}");
        return Ok(());
    }
    let capture = std::env::var("FP6_CAPTURE_PROGRAM").unwrap_or_else(|_| "pw-record".into());
    let service = Service {
        inner: Default::default(),
        capture,
    };
    let _conn = zbus::connection::Builder::session()?
        .name(BUS)?
        .serve_at(PATH, service.clone())?
        .build()
        .await?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => { result?; }
        _ = terminate.recv() => {}
    }
    if let Some(session) = service.inner.lock().await.session.as_ref() {
        let _ = session.cancel.send(true);
    }
    // Give cancellation a chance to drop and reap the child before runtime teardown.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while service.inner.lock().await.session.is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_ownership_and_generation() {
        let (tx, _) = watch::channel(false);
        let s = Session {
            id: 7,
            owner: ":1.2".into(),
            stop: None,
            cancel: tx,
            result: None,
        };
        assert!(authorize(&s, ":1.2", 7).is_ok());
        assert!(authorize(&s, ":1.3", 7).is_err());
        assert!(authorize(&s, ":1.2", 6).is_err());
    }
}
