// SPDX-License-Identifier: Apache-2.0
//! Calendar dates in UTC from the system clock, and back: days since
//! 1970-01-01 to year, month and day by Howard Hinnant's civil-date
//! algorithm, so no date crate is needed.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub days: i64,
}

impl Date {
    pub fn today() -> Date {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Date { days: secs.div_euclid(86_400) }
    }

    pub fn from_ymd(y: i64, m: i64, d: i64) -> Date {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Date { days: era * 146_097 + doe - 719_468 }
    }

    pub fn ymd(self) -> (i64, i64, i64) {
        let z = self.days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        (y, m, d)
    }

    /// YYYY-MM-DD, strictly.
    pub fn parse(s: &str) -> Option<Date> {
        let p: Vec<&str> = s.split('-').collect();
        if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
            return None;
        }
        let (y, m, d) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
        let date = Date::from_ymd(y, m, d);
        (date.ymd() == (y, m, d)).then_some(date)
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_days() {
        assert_eq!(Date::from_ymd(1970, 1, 1).days, 0);
        assert_eq!(Date::from_ymd(2000, 3, 1).days, 11017);
        assert_eq!(Date::from_ymd(2026, 10, 23).to_string(), "2026-10-23");
        assert_eq!(Date::parse("2024-02-29").map(|d| d.to_string()), Some("2024-02-29".into()));
        assert_eq!(Date::parse("2023-02-29"), None);
        assert_eq!(Date::parse("2023-2-1"), None);
    }

    #[test]
    fn every_day_round_trips() {
        for days in -800_000..800_000 {
            let d = Date { days };
            let (y, m, dd) = d.ymd();
            assert_eq!(Date::from_ymd(y, m, dd), d);
        }
    }
}
