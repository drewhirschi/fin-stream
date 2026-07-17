use argon2::{
    Argon2, Params,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

pub const SESSION_USER_ID_KEY: &str = "user_id";

/// A valid Argon2id hash used only to equalize the work for unknown logins.
/// The corresponding password is intentionally irrelevant: callers never
/// authenticate unless a database user id was found.
pub const DUMMY_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$Het/Y30nArPvuld+9WgiAw$4iaKMEoLQ0Aq5yU0tvoxZhAQPF/kiyw8W5JCUlq8EG4";

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("argon2 hash failed: {error}"))
}

pub fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    validate_password_hash(hash)?;
    let parsed = PasswordHash::new(hash)
        .map_err(|error| anyhow::anyhow!("invalid password hash: {error}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Reject malformed, unsupported, or unreasonably expensive imported hashes
/// before the deployment can serve login traffic.
pub fn validate_password_hash(hash: &str) -> anyhow::Result<()> {
    let parsed = PasswordHash::new(hash)
        .map_err(|error| anyhow::anyhow!("invalid password hash: {error}"))?;
    if parsed.algorithm.as_str() != "argon2id" || parsed.version != Some(19) {
        anyhow::bail!("password hash must use Argon2id version 19");
    }
    if parsed.salt.is_none() || parsed.hash.is_none() {
        anyhow::bail!("password hash must contain a salt and output");
    }
    let params = Params::try_from(&parsed)
        .map_err(|error| anyhow::anyhow!("invalid Argon2 parameters: {error}"))?;
    if params.m_cost() > 65_536 || params.t_cost() > 10 || params.p_cost() > 4 {
        anyhow::bail!("password hash parameters exceed the login safety limit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_hash_is_runtime_compatible() {
        validate_password_hash(DUMMY_PASSWORD_HASH).unwrap();
        assert!(!verify_password("not-the-dummy-password", DUMMY_PASSWORD_HASH).unwrap());
    }

    #[test]
    fn malformed_or_unsupported_hashes_fail_closed() {
        assert!(validate_password_hash("not-a-phc-string").is_err());
        assert!(validate_password_hash("$argon2i$v=19$m=19456,t=2,p=1$c2FsdA$YWJjZA").is_err());
        assert!(validate_password_hash("$argon2id$v=19$m=999999,t=2,p=1$c2FsdA$YWJjZA").is_err());
    }
}
