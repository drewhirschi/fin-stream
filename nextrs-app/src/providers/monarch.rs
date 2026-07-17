//! Monarch Money GraphQL boundary using an existing API token.
//!
//! The token is sent directly to the documented-by-observation GraphQL
//! endpoint. No browser session or browser automation is involved.

use reqwest::{Client, Url, header::HeaderValue};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    HttpSettings, ProviderError, ProviderName, ProviderResult,
    http::{build_client, request_error, response_json},
};

const DEFAULT_GRAPHQL_URL: &str = "https://api.monarch.com/graphql";

#[derive(Clone, Debug)]
pub struct MonarchClientOptions {
    pub graphql_url: String,
    pub http: HttpSettings,
}

impl Default for MonarchClientOptions {
    fn default() -> Self {
        Self {
            graphql_url: DEFAULT_GRAPHQL_URL.to_owned(),
            http: HttpSettings::default(),
        }
    }
}

pub struct MonarchClient {
    http: Client,
    graphql_url: Url,
    authorization: HeaderValue,
    max_response_bytes: usize,
}

impl MonarchClient {
    pub fn with_token(token: &str) -> ProviderResult<Self> {
        Self::with_options(token, MonarchClientOptions::default())
    }

    /// Create a client with an injectable GraphQL URL for tests and canaries.
    pub fn with_options(token: &str, options: MonarchClientOptions) -> ProviderResult<Self> {
        if token.trim().is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                provider: ProviderName::Monarch,
            });
        }
        let graphql_url = validated_url(&options.graphql_url)?;
        let mut authorization =
            HeaderValue::from_str(&format!("Token {}", token.trim())).map_err(|_| {
                ProviderError::InvalidConfiguration {
                    provider: ProviderName::Monarch,
                }
            })?;
        authorization.set_sensitive(true);
        let http = build_client(ProviderName::Monarch, options.http, false)?;
        Ok(Self {
            http,
            graphql_url,
            authorization,
            max_response_bytes: options.http.max_response_bytes,
        })
    }

    pub async fn get_account_balance(&self, account_id: &str) -> ProviderResult<AccountBalance> {
        if account_id.trim().is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                provider: ProviderName::Monarch,
            });
        }
        let data: AccountData = self
            .graphql(GraphQlRequest {
                operation_name: "GetAccountBalance",
                query: r#"
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
                "#,
                variables: serde_json::json!({ "id": account_id }),
            })
            .await?;
        data.account.ok_or(ProviderError::MissingData {
            provider: ProviderName::Monarch,
        })
    }

    pub async fn get_pending_transactions(
        &self,
        account_id: &str,
    ) -> ProviderResult<Vec<Transaction>> {
        if account_id.trim().is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                provider: ProviderName::Monarch,
            });
        }
        let data: TransactionsData = self
            .graphql(GraphQlRequest {
                operation_name: "GetPendingTransactions",
                query: r#"
                    query GetPendingTransactions($filters: TransactionFilterInput) {
                        allTransactions(filters: $filters) {
                            results(offset: 0, limit: 100) {
                                id
                                amount
                                pending
                            }
                        }
                    }
                "#,
                variables: serde_json::json!({
                    "filters": {
                        "accounts": [account_id],
                        "isPending": true,
                        "transactionVisibility": "non_hidden_transactions_only"
                    }
                }),
            })
            .await?;
        Ok(data
            .all_transactions
            .results
            .into_iter()
            .filter(|transaction| transaction.pending)
            .collect())
    }

    /// Return the provider account, pending-adjusted balance, and pending
    /// total. Monarch pending debits are negative, matching the legacy rule.
    pub async fn get_adjusted_balance(
        &self,
        account_id: &str,
    ) -> ProviderResult<(AccountBalance, f64, f64)> {
        let balance = self.get_account_balance(account_id).await?;
        let pending = self.get_pending_transactions(account_id).await?;
        let pending_total = pending
            .iter()
            .map(|transaction| transaction.amount)
            .sum::<f64>();
        let adjusted = balance.current_balance + pending_total;
        Ok((balance, adjusted, pending_total))
    }

    async fn graphql<T>(&self, body: GraphQlRequest<'_>) -> ProviderResult<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .http
            .post(self.graphql_url.clone())
            .header("authorization", self.authorization.clone())
            .header("content-type", "application/json")
            .header("client-platform", "web")
            .header("monarch-client", "monarch-core-web-app-graphql")
            .header("origin", "https://app.monarch.com")
            .json(&body)
            .send()
            .await
            .map_err(|error| request_error(ProviderName::Monarch, error))?;
        let response: GraphQlResponse<T> =
            response_json(ProviderName::Monarch, response, self.max_response_bytes).await?;
        if response
            .errors
            .as_ref()
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(ProviderError::RequestRejected {
                provider: ProviderName::Monarch,
            });
        }
        response.data.ok_or(ProviderError::MissingData {
            provider: ProviderName::Monarch,
        })
    }
}

fn validated_url(raw: &str) -> ProviderResult<Url> {
    let url = Url::parse(raw.trim()).map_err(|_| ProviderError::InvalidConfiguration {
        provider: ProviderName::Monarch,
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::InvalidConfiguration {
            provider: ProviderName::Monarch,
        });
    }
    Ok(url)
}

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    #[serde(rename = "operationName")]
    operation_name: &'a str,
    query: &'a str,
    variables: serde_json::Value,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct AccountData {
    account: Option<AccountBalance>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountBalance {
    pub id: String,
    pub display_name: String,
    pub display_balance: f64,
    pub current_balance: f64,
    pub updated_at: String,
    pub mask: Option<String>,
}

#[derive(Deserialize)]
struct TransactionsData {
    #[serde(rename = "allTransactions")]
    all_transactions: AllTransactions,
}

#[derive(Deserialize)]
struct AllTransactions {
    results: Vec<Transaction>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Transaction {
    pub id: String,
    pub amount: f64,
    pub pending: bool,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{AccountData, GraphQlResponse, MonarchClient, MonarchClientOptions};
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
        format!("http://{address}/graphql")
    }

    fn options(graphql_url: String, request_timeout: Duration) -> MonarchClientOptions {
        MonarchClientOptions {
            graphql_url,
            http: HttpSettings {
                connect_timeout: Duration::from_secs(1),
                request_timeout,
                max_response_bytes: 1_024,
            },
        }
    }

    #[test]
    fn decodes_account_dto_fixture() {
        let response: GraphQlResponse<AccountData> =
            serde_json::from_str(include_str!("fixtures/monarch_account_success.json")).unwrap();
        let account = response.data.unwrap().account.unwrap();
        assert_eq!(account.id, "account-42");
        assert_eq!(account.display_name, "Trust Account");
        assert_eq!(account.current_balance, 12_345.67);
        assert_eq!(account.mask.as_deref(), Some("6789"));
    }

    #[tokio::test]
    async fn non_success_response_is_redacted() {
        const RESPONSE: &[u8] = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 45\r\nConnection: close\r\n\r\nsecret response body: token-should-not-appear";
        let url = mock_url(RESPONSE, Duration::ZERO).await;
        let client =
            MonarchClient::with_options("super-secret-token", options(url, Duration::from_secs(1)))
                .unwrap();
        let error = client.get_account_balance("account-1").await.unwrap_err();
        assert_eq!(
            error,
            ProviderError::HttpStatus {
                provider: ProviderName::Monarch,
                status: 401,
            }
        );
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(!rendered.contains("token-should-not-appear"));
    }

    #[tokio::test]
    async fn slow_response_is_classified_without_reqwest_details() {
        const RESPONSE: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"data\":{}}";
        let url = mock_url(RESPONSE, Duration::from_millis(200)).await;
        let client = MonarchClient::with_options(
            "super-secret-token",
            options(url, Duration::from_millis(25)),
        )
        .unwrap();
        let error = client.get_account_balance("account-1").await.unwrap_err();
        assert_eq!(
            error,
            ProviderError::Timeout {
                provider: ProviderName::Monarch,
            }
        );
        assert!(!format!("{error:?} {error}").contains("super-secret-token"));
    }

    #[tokio::test]
    async fn declared_oversize_response_is_rejected_before_decode() {
        const RESPONSE: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Length: 2048\r\nConnection: close\r\n\r\n";
        let url = mock_url(RESPONSE, Duration::ZERO).await;
        let client =
            MonarchClient::with_options("secret-token", options(url, Duration::from_secs(1)))
                .unwrap();
        let error = client.get_account_balance("account-1").await.unwrap_err();
        assert_eq!(
            error,
            ProviderError::ResponseTooLarge {
                provider: ProviderName::Monarch,
                limit_bytes: 1_024,
            }
        );
    }
}
