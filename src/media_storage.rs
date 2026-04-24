use anyhow::Context;
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client, primitives::ByteStream};
use aws_types::region::Region;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::config;

#[derive(Clone)]
pub enum MediaStorage {
    Local {
        base_dir: PathBuf,
        base_url: String,
    },
    S3 {
        client: Client,
        bucket: String,
        key_prefix: String,
    },
}

pub struct StoredMedia {
    pub public_url: String,
}

pub struct RetrievedMedia {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

impl MediaStorage {
    pub async fn from_env() -> anyhow::Result<Self> {
        let endpoint = config::s3_endpoint();
        let access_key = config::s3_access_key();
        let secret_key = config::s3_secret_key();
        let bucket = config::s3_bucket();

        let has_partial_s3_config =
            endpoint.is_some() || access_key.is_some() || secret_key.is_some() || bucket.is_some();

        if let (Some(client), Some(bucket)) = (
            build_s3_client(endpoint, access_key, secret_key).await?,
            bucket.clone(),
        ) {
            return Ok(Self::S3 {
                client,
                bucket,
                key_prefix: config::s3_key_prefix(),
            });
        }

        if has_partial_s3_config {
            tracing::warn!(
                "partial S3 configuration detected; falling back to local loan image storage"
            );
        }

        Ok(Self::Local {
            base_dir: config::loan_image_storage_dir(),
            base_url: config::loan_image_base_url(),
        })
    }

    pub async fn store(
        &self,
        object_key: &str,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> anyhow::Result<StoredMedia> {
        match self {
            Self::Local { base_dir, base_url } => {
                let path = base_dir.join(object_key);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::write(&path, bytes)
                    .await
                    .with_context(|| format!("writing media to {}", path.display()))?;

                Ok(StoredMedia {
                    public_url: format!(
                        "{}/{}",
                        base_url.trim_end_matches('/'),
                        object_key.trim_start_matches('/')
                    ),
                })
            }
            Self::S3 {
                client,
                bucket,
                key_prefix,
            } => {
                let key = build_s3_key(key_prefix, object_key);
                let mut request = client
                    .put_object()
                    .bucket(bucket)
                    .key(&key)
                    .body(ByteStream::from(bytes));

                if let Some(content_type) = content_type {
                    request = request.content_type(content_type);
                }

                request.send().await?;

                Ok(StoredMedia {
                    public_url: format!("/media/loan-workspace/{}", object_key),
                })
            }
        }
    }

    /// Delete an object by key. Idempotent — missing objects return Ok(()).
    pub async fn delete(&self, object_key: &str) -> anyhow::Result<()> {
        match self {
            Self::Local { base_dir, .. } => {
                let path = base_dir.join(object_key);
                match fs::remove_file(&path).await {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
                }
            }
            Self::S3 {
                client,
                bucket,
                key_prefix,
            } => {
                let key = build_s3_key(key_prefix, object_key);
                client
                    .delete_object()
                    .bucket(bucket)
                    .key(&key)
                    .send()
                    .await
                    .with_context(|| format!("S3 delete {}", key))?;
                Ok(())
            }
        }
    }

    pub async fn get(&self, object_key: &str) -> anyhow::Result<Option<RetrievedMedia>> {
        match self {
            Self::Local { base_dir, .. } => {
                let path = base_dir.join(object_key);
                if !Path::new(&path).exists() {
                    return Ok(None);
                }

                let bytes = fs::read(&path)
                    .await
                    .with_context(|| format!("reading media from {}", path.display()))?;
                Ok(Some(RetrievedMedia {
                    bytes,
                    content_type: guess_content_type(&path).map(str::to_string),
                }))
            }
            Self::S3 {
                client,
                bucket,
                key_prefix,
            } => {
                let key = build_s3_key(key_prefix, object_key);
                let response = match client.get_object().bucket(bucket).key(key).send().await {
                    Ok(response) => response,
                    Err(error) => {
                        let message = error.to_string();
                        if message.contains("NoSuchKey") || message.contains("not found") {
                            return Ok(None);
                        }
                        return Err(error.into());
                    }
                };

                let content_type = response.content_type().map(ToString::to_string);
                let bytes = response.body.collect().await?.into_bytes().to_vec();

                Ok(Some(RetrievedMedia {
                    bytes,
                    content_type,
                }))
            }
        }
    }

    pub async fn list_buckets(&self) -> anyhow::Result<Vec<String>> {
        match self {
            Self::S3 { client, .. } => {
                let response = client.list_buckets().send().await?;
                Ok(response
                    .buckets()
                    .iter()
                    .filter_map(|bucket| bucket.name().map(ToString::to_string))
                    .collect())
            }
            Self::Local { .. } => Ok(Vec::new()),
        }
    }
}

pub async fn list_configured_buckets_from_env() -> anyhow::Result<Vec<String>> {
    let client = build_s3_client(
        config::s3_endpoint(),
        config::s3_access_key(),
        config::s3_secret_key(),
    )
    .await?
    .context("S3 credentials are incomplete; set endpoint/access-key/secret-key first")?;

    let response = client.list_buckets().send().await?;
    Ok(response
        .buckets()
        .iter()
        .filter_map(|bucket| bucket.name().map(ToString::to_string))
        .collect())
}

async fn build_s3_client(
    endpoint: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> anyhow::Result<Option<Client>> {
    let (Some(endpoint), Some(access_key), Some(secret_key)) = (endpoint, access_key, secret_key)
    else {
        return Ok(None);
    };

    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(config::s3_region()))
        .credentials_provider(Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "trust-deeds-env",
        ))
        .load()
        .await;

    Ok(Some(Client::from_conf(
        aws_sdk_s3::config::Builder::from(&shared_config)
            .endpoint_url(endpoint)
            .force_path_style(true)
            .build(),
    )))
}

fn build_s3_key(prefix: &str, object_key: &str) -> String {
    format!(
        "{}/{}",
        prefix.trim_matches('/'),
        object_key.trim_start_matches('/')
    )
}

fn guess_content_type(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}
