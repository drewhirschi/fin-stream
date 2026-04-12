/// Monarch Money GraphQL API Client
///
/// Reverse-engineered from HAR capture of app.monarch.com.
/// API endpoint: POST https://api.monarch.com/graphql
/// Auth: Token header from login or Google OAuth session.
use reqwest::Client;
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.monarch.com/graphql";

pub struct MonarchClient {
    http: Client,
    token: String,
}

#[derive(Debug, Serialize)]
struct GraphQLRequest {
    #[serde(rename = "operationName")]
    operation_name: String,
    query: String,
    variables: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GraphQLResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct AccountData {
    account: Option<AccountBalance>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AccountBalance {
    pub id: String,
    pub display_name: String,
    pub display_balance: f64,
    pub current_balance: f64,
    pub updated_at: String,
    pub mask: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransactionsData {
    #[serde(rename = "allTransactions")]
    all_transactions: AllTransactions,
}

#[derive(Debug, Deserialize)]
struct AllTransactions {
    results: Vec<Transaction>,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub amount: f64,
    pub pending: bool,
}

impl MonarchClient {
    /// Create a client using a pre-existing session token.
    pub fn with_token(token: &str) -> anyhow::Result<Self> {
        let http = Client::builder().build()?;
        Ok(Self {
            http,
            token: token.to_string(),
        })
    }

    /// Fetch the balance for a specific account, adjusted for pending transactions.
    /// Returns the reported balance and the pending-adjusted balance.
    pub async fn get_account_balance(&self, account_id: &str) -> anyhow::Result<AccountBalance> {
        let query = r#"
            query GetAccountBalance($id: UUID!) {
                account(id: $id) {
                    id
                    displayName
                    displayBalance
                    currentBalance
                    updatedAt
                    mask
                }
            }
        "#;

        let body = GraphQLRequest {
            operation_name: "GetAccountBalance".into(),
            query: query.into(),
            variables: serde_json::json!({ "id": account_id }),
        };

        let res = self.graphql_request(&body).await?;

        let resp: GraphQLResponse<AccountData> = res.json().await?;

        if let Some(errors) = &resp.errors {
            if !errors.is_empty() {
                anyhow::bail!(
                    "Monarch GraphQL error: {}",
                    errors
                        .iter()
                        .map(|e| e.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        resp.data
            .and_then(|d| d.account)
            .ok_or_else(|| anyhow::anyhow!("No account data returned for ID {account_id}"))
    }

    /// Fetch pending transactions for a specific account.
    pub async fn get_pending_transactions(
        &self,
        account_id: &str,
    ) -> anyhow::Result<Vec<Transaction>> {
        let query = r#"
            query GetPendingTransactions($filters: TransactionFilterInput) {
                allTransactions(filters: $filters) {
                    results(offset: 0, limit: 100) {
                        id
                        amount
                        pending
                    }
                }
            }
        "#;

        let body = GraphQLRequest {
            operation_name: "GetPendingTransactions".into(),
            query: query.into(),
            variables: serde_json::json!({
                "filters": {
                    "accounts": [account_id],
                    "isPending": true,
                    "transactionVisibility": "non_hidden_transactions_only"
                }
            }),
        };

        let res = self.graphql_request(&body).await?;
        let resp: GraphQLResponse<TransactionsData> = res.json().await?;

        if let Some(errors) = &resp.errors {
            if !errors.is_empty() {
                tracing::warn!(
                    "Monarch pending transactions query had errors: {:?}",
                    errors
                );
            }
        }

        Ok(resp
            .data
            .map(|d| {
                d.all_transactions
                    .results
                    .into_iter()
                    .filter(|t| t.pending)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Fetch account balance adjusted for pending transactions.
    /// Returns (reported_balance, adjusted_balance, pending_total).
    pub async fn get_adjusted_balance(
        &self,
        account_id: &str,
    ) -> anyhow::Result<(AccountBalance, f64, f64)> {
        let balance = self.get_account_balance(account_id).await?;
        let pending = self
            .get_pending_transactions(account_id)
            .await
            .unwrap_or_default();

        let pending_total: f64 = pending.iter().map(|t| t.amount).sum();
        let adjusted = balance.current_balance + pending_total; // pending amounts are negative

        tracing::info!(
            "Monarch {} balance: ${:.2} reported, ${:.2} pending, ${:.2} adjusted",
            balance.display_name,
            balance.current_balance,
            pending_total,
            adjusted
        );

        Ok((balance, adjusted, pending_total))
    }

    /// Internal helper for making GraphQL requests with auth headers.
    async fn graphql_request(&self, body: &GraphQLRequest) -> anyhow::Result<reqwest::Response> {
        let res = self
            .http
            .post(API_URL)
            .header("Authorization", format!("Token {}", self.token))
            .header("Content-Type", "application/json")
            .header("client-platform", "web")
            .header("monarch-client", "monarch-core-web-app-graphql")
            .header("origin", "https://app.monarch.com")
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36")
            .json(body)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("Monarch API error {status}: {body}");
        }

        Ok(res)
    }
}

/// Create a Monarch client from environment variables.
pub fn create_client() -> anyhow::Result<MonarchClient> {
    let token = crate::config::monarch_token();
    MonarchClient::with_token(&token)
}
