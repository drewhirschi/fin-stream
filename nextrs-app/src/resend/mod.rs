mod client;
pub mod http;
mod repository;
mod signature;

#[cfg(all(test, feature = "local-db"))]
mod tests;

pub use client::{AttachmentDownload, ProviderAttachment, ProviderEmail, ResendProvider};
pub use repository::{
    ClaimOutcome, ClaimedEmail, InboundAttachmentDraft, InboundEmailDraft, InboundEmailRepository,
};
pub use signature::{SignatureError, WebhookVerifier};

use std::{env, fmt, sync::Arc};

use anyhow::Context;

use client::ResendClient;

#[derive(Clone)]
pub struct ResendService {
    verifier: Option<Arc<WebhookVerifier>>,
    provider: Option<Arc<dyn ResendProvider>>,
}

impl fmt::Debug for ResendService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResendService")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

impl ResendService {
    pub fn from_env() -> anyhow::Result<Self> {
        let webhook_secret = optional_env("RESEND_WEBHOOK_SECRET");
        let api_key = optional_env("RESEND_API_KEY");
        let remote_production = cfg!(all(feature = "remote-db", not(feature = "local-db")))
            && env::var("VERCEL_ENV")
                .or_else(|_| env::var("APP_ENV"))
                .is_ok_and(|value| value.eq_ignore_ascii_case("production"));
        resolve_service(webhook_secret, api_key, remote_production)
    }

    pub fn disabled() -> Self {
        Self {
            verifier: None,
            provider: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.verifier.is_some() && self.provider.is_some()
    }

    pub(crate) fn verifier(&self) -> Option<&WebhookVerifier> {
        self.verifier.as_deref()
    }

    pub(crate) fn provider(&self) -> Option<&Arc<dyn ResendProvider>> {
        self.provider.as_ref()
    }

    #[cfg(all(test, feature = "local-db"))]
    pub(crate) fn for_test(secret: &str, provider: Arc<dyn ResendProvider>) -> Self {
        Self {
            verifier: Some(Arc::new(WebhookVerifier::new(secret).unwrap())),
            provider: Some(provider),
        }
    }
}

fn resolve_service(
    webhook_secret: Option<String>,
    api_key: Option<String>,
    require_complete: bool,
) -> anyhow::Result<ResendService> {
    if require_complete && (webhook_secret.is_none() || api_key.is_none()) {
        anyhow::bail!(
            "RESEND_WEBHOOK_SECRET and RESEND_API_KEY are required for remote production"
        );
    }
    match (webhook_secret, api_key) {
        (None, None) => Ok(ResendService::disabled()),
        (Some(webhook_secret), Some(api_key)) => Ok(ResendService {
            verifier: Some(Arc::new(
                WebhookVerifier::new(&webhook_secret).context("invalid RESEND_WEBHOOK_SECRET")?,
            )),
            provider: Some(Arc::new(
                ResendClient::new(&api_key).context("configure Resend API client")?,
            )),
        }),
        _ => anyhow::bail!("RESEND_WEBHOOK_SECRET and RESEND_API_KEY must be configured together"),
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
