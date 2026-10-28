// SPDX-License-Identifier: GPL-3.0-or-later
//! Decision logic: who gets what without asking, when to prompt, and what
//! "Allow only while using" means.

use crate::model::Decision;

/// Who is asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A Lomiri app (cgroup of a lomiri-app-launch unit / app scope, or an
    /// AppArmor profile).
    App { id: String, confined: bool },
    /// Root, a system service, a user service that is not an app, or the
    /// owner's own login session (SSH, console): system components keep
    /// their access and are only logged.
    System { what: String },
}

impl Subject {
    pub fn key(&self) -> &str {
        match self {
            Subject::App { id, .. } => id,
            Subject::System { what } => what,
        }
    }
}

/// Is the app in use? `None` = no focus information (shell hook missing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    pub focused: Option<bool>,
    /// Seconds since the app last had focus (u64::MAX = never).
    pub since_focus: u64,
}

impl Usage {
    pub const UNKNOWN: Usage = Usage { focused: None, since_focus: u64::MAX };

    /// "While using": focused, or focused within the grace period (a file
    /// chooser, the prompt itself or a notification briefly took focus).
    pub fn in_use(&self, grace: u64) -> bool {
        match self.focused {
            None => true,
            Some(true) => true,
            Some(false) => self.since_focus <= grace,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Granted,
    Denied,
    Prompt,
}

pub fn evaluate(decision: Decision, system_app: bool, usage: Usage, grace: u64) -> Outcome {
    if system_app {
        return Outcome::Granted;
    }
    match decision {
        Decision::Allow => Outcome::Granted,
        Decision::Deny => Outcome::Denied,
        Decision::WhileUsing => {
            if usage.in_use(grace) {
                Outcome::Granted
            } else {
                Outcome::Denied
            }
        }
        // Only the app the user is looking at may put up a prompt; a
        // background request is refused quietly (and logged), as on Android.
        Decision::Ask => {
            if usage.in_use(grace) {
                Outcome::Prompt
            } else {
                Outcome::Denied
            }
        }
    }
}

/// "Only this time" lasts while the app is in use (focused, or in the
/// background for at most the grace period).
pub fn once_valid(usage: Usage, grace: u64) -> bool {
    usage.in_use(grace)
}

/// Should an open grant (device fd) of an app be taken back now?
pub fn revoke_due(decision: Decision, usage: Usage, grace: u64) -> bool {
    match decision {
        Decision::Allow => false,
        Decision::Deny | Decision::Ask => true,
        Decision::WhileUsing => !usage.in_use(grace),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: Usage = Usage { focused: Some(true), since_focus: 0 };
    const BG: Usage = Usage { focused: Some(false), since_focus: 600 };
    const JUST_LEFT: Usage = Usage { focused: Some(false), since_focus: 5 };

    #[test]
    fn system_apps_never_prompt() {
        for d in [Decision::Ask, Decision::Deny, Decision::WhileUsing] {
            assert_eq!(evaluate(d, true, BG, 10), Outcome::Granted);
        }
    }

    #[test]
    fn ask_prompts_only_in_foreground() {
        assert_eq!(evaluate(Decision::Ask, false, FG, 10), Outcome::Prompt);
        assert_eq!(evaluate(Decision::Ask, false, BG, 10), Outcome::Denied);
        assert_eq!(evaluate(Decision::Ask, false, Usage::UNKNOWN, 10), Outcome::Prompt);
    }

    #[test]
    fn while_using_has_a_grace() {
        assert_eq!(evaluate(Decision::WhileUsing, false, FG, 10), Outcome::Granted);
        assert_eq!(evaluate(Decision::WhileUsing, false, JUST_LEFT, 10), Outcome::Granted);
        assert_eq!(evaluate(Decision::WhileUsing, false, BG, 10), Outcome::Denied);
        assert!(!revoke_due(Decision::WhileUsing, JUST_LEFT, 10));
        assert!(revoke_due(Decision::WhileUsing, BG, 10));
        assert!(!revoke_due(Decision::Allow, BG, 10));
        assert!(revoke_due(Decision::Deny, FG, 10));
    }

    #[test]
    fn once_ends_in_the_background() {
        assert!(once_valid(FG, 10));
        assert!(once_valid(JUST_LEFT, 10));
        assert!(!once_valid(BG, 10));
        assert!(once_valid(Usage::UNKNOWN, 10));
    }

    #[test]
    fn remembered_answers() {
        assert_eq!(evaluate(Decision::Allow, false, BG, 10), Outcome::Granted);
        assert_eq!(evaluate(Decision::Deny, false, FG, 10), Outcome::Denied);
    }
}
