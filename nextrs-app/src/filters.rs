use std::fmt::Display;

use askama::{Result, Values};
use chrono::{DateTime, NaiveDate, NaiveDateTime};

#[askama::filter_fn]
pub fn money(value: impl Display, _: &dyn Values) -> Result<String> {
    Ok(format_money_value(&value.to_string()))
}

#[askama::filter_fn]
pub fn money_input(value: impl Display, _: &dyn Values) -> Result<String> {
    Ok(format_money_input_value(&value.to_string()))
}

fn format_money_input_value(raw: &str) -> String {
    let value = raw.parse::<f64>().unwrap_or(0.0);
    if value.abs() < 0.005 {
        "0".to_owned()
    } else {
        format!("{value:.2}")
    }
}

#[askama::filter_fn]
pub fn number(value: impl Display, _: &dyn Values) -> Result<String> {
    Ok(format_number_value(&value.to_string()))
}

fn format_number_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return raw.to_owned();
    }

    let (sign, rest) = trimmed
        .strip_prefix('-')
        .map_or(("", trimmed), |rest| ("-", rest));
    let mut parts = rest.splitn(2, '.');
    let integer = parts.next().unwrap_or_default();
    let grouped = integer
        .parse::<u64>()
        .map(format_with_commas)
        .unwrap_or_else(|_| integer.to_owned());
    match parts.next() {
        Some(fraction) if !fraction.is_empty() => format!("{sign}{grouped}.{fraction}"),
        _ => format!("{sign}{grouped}"),
    }
}

fn format_money_value(value: &str) -> String {
    let value = value.parse::<f64>().unwrap_or(0.0);
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
        format!("-{formatted}")
    } else {
        formatted
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

/// Render an RFC3339 timestamp with a useful UTC fallback, then let the
/// vendored local-time helper upgrade it to the viewer's timezone.
#[askama::filter_fn]
pub fn datetime_local(value: impl AsRef<str>, _: &dyn Values) -> Result<String> {
    Ok(format_datetime_local_value(value.as_ref()))
}

fn format_datetime_local_value(value: &str) -> String {
    let raw = value.trim();
    if raw.is_empty() || raw == "—" || raw == "-" {
        return raw.to_string();
    }
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        return format_date_value(raw);
    };
    let fallback = parsed.format("%m-%d-%Y %I:%M %p");
    format!(
        "<time class=\"local-time\" data-local=\"{raw}\" datetime=\"{raw}\">{fallback} UTC</time>"
    )
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

fn format_with_commas(number: u64) -> String {
    let raw = number.to_string();
    let mut result = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_display_values_consistently() {
        assert_eq!(format_money_value("0"), "0");
        assert_eq!(format_money_value("1200"), "1,200.00");
        assert_eq!(format_money_value("-12.5"), "-12.50");
        assert_eq!(format_date_value("2026-07-14"), "07-14-2026");
    }

    #[test]
    fn formats_money_inputs_without_invalid_grouping() {
        assert_eq!(format_money_input_value("0"), "0");
        assert_eq!(format_money_input_value("12345.6"), "12345.60");
    }

    #[test]
    fn groups_counts_and_emits_local_time_markup() {
        assert_eq!(format_number_value("1234567"), "1,234,567");
        let timestamp = format_datetime_local_value("2026-07-14T18:20:30.456Z");
        assert!(timestamp.contains("data-local=\"2026-07-14T18:20:30.456Z\""));
        assert!(timestamp.contains("07-14-2026 06:20 PM UTC"));
    }
}
