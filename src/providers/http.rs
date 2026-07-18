use std::time::Duration;

use reqwest::{Client, Response, redirect::Policy};
use serde::de::DeserializeOwned;

use super::{ProviderError, ProviderName, ProviderResult};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 4 * 1_024 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpSettings {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_response_bytes: usize,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

impl HttpSettings {
    pub(crate) fn validate(self, provider: ProviderName) -> ProviderResult<Self> {
        if self.connect_timeout.is_zero()
            || self.request_timeout.is_zero()
            || self.max_response_bytes == 0
        {
            return Err(ProviderError::InvalidConfiguration { provider });
        }
        Ok(self)
    }
}

pub(crate) fn build_client(
    provider: ProviderName,
    settings: HttpSettings,
    cookie_store: bool,
) -> ProviderResult<Client> {
    let settings = settings.validate(provider)?;
    Client::builder()
        .connect_timeout(settings.connect_timeout)
        .timeout(settings.request_timeout)
        // Provider calls should not silently move credentials to a redirect
        // target. A redirect is surfaced as its 3xx status instead.
        .redirect(Policy::none())
        .cookie_store(cookie_store)
        .user_agent("trust-deeds/1 provider-client")
        .build()
        .map_err(|_| ProviderError::InvalidConfiguration { provider })
}

pub(crate) fn request_error(provider: ProviderName, error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout { provider }
    } else {
        ProviderError::Transport { provider }
    }
}

pub(crate) async fn response_json<T>(
    provider: ProviderName,
    mut response: Response,
    max_response_bytes: usize,
) -> ProviderResult<T>
where
    T: DeserializeOwned,
{
    if !response.status().is_success() {
        return Err(ProviderError::HttpStatus {
            provider,
            status: response.status().as_u16(),
        });
    }

    if response
        .content_length()
        .is_some_and(|length| length > max_response_bytes as u64)
    {
        return Err(ProviderError::ResponseTooLarge {
            provider,
            limit_bytes: max_response_bytes,
        });
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(max_response_bytes),
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| request_error(provider, error))?
    {
        let next_length =
            body.len()
                .checked_add(chunk.len())
                .ok_or(ProviderError::ResponseTooLarge {
                    provider,
                    limit_bytes: max_response_bytes,
                })?;
        if next_length > max_response_bytes {
            return Err(ProviderError::ResponseTooLarge {
                provider,
                limit_bytes: max_response_bytes,
            });
        }
        body.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&body).map_err(|_| ProviderError::InvalidResponse { provider })
}
