//! Dates printed on cards. PDF metadata and certificate validity are never used.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct CardDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl CardDate {
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<_> = text.trim().split(['.', '/', '-']).collect();
        if parts.len() != 3 {
            return None;
        }
        let (year, month, day) = if parts[0].len() == 4 {
            (
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            )
        } else if parts[2].len() == 4 {
            (
                parts[2].parse().ok()?,
                parts[1].parse().ok()?,
                parts[0].parse().ok()?,
            )
        } else {
            return None;
        };
        let value = Self { year, month, day };
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        };
        ((1970..=2100).contains(&year) && day >= 1 && day <= days).then_some(value)
    }

    pub fn russian(self) -> String {
        format!("{:02}.{:02}.{:04}", self.day, self.month, self.year)
    }
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Cards have no time of day. Use midnight Moscow (UTC+03:00).
    pub fn system_time(self) -> Result<SystemTime> {
        if Self::parse(&self.iso()) != Some(self) {
            bail!("Некорректная дата карточки");
        }
        let y = self.year as i64 - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let year = y - era * 400;
        let month = self.month as i64 + if self.month > 2 { -3 } else { 9 };
        let day = (153 * month + 2) / 5 + self.day as i64 - 1;
        let days = era * 146097 + year * 365 + year / 4 - year / 100 + day - 719468;
        let seconds = days * 86400 - 3 * 3600;
        Ok(if seconds >= 0 {
            UNIX_EPOCH + Duration::from_secs(seconds as u64)
        } else {
            UNIX_EPOCH - Duration::from_secs((-seconds) as u64)
        })
    }

    pub fn windows_ticks(self) -> Result<u64> {
        let time = self.system_time()?;
        let seconds = match time.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        };
        Ok((seconds + 11644473600) as u64 * 10_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_dates_and_midnight_moscow() {
        assert!(CardDate::parse("29.02.2024").is_some());
        for invalid in ["29.02.2025", "31.04.2020", "00.12.2020", "01.13.2020"] {
            assert!(CardDate::parse(invalid).is_none());
        }
        let d = CardDate::parse("2025-02-06").unwrap();
        assert_eq!(d.russian(), "06.02.2025");
        assert_eq!(
            d.system_time()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1738789200
        );
        // There is deliberately no retention/age policy: old cards are valid.
        assert!(CardDate::parse("13.01.2017").is_some());
    }
}
