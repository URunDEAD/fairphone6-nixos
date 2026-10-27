use fp6_callkit::model::*;
fn idle() -> Model {
    let mut m = Model::default();
    m.set_cellular(Cellular::Idle, 0);
    m
}
fn incoming(m: &mut Model) -> u64 {
    m.report(":1.10", "org.example.Test", "Synthetic", true, 0)
        .unwrap()
}
fn action(m: &mut Model, id: u64, a: Action) {
    let serial = m.request(id, a, 1).unwrap();
    m.complete(":1.10", id, serial, true, 2).unwrap();
}
#[test]
fn lifecycle_acknowledges_before_activation() {
    let mut m = idle();
    let id = incoming(&mut m);
    m.set_alert(
        id,
        Alert {
            sound: true,
            vibration: true,
        },
    );
    assert!(m.effects().ringing);
    let serial = m.request(id, Action::Answer, 1).unwrap();
    assert_eq!(m.calls[&id].state, State::Ringing);
    m.complete(":1.10", id, serial, true, 2).unwrap();
    assert_eq!(m.calls[&id].state, State::Active);
    assert!(!m.effects().ringing);
    action(&mut m, id, Action::Hold);
    assert_eq!(m.calls[&id].state, State::Held);
    assert_eq!(m.effects().audio_call, None);
    action(&mut m, id, Action::Resume);
    assert_eq!(m.calls[&id].state, State::Active);
    action(&mut m, id, Action::End);
    assert_eq!(m.calls[&id].state, State::Ended);
    assert_eq!(m.history.len(), 1);
    assert_eq!(m.history[0].answered_at, Some(2));
    assert_eq!(m.effects(), Effects::default());
}
#[test]
fn invalid_transitions_and_wrong_owner() {
    let mut m = idle();
    let id = incoming(&mut m);
    assert!(m.request(id, Action::Hold, 0).is_err());
    assert!(m.connected(":1.10", id, 1).is_err());
    let serial = m.request(id, Action::Answer, 0).unwrap();
    assert!(m.complete(":1.11", id, serial, true, 1).is_err());
    assert!(m.complete(":1.10", id, serial + 1, true, 1).is_err());
    m.complete(":1.10", id, serial, false, 1).unwrap();
    assert_eq!(m.calls[&id].state, State::Ringing);
}
#[test]
fn outgoing_and_focus_reservation() {
    let mut m = idle();
    let id = m.report(":1.10", "test", "", false, 0).unwrap();
    assert!(!m.effects().ringing);
    assert!(m.report(":1.11", "test", "", false, 0).is_err());
    m.connected(":1.10", id, 1).unwrap();
    assert_eq!(m.calls[&id].state, State::Active);
}
#[test]
fn pending_answer_reserves_focus() {
    let mut m = idle();
    let a = incoming(&mut m);
    let b = incoming(&mut m);
    m.request(a, Action::Answer, 0).unwrap();
    assert!(m.request(b, Action::Answer, 0).is_err());
    assert!(m.report(":1.11", "test", "", false, 0).is_err());
}
#[test]
fn call_waiting_requires_explicit_hold() {
    let mut m = idle();
    let a = incoming(&mut m);
    action(&mut m, a, Action::Answer);
    let b = incoming(&mut m);
    m.set_alert(
        b,
        Alert {
            sound: true,
            vibration: true,
        },
    );
    assert!(!m.effects().ringing);
    assert!(m.effects().incoming_ui);
    assert!(m.request(b, Action::Answer, 0).is_err());
    action(&mut m, a, Action::Hold);
    action(&mut m, b, Action::Answer);
    assert!(m.request(a, Action::Resume, 0).is_err());
}
#[test]
fn cellular_preempts_and_invalidates_pending_ack() {
    let mut m = idle();
    let id = incoming(&mut m);
    let serial = m.request(id, Action::Answer, 0).unwrap();
    m.set_cellular(Cellular::Busy, 1);
    assert_eq!(m.calls[&id].state, State::Ended);
    assert!(m.complete(":1.10", id, serial, true, 2).is_err());
    assert_eq!(m.calls[&id].pending.as_ref().unwrap().action, Action::End);
    assert_eq!(m.history[0].reason, "cellular_priority");
}
#[test]
fn cellular_waiting_and_unknown_fail_closed() {
    let mut m = Model::default();
    let id = incoming(&mut m);
    assert!(m.request(id, Action::Answer, 0).is_err());
    assert!(m.report(":1.11", "test", "", false, 0).is_err());
    m.set_cellular(Cellular::Busy, 0);
    m.set_alert(
        id,
        Alert {
            sound: true,
            vibration: true,
        },
    );
    assert!(!m.effects().ringing);
    assert!(!m.effects().vibrating);
    assert_eq!(m.calls[&id].state, State::Ringing);
    m.set_cellular(Cellular::Idle, 1);
    action(&mut m, id, Action::Answer);
    m.set_cellular(Cellular::Unknown, 3);
    assert_eq!(m.calls[&id].state, State::Ended);
}
#[test]
fn emergency_ends_held_and_rejects_incoming() {
    let mut m = idle();
    let id = incoming(&mut m);
    action(&mut m, id, Action::Answer);
    action(&mut m, id, Action::Hold);
    m.set_cellular(Cellular::Emergency, 3);
    assert_eq!(m.calls[&id].state, State::Ended);
    assert!(m.report(":1.10", "test", "", true, 4).is_err());
}
#[test]
fn dnd_decisions_are_independent_and_reversible() {
    let mut m = idle();
    let id = incoming(&mut m);
    for sound in [false, true] {
        for vibration in [false, true] {
            m.set_alert(id, Alert { sound, vibration });
            assert_eq!(m.effects().ringing, sound);
            assert_eq!(m.effects().vibrating, vibration);
            assert!(m.effects().incoming_ui);
        }
    }
    m.set_alert(id, Alert::default());
    assert!(!m.effects().ringing);
}
#[test]
fn proximity_only_active_earpiece_and_mute_acknowledged() {
    let mut m = idle();
    let id = incoming(&mut m);
    m.near = true;
    assert!(!m.effects().screen_off);
    action(&mut m, id, Action::Answer);
    assert!(m.effects().screen_off);
    action(&mut m, id, Action::Mute(true));
    assert!(m.effects().muted);
    action(&mut m, id, Action::Route(Route::Speaker));
    assert!(!m.effects().screen_off);
    action(&mut m, id, Action::Route(Route::Bluetooth));
    assert_eq!(m.effects().route, Some(Route::Bluetooth));
    action(&mut m, id, Action::Route(Route::Earpiece));
    assert!(m.effects().screen_off);
    action(&mut m, id, Action::Hold);
    assert!(!m.effects().screen_off);
}
#[test]
fn disconnect_timeout_and_end_are_idempotent() {
    let mut m = idle();
    let id = incoming(&mut m);
    m.tick(1, &[]);
    m.end(id, "remote_end", 2);
    assert_eq!(m.history.len(), 1);
    assert_eq!(m.history[0].reason, "provider_disconnected");
    m.tick(11, &[]);
    assert!(m.calls.is_empty());
}
#[test]
fn pending_timeout_and_ring_timeout_release_effects() {
    let mut m = idle();
    let id = incoming(&mut m);
    m.request(id, Action::Answer, 0).unwrap();
    m.tick(5, &[":1.10".into()]);
    assert_eq!(m.history[0].reason, "command_timeout");
    assert_eq!(m.effects(), Effects::default());
    let id = incoming(&mut m);
    m.tick(45, &[":1.10".into()]);
    assert_eq!(m.calls[&id].state, State::Ended);
    assert_eq!(m.history.back().unwrap().reason, "no_answer");
}
#[test]
fn privacy_and_input_limits() {
    let mut m = idle();
    let id = incoming(&mut m);
    assert_eq!(m.snapshot(false).calls[0].display_name, "Internet caller");
    assert!(!serde_json::to_string(&m.snapshot(false))
        .unwrap()
        .contains(":1.10"));
    assert!(m.report(":1.11", "bad app", "", true, 0).is_err());
    assert!(m.report(":1.11", "test", "line\nbreak", true, 0).is_err());
    assert!(m
        .report(":1.11", "test", &"a".repeat(129), true, 0)
        .is_err());
    incoming(&mut m);
    assert!(m.report(":1.10", "test", "", true, 0).is_err());
    m.end(id, "remote_end", 1);
    assert!(!serde_json::to_string(&m.history)
        .unwrap()
        .contains("Synthetic"));
}
#[test]
fn history_is_bounded() {
    let mut m = idle();
    for n in 0..150 {
        let id = incoming(&mut m);
        m.end(id, "remote_end", n);
        m.tick(n + 10, &[":1.10".into()]);
    }
    assert_eq!(m.history.len(), 100);
    assert!(m.calls.is_empty());
}

#[test]
fn clearing_history_does_not_drop_terminal_commands_early() {
    let mut m = idle();
    let id = incoming(&mut m);
    m.request(id, Action::End, 1).unwrap();
    m.history.clear();
    m.tick(2, &[":1.10".into()]);
    assert!(m.calls[&id].pending.is_some());
    m.tick(11, &[":1.10".into()]);
    assert!(m.calls.is_empty());
}
#[test]
fn rapid_terminal_calls_are_bounded_and_recover() {
    let mut m = idle();
    for _ in 0..128 {
        let id = incoming(&mut m);
        m.end(id, "remote_end", 0);
    }
    assert!(m.report(":1.10", "test", "", true, 1).is_err());
    m.tick(10, &[":1.10".into()]);
    incoming(&mut m);
}

#[test]
fn missed_and_answered_history_are_distinguishable() {
    let mut m = idle();
    let missed = incoming(&mut m);
    m.end(missed, "remote_end", 3);
    assert_eq!(m.history[0].answered_at, None);
    let answered = incoming(&mut m);
    action(&mut m, answered, Action::Answer);
    m.end(answered, "remote_end", 3);
    assert_eq!(m.history[1].answered_at, Some(2));
}
