use std::{fmt, str::FromStr};

use anyhow::{Context, bail};
use serde::{Serialize, Serializer};
use time::{Date, Duration, Month};

/// A validated calendar date whose storage representation is `YYYY-MM-DD`.
///
/// Keeping date parsing at the repository boundary avoids relying on SQLite's
/// permissive date functions and gives local libSQL and Turso identical rules.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IsoDate(Date);

impl IsoDate {
    pub fn new(year: i32, month: u8, day: u8) -> anyhow::Result<Self> {
        let month = Month::try_from(month).context("month must be between 1 and 12")?;
        let date = Date::from_calendar_date(year, month, day).with_context(|| {
            format!(
                "invalid calendar date {year:04}-{:02}-{day:02}",
                month as u8
            )
        })?;
        Ok(Self(date))
    }

    pub fn year(self) -> i32 {
        self.0.year()
    }

    pub fn month(self) -> u8 {
        self.0.month() as u8
    }

    pub fn day(self) -> u8 {
        self.0.day()
    }

    pub fn add_days(self, days: i64) -> anyhow::Result<Self> {
        self.0
            .checked_add(Duration::days(days))
            .map(Self)
            .context("date arithmetic overflow")
    }

    pub fn days_until(self, later: Self) -> i64 {
        (later.0 - self.0).whole_days()
    }

    pub fn first_of_month(self) -> Self {
        Self(
            Date::from_calendar_date(self.year(), self.0.month(), 1)
                .expect("an existing date's first day is valid"),
        )
    }

    pub fn last_of_month(self) -> Self {
        let (year, month) = if self.month() == 12 {
            (self.year() + 1, Month::January)
        } else {
            (
                self.year(),
                Month::try_from(self.month() + 1).expect("month is in range"),
            )
        };
        let first_next =
            Date::from_calendar_date(year, month, 1).expect("first day of a month is valid");
        Self(first_next - Duration::days(1))
    }

    pub fn with_day_clamped(self, day: u8) -> Self {
        let day = day.clamp(1, self.last_of_month().day());
        Self(
            Date::from_calendar_date(self.year(), self.0.month(), day)
                .expect("clamped day is valid"),
        )
    }

    pub fn next_month(self) -> anyhow::Result<Self> {
        let (year, month) = if self.month() == 12 {
            (self.year() + 1, 1)
        } else {
            (self.year(), self.month() + 1)
        };
        Self::new(year, month, 1)
    }

    pub fn in_year_clamped(self, year: i32) -> anyhow::Result<Self> {
        let first = Self::new(year, self.month(), 1)?;
        Ok(first.with_day_clamped(self.day()))
    }
}

impl FromStr for IsoDate {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 10 {
            bail!("date must use YYYY-MM-DD");
        }
        let bytes = value.as_bytes();
        if bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        {
            bail!("date must use YYYY-MM-DD");
        }
        let year = value[0..4].parse::<i32>().context("invalid date year")?;
        let month = value[5..7].parse::<u8>().context("invalid date month")?;
        let day = value[8..10].parse::<u8>().context("invalid date day")?;
        Self::new(year, month, day)
    }
}

impl fmt::Display for IsoDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year(),
            self.month(),
            self.day()
        )
    }
}

impl Serialize for IsoDate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::IsoDate;

    #[test]
    fn validates_and_formats_dates() {
        assert_eq!(
            "2024-02-29".parse::<IsoDate>().unwrap().to_string(),
            "2024-02-29"
        );
        assert!("2023-02-29".parse::<IsoDate>().is_err());
        assert!("2024-2-09".parse::<IsoDate>().is_err());
    }

    #[test]
    fn month_helpers_clamp_leap_days() {
        let january = "2024-01-31".parse::<IsoDate>().unwrap();
        let february = january.next_month().unwrap();
        assert_eq!(february.with_day_clamped(31).to_string(), "2024-02-29");

        let leap_day = "2024-02-29".parse::<IsoDate>().unwrap();
        assert_eq!(
            leap_day.in_year_clamped(2025).unwrap().to_string(),
            "2025-02-28"
        );
    }
}
