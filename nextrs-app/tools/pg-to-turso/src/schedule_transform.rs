use anyhow::{Context, Result, bail};
use chrono::{Datelike, Duration, Months, NaiveDate};

use crate::{
    blocker::ManifestSafeBlocker,
    model::{StreamEventRow, StreamRow, StreamScheduleRow},
};

#[derive(Clone, Debug)]
pub(crate) struct LegacyEventRow {
    pub id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub amount: f64,
    pub status: String,
    pub source_id: Option<String>,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub(crate) fn transform_event(
    event: LegacyEventRow,
    streams: &[StreamRow],
    schedules: &[StreamScheduleRow],
) -> Result<StreamEventRow> {
    if event.status == "received" && event.actual_date.is_none() {
        bail!(
            "stream_event[{}] is received but has no actual_date",
            event.id
        )
    }
    if event.status != "received" && event.actual_date.is_some() {
        bail!(
            "stream_event[{}] has an actual_date without received status",
            event.id
        )
    }

    if event.source_type.as_deref() != Some("stream_schedule") {
        let actual_amount = event.actual_date.as_ref().map(|_| event.amount);
        return Ok(StreamEventRow {
            id: event.id,
            stream_id: event.stream_id,
            account_id: event.account_id,
            label: event.label,
            expected_date: event.expected_date,
            amount: event.amount,
            override_label: None,
            has_label_override: 0,
            override_date: None,
            override_amount: None,
            override_account_id: None,
            has_account_override: 0,
            actual_date: event.actual_date,
            actual_amount,
            status: event.status,
            is_excluded: 0,
            exclusion_reason: None,
            source_id: event.source_id,
            source_type: event.source_type,
            metadata: event.metadata,
            notes: event.notes,
            created_at: event.created_at,
            updated_at: event.updated_at,
        });
    }

    let source_id = event.source_id.as_deref().with_context(|| {
        format!(
            "stream_event[{}] is schedule-generated but has no source_id",
            event.id
        )
    })?;
    let (schedule_id, occurrence) = parse_legacy_source_id(source_id, event.id)?;
    let schedule = schedules
        .iter()
        .find(|schedule| schedule.id == schedule_id)
        .with_context(|| {
            format!(
                "stream_event[{}] references missing stream_schedule[{schedule_id}]",
                event.id
            )
        })?;
    if schedule.stream_id != event.stream_id {
        bail!(
            "stream_event[{}] and stream_schedule[{schedule_id}] belong to different streams",
            event.id
        )
    }
    let stream = streams
        .iter()
        .find(|stream| stream.id == event.stream_id)
        .with_context(|| format!("stream_event[{}] references a missing stream", event.id))?;
    let anchor = parse_date(&schedule.start_date, "stream_schedule.start_date", event.id)?;
    let slot = match projection_slot(&schedule.frequency, anchor, occurrence, event.id) {
        Ok(slot) => slot,
        Err(error) if event.status == "received" => {
            let detail = error.to_string();
            return Err(anyhow::Error::new(ManifestSafeBlocker::new(
                "immutable received schedule history has a legacy occurrence that the current schedule cadence/anchor cannot recognize; safe stable-slot canonicalization requires reviewed schedule history",
            )))
            .with_context(|| {
                format!(
                    "stream_event[{}] cannot map through stream_schedule[{schedule_id}]: {detail}",
                    event.id
                )
            });
        }
        Err(error) => return Err(error),
    };
    let stable_source_id = format!("stream_schedule:{schedule_id}:{slot}");

    let seed_label = schedule
        .label
        .clone()
        .unwrap_or_else(|| format!("{} due", stream.name));
    let label_changed = event.label.as_deref() != Some(seed_label.as_str());
    let date_changed = event.expected_date != occurrence.format("%Y-%m-%d").to_string();
    let amount_changed = event.amount.to_bits() != schedule.amount.to_bits();
    // PostgreSQL generated occurrences with COALESCE(schedule.account_id,
    // stream.default_account_id). The target stores only the schedule account
    // as its refreshable base and applies the same fallback at read time.
    let effective_seed_account = schedule.account_id.or(stream.default_account_id);
    let account_changed = event.account_id != effective_seed_account;

    Ok(StreamEventRow {
        id: event.id,
        stream_id: event.stream_id,
        account_id: schedule.account_id,
        label: Some(seed_label),
        expected_date: occurrence.format("%Y-%m-%d").to_string(),
        amount: schedule.amount,
        override_label: label_changed.then_some(event.label).flatten(),
        has_label_override: i64::from(label_changed),
        override_date: date_changed.then_some(event.expected_date),
        override_amount: amount_changed.then_some(event.amount),
        override_account_id: account_changed.then_some(event.account_id).flatten(),
        has_account_override: i64::from(account_changed),
        actual_date: event.actual_date,
        actual_amount: (event.status == "received").then_some(event.amount),
        status: event.status,
        is_excluded: 0,
        exclusion_reason: None,
        source_id: Some(stable_source_id),
        source_type: event.source_type,
        metadata: event.metadata,
        notes: event.notes,
        created_at: event.created_at,
        updated_at: event.updated_at,
    })
}

fn parse_legacy_source_id(source_id: &str, event_id: i64) -> Result<(i64, NaiveDate)> {
    let remainder = source_id
        .strip_prefix("stream_schedule:")
        .with_context(|| {
            format!("stream_event[{event_id}] has an incompatible schedule source_id")
        })?;
    let mut parts = remainder.split(':');
    let schedule_text = parts
        .next()
        .context("legacy schedule source ID is missing its schedule")?;
    let occurrence_text = parts
        .next()
        .context("legacy schedule source ID is missing its occurrence")?;
    if parts.next().is_some() {
        bail!("stream_event[{event_id}] has an incompatible schedule source_id")
    }
    let schedule_id = schedule_text
        .parse::<i64>()
        .with_context(|| format!("stream_event[{event_id}] has an invalid schedule ID"))?;
    if schedule_id <= 0 || schedule_id.to_string() != schedule_text {
        bail!("stream_event[{event_id}] has a non-canonical schedule ID")
    }
    let occurrence = parse_date(occurrence_text, "source_id occurrence", event_id)?;
    if occurrence.format("%Y-%m-%d").to_string() != occurrence_text {
        bail!("stream_event[{event_id}] has a non-canonical occurrence date")
    }
    Ok((schedule_id, occurrence))
}

fn projection_slot(
    frequency: &str,
    anchor: NaiveDate,
    occurrence: NaiveDate,
    event_id: i64,
) -> Result<String> {
    match frequency {
        // Monthly/annual/one-time slots intentionally survive due-date edits;
        // only their cadence period is part of the stable identity.
        "monthly" => Ok(format!(
            "monthly:{:04}-{:02}",
            occurrence.year(),
            occurrence.month()
        )),
        "semimonthly" => {
            let half = if occurrence.day() == 15 {
                "mid"
            } else if occurrence == last_day_of_month(occurrence)? {
                "end"
            } else {
                bail!("stream_event[{event_id}] occurrence does not match semimonthly cadence")
            };
            Ok(format!(
                "semimonthly:{:04}-{:02}:{half}",
                occurrence.year(),
                occurrence.month()
            ))
        }
        "weekly" => indexed_slot("weekly", anchor, occurrence, 7, event_id),
        "biweekly" => indexed_slot("biweekly", anchor, occurrence, 14, event_id),
        "annual" => Ok(format!("annual:{:04}", occurrence.year())),
        "one_time" => Ok("one_time".to_owned()),
        _ => bail!("stream_event[{event_id}] references an unsupported schedule cadence"),
    }
}

fn indexed_slot(
    name: &str,
    anchor: NaiveDate,
    occurrence: NaiveDate,
    step_days: i64,
    event_id: i64,
) -> Result<String> {
    let days = occurrence.signed_duration_since(anchor).num_days();
    if days < 0 || days % step_days != 0 {
        bail!("stream_event[{event_id}] occurrence does not align to its schedule cadence")
    }
    Ok(format!("{name}:{}", days / step_days))
}

fn parse_date(value: &str, field: &str, event_id: i64) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .with_context(|| format!("stream_event[{event_id}] has an invalid {field}"))
}

fn last_day_of_month(date: NaiveDate) -> Result<NaiveDate> {
    let next_month = date
        .with_day(1)
        .context("date has no first day")?
        .checked_add_months(Months::new(1))
        .context("date arithmetic overflow")?;
    Ok(next_month - Duration::days(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> StreamRow {
        StreamRow {
            id: 10,
            name: "Paycheck".into(),
            stream_type: "manual_income".into(),
            kind: "manual_income".into(),
            direction: "in".into(),
            amount_certainty: "known".into(),
            description: None,
            default_account_id: Some(1),
            configuration: None,
            parent_id: None,
            is_active: 1,
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        }
    }

    fn schedule(frequency: &str) -> StreamScheduleRow {
        StreamScheduleRow {
            id: 20,
            stream_id: 10,
            account_id: Some(1),
            label: Some("Payday".into()),
            amount: 50.0,
            frequency: frequency.into(),
            day_of_month: Some(15),
            start_date: "2025-01-01".into(),
            end_date: None,
            is_active: 1,
            metadata: None,
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        }
    }

    fn event(source_date: &str) -> LegacyEventRow {
        LegacyEventRow {
            id: 30,
            stream_id: 10,
            account_id: Some(1),
            label: Some("Payday".into()),
            expected_date: source_date.into(),
            actual_date: None,
            amount: 50.0,
            status: "projected".into(),
            source_id: Some(format!("stream_schedule:20:{source_date}")),
            source_type: Some("stream_schedule".into()),
            metadata: Some("{}".into()),
            notes: None,
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn schedule_edits_become_overrides_on_a_stable_slot() {
        let mut legacy = event("2025-01-15");
        legacy.expected_date = "2025-01-20".into();
        legacy.amount = 75.0;
        legacy.label = Some("Moved payday".into());
        legacy.account_id = Some(2);
        let transformed = transform_event(legacy, &[stream()], &[schedule("monthly")]).unwrap();
        assert_eq!(
            transformed.source_id.as_deref(),
            Some("stream_schedule:20:monthly:2025-01")
        );
        assert_eq!(transformed.expected_date, "2025-01-15");
        assert_eq!(transformed.amount, 50.0);
        assert_eq!(transformed.override_date.as_deref(), Some("2025-01-20"));
        assert_eq!(transformed.override_amount, Some(75.0));
        assert_eq!(transformed.override_label.as_deref(), Some("Moved payday"));
        assert_eq!(transformed.override_account_id, Some(2));
        assert_eq!(transformed.has_label_override, 1);
        assert_eq!(transformed.has_account_override, 1);
    }

    #[test]
    fn received_event_keeps_schedule_seed_and_maps_reality_separately() {
        let mut legacy = event("2025-02-15");
        legacy.expected_date = "2025-02-22".into();
        legacy.actual_date = Some("2025-02-22".into());
        legacy.amount = 80.0;
        legacy.status = "received".into();
        let transformed = transform_event(legacy, &[stream()], &[schedule("monthly")]).unwrap();
        assert_eq!(transformed.expected_date, "2025-02-15");
        assert_eq!(transformed.amount, 50.0);
        assert_eq!(transformed.override_date.as_deref(), Some("2025-02-22"));
        assert_eq!(transformed.override_amount, Some(80.0));
        assert_eq!(transformed.actual_date.as_deref(), Some("2025-02-22"));
        assert_eq!(transformed.actual_amount, Some(80.0));
    }

    #[test]
    fn cadence_or_ownership_ambiguity_fails_closed() {
        let misaligned = event("2025-01-03");
        assert!(transform_event(misaligned, &[stream()], &[schedule("weekly")]).is_err());
        let mut wrong_stream = event("2025-01-15");
        wrong_stream.stream_id = 11;
        assert!(transform_event(wrong_stream, &[stream()], &[schedule("monthly")]).is_err());
    }

    #[test]
    fn inherited_stream_account_is_not_a_false_override() {
        let legacy = event("2025-01-15");
        let mut inherited = schedule("monthly");
        inherited.account_id = None;
        let transformed = transform_event(legacy, &[stream()], &[inherited]).unwrap();
        assert_eq!(transformed.account_id, None);
        assert_eq!(transformed.has_account_override, 0);
        assert_eq!(transformed.override_account_id, None);
    }

    #[test]
    fn unrecognizable_received_history_is_a_manifest_safe_blocker() {
        let mut legacy = event("2025-01-03");
        legacy.status = "received".into();
        legacy.actual_date = Some("2025-01-03".into());
        let error = transform_event(legacy, &[stream()], &[schedule("weekly")]).unwrap_err();
        let blocker = error.downcast_ref::<ManifestSafeBlocker>().unwrap();
        assert!(
            blocker
                .message()
                .contains("immutable received schedule history")
        );
        assert!(error.to_string().contains("stream_event[30]"));
    }
}
