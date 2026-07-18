use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use trust_deeds_object_cutover::{
    CutoverError,
    cutover::{backfill_local_to_s3, inventory, promote_s3_to_s3},
    manifest::{ManifestPublisher, RunMode},
    source_db::read_database,
    storage::{LocalDirectoryStore, ObjectKeyLayout, S3CompatibleStore},
};

#[derive(Debug, Parser)]
#[command(
    name = "trust-deeds-object-cutover",
    about = "Fail-closed byte-object inventory and local-to-S3 cutover"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify referenced objects in a retained local directory or S3-compatible store.
    Inventory {
        /// Absolute path to the validated target SQLite artifact.
        #[arg(long)]
        sqlite: PathBuf,
        /// Absolute, new path for the private JSON manifest.
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, value_enum)]
        source: SourceKind,
        /// Absolute local object root. Required for --source local; forbidden for S3.
        #[arg(long)]
        local_source: Option<PathBuf>,
    },
    /// Copy a verified local corpus to env-configured S3 without overwriting objects.
    BackfillLocalToS3 {
        /// Absolute path to the validated target SQLite artifact.
        #[arg(long)]
        sqlite: PathBuf,
        /// Absolute local object root.
        #[arg(long)]
        local_source: PathBuf,
        /// Absolute, new path for the private JSON manifest.
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Copy a verified legacy S3 corpus into a distinct canonical prefix.
    PromoteS3ToS3 {
        /// Absolute path to the validated target SQLite artifact.
        #[arg(long)]
        sqlite: PathBuf,
        /// Absolute, new path for the private JSON manifest.
        #[arg(long)]
        manifest: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceKind {
    Local,
    S3,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("object cutover failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CutoverError> {
    let manifest = match cli.command {
        Command::Inventory {
            sqlite,
            manifest,
            source,
            local_source,
        } => {
            let output = ManifestPublisher::reserve(&manifest)?;
            let database = read_database(&sqlite)?;
            let result = match source {
                SourceKind::Local => {
                    let local_source = local_source.ok_or(CutoverError::RelativeSourcePath)?;
                    let store = LocalDirectoryStore::open(&local_source)?;
                    inventory(database, &store, RunMode::InventoryLocal).await?
                }
                SourceKind::S3 => {
                    if local_source.is_some() {
                        return Err(CutoverError::S3Configuration);
                    }
                    let store = S3CompatibleStore::from_environment(ObjectKeyLayout::LegacySource)?;
                    inventory(database, &store, RunMode::InventoryS3).await?
                }
            };
            output.publish(&result)?;
            result
        }
        Command::BackfillLocalToS3 {
            sqlite,
            local_source,
            manifest,
        } => {
            let output = ManifestPublisher::reserve(&manifest)?;
            let database = read_database(&sqlite)?;
            let local = LocalDirectoryStore::open(&local_source)?;
            let destination = S3CompatibleStore::from_environment(ObjectKeyLayout::Canonical)?;
            let result = backfill_local_to_s3(database, &local, &destination).await?;
            output.publish(&result)?;
            result
        }
        Command::PromoteS3ToS3 { sqlite, manifest } => {
            let output = ManifestPublisher::reserve(&manifest)?;
            let database = read_database(&sqlite)?;
            let source = S3CompatibleStore::from_environment_with_prefix(
                ObjectKeyLayout::LegacySource,
                "OBJECT_CUTOVER_S3_PREFIX",
            )?;
            let destination = S3CompatibleStore::from_environment_with_prefix(
                ObjectKeyLayout::Canonical,
                "OBJECT_CUTOVER_DESTINATION_S3_PREFIX",
            )?;
            if destination.prefix().is_empty() || source.prefix() == destination.prefix() {
                return Err(CutoverError::S3PrefixesMustDiffer);
            }
            let result = promote_s3_to_s3(database, &source, &destination).await?;
            output.publish(&result)?;
            result
        }
    };

    if manifest.valid {
        println!(
            "object cutover verified {} referenced objects ({} external-only photos)",
            manifest.counts.present_objects, manifest.counts.external_only_photos
        );
        Ok(())
    } else {
        Err(CutoverError::ValidationBlocked)
    }
}
