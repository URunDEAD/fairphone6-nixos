use crate::{
    index::Index,
    ocr::{self, Recognition},
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub const BUS: &str = "com.fairphone6.Ocr1";
pub const PATH: &str = "/com/fairphone6/Ocr1";
#[derive(Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub status: String,
    pub progress: u8,
    pub stage: String,
    pub result: Option<Recognition>,
    pub error: Option<String>,
    pub warning: Option<String>,
    #[serde(skip)]
    created: Option<Instant>,
}
struct State {
    jobs: BTreeMap<String, Job>,
    index: Index,
}
#[derive(Clone)]
pub struct Service {
    state: Arc<Mutex<State>>,
    sender: mpsc::SyncSender<(String, String, String)>,
    screenshot_dir: PathBuf,
}
impl Service {
    pub fn new(data: PathBuf, screenshot_dir: PathBuf) -> Result<Self> {
        let state = Arc::new(Mutex::new(State {
            jobs: BTreeMap::new(),
            index: Index::open(data)?,
        }));
        let (sender, receiver) = mpsc::sync_channel::<(String, String, String)>(4);
        let expiration = Arc::downgrade(&state);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(30));
            let Some(state) = expiration.upgrade() else {
                break;
            };
            prune(&mut state.lock().unwrap().jobs);
        });
        let worker = Arc::clone(&state);
        std::thread::spawn(move || {
            for (id, path, lang) in receiver {
                let canceled = || {
                    worker
                        .lock()
                        .unwrap()
                        .jobs
                        .get(&id)
                        .is_none_or(|j| j.status == "canceled")
                };
                if canceled() {
                    continue;
                }
                let result = ocr::recognize(
                    &path,
                    &lang,
                    |value, stage| {
                        if let Some(job) = worker.lock().unwrap().jobs.get_mut(&id) {
                            if job.status != "canceled" {
                                job.status = "running".into();
                                job.progress = value;
                                job.stage = stage.into();
                            }
                        }
                    },
                    canceled,
                );
                let mut state = worker.lock().unwrap();
                if state.jobs.get(&id).is_none_or(|j| j.status == "canceled") {
                    continue;
                }
                let warning = result
                    .as_ref()
                    .ok()
                    .and_then(|r| state.index.add(r.path.clone(), &r.text).err())
                    .map(|e| format!("Text recognized, but indexing failed: {e}"));
                let job = state.jobs.get_mut(&id).unwrap();
                match result {
                    Ok(result) => {
                        job.status = "done".into();
                        job.stage = "Done".into();
                        job.progress = 100;
                        job.result = Some(result);
                        job.warning = warning;
                    }
                    Err(error) => {
                        job.status = "failed".into();
                        job.stage = "Failed".into();
                        job.error = Some(format!("{error:#}"));
                    }
                }
            }
        });
        Ok(Self {
            state,
            sender,
            screenshot_dir,
        })
    }
    pub fn submit(&self, path: &str, lang: &str) -> Result<String> {
        ocr::languages(lang)?;
        let path = ocr::local_path(path)?.to_string_lossy().into_owned();
        let id = uuid::Uuid::new_v4().to_string();
        let mut state = self.state.lock().unwrap();
        // Keep at most 32 jobs; expire finished text after 15 minutes on access.
        prune(&mut state.jobs);
        if state.jobs.len() >= 32 {
            let oldest = state
                .jobs
                .values()
                .filter(|j| terminal(j))
                .min_by_key(|j| j.created)
                .map(|j| j.id.clone());
            if let Some(oldest) = oldest {
                state.jobs.remove(&oldest);
            } else {
                bail!("OCR queue is full");
            }
        }
        state.jobs.insert(
            id.clone(),
            Job {
                id: id.clone(),
                status: "queued".into(),
                progress: 0,
                stage: "Queued".into(),
                result: None,
                error: None,
                warning: None,
                created: Some(Instant::now()),
            },
        );
        if self
            .sender
            .try_send((id.clone(), path, lang.to_owned()))
            .is_err()
        {
            state.jobs.remove(&id);
            bail!("OCR queue is full; retry shortly");
        }
        Ok(id)
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        let mut state = self.state.lock().unwrap();
        prune(&mut state.jobs);
        state
            .jobs
            .get(id)
            .cloned()
            .context("Unknown or expired job")
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let job = state.jobs.get_mut(id).context("Unknown job")?;
        if !terminal(job) {
            job.status = "canceled".into();
            job.stage = "Canceled".into();
        }
        Ok(())
    }
}
fn terminal(job: &Job) -> bool {
    matches!(job.status.as_str(), "done" | "failed" | "canceled")
}
fn prune(jobs: &mut BTreeMap<String, Job>) {
    jobs.retain(|_, job| {
        !terminal(job)
            || job
                .created
                .is_some_and(|t| t.elapsed() < Duration::from_secs(900))
    });
}
fn dbus_error(error: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(error.to_string())
}
#[zbus::interface(name = "com.fairphone6.Ocr1")]
impl Service {
    fn recognize(&self, path: &str, languages: &str) -> zbus::fdo::Result<String> {
        self.submit(path, languages).map_err(dbus_error)
    }
    fn get_job(&self, id: &str) -> zbus::fdo::Result<String> {
        serde_json::to_string(&self.job(id).map_err(dbus_error)?).map_err(dbus_error)
    }
    fn cancel_job(&self, id: &str) -> zbus::fdo::Result<()> {
        self.cancel(id).map_err(dbus_error)
    }
    fn latest_screenshot(&self) -> zbus::fdo::Result<String> {
        ocr::latest_screenshot(&self.screenshot_dir)
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(dbus_error)
    }
    fn search(&self, query: &str) -> zbus::fdo::Result<String> {
        serde_json::to_string(&self.state.lock().unwrap().index.search(query)).map_err(dbus_error)
    }
    fn set_index_enabled(&self, enabled: bool) -> zbus::fdo::Result<()> {
        self.state
            .lock()
            .unwrap()
            .index
            .set_enabled(enabled)
            .map_err(dbus_error)
    }
    fn get_settings(&self) -> String {
        serde_json::json!({"index_enabled":self.state.lock().unwrap().index.enabled()}).to_string()
    }
}
pub fn serve(data: PathBuf, screenshots: PathBuf) -> Result<()> {
    let service = Service::new(data, screenshots)?;
    let _connection = zbus::blocking::connection::Builder::session()?
        .name(BUS)?
        .serve_at(PATH, service)?
        .build()?;
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}
