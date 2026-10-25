//! Daily policy clock shared by native and legacy Windows clients.
use chrono::{DateTime, Timelike, Utc};
use std::time::Instant;
pub fn within_window(start: &str, end: &str, minute: u32) -> bool {
    if start.is_empty() && end.is_empty() {
        return true;
    }
    let parse = |s: &str| -> Option<u32> {
        let (h, m) = s.split_once(':')?;
        let h: u32 = h.parse().ok()?;
        let m: u32 = m.parse().ok()?;
        if h < 24 && m < 60 {
            Some(h * 60 + m)
        } else {
            None
        }
    };
    match (parse(start), parse(end)) {
        (Some(a), Some(b)) if a < b => minute >= a && minute < b,
        (Some(a), Some(b)) if a > b => minute >= a || minute < b,
        (Some(_), Some(_)) => true,
        _ => false,
    }
}
#[derive(Debug, Clone)]
pub struct PolicyClock {
    timestamp: DateTime<Utc>,
    received: Instant,
    offset: i32,
}
impl PolicyClock {
    pub fn new(timestamp: DateTime<Utc>, offset: i32) -> Self {
        Self {
            timestamp,
            received: Instant::now(),
            offset,
        }
    }
    pub fn active(&self, start: &str, end: &str) -> bool {
        let now = self.timestamp
            + chrono::Duration::from_std(self.received.elapsed()).unwrap_or_default()
            + chrono::Duration::seconds(self.offset.into());
        within_window(start, end, now.hour() * 60 + now.minute())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midnight_and_equal_endpoints() {
        assert!(within_window("23:00", "01:00", 1430));
        assert!(within_window("23:00", "01:00", 30));
        assert!(!within_window("23:00", "01:00", 60));
        assert!(within_window("08:00", "08:00", 700));
        assert!(!within_window("08:00", "", 480));
        assert!(!within_window("25:00", "08:00", 480));
    }
    #[test]
    fn uses_server_timezone_without_local_wall_clock() {
        let now = DateTime::parse_from_rfc3339("2026-10-25T23:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let clock = PolicyClock::new(now, 8 * 3600);
        assert!(clock.active("07:00", "08:00"));
        assert!(!clock.active("08:00", "09:00"));
    }
}
