use std::{env, fmt};

use zeroize::Zeroize;

#[cfg(feature = "local-db")]
use std::path::PathBuf;

#[cfg(all(feature = "remote-db", not(feature = "local-db")))]
use anyhow::Context;
use anyhow::bail;

use crate::cron_auth::CronAuthenticator;

#[derive(Clone)]
pub struct AppConfig {
    pub cookie_secure: bool,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub(crate) app_encryption_key: AppEncryptionKey,
    pub(crate) cron_authenticator: CronAuthenticator,
    #[cfg(feature = "local-db")]
    pub local_database_path: PathBuf,
    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    pub turso_database_url: String,
    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    pub turso_auth_token: String,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("AppConfig");
        debug
            .field("cookie_secure", &self.cookie_secure)
            .field("admin_email", &self.admin_email)
            .field(
                "admin_password",
                &self.admin_password.as_ref().map(|_| "[REDACTED]"),
            )
            .field("app_encryption_key", &self.app_encryption_key)
            .field("cron_authenticator", &self.cron_authenticator);
        #[cfg(feature = "local-db")]
        debug.field("local_database_path", &self.local_database_path);
        #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
        debug
            .field("turso_database_url", &self.turso_database_url)
            .field("turso_auth_token", &"[REDACTED]");
        debug.finish()
    }
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let is_production = env::var("VERCEL_ENV")
            .or_else(|_| env::var("APP_ENV"))
            .is_ok_and(|value| value.eq_ignore_ascii_case("production"));
        let cookie_secure = match env::var("SESSION_COOKIE_SECURE") {
            Ok(value) => parse_bool("SESSION_COOKIE_SECURE", &value)?,
            Err(_) => cfg!(feature = "remote-db"),
        };

        if is_production && !cookie_secure {
            bail!("SESSION_COOKIE_SECURE must not be disabled in production");
        }

        let admin_email = optional_env("ADMIN_EMAIL").map(|email| email.to_lowercase());
        let admin_password = optional_env("ADMIN_PASSWORD");
        if admin_email.is_some() != admin_password.is_some() {
            bail!("ADMIN_EMAIL and ADMIN_PASSWORD must be set together");
        }
        let app_encryption_key = resolve_app_encryption_key(
            optional_env("APP_ENCRYPTION_KEY").as_deref(),
            optional_env("APP_ENV").as_deref(),
            is_production,
            cfg!(test),
            cfg!(all(feature = "remote-db", not(feature = "local-db"))),
        )?;
        let cron_authenticator = resolve_cron_authenticator(
            optional_env("CRON_SECRET").as_deref(),
            is_production && cfg!(all(feature = "remote-db", not(feature = "local-db"))),
        )?;

        #[cfg(feature = "local-db")]
        let config = Self {
            cookie_secure,
            admin_email,
            admin_password,
            app_encryption_key,
            cron_authenticator,
            local_database_path: env::var("LIBSQL_LOCAL_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("data/trust-deeds.db")),
        };

        #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
        let config = Self {
            cookie_secure,
            admin_email,
            admin_password,
            app_encryption_key,
            cron_authenticator,
            turso_database_url: env::var("TURSO_DATABASE_URL")
                .context("TURSO_DATABASE_URL is required for remote-db")?,
            turso_auth_token: env::var("TURSO_AUTH_TOKEN")
                .context("TURSO_AUTH_TOKEN is required for remote-db")?,
        };

        Ok(config)
    }
}

fn resolve_cron_authenticator(
    configured: Option<&str>,
    required_for_remote_production: bool,
) -> anyhow::Result<CronAuthenticator> {
    let configured = configured.map(str::trim).filter(|value| !value.is_empty());
    if required_for_remote_production && configured.is_none() {
        bail!("CRON_SECRET is required for remote production environments");
    }
    Ok(CronAuthenticator::new(configured))
}

#[derive(Clone)]
pub(crate) struct AppEncryptionKey(String);

impl AppEncryptionKey {
    pub(crate) fn expose_secret(&self) -> &str {
        &self.0
    }

    #[cfg(all(test, feature = "local-db"))]
    pub(crate) fn for_test() -> Self {
        Self("trust-deeds-test-encryption-key".to_owned())
    }
}

impl fmt::Debug for AppEncryptionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AppEncryptionKey([REDACTED])")
    }
}

impl Drop for AppEncryptionKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn resolve_app_encryption_key(
    configured: Option<&str>,
    app_environment: Option<&str>,
    is_production: bool,
    is_unit_test: bool,
    is_remote_build: bool,
) -> anyhow::Result<AppEncryptionKey> {
    if let Some(value) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(AppEncryptionKey(value.to_owned()));
    }

    // Serverless builds require the real key in every Vercel environment,
    // including previews. Production never accepts a fallback either.
    if is_remote_build || is_production {
        bail!("APP_ENCRYPTION_KEY is required for remote and production environments");
    }

    let explicit_development = app_environment.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "dev" | "development" | "test"
        )
    });
    if explicit_development || is_unit_test {
        return Ok(AppEncryptionKey(
            "trust-deeds-dev-only-encryption-key".to_owned(),
        ));
    }

    bail!(
        "APP_ENCRYPTION_KEY is required; set APP_ENV=development only for an explicit local dev fallback"
    )
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_bool(name: &str, value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{name} must be true or false"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, AppEncryptionKey, resolve_app_encryption_key, resolve_cron_authenticator,
    };
    use crate::cron_auth::CronAuthenticator;

    #[cfg(feature = "local-db")]
    #[test]
    fn app_config_debug_redacts_every_local_secret() {
        let config = AppConfig {
            cookie_secure: false,
            admin_email: Some("admin@example.com".into()),
            admin_password: Some("sentinel-admin-password".into()),
            app_encryption_key: AppEncryptionKey("sentinel-encryption-key".into()),
            cron_authenticator: CronAuthenticator::new(Some("sentinel-cron-secret")),
            local_database_path: "data/test.db".into(),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("sentinel-admin-password"));
        assert!(!rendered.contains("sentinel-encryption-key"));
        assert!(!rendered.contains("sentinel-cron-secret"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    #[test]
    fn app_config_debug_redacts_every_remote_secret() {
        let config = AppConfig {
            cookie_secure: true,
            admin_email: None,
            admin_password: Some("sentinel-admin-password".into()),
            app_encryption_key: AppEncryptionKey("sentinel-encryption-key".into()),
            cron_authenticator: CronAuthenticator::new(Some("sentinel-cron-secret")),
            turso_database_url: "libsql://example.turso.io".into(),
            turso_auth_token: "sentinel-turso-token".into(),
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("sentinel-admin-password"));
        assert!(!rendered.contains("sentinel-encryption-key"));
        assert!(!rendered.contains("sentinel-turso-token"));
        assert!(!rendered.contains("sentinel-cron-secret"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn encryption_key_is_redacted_and_trimmed() {
        let key =
            resolve_app_encryption_key(Some("  real-secret  "), None, false, false, true).unwrap();
        assert_eq!(key.expose_secret(), "real-secret");
        assert!(!format!("{key:?}").contains("real-secret"));
    }

    #[test]
    fn remote_production_requires_a_nonempty_cron_secret() {
        assert!(resolve_cron_authenticator(None, true).is_err());
        assert!(resolve_cron_authenticator(Some("   "), true).is_err());
        assert!(
            resolve_cron_authenticator(Some(" cron-secret "), true)
                .unwrap()
                .is_configured()
        );
        assert!(
            !resolve_cron_authenticator(None, false)
                .unwrap()
                .is_configured()
        );
    }

    #[test]
    fn fallback_requires_explicit_dev_or_unit_test() {
        assert!(resolve_app_encryption_key(None, None, false, false, false).is_err());
        assert!(resolve_app_encryption_key(None, Some("preview"), false, false, false).is_err());
        assert!(resolve_app_encryption_key(None, Some("development"), false, false, false).is_ok());
        assert!(resolve_app_encryption_key(None, None, false, true, false).is_ok());
    }

    #[test]
    fn production_and_remote_never_accept_fallback() {
        assert!(resolve_app_encryption_key(None, Some("development"), true, true, false).is_err());
        assert!(resolve_app_encryption_key(None, Some("test"), false, true, true).is_err());
        assert!(resolve_app_encryption_key(Some("remote-key"), None, true, false, true).is_ok());
    }
}
