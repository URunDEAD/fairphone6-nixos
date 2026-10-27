// SPDX-License-Identifier: GPL-3.0-only
//! Calls as both call sources (oFono for CS calls, imsd for VoLTE) report
//! them, reduced to what the recorder needs.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallState {
    /// Outgoing, not answered yet (oFono dialing/alerting, imsd dialing/ringing).
    Dialing,
    /// Incoming, not answered yet (oFono incoming/waiting, imsd incoming).
    Ringing,
    Active,
    Held,
    Ended,
}

impl CallState {
    pub fn from_ofono(s: &str) -> Self {
        match s {
            "dialing" | "alerting" => CallState::Dialing,
            "incoming" | "waiting" => CallState::Ringing,
            "active" => CallState::Active,
            "held" => CallState::Held,
            _ => CallState::Ended, // "disconnected"
        }
    }

    pub fn from_imsd(s: &str) -> Self {
        match s {
            // imsd "ringing" = the far end rings (our outgoing call, 180).
            "dialing" | "ringing" => CallState::Dialing,
            "incoming" => CallState::Ringing,
            "active" => CallState::Active,
            "held" => CallState::Held,
            _ => CallState::Ended, // "terminated"
        }
    }

    /// Connected: there is call audio to record.
    pub fn connected(self) -> bool {
        matches!(self, CallState::Active | CallState::Held)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    /// First seen already connected (the recorder started mid-call).
    Unknown,
}

impl Direction {
    /// File-name form: "in", "out" ("unk" when not known).
    pub fn short(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
            Direction::Unknown => "unk",
        }
    }

    pub fn long(self) -> &'static str {
        match self {
            Direction::In => "incoming",
            Direction::Out => "outgoing",
            Direction::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Ofono,
    Imsd,
}

/// The audio path a recording taps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallPath {
    Volte,
    Cs,
    Fallback,
}

impl fmt::Display for CallPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CallPath::Volte => "volte",
            CallPath::Cs => "cs",
            CallPath::Fallback => "fallback",
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub source: Source,
    /// oFono object path or imsd call uni.
    pub id: String,
    pub state: CallState,
    /// Remote party (oFono LineIdentification, imsd number); may be empty
    /// or "withheld".
    pub number: String,
    /// Network-supplied name (oFono Name, CNAP); usually empty.
    pub network_name: String,
    /// oFono modem path or "imsd".
    pub line: String,
    /// oFono IncomingLine (the own number that was called), if any.
    pub incoming_line: String,
    pub multiparty: bool,
    pub emergency: bool,
    /// Direction as reported by the source, when it reports one (imsd).
    pub direction: Option<Direction>,
}

impl Call {
    pub fn key(&self) -> String {
        format!("{:?}:{}", self.source, self.id)
    }
}

/// Digits of a phone number, without a leading international prefix marker.
pub fn digits(number: &str) -> String {
    number.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Loose phone-number equality: the last 8 digits (or all digits when either
/// side is shorter) are equal. "+999 1234 5678" == "0012345678" style
/// national/international variants.
pub fn same_number(a: &str, b: &str) -> bool {
    let (da, db) = (digits(a), digits(b));
    if da.is_empty() || db.is_empty() {
        return false;
    }
    let shorter = da.len().min(db.len());
    // Short codes (112, 1234) only match exactly.
    if shorter < 7 {
        return da == db;
    }
    let n = shorter.min(8);
    da[da.len() - n..] == db[db.len() - n..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        assert_eq!(CallState::from_ofono("alerting"), CallState::Dialing);
        assert_eq!(CallState::from_ofono("waiting"), CallState::Ringing);
        assert_eq!(CallState::from_ofono("disconnected"), CallState::Ended);
        assert_eq!(CallState::from_imsd("ringing"), CallState::Dialing);
        assert_eq!(CallState::from_imsd("incoming"), CallState::Ringing);
        assert_eq!(CallState::from_imsd("terminated"), CallState::Ended);
        assert!(CallState::Held.connected());
        assert!(!CallState::Dialing.connected());
    }

    #[test]
    fn numbers() {
        assert!(same_number("+999 1234 5678", "0012345678"));
        assert!(same_number("+99912345678", "+999-1234-5678"));
        assert!(!same_number("+99912345678", "+99912345679"));
        assert!(!same_number("", "+999"));
        assert!(same_number("112", "112"));
        assert!(!same_number("112", "1112"));
    }
}
