use anyhow::{Context, Result, bail};
use argon2::{
    Params,
    password_hash::{PasswordHash, PasswordVerifier},
};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

pub fn boolean_integer(value: i64, field: &str) -> Result<i64> {
    if matches!(value, 0 | 1) {
        Ok(value)
    } else {
        bail!("{field} must be 0 or 1")
    }
}

pub fn finite_number(value: f64, field: &str) -> Result<f64> {
    if !value.is_finite() {
        bail!("{field} is NaN or infinite")
    }
    if value.abs() >= 1.0e308 {
        bail!("{field} exceeds the target REAL domain")
    }
    Ok(value)
}

pub fn finite_magnitude(value: f64, field: &str) -> Result<f64> {
    let value = finite_number(value, field)?;
    if value < 0.0 {
        bail!("{field} is negative; the upgraded source must store magnitudes")
    }
    Ok(value)
}

pub fn iso_date(value: &str, field: &str) -> Result<String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .with_context(|| format!("{field} is not a valid YYYY-MM-DD date"))
        .map(|date| date.format("%Y-%m-%d").to_string())
}

pub fn canonical_timestamp(value: &str, field: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("{field} is not a timezone-qualified RFC3339 timestamp"))
        .map(|value| {
            value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
}

pub fn timestamp_unix_seconds(value: &str, field: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("{field} is not a timezone-qualified RFC3339 timestamp"))
        .map(|value| value.timestamp())
}

pub fn json_text(value: Option<String>, field: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            serde_json::from_str::<serde_json::Value>(&value)
                .with_context(|| format!("{field} is not valid JSON"))?;
            Ok(value)
        })
        .transpose()
}

pub fn json_array_text(value: String, field: &str) -> Result<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(&value)
        .with_context(|| format!("{field} is not valid JSON"))?;
    if !parsed.is_array() {
        bail!("{field} is not a JSON array")
    }
    Ok(value)
}

pub fn nonempty(value: String, field: &str) -> Result<String> {
    if value.trim().is_empty() {
        bail!("{field} is empty")
    }
    Ok(value)
}

/// Validate the exact PHC shape emitted by both the legacy application and the
/// target application. Keeping this deliberately narrower than everything the
/// Argon2 crate can parse prevents a hash from surviving the copy only to fail
/// (or consume surprising resources) in a Vercel login invocation.
pub fn argon2id_password_hash(value: String, field: &str) -> Result<String> {
    validate_argon2id_password_hash(&value, field)?;
    Ok(value)
}

pub fn verify_login_password(password: &str, hash: &str, field: &str) -> Result<bool> {
    let parsed = validate_argon2id_password_hash(hash, field)?;
    Ok(argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

fn validate_argon2id_password_hash<'a>(value: &'a str, field: &str) -> Result<PasswordHash<'a>> {
    let parsed = PasswordHash::new(value)
        .map_err(|error| anyhow::anyhow!("{field} is not a valid PHC password hash: {error}"))?;
    if parsed.algorithm.as_str() != "argon2id" {
        bail!("{field} is not an Argon2id password hash")
    }
    if parsed.version != Some(19) {
        bail!("{field} is not Argon2 version 19")
    }
    if parsed.salt.is_none() || parsed.hash.is_none() {
        bail!("{field} is missing its salt or password output")
    }
    let mut salt_buffer = [0_u8; 64];
    let salt_len = parsed
        .salt
        .as_ref()
        .expect("salt presence checked")
        .decode_b64(&mut salt_buffer)
        .map_err(|error| anyhow::anyhow!("{field} has an invalid Argon2 salt: {error}"))?
        .len();
    if salt_len < argon2::MIN_SALT_LEN {
        bail!("{field} has a salt shorter than the target Argon2 runtime accepts")
    }
    if parsed.params.iter().count() != 3
        || parsed.params.get_decimal("m").is_none()
        || parsed.params.get_decimal("t").is_none()
        || parsed.params.get_decimal("p").is_none()
    {
        bail!("{field} does not have the supported m/t/p Argon2 parameter shape")
    }
    let params = Params::try_from(&parsed)
        .map_err(|error| anyhow::anyhow!("{field} has unsupported Argon2 parameters: {error}"))?;
    if params.m_cost() != Params::DEFAULT_M_COST
        || params.t_cost() != Params::DEFAULT_T_COST
        || params.p_cost() != Params::DEFAULT_P_COST
        || params.output_len() != Some(Params::DEFAULT_OUTPUT_LEN)
        || !params.keyid().is_empty()
        || !params.data().is_empty()
    {
        bail!("{field} is outside the target Argon2 runtime policy")
    }
    Ok(parsed)
}

pub fn one_of(value: String, allowed: &[&str], field: &str) -> Result<String> {
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        bail!("{field} has an unsupported value")
    }
}

pub fn balance_as_of_date(
    balance: Option<f64>,
    updated_at: Option<&str>,
) -> Result<Option<String>> {
    match (balance, updated_at) {
        (None, _) => Ok(None),
        (Some(_), Some(value)) => {
            let timestamp = canonical_timestamp(value, "account.balance_updated_at")?;
            Ok(Some(timestamp[..10].to_owned()))
        }
        (Some(_), None) => bail!(
            "account with a balance has no balance_updated_at; refusing to invent an as-of date"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::{PasswordHasher, SaltString};

    fn test_password_hash() -> String {
        let salt = SaltString::encode_b64(b"0123456789abcdef").unwrap();
        argon2::Argon2::default()
            .hash_password(b"cutover password", &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn conversions_reject_lossy_or_unsupported_values() {
        assert!(finite_number(f64::NAN, "amount").is_err());
        assert!(finite_number(f64::INFINITY, "amount").is_err());
        assert!(finite_magnitude(-0.01, "amount").is_err());
        assert!(boolean_integer(2, "active").is_err());
        assert!(iso_date("2025-02-29", "date").is_err());
        assert!(canonical_timestamp("2025-01-01 12:00:00", "time").is_err());
        assert!(json_text(Some("{".to_owned()), "metadata").is_err());
        assert!(
            one_of(
                "late".to_owned(),
                &["projected", "confirmed", "received"],
                "status"
            )
            .is_err()
        );
    }

    #[test]
    fn timestamps_are_normalized_and_balance_dates_are_explicit() {
        assert_eq!(
            canonical_timestamp("2025-01-02T03:04:05.123456-07:00", "time").unwrap(),
            "2025-01-02T10:04:05.123Z"
        );
        assert_eq!(
            balance_as_of_date(Some(0.0), Some("2025-01-02T03:04:05.000Z")).unwrap(),
            Some("2025-01-02".to_owned())
        );
        assert!(balance_as_of_date(Some(1.0), None).is_err());
    }

    #[test]
    fn active_login_hash_shape_matches_the_target_verifier() {
        let hash = test_password_hash();
        assert_eq!(
            argon2id_password_hash(hash.clone(), "password_hash").unwrap(),
            hash
        );
        assert!(verify_login_password("cutover password", &hash, "password_hash").unwrap());
        assert!(!verify_login_password("wrong", &hash, "password_hash").unwrap());
        assert!(argon2id_password_hash("not-phc".into(), "password_hash").is_err());
        assert!(
            argon2id_password_hash(
                "$argon2i$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY".into(),
                "password_hash",
            )
            .is_err()
        );
        assert!(
            argon2id_password_hash(
                "$argon2id$v=19$m=19456,t=2,p=1$YWJjZA$MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY"
                    .into(),
                "password_hash",
            )
            .is_err()
        );
    }
}
