use anyhow::{ensure, Result};
use evdev::{
    uinput::VirtualDevice, AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct InputManager {
    grants: HashMap<String, (Instant, VirtualDevice)>,
}
impl InputManager {
    pub fn grant(&mut self, id: &str) -> Result<()> {
        let keys: AttributeSet<KeyCode> = (1..=127)
            .map(KeyCode::new)
            .chain([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE])
            .collect();
        let axes: AttributeSet<RelativeAxisCode> = [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
        ]
        .into_iter()
        .collect();
        let device = VirtualDevice::builder()?
            .name("KDE Connect consented remote input")
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()?;
        self.grants.insert(
            id.into(),
            (Instant::now() + Duration::from_secs(600), device),
        );
        Ok(())
    }
    pub fn revoke(&mut self, id: &str) {
        self.grants.remove(id);
    }
    pub fn expire(&mut self) -> Vec<String> {
        let ids: Vec<_> = self
            .grants
            .iter()
            .filter(|(_, (until, _))| *until <= Instant::now())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.revoke(id);
        }
        ids
    }
    pub fn inject(&mut self, id: &str, body: &Value) -> Result<()> {
        self.expire();
        let (_, device) = self
            .grants
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("remote input consent expired"))?;
        device.emit(&events(body)?)?;
        Ok(())
    }
}
fn event(kind: EventType, code: u16, value: i32) -> InputEvent {
    InputEvent::new(kind.0, code, value)
}
fn key_events(events: &mut Vec<InputEvent>, key: KeyCode) {
    events.push(event(EventType::KEY, key.0, 1));
    events.push(event(EventType::KEY, key.0, 0));
}
pub fn events(body: &Value) -> Result<Vec<InputEvent>> {
    let mut out = Vec::new();
    for (field, axis) in [
        ("dx", RelativeAxisCode::REL_X),
        ("dy", RelativeAxisCode::REL_Y),
    ] {
        if let Some(n) = body[field].as_f64() {
            ensure!(
                n.is_finite() && n.abs() <= 4096.0,
                "input motion out of range"
            );
            let axis = if body["scroll"].as_bool() == Some(true) {
                if field == "dy" {
                    RelativeAxisCode::REL_WHEEL
                } else {
                    RelativeAxisCode::REL_HWHEEL
                }
            } else {
                axis
            };
            out.push(event(EventType::RELATIVE, axis.0, n.round() as i32));
        }
    }
    for (field, key) in [
        ("singleclick", KeyCode::BTN_LEFT),
        ("rightclick", KeyCode::BTN_RIGHT),
        ("middleclick", KeyCode::BTN_MIDDLE),
    ] {
        if body[field].as_bool() == Some(true) {
            key_events(&mut out, key);
        }
    }
    if body["doubleclick"].as_bool() == Some(true) {
        key_events(&mut out, KeyCode::BTN_LEFT);
        key_events(&mut out, KeyCode::BTN_LEFT);
    }
    // Drag holds are intentionally unsupported: no stuck button on disconnect.
    let key = if let Some(special) = body["specialKey"].as_u64() {
        Some(match special {
            1 => KeyCode::KEY_BACKSPACE,
            2 => KeyCode::KEY_TAB,
            3 => KeyCode::KEY_ENTER,
            4 => KeyCode::KEY_LEFT,
            5 => KeyCode::KEY_UP,
            6 => KeyCode::KEY_RIGHT,
            7 => KeyCode::KEY_DOWN,
            8 => KeyCode::KEY_PAGEUP,
            9 => KeyCode::KEY_PAGEDOWN,
            10 => KeyCode::KEY_HOME,
            11 => KeyCode::KEY_END,
            12 => KeyCode::KEY_ENTER,
            13 => KeyCode::KEY_DELETE,
            14 => KeyCode::KEY_ESC,
            15 => KeyCode::KEY_SYSRQ,
            16 => KeyCode::KEY_SCROLLLOCK,
            21..=30 => KeyCode::new(KeyCode::KEY_F1.0 + special as u16 - 21),
            31 => KeyCode::KEY_F11,
            32 => KeyCode::KEY_F12,
            _ => anyhow::bail!("unsupported special key"),
        })
    } else {
        None
    };
    let modifiers = [
        ("ctrl", KeyCode::KEY_LEFTCTRL),
        ("alt", KeyCode::KEY_LEFTALT),
        ("shift", KeyCode::KEY_LEFTSHIFT),
        ("super", KeyCode::KEY_LEFTMETA),
    ];
    for (name, key) in modifiers {
        if body[name].as_bool() == Some(true) {
            out.push(event(EventType::KEY, key.0, 1));
        }
    }
    if let Some(key) = key {
        key_events(&mut out, key);
    }
    if let Some(text) = body["key"].as_str() {
        ensure!(text.len() <= 256, "input text too long");
        for c in text.chars() {
            let (key, shift) =
                ascii_key(c).ok_or_else(|| anyhow::anyhow!("only US ASCII input is supported"))?;
            if shift && body["shift"].as_bool() != Some(true) {
                out.push(event(EventType::KEY, KeyCode::KEY_LEFTSHIFT.0, 1));
            }
            key_events(&mut out, key);
            if shift && body["shift"].as_bool() != Some(true) {
                out.push(event(EventType::KEY, KeyCode::KEY_LEFTSHIFT.0, 0));
            }
        }
    }
    for (name, key) in modifiers.into_iter().rev() {
        if body[name].as_bool() == Some(true) {
            out.push(event(EventType::KEY, key.0, 0));
        }
    }
    Ok(out)
}
fn ascii_key(c: char) -> Option<(KeyCode, bool)> {
    let letters = [
        KeyCode::KEY_A,
        KeyCode::KEY_B,
        KeyCode::KEY_C,
        KeyCode::KEY_D,
        KeyCode::KEY_E,
        KeyCode::KEY_F,
        KeyCode::KEY_G,
        KeyCode::KEY_H,
        KeyCode::KEY_I,
        KeyCode::KEY_J,
        KeyCode::KEY_K,
        KeyCode::KEY_L,
        KeyCode::KEY_M,
        KeyCode::KEY_N,
        KeyCode::KEY_O,
        KeyCode::KEY_P,
        KeyCode::KEY_Q,
        KeyCode::KEY_R,
        KeyCode::KEY_S,
        KeyCode::KEY_T,
        KeyCode::KEY_U,
        KeyCode::KEY_V,
        KeyCode::KEY_W,
        KeyCode::KEY_X,
        KeyCode::KEY_Y,
        KeyCode::KEY_Z,
    ];
    if c.is_ascii_alphabetic() {
        return Some((
            letters[(c.to_ascii_lowercase() as u8 - b'a') as usize],
            c.is_ascii_uppercase(),
        ));
    }
    let plain = "1234567890-=[];\\',./` ";
    let shifted = "!@#$%^&*()_+{}:|\"<>?~ ";
    let keys = [
        KeyCode::KEY_1,
        KeyCode::KEY_2,
        KeyCode::KEY_3,
        KeyCode::KEY_4,
        KeyCode::KEY_5,
        KeyCode::KEY_6,
        KeyCode::KEY_7,
        KeyCode::KEY_8,
        KeyCode::KEY_9,
        KeyCode::KEY_0,
        KeyCode::KEY_MINUS,
        KeyCode::KEY_EQUAL,
        KeyCode::KEY_LEFTBRACE,
        KeyCode::KEY_RIGHTBRACE,
        KeyCode::KEY_SEMICOLON,
        KeyCode::KEY_BACKSLASH,
        KeyCode::KEY_APOSTROPHE,
        KeyCode::KEY_COMMA,
        KeyCode::KEY_DOT,
        KeyCode::KEY_SLASH,
        KeyCode::KEY_GRAVE,
        KeyCode::KEY_SPACE,
    ];
    if let Some(i) = plain.chars().position(|v| v == c) {
        return Some((keys[i], false));
    }
    if let Some(i) = shifted.chars().position(|v| v == c) {
        return Some((keys[i], true));
    }
    match c {
        '\n' => Some((KeyCode::KEY_ENTER, false)),
        '\t' => Some((KeyCode::KEY_TAB, false)),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_is_bounded_and_balanced() {
        assert!(events(&serde_json::json!({"dx":100000})).is_err());
        assert!(events(&serde_json::json!({"key":"é"})).is_err());
        let e = events(&serde_json::json!({"key":"A!", "ctrl":true})).unwrap();
        let sum: i32 = e
            .iter()
            .filter(|e| e.event_type() == EventType::KEY)
            .map(|e| if e.value() == 1 { 1 } else { -1 })
            .sum();
        assert_eq!(sum, 0);
        assert!(InputManager::default()
            .inject("untrusted", &serde_json::json!({"key":"a"}))
            .is_err());
    }
}
