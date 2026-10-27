//! Deterministic policy. Time is monotonic seconds supplied by the service.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ringing,
    Dialing,
    Active,
    Held,
    Ended,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cellular {
    Unknown,
    Idle,
    Busy,
    Emergency,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Earpiece,
    Speaker,
    Bluetooth,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub sound: bool,
    pub vibration: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Action {
    Answer,
    Hold,
    Resume,
    End,
    Mute(bool),
    Route(Route),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Command {
    pub serial: u64,
    pub action: Action,
    pub deadline: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Call {
    pub id: u64,
    #[serde(skip)]
    pub owner: String,
    pub app: String,
    pub display_name: String,
    pub incoming: bool,
    pub state: State,
    pub muted: bool,
    pub route: Route,
    pub alert: Alert,
    pub created: u64,
    pub pending: Option<Command>,
    pub reason: Option<String>,
    pub ended_at: Option<u64>,
    pub answered_at: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    pub id: u64,
    pub app: String,
    pub incoming: bool,
    pub created: u64,
    pub ended: u64,
    pub answered_at: Option<u64>,
    pub reason: String,
}
/// Idempotent desired hardware state, without caller identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effects {
    pub ringing: bool,
    pub vibrating: bool,
    pub incoming_ui: bool,
    pub audio_call: Option<u64>,
    pub route: Option<Route>,
    pub muted: bool,
    pub screen_off: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub cellular: Cellular,
    pub calls: Vec<Call>,
    pub effects: Effects,
    pub platform_ready: bool,
}
#[derive(Debug)]
pub struct Model {
    pub calls: BTreeMap<u64, Call>,
    pub history: VecDeque<History>,
    pub cellular: Cellular,
    pub near: bool,
    pub revision: u64,
    next_id: u64,
    next_serial: u64,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            calls: BTreeMap::new(),
            history: VecDeque::new(),
            cellular: Cellular::Unknown,
            near: false,
            revision: 0,
            next_id: 1,
            next_serial: 1,
        }
    }
}
pub type Result<T> = std::result::Result<T, &'static str>;
impl Model {
    pub fn report(
        &mut self,
        owner: &str,
        app: &str,
        name: &str,
        incoming: bool,
        now: u64,
    ) -> Result<u64> {
        if app.is_empty()
            || app.len() > 128
            || !app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || name.len() > 128
            || name.chars().any(char::is_control)
        {
            return Err("invalid metadata");
        }
        if self.calls.len() >= 128
            || self
                .calls
                .values()
                .filter(|c| c.state != State::Ended)
                .count()
                >= 8
            || self
                .calls
                .values()
                .filter(|c| c.owner == owner && c.state != State::Ended)
                .count()
                >= 2
        {
            return Err("call limit reached");
        }
        if self.cellular == Cellular::Emergency {
            return Err("cellular emergency has priority");
        }
        if !incoming && !self.can_activate(None) {
            return Err("audio focus unavailable");
        }
        let id = self.next_id;
        self.next_id += 1;
        self.calls.insert(
            id,
            Call {
                id,
                owner: owner.into(),
                app: app.into(),
                display_name: name.into(),
                incoming,
                state: if incoming {
                    State::Ringing
                } else {
                    State::Dialing
                },
                muted: false,
                route: Route::Earpiece,
                alert: Alert::default(),
                created: now,
                pending: None,
                reason: None,
                ended_at: None,
                answered_at: None,
            },
        );
        self.revision += 1;
        Ok(id)
    }
    pub fn owned(&self, owner: &str, id: u64) -> Result<&Call> {
        self.calls
            .get(&id)
            .filter(|c| c.owner == owner)
            .ok_or("unknown call or wrong owner")
    }
    fn can_activate(&self, except: Option<u64>) -> bool {
        self.cellular == Cellular::Idle
            && !self.calls.values().any(|c| {
                Some(c.id) != except
                    && (matches!(c.state, State::Active | State::Dialing)
                        || c.pending
                            .as_ref()
                            .is_some_and(|p| matches!(p.action, Action::Answer | Action::Resume)))
            })
    }
    pub fn connected(&mut self, owner: &str, id: u64, now: u64) -> Result<()> {
        if self.owned(owner, id)?.state != State::Dialing {
            return Err("not dialing");
        }
        if !self.can_activate(Some(id)) {
            return Err("audio focus unavailable");
        }
        let c = self.calls.get_mut(&id).unwrap();
        c.state = State::Active;
        c.answered_at = Some(now);
        self.revision += 1;
        Ok(())
    }
    pub fn request(&mut self, id: u64, action: Action, now: u64) -> Result<u64> {
        let c = self.calls.get(&id).ok_or("unknown call")?;
        if c.state == State::Ended {
            return Err("call ended");
        }
        if c.pending.is_some() && action != Action::End {
            return Err("action pending");
        }
        let valid = match action {
            Action::Answer => c.state == State::Ringing && self.can_activate(Some(id)),
            Action::Resume => c.state == State::Held && self.can_activate(Some(id)),
            Action::Hold => c.state == State::Active,
            Action::Mute(_) | Action::Route(_) => matches!(c.state, State::Active | State::Held),
            Action::End => true,
        };
        if !valid {
            return Err("invalid transition or audio focus unavailable");
        }
        let serial = self.next_serial;
        self.next_serial += 1;
        self.calls.get_mut(&id).unwrap().pending = Some(Command {
            serial,
            action: action.clone(),
            deadline: now + 5,
        });
        if action == Action::End {
            self.end(id, "local_end", now);
        }
        self.revision += 1;
        Ok(serial)
    }
    pub fn complete(
        &mut self,
        owner: &str,
        id: u64,
        serial: u64,
        success: bool,
        now: u64,
    ) -> Result<()> {
        let c = self.owned(owner, id)?;
        let p = c
            .pending
            .clone()
            .filter(|p| p.serial == serial)
            .ok_or("stale command")?;
        if now >= p.deadline {
            return Err("command expired");
        }
        if success
            && matches!(p.action, Action::Answer | Action::Resume)
            && !self.can_activate(Some(id))
        {
            return Err("audio focus unavailable");
        }
        let c = self.calls.get_mut(&id).unwrap();
        c.pending = None;
        if success && c.state != State::Ended {
            match p.action {
                Action::Answer | Action::Resume => {
                    c.state = State::Active;
                    c.answered_at.get_or_insert(now);
                }
                Action::Hold => c.state = State::Held,
                Action::Mute(muted) => c.muted = muted,
                Action::Route(route) => c.route = route,
                Action::End => (),
            }
        }
        self.revision += 1;
        Ok(())
    }
    pub fn end(&mut self, id: u64, reason: &str, now: u64) {
        let Some(c) = self.calls.get_mut(&id) else {
            return;
        };
        if c.state == State::Ended {
            return;
        }
        c.state = State::Ended;
        c.reason = Some(reason.into());
        c.ended_at = Some(now);
        c.alert = Alert::default();
        // Never leave an Answer/Hold command valid after a remote end or preemption.
        if c.pending.as_ref().is_some_and(|p| p.action != Action::End) {
            c.pending = None;
        }
        self.history.push_back(History {
            id,
            app: c.app.clone(),
            incoming: c.incoming,
            created: c.created,
            ended: now,
            answered_at: c.answered_at,
            reason: reason.into(),
        });
        while self.history.len() > 100 {
            self.history.pop_front();
        }
        self.revision += 1;
    }
    pub fn set_cellular(&mut self, state: Cellular, now: u64) {
        if self.cellular == state {
            return;
        }
        self.cellular = state;
        // Conservative arbitration: never instruct oFono to hold/hang up a cellular call.
        let ids: Vec<_> = self
            .calls
            .values()
            .filter(|c| {
                c.state != State::Ended
                    && state != Cellular::Idle
                    && (state == Cellular::Emergency
                        || matches!(c.state, State::Active | State::Dialing)
                        || c.pending
                            .as_ref()
                            .is_some_and(|p| matches!(p.action, Action::Answer | Action::Resume)))
            })
            .map(|c| c.id)
            .collect();
        for id in ids {
            let _ = self.request(id, Action::End, now);
            if let Some(h) = self.history.back_mut().filter(|h| h.id == id) {
                h.reason = "cellular_priority".into();
            }
            self.calls.get_mut(&id).unwrap().reason = Some("cellular_priority".into());
        }
        self.revision += 1;
    }
    pub fn set_alert(&mut self, id: u64, alert: Alert) {
        if let Some(c) = self
            .calls
            .get_mut(&id)
            .filter(|c| c.state == State::Ringing)
        {
            if c.alert != alert {
                c.alert = alert;
                self.revision += 1;
            }
        }
    }
    pub fn tick(&mut self, now: u64, owners: &[String]) {
        let ids: Vec<_> = self.calls.keys().copied().collect();
        for id in ids {
            let c = &self.calls[&id];
            if !owners.contains(&c.owner) {
                self.end(id, "provider_disconnected", now);
            } else if c.pending.as_ref().is_some_and(|p| now >= p.deadline) {
                // A lost hold/mute/answer acknowledgement leaves media uncertain: terminate.
                self.end(id, "command_timeout", now);
                self.calls.get_mut(&id).unwrap().pending = None;
            } else if (c.state == State::Ringing && now >= c.created + 45)
                || (c.state == State::Dialing && now >= c.created + 60)
            {
                let _ = self.request(id, Action::End, now);
                self.calls.get_mut(&id).unwrap().reason = Some("no_answer".into());
                if let Some(h) = self.history.back_mut().filter(|h| h.id == id) {
                    h.reason = "no_answer".into();
                }
            }
        }
        // Terminal calls retained for 10 seconds for command delivery, then forgotten.
        let before = self.calls.len();
        self.calls
            .retain(|_, c| c.ended_at.is_none_or(|ended| now < ended + 10));
        if self.calls.len() != before {
            self.revision += 1;
        }
    }
    pub fn effects(&self) -> Effects {
        let ring: Vec<_> = self
            .calls
            .values()
            .filter(|c| c.state == State::Ringing)
            .collect();
        let active = self.calls.values().find(|c| c.state == State::Active);
        let quiet = self.cellular != Cellular::Idle || active.is_some();
        Effects {
            incoming_ui: !ring.is_empty(),
            ringing: !quiet && ring.iter().any(|c| c.alert.sound),
            vibrating: !quiet && ring.iter().any(|c| c.alert.vibration),
            audio_call: active.map(|c| c.id),
            route: active.map(|c| c.route),
            muted: active.is_some_and(|c| c.muted),
            screen_off: self.near && active.is_some_and(|c| c.route == Route::Earpiece),
        }
    }
    pub fn snapshot(&self, platform_ready: bool) -> Snapshot {
        Snapshot {
            revision: self.revision,
            cellular: self.cellular,
            calls: self
                .calls
                .values()
                .cloned()
                .map(|mut c| {
                    c.display_name = "Internet caller".into();
                    c
                })
                .collect(),
            effects: self.effects(),
            platform_ready,
        }
    }
}
