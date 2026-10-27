use anyhow::{ensure, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Incoming,
    Outgoing,
}
#[derive(Debug, Clone, Serialize)]
pub struct Pending {
    pub direction: Direction,
    pub timestamp: i64,
    pub expires: i64,
    pub code: String,
    #[serde(skip)]
    deadline: std::time::Instant,
}
impl Pending {
    pub fn new(direction: Direction, timestamp: i64, now: i64, code: String) -> Result<Self> {
        ensure!(
            timestamp.abs_diff(now) <= 1800,
            "device clocks differ by more than 30 minutes"
        );
        Ok(Self {
            direction,
            timestamp,
            expires: now + 30,
            code,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
        })
    }
    pub fn active(&self, now: i64) -> bool {
        now < self.expires && std::time::Instant::now() < self.deadline
    }
    pub fn can_accept(&self, now: i64) -> bool {
        self.direction == Direction::Incoming && self.active(now)
    }
    pub fn can_complete(&self, now: i64) -> bool {
        self.direction == Direction::Outgoing && self.active(now)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_is_directional_and_expires() {
        let p = Pending::new(Direction::Incoming, 1000, 1000, "TEST".into()).unwrap();
        assert!(p.can_accept(1029));
        assert!(!p.can_accept(1030));
        assert!(!p.can_complete(1001));
        assert!(Pending::new(Direction::Incoming, 1, 2000, "".into()).is_err());
        let p = Pending::new(Direction::Outgoing, 1000, 1000, "TEST".into()).unwrap();
        assert!(p.can_complete(1029));
        assert!(!p.can_accept(1001));
    }
    #[test]
    fn clock_changes_cannot_extend_consent() {
        let mut p = Pending::new(Direction::Incoming, 1000, 1000, "TEST".into()).unwrap();
        p.deadline = std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert!(!p.active(999));
    }
}
