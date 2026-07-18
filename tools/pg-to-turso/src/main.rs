use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use trust_deeds_pg_to_turso::CredentialKey;
use trust_deeds_pg_to_turso::{ExportRequest, InventoryRequest, LoginCanary, export, inventory};

#[derive(Debug, Parser)]
#[command(name = "pg-to-turso")]
#[command(about = "Fail-closed Trust Deeds PostgreSQL to Turso cutover exporter")]
struct Cli {
    /// Name of the environment variable containing the PostgreSQL URL.
    /// The URL itself is never accepted as a command-line argument or logged.
    #[arg(long, default_value = "SOURCE_DATABASE_URL", global = true)]
    source_url_env: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inventory and classify the source without creating a SQLite artifact.
    Inventory {
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Export only when the complete source inventory is supported.
    Export {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        /// Environment variable holding a known active user's email.
        #[arg(long, default_value = "CUTOVER_LOGIN_EMAIL")]
        login_canary_email_env: String,
        /// Environment variable holding that user's known-good password.
        #[arg(long, default_value = "CUTOVER_LOGIN_PASSWORD")]
        login_canary_password_env: String,
        /// Environment variable holding APP_ENCRYPTION_KEY. Required only
        /// when provider credential rows exist.
        #[arg(long, default_value = "APP_ENCRYPTION_KEY")]
        credential_key_env: String,
        /// Optional environment variable holding a prior APP_ENCRYPTION_KEY.
        /// Used only to rewrap rows that the current key cannot decrypt.
        #[arg(long, default_value = "LEGACY_APP_ENCRYPTION_KEY")]
        legacy_credential_key_env: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_env_name(&cli.source_url_env)?;
    let source_url = env::var(&cli.source_url_env)
        .with_context(|| format!("{} is not set", cli.source_url_env))?;
    if source_url.trim().is_empty() {
        bail!("{} is empty", cli.source_url_env);
    }

    match cli.command {
        Command::Inventory { manifest } => {
            inventory(InventoryRequest {
                source_url,
                manifest_path: manifest,
            })
            .await?;
            eprintln!("inventory manifest written; inspect its status and blockers");
        }
        Command::Export {
            output,
            manifest,
            login_canary_email_env,
            login_canary_password_env,
            credential_key_env,
            legacy_credential_key_env,
        } => {
            let login_canary = LoginCanary {
                email: required_private_env(&login_canary_email_env, "login canary email")?,
                password: required_private_env(
                    &login_canary_password_env,
                    "login canary password",
                )?,
            };
            validate_env_name(&credential_key_env)?;
            let credential_key = env::var(&credential_key_env)
                .ok()
                .and_then(CredentialKey::new);
            validate_env_name(&legacy_credential_key_env)?;
            let legacy_credential_key = env::var(&legacy_credential_key_env)
                .ok()
                .and_then(CredentialKey::new);
            export(ExportRequest {
                source_url,
                output_path: output,
                manifest_path: manifest,
                login_canary,
                credential_key,
                legacy_credential_key,
            })
            .await?;
            eprintln!("validated single-file SQLite artifact and manifest written");
        }
    }

    Ok(())
}

fn required_private_env(name: &str, label: &str) -> Result<String> {
    validate_env_name(name)?;
    let value = env::var(name).with_context(|| format!("{name} is not set ({label})"))?;
    if value.is_empty() {
        bail!("{name} is empty ({label})");
    }
    Ok(value)
}

fn validate_env_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
        });
    if !valid {
        bail!("source URL environment-variable name is invalid");
    }
    Ok(())
}
