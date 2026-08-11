//! The Mortgage Office HTTP boundary.
//!
//! Authentication is the provider's JSON login endpoint plus its session
//! cookie. There is no browser automation or BrowserBase dependency.

use std::fmt;

use reqwest::{Client, Url, header::HeaderValue};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroize;

use super::{
    HttpSettings, ProviderError, ProviderName, ProviderResult,
    http::{build_client, request_error, response_json},
};

const DEFAULT_API_BASE_URL: &str = "https://lvcprod.themortgageoffice.com/";
const DEFAULT_WEB_ORIGIN: &str = "https://lenders.themortgageoffice.com";

#[derive(Clone)]
pub struct TmoCredentials {
    pub company_id: String,
    pub account: String,
    pub pin: String,
}

impl fmt::Debug for TmoCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TmoCredentials")
            .field("company_id", &self.company_id)
            .field("account", &"[REDACTED]")
            .field("pin", &"[REDACTED]")
            .finish()
    }
}

impl Drop for TmoCredentials {
    fn drop(&mut self) {
        self.account.zeroize();
        self.pin.zeroize();
    }
}

impl TmoCredentials {
    pub fn new(
        company_id: impl Into<String>,
        account: impl Into<String>,
        pin: impl Into<String>,
    ) -> Self {
        Self {
            company_id: company_id.into(),
            account: account.into(),
            pin: pin.into(),
        }
    }

    fn is_valid(&self) -> bool {
        [&self.company_id, &self.account, &self.pin]
            .into_iter()
            .all(|value| !value.trim().is_empty())
    }
}

#[derive(Clone, Debug)]
pub struct TmoClientOptions {
    pub api_base_url: String,
    pub web_origin: String,
    pub http: HttpSettings,
}

impl Default for TmoClientOptions {
    fn default() -> Self {
        Self {
            api_base_url: DEFAULT_API_BASE_URL.to_owned(),
            web_origin: DEFAULT_WEB_ORIGIN.to_owned(),
            http: HttpSettings::default(),
        }
    }
}

pub struct TmoClient {
    http: Client,
    api_base_url: Url,
    web_origin: HeaderValue,
    web_referer: HeaderValue,
    max_response_bytes: usize,
    user: TmoUserInfo,
}

impl TmoClient {
    pub async fn login(credentials: &TmoCredentials) -> ProviderResult<Self> {
        Self::login_with_options(credentials, TmoClientOptions::default()).await
    }

    /// Login against an injectable endpoint. Production callers normally use
    /// [`Self::login`]; tests and canaries can point this at a local server.
    pub async fn login_with_options(
        credentials: &TmoCredentials,
        options: TmoClientOptions,
    ) -> ProviderResult<Self> {
        if !credentials.is_valid() {
            return Err(ProviderError::InvalidConfiguration {
                provider: ProviderName::Tmo,
            });
        }
        let api_base_url = validated_base_url(&options.api_base_url)?;
        let web_origin = HeaderValue::from_str(options.web_origin.trim()).map_err(|_| {
            ProviderError::InvalidConfiguration {
                provider: ProviderName::Tmo,
            }
        })?;
        let referer = format!("{}/", options.web_origin.trim().trim_end_matches('/'));
        let web_referer =
            HeaderValue::from_str(&referer).map_err(|_| ProviderError::InvalidConfiguration {
                provider: ProviderName::Tmo,
            })?;
        let http = build_client(ProviderName::Tmo, options.http, true)?;

        let login_url = endpoint(&api_base_url, &["api", "login"])?;
        let response = http
            .post(login_url)
            .header("origin", web_origin.clone())
            .header("referer", web_referer.clone())
            .json(&TmoLoginRequest {
                company_id: &credentials.company_id,
                account: &credentials.account,
                pin: &credentials.pin,
            })
            .send()
            .await
            .map_err(|error| request_error(ProviderName::Tmo, error))?;
        let envelope: TmoResponse<TmoLoginData> =
            response_json(ProviderName::Tmo, response, options.http.max_response_bytes).await?;
        if !envelope.success {
            return Err(ProviderError::AuthenticationRejected {
                provider: ProviderName::Tmo,
            });
        }
        let login = envelope.data.ok_or(ProviderError::InvalidResponse {
            provider: ProviderName::Tmo,
        })?;
        if !login.is_valid_user || login.requires_mfa {
            return Err(ProviderError::AuthenticationRejected {
                provider: ProviderName::Tmo,
            });
        }
        let user = login
            .user_information
            .ok_or(ProviderError::InvalidResponse {
                provider: ProviderName::Tmo,
            })?;

        Ok(Self {
            http,
            api_base_url,
            web_origin,
            web_referer,
            max_response_bytes: options.http.max_response_bytes,
            user,
        })
    }

    pub fn user(&self) -> &TmoUserInfo {
        &self.user
    }

    pub async fn get_overview(&self) -> ProviderResult<TmoOverview> {
        self.get_data(&["api", "overview"], Some(&[("showPaidOffLoans", "false")]))
            .await
    }

    pub async fn get_portfolio(&self) -> ProviderResult<Vec<TmoLoanSummary>> {
        const ROWS_PER_PAGE: i32 = 100;
        let mut page = 1_i32;
        let mut expected_total = None;
        let mut loans = Vec::new();

        loop {
            let request = serde_json::json!({
                "filters": { "showPaidOffLoans": false },
                "params": {
                    "page": page,
                    "rowsPerPage": ROWS_PER_PAGE,
                    "order": { "name": "loanAccount", "direction": "asc" }
                }
            });
            let request = request.to_string();
            let response: TmoPaginatedResponse<TmoLoanSummary> = self
                .get_data(
                    &["api", "portfolio", "getPortfolioData"],
                    Some(&[("request", request.as_str())]),
                )
                .await?;
            if append_paginated_page(&mut loans, &mut expected_total, response, page)? {
                return Ok(loans);
            }
            page = page.checked_add(1).ok_or(ProviderError::InvalidResponse {
                provider: ProviderName::Tmo,
            })?;
        }
    }

    pub async fn get_loan_detail(&self, loan_account: &str) -> ProviderResult<TmoLoanDetail> {
        if loan_account.trim().is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                provider: ProviderName::Tmo,
            });
        }
        self.get_data(&["api", "loanDetail", "getLoanDetail", loan_account], None)
            .await
    }

    pub async fn get_history(&self, loan_account: Option<&str>) -> ProviderResult<Vec<TmoPayment>> {
        let request = serde_json::json!({
            "filters": {
                "loanAccount": loan_account,
                "startDate": "1900-01-01T07:00:00.000Z",
                "endDate": "9999-12-31T07:00:00.000Z"
            },
            "params": {
                "page": 1,
                "rowsPerPage": 1000,
                "order": { "name": "checkDate", "direction": "desc" }
            }
        });
        let request = request.to_string();
        let response: TmoPaginatedResponse<TmoPayment> = self
            .get_data(&["api", "history"], Some(&[("request", request.as_str())]))
            .await?;
        Ok(response.data)
    }

    async fn get_data<T>(&self, path: &[&str], query: Option<&[(&str, &str)]>) -> ProviderResult<T>
    where
        T: DeserializeOwned,
    {
        let mut url = endpoint(&self.api_base_url, path)?;
        if let Some(query) = query {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let response = self
            .http
            .get(url)
            .header("accept", "application/json, text/plain, */*")
            .header("origin", self.web_origin.clone())
            .header("referer", self.web_referer.clone())
            .send()
            .await
            .map_err(|error| request_error(ProviderName::Tmo, error))?;
        let envelope: TmoResponse<T> =
            response_json(ProviderName::Tmo, response, self.max_response_bytes).await?;
        if !envelope.success {
            return Err(ProviderError::RequestRejected {
                provider: ProviderName::Tmo,
            });
        }
        envelope.data.ok_or(ProviderError::MissingData {
            provider: ProviderName::Tmo,
        })
    }
}

fn validated_base_url(raw: &str) -> ProviderResult<Url> {
    let mut url = Url::parse(raw.trim()).map_err(|_| ProviderError::InvalidConfiguration {
        provider: ProviderName::Tmo,
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.cannot_be_a_base()
    {
        return Err(ProviderError::InvalidConfiguration {
            provider: ProviderName::Tmo,
        });
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn endpoint(base: &Url, segments: &[&str]) -> ProviderResult<Url> {
    let mut url = base.clone();
    url.path_segments_mut()
        .map_err(|_| ProviderError::InvalidConfiguration {
            provider: ProviderName::Tmo,
        })?
        .pop_if_empty()
        .extend(segments.iter().copied());
    Ok(url)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TmoLoginRequest<'a> {
    company_id: &'a str,
    account: &'a str,
    pin: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TmoResponse<T> {
    pub data: Option<T>,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "errorType")]
    pub error_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoginData {
    pub is_valid_user: bool,
    #[serde(default)]
    pub user_information: Option<TmoUserInfo>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub requires_mfa: bool,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoUserInfo {
    pub source_rec_id: String,
    pub company_id: String,
    pub account: String,
    pub name: String,
    pub email: String,
}

impl fmt::Debug for TmoUserInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TmoUserInfo")
            .field("source_rec_id", &self.source_rec_id)
            .field("company_id", &self.company_id)
            .field("account", &"[REDACTED]")
            .field("name", &self.name)
            .field("email", &self.email)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoPaginatedResponse<T> {
    pub page: i32,
    pub rows_per_page: i32,
    pub total_count: i32,
    pub data: Vec<T>,
}

fn append_paginated_page<T>(
    rows: &mut Vec<T>,
    expected_total: &mut Option<usize>,
    response: TmoPaginatedResponse<T>,
    requested_page: i32,
) -> ProviderResult<bool> {
    let total_count =
        usize::try_from(response.total_count).map_err(|_| ProviderError::InvalidResponse {
            provider: ProviderName::Tmo,
        })?;
    let rows_per_page =
        usize::try_from(response.rows_per_page).map_err(|_| ProviderError::InvalidResponse {
            provider: ProviderName::Tmo,
        })?;
    if response.page != requested_page
        || rows_per_page == 0
        || response.data.len() > rows_per_page
        || expected_total.is_some_and(|expected| expected != total_count)
        || (response.data.is_empty() && rows.len() < total_count)
    {
        return Err(ProviderError::InvalidResponse {
            provider: ProviderName::Tmo,
        });
    }

    *expected_total = Some(total_count);
    rows.extend(response.data);
    if rows.len() > total_count {
        return Err(ProviderError::InvalidResponse {
            provider: ProviderName::Tmo,
        });
    }
    Ok(rows.len() == total_count)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoanSummary {
    pub loan_account: String,
    pub borrower_name: String,
    pub primary_street: String,
    pub primary_city: String,
    pub primary_state: String,
    pub primary_zip: String,
    pub percent_owned: f64,
    pub interest_rate: f64,
    pub maturity_date: String,
    pub term_left: i32,
    pub next_payment_date: String,
    pub interest_paid_to_date: String,
    pub billed_through: Option<String>,
    pub regular_payment: f64,
    pub loan_balance: f64,
    pub is_delinquent: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoanDetail {
    pub loan_account: String,
    pub borrower_name: String,
    pub primary_street: String,
    pub primary_city: String,
    pub primary_state: String,
    pub primary_zip: String,
    pub property_description: Option<String>,
    pub property_type: Option<String>,
    pub property_priority: Option<i32>,
    pub occupancy: Option<String>,
    pub ltv: Option<f64>,
    pub appraised_value: Option<f64>,
    pub priority: Option<i32>,
    pub original_balance: f64,
    pub principal_balance: f64,
    pub note_rate: f64,
    pub maturity_date: String,
    pub next_payment_date: String,
    pub interest_paid_to_date: String,
    pub regular_payment: f64,
    pub payment_frequency: String,
    pub loan_type: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoPayment {
    pub check_number: String,
    pub loan_account: String,
    pub check_date: String,
    pub amount: f64,
    pub service_fee: f64,
    pub interest: f64,
    pub principal: f64,
    pub charges: f64,
    pub late_charges: f64,
    pub other: f64,
    pub borrower_name: String,
    pub property_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoOverview {
    pub portfolio_value: f64,
    pub portfolio_yield: f64,
    pub ytd_interest: f64,
    pub ytd_principal: f64,
    pub portfolio_count: i32,
    pub trust_balance: f64,
    pub outstanding_checks_value: f64,
    pub ytd_serv_fees: f64,
}

/// `None` means TMO has not issued a real check number yet.
pub fn normalize_check_number(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.to_ascii_lowercase().contains("print") {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{
        TmoClient, TmoClientOptions, TmoCredentials, TmoPaginatedResponse, TmoResponse,
        TmoUserInfo, append_paginated_page, normalize_check_number,
    };
    use crate::providers::{HttpSettings, ProviderError, ProviderName};

    async fn mock_url(response: &'static [u8], delay: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 8 * 1_024];
            let _ = stream.read(&mut request).await;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let _ = stream.write_all(response).await;
        });
        format!("http://{address}/")
    }

    fn options(api_base_url: String, request_timeout: Duration) -> TmoClientOptions {
        TmoClientOptions {
            api_base_url,
            web_origin: "http://localhost".into(),
            http: HttpSettings {
                connect_timeout: Duration::from_secs(1),
                request_timeout,
                max_response_bytes: 1_024,
            },
        }
    }

    fn credentials() -> TmoCredentials {
        TmoCredentials::new("vci", "secret-account", "secret-pin")
    }

    #[test]
    fn decodes_legacy_login_fixture() {
        let response: TmoResponse<super::TmoLoginData> =
            serde_json::from_str(include_str!("fixtures/tmo_login_success.json"))
                .expect("fixture decodes");
        assert!(response.success);
        let login = response.data.expect("login data");
        assert!(login.is_valid_user);
        assert_eq!(
            login.user_information,
            Some(TmoUserInfo {
                source_rec_id: "source-42".into(),
                company_id: "vci".into(),
                account: "10001".into(),
                name: "Fixture Lender".into(),
                email: "lender@example.com".into(),
            })
        );
    }

    #[test]
    fn check_number_normalization_matches_legacy_behavior() {
        assert_eq!(normalize_check_number(" Print Check "), None);
        assert_eq!(normalize_check_number(" 12345 "), Some("12345".into()));
    }

    #[test]
    fn portfolio_pagination_collects_more_than_one_page_and_rejects_gaps() {
        let mut rows = Vec::new();
        let mut expected_total = None;
        assert!(
            !append_paginated_page(
                &mut rows,
                &mut expected_total,
                TmoPaginatedResponse {
                    page: 1,
                    rows_per_page: 100,
                    total_count: 101,
                    data: (0..100).collect(),
                },
                1,
            )
            .unwrap()
        );
        assert!(
            append_paginated_page(
                &mut rows,
                &mut expected_total,
                TmoPaginatedResponse {
                    page: 2,
                    rows_per_page: 100,
                    total_count: 101,
                    data: vec![100],
                },
                2,
            )
            .unwrap()
        );
        assert_eq!(rows.len(), 101);

        let mut incomplete = Vec::<i32>::new();
        let mut incomplete_total = None;
        assert!(
            append_paginated_page(
                &mut incomplete,
                &mut incomplete_total,
                TmoPaginatedResponse {
                    page: 1,
                    rows_per_page: 100,
                    total_count: 1,
                    data: Vec::new(),
                },
                1,
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn login_non_success_response_is_redacted() {
        const RESPONSE: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 41\r\nConnection: close\r\n\r\nsecret-pin and secret-account in response";
        let url = mock_url(RESPONSE, Duration::ZERO).await;
        let error =
            TmoClient::login_with_options(&credentials(), options(url, Duration::from_secs(1)))
                .await
                .err()
                .expect("login should fail");
        assert_eq!(
            error,
            ProviderError::HttpStatus {
                provider: ProviderName::Tmo,
                status: 403,
            }
        );
        let rendered = format!("{error:?} {error} {:?}", credentials());
        assert!(!rendered.contains("secret-pin"));
        assert!(!rendered.contains("secret-account"));
    }

    #[tokio::test]
    async fn login_timeout_is_sanitized() {
        const RESPONSE: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let url = mock_url(RESPONSE, Duration::from_millis(200)).await;
        let error =
            TmoClient::login_with_options(&credentials(), options(url, Duration::from_millis(25)))
                .await
                .err()
                .expect("login should time out");
        assert_eq!(
            error,
            ProviderError::Timeout {
                provider: ProviderName::Tmo,
            }
        );
        assert!(!format!("{error:?} {error}").contains("secret-pin"));
    }
}
