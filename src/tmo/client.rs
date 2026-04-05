/// The Mortgage Office API Client
///
/// Reverse-engineered from HAR capture of lenders.themortgageoffice.com.
/// API base: https://lvcprod.themortgageoffice.com
/// Auth: session-based (cookies after POST /api/login)
use reqwest::Client;
use std::sync::Arc;

use crate::models::*;

const BASE_URL: &str = "https://lvcprod.themortgageoffice.com";

pub struct TmoClient {
    http: Client,
}

impl TmoClient {
    /// Login and return a client with session cookies.
    pub async fn login(company_id: &str, account: &str, pin: &str) -> anyhow::Result<Self> {
        // reqwest cookie_store handles session cookies automatically
        let http = Client::builder()
            .cookie_store(true)
            .build()?;

        let body = serde_json::json!({
            "companyId": company_id,
            "account": account,
            "pin": pin,
        });

        let res = http
            .post(format!("{BASE_URL}/api/login"))
            .header("Content-Type", "application/json")
            .header("Origin", "https://lenders.themortgageoffice.com")
            .header("Referer", "https://lenders.themortgageoffice.com/")
            .json(&body)
            .send()
            .await?;

        let resp: TmoResponse<TmoLoginData> = res.json().await?;

        if !resp.success || !resp.data.is_valid_user {
            anyhow::bail!(
                "TMO login failed: {}",
                resp.error
                    .or(resp.data.message)
                    .unwrap_or_else(|| "invalid credentials".into())
            );
        }

        tracing::info!("TMO login successful for account {}", account);
        Ok(Self { http })
    }

    pub async fn get_overview(&self) -> anyhow::Result<TmoOverview> {
        let resp: TmoResponse<TmoOverview> = self
            .get("/api/overview?showPaidOffLoans=false")
            .await?;
        Ok(resp.data)
    }

    pub async fn get_portfolio(&self) -> anyhow::Result<Vec<TmoLoanSummary>> {
        let request = serde_json::json!({
            "filters": { "showPaidOffLoans": false },
            "params": {
                "page": 1,
                "rowsPerPage": 100,
                "order": { "name": "loanAccount", "direction": "asc" }
            }
        });

        let resp: TmoResponse<TmoPaginatedResponse<TmoLoanSummary>> = self
            .get(&format!(
                "/api/portfolio/getPortfolioData?request={}",
                urlencoding::encode(&request.to_string())
            ))
            .await?;

        Ok(resp.data.data)
    }

    pub async fn get_loan_detail(&self, loan_account: &str) -> anyhow::Result<TmoLoanDetail> {
        let resp: TmoResponse<TmoLoanDetail> = self
            .get(&format!("/api/loanDetail/getLoanDetail/{loan_account}"))
            .await?;
        Ok(resp.data)
    }

    pub async fn get_history(&self, loan_account: Option<&str>) -> anyhow::Result<Vec<TmoPayment>> {
        let request = serde_json::json!({
            "filters": {
                "loanAccount": loan_account,
                "startDate": "1900-01-01T07:00:00.000Z",
                "endDate": "9999-12-31T07:00:00.000Z",
            },
            "params": {
                "page": 1,
                "rowsPerPage": 1000,
                "order": { "name": "checkDate", "direction": "desc" }
            }
        });

        let resp: TmoResponse<TmoPaginatedResponse<TmoPayment>> = self
            .get(&format!(
                "/api/history?request={}",
                urlencoding::encode(&request.to_string())
            ))
            .await?;

        Ok(resp.data.data)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let res = self
            .http
            .get(format!("{BASE_URL}{path}"))
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://lenders.themortgageoffice.com")
            .header("Referer", "https://lenders.themortgageoffice.com/")
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("TMO API error: {} on {}", res.status(), path);
        }

        let body = res.json::<T>().await?;
        Ok(body)
    }
}

/// Create a shared TMO client (login once, reuse for the sync).
pub async fn create_client() -> anyhow::Result<Arc<TmoClient>> {
    let company_id = crate::config::tmo_company_id();
    let account = crate::config::tmo_account();
    let pin = crate::config::tmo_pin();

    let client = TmoClient::login(&company_id, &account, &pin).await?;
    Ok(Arc::new(client))
}
