use std::{fmt, str::FromStr};

use anyhow::bail;
use serde::Serialize;

use super::IsoDate;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant)),+,
                    _ => bail!("unsupported {}: {value}", stringify!($name)),
                }
            }
        }
    };
}

string_enum!(Direction {
    In => "in",
    Out => "out",
});

impl Direction {
    pub fn for_kind(kind: &str) -> Self {
        match kind {
            "manual_expense" | "credit_card" => Self::Out,
            _ => Self::In,
        }
    }

    pub fn signed(self, magnitude: f64) -> f64 {
        match self {
            Self::In => magnitude.abs(),
            Self::Out => -magnitude.abs(),
        }
    }
}

string_enum!(AmountCertainty {
    Known => "known",
    Estimated => "estimated",
});

impl AmountCertainty {
    pub fn for_kind(kind: &str) -> Self {
        if kind == "credit_card" {
            Self::Estimated
        } else {
            Self::Known
        }
    }
}

string_enum!(ScheduleFrequency {
    Monthly => "monthly",
    Semimonthly => "semimonthly",
    Biweekly => "biweekly",
    Weekly => "weekly",
    Annual => "annual",
    OneTime => "one_time",
});

string_enum!(EventStatus {
    Projected => "projected",
    Confirmed => "confirmed",
    Received => "received",
});

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Patch<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

#[derive(Clone, Debug)]
pub struct AccountDraft {
    pub id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub balance: Option<f64>,
    pub balance_as_of_date: Option<IsoDate>,
    pub is_primary: bool,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub balance: Option<f64>,
    pub balance_as_of_date: Option<String>,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub metadata: Option<String>,
    pub balance_updated_at: Option<String>,
    pub is_primary: i64,
    pub is_active: i64,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CashSourceView {
    pub amount: f64,
    pub as_of_date: String,
    pub account_name: Option<String>,
    pub source_kind: String,
    pub detail: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BootstrapResult {
    pub primary_account_id: i64,
    pub default_view_id: i64,
    pub stream_ids: Vec<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionWindow {
    pub from: IsoDate,
    pub through: IsoDate,
}

impl ProjectionWindow {
    pub fn new(from: IsoDate, through: IsoDate) -> anyhow::Result<Self> {
        if through < from {
            bail!("projection window ends before it starts");
        }
        Ok(Self { from, through })
    }
}

#[derive(Clone, Debug)]
pub struct ScheduleDraft {
    pub id: Option<i64>,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub amount: f64,
    pub frequency: ScheduleFrequency,
    pub day_of_month: Option<u8>,
    pub start_date: IsoDate,
    pub end_date: Option<IsoDate>,
    pub metadata: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StreamDraft {
    pub id: Option<i64>,
    pub name: String,
    pub stream_type: String,
    pub kind: String,
    pub direction: Direction,
    pub amount_certainty: AmountCertainty,
    pub description: Option<String>,
    pub default_account_id: Option<i64>,
    pub configuration: Option<String>,
    pub parent_id: Option<i64>,
    pub schedules: Vec<ScheduleDraft>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamScheduleView {
    pub id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub amount: f64,
    pub frequency: String,
    pub day_of_month: Option<i64>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub is_active: i64,
    pub metadata: Option<String>,
}

/// Route-facing stream shape. The flattened first-schedule fields preserve the
/// existing Askama contract while `schedules` carries the complete model.
#[derive(Clone, Debug, Serialize)]
pub struct StreamConfigView {
    pub id: i64,
    pub name: String,
    pub stream_type: String,
    pub kind: String,
    pub direction: String,
    pub amount_certainty: String,
    pub description: Option<String>,
    pub is_active: i64,
    pub default_account_id: Option<i64>,
    pub default_account_name: Option<String>,
    /// Provider-owned configuration is not editable in the current form, but
    /// must be round-tripped by ordinary PATCH requests.
    #[serde(skip)]
    pub configuration: Option<String>,
    /// Imported parent relationships are likewise hidden from the form.
    #[serde(skip)]
    pub parent_id: Option<i64>,
    pub schedule_id: Option<i64>,
    pub schedule_label: Option<String>,
    pub schedule_amount: Option<f64>,
    pub schedule_frequency: Option<String>,
    pub due_day: Option<i64>,
    pub schedule_start_date: Option<String>,
    pub schedules: Vec<StreamScheduleView>,
}

/// The deliberately small read model needed by the Canvas stream picker.
/// Canvas obtains event lanes from the forecast API, so loading the page does
/// not need schedule configuration or account details.
#[derive(Clone, Debug, Serialize)]
pub struct CanvasStreamView {
    pub id: i64,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct StreamViewDraft {
    pub id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub is_default: bool,
    pub stream_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamViewSummary {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_default: i64,
    pub is_active: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamViewMember {
    pub stream_id: i64,
    pub stream_name: String,
    pub included: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreamViewEditor {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_default: i64,
    pub is_active: i64,
    pub members: Vec<StreamViewMember>,
}

#[derive(Clone, Debug)]
pub struct EventDraft {
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: String,
    pub expected_date: IsoDate,
    pub amount: f64,
    pub status: EventStatus,
    pub metadata: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct EventPatch {
    pub label: Patch<String>,
    pub expected_date: Patch<IsoDate>,
    pub amount: Patch<f64>,
    pub account_id: Patch<i64>,
    pub notes: Patch<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventView {
    pub id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub expected_date: String,
    pub amount: f64,
    pub override_label: Option<String>,
    pub override_date: Option<String>,
    pub override_amount: Option<f64>,
    pub override_account_id: Option<i64>,
    pub has_account_override: bool,
    pub actual_date: Option<String>,
    pub actual_amount: Option<f64>,
    pub effective_date: String,
    pub effective_amount: f64,
    pub status: String,
    pub is_excluded: bool,
    pub source_id: Option<String>,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct ForecastQuery {
    pub from: IsoDate,
    pub through: IsoDate,
    /// Injected by the HTTP boundary so lateness is independent of a stale
    /// cash anchor and deterministic in repository tests.
    pub today: IsoDate,
    pub stream_id: Option<i64>,
    pub view_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ForecastRow {
    pub event_id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub has_account_override: bool,
    pub date: String,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub account_name: Option<String>,
    pub amount: f64,
    pub status: String,
    pub direction: String,
    pub amount_certainty: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ForecastRowWithBalance {
    pub event_id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub has_account_override: bool,
    pub date: String,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub label: Option<String>,
    pub stream_name: Option<String>,
    pub account_name: Option<String>,
    pub amount: f64,
    pub running_balance: f64,
    pub status: String,
    pub direction: String,
    pub amount_certainty: String,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub is_late: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ForecastResponse {
    /// Confirmed cash at `balance_as_of_date`; a confirmed zero remains `Some`.
    pub starting_balance: f64,
    pub balance_as_of_date: String,
    pub cash_source: CashSourceView,
    /// Balance immediately before events on `ForecastQuery::from` are applied.
    pub opening_balance: f64,
    pub rows: Vec<ForecastRowWithBalance>,
    /// Balance at the end of `ForecastQuery::through`, even if no event lands there.
    pub ending_balance: f64,
}
