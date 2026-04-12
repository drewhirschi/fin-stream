use std::fmt::Display;

use chrono::{DateTime, NaiveDate, NaiveDateTime};

use askama::{Result, Values};

#[askama::filter_fn]
pub fn money(value: impl Display, _: &dyn Values) -> Result<String> {
    let value = value.to_string().parse::<f64>().unwrap_or(0.0);
    let is_negative = value < 0.0;
    let total_cents = (value.abs() * 100.0).round() as u64;
    let whole = total_cents / 100;
    let cents = total_cents % 100;
    let whole_str = format_with_commas(whole);

    let formatted = if total_cents == 0 {
        "0".to_string()
    } else if cents == 0 {
        format!("{whole_str}.00")
    } else {
        format!("{whole_str}.{cents:02}")
    };

    if is_negative {
        Ok(format!("-{formatted}"))
    } else {
        Ok(formatted)
    }
}

#[askama::filter_fn]
pub fn whole(value: impl Display, _: &dyn Values) -> Result<String> {
    let value = value.to_string().parse::<f64>().unwrap_or(0.0);
    let is_negative = value < 0.0;
    let abs = value.abs().round() as u64;
    let formatted = format_with_commas(abs);

    if is_negative {
        Ok(format!("-{formatted}"))
    } else {
        Ok(formatted)
    }
}

#[askama::filter_fn]
pub fn number(value: impl Display, _: &dyn Values) -> Result<String> {
    let raw = value.to_string();
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Ok(raw);
    }

    let (sign, rest) = if let Some(rest) = trimmed.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", trimmed)
    };

    let mut parts = rest.splitn(2, '.');
    let int_part = parts.next().unwrap_or_default();
    let frac_part = parts.next();

    let int_value = int_part.parse::<u64>().ok();
    let grouped = int_value
        .map(format_with_commas)
        .unwrap_or_else(|| int_part.to_string());

    match frac_part {
        Some(frac) if !frac.is_empty() => Ok(format!("{sign}{grouped}.{frac}")),
        _ => Ok(format!("{sign}{grouped}")),
    }
}

#[askama::filter_fn]
pub fn date(value: impl AsRef<str>, _: &dyn Values) -> Result<String> {
    let value = value.as_ref().trim();
    if value.is_empty() || value == "—" || value == "-" {
        return Ok(value.to_string());
    }

    Ok(format_date_value(value))
}

#[askama::filter_fn]
pub fn datetime(value: impl AsRef<str>, _: &dyn Values) -> Result<String> {
    let value = value.as_ref().trim();
    if value.is_empty() || value == "—" || value == "-" {
        return Ok(value.to_string());
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.format("%m-%d-%Y %I:%M %p").to_string());
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S") {
        return Ok(parsed.format("%m-%d-%Y %I:%M %p").to_string());
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(parsed.format("%m-%d-%Y %I:%M %p").to_string());
    }

    Ok(format_date_value(value))
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|parsed| parsed.date_naive())
        })
        .or_else(|| {
            value
                .get(..19)
                .and_then(|partial| {
                    NaiveDateTime::parse_from_str(partial, "%Y-%m-%dT%H:%M:%S").ok()
                })
                .map(|parsed| parsed.date())
        })
        .or_else(|| NaiveDate::parse_from_str(value, "%m/%d/%Y").ok())
        .or_else(|| NaiveDate::parse_from_str(value, "%m-%d-%Y").ok())
}

fn format_date_value(value: &str) -> String {
    parse_date(value)
        .map(|parsed| parsed.format("%m-%d-%Y").to_string())
        .unwrap_or_else(|| value.to_string())
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}
