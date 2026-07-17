use std::fmt::Display;

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use libsql::Connection;

use crate::{
    db::AppContext,
    media::{WorkspaceFormValues, WorkspacePhotoView},
    operations::OperationRepository,
    templates::{
        self, IntegrationDebugTemplate, IntegrationLoanDetailTemplate, IntegrationLoansTemplate,
        IntegrationOverviewTemplate, IntegrationPaymentsTemplate, IntegrationSyncTemplate,
        IntegrationsTemplate,
    },
    workspace_inbox::WorkspaceInboxRepository,
};

use super::{IntegrationConnectionView, IntegrationRepository};

pub async fn integrations_page(Extension(context): Extension<AppContext>) -> Response {
    let connection = match page_connection(&context, "open integrations database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let connections = match IntegrationRepository::new(&connection)
        .list_connection_views()
        .await
    {
        Ok(connections) => connections,
        Err(error) => return page_storage_error("load integration connections", error),
    };
    templates::integrations_response(IntegrationsTemplate {
        title: "Trust Deeds - Integrations".into(),
        connections,
    })
}

pub async fn integration_overview_page(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Response {
    let connection = match page_connection(&context, "open integration overview database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };

    let (loans, payments, snapshot) = if integration.slug == "tmo" {
        let loans = match repository.list_active_tmo_loan_views(integration.id).await {
            Ok(loans) => loans,
            Err(error) => return page_storage_error("load integration loans", error),
        };
        let payments = match repository
            .list_normalized_tmo_payments(integration.id, 8)
            .await
        {
            Ok(payments) => payments,
            Err(error) => return page_storage_error("load normalized integration payments", error),
        };
        let snapshot = match repository.list_portfolio_snapshots().await {
            Ok(mut snapshots) => snapshots.drain(..).next(),
            Err(error) => return page_storage_error("load latest portfolio snapshot", error),
        };
        (loans, payments, snapshot)
    } else {
        (Vec::new(), Vec::new(), None)
    };
    let active_loans_count = loans.len();
    let (portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks) =
        snapshot.map_or((None, None, None, None, None), |snapshot| {
            (
                snapshot.portfolio_value,
                snapshot.portfolio_yield,
                snapshot.ytd_interest,
                snapshot.trust_balance,
                snapshot.outstanding_checks,
            )
        });

    templates::integration_overview_response(IntegrationOverviewTemplate {
        title: format!("Trust Deeds - {}", integration.name),
        connection: integration,
        current_section: "overview",
        loans,
        payments,
        portfolio_value,
        portfolio_yield,
        ytd_interest,
        trust_balance,
        outstanding_checks,
        active_loans_count,
    })
}

pub async fn integration_loans_page(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Response {
    let connection = match page_connection(&context, "open integration loans database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let loans = if integration.slug == "tmo" {
        match repository.list_active_tmo_loan_views(integration.id).await {
            Ok(loans) => loans,
            Err(error) => return page_storage_error("load integration loans", error),
        }
    } else {
        Vec::new()
    };
    templates::integration_loans_response(IntegrationLoansTemplate {
        title: format!("Trust Deeds - {} Loans", integration.name),
        connection: integration,
        current_section: "loans",
        loans,
    })
}

pub async fn integration_loan_detail_page(
    Extension(context): Extension<AppContext>,
    Path((slug, loan_account)): Path<(String, String)>,
) -> Response {
    let connection = match page_connection(&context, "open integration loan database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    if integration.slug != "tmo" {
        return templates::not_found_response();
    }
    let loan = match repository
        .tmo_loan_by_account(integration.id, &loan_account)
        .await
    {
        Ok(Some(loan)) => loan,
        Ok(None) => return templates::not_found_response(),
        Err(error) => return page_storage_error("load integration loan", error),
    };
    let payment_history = match repository
        .list_tmo_payments_for_loan(integration.id, &loan_account, 36)
        .await
    {
        Ok(payments) => payments,
        Err(error) => return page_storage_error("load integration loan payments", error),
    };
    let workspace_repository = WorkspaceInboxRepository::new(&connection);
    let workspace = match workspace_repository
        .workspace(integration.id, &loan_account)
        .await
    {
        Ok(workspace) => workspace,
        Err(error) => return page_storage_error("load integration loan workspace", error),
    };
    let workspace_photos = match workspace_repository
        .list_photos(integration.id, &loan_account)
        .await
    {
        Ok(photos) => photos,
        Err(error) => return page_storage_error("load integration loan photos", error),
    };
    let workspace_form = WorkspaceFormValues::from(workspace.as_ref());
    let workspace_photos = workspace_photos
        .into_iter()
        .map(WorkspacePhotoView::from)
        .collect();
    let loan_emails = match workspace_repository
        .list_emails_for_loan(&loan_account)
        .await
    {
        Ok(emails) => emails,
        Err(error) => return page_storage_error("load integration loan mail", error),
    };

    templates::integration_loan_detail_response(IntegrationLoanDetailTemplate {
        title: format!("Trust Deeds - {} {}", integration.name, loan.loan_account),
        connection: integration,
        current_section: "loans",
        loan,
        workspace_form,
        workspace_photos,
        payment_history,
        loan_emails,
    })
}

pub async fn integration_payments_page(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Response {
    let connection = match page_connection(&context, "open integration payments database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let payments = if integration.slug == "tmo" {
        match repository
            .list_recent_tmo_payments(integration.id, 100)
            .await
        {
            Ok(payments) => payments,
            Err(error) => return page_storage_error("load integration payments", error),
        }
    } else {
        Vec::new()
    };
    templates::integration_payments_response(IntegrationPaymentsTemplate {
        title: format!("Trust Deeds - {} Payments", integration.name),
        connection: integration,
        current_section: "payments",
        payments,
    })
}

pub async fn integration_sync_page(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Response {
    let connection = match page_connection(&context, "open integration sync database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let operation_repository = OperationRepository::new(&connection);
    let sync_logs = match operation_repository.list_recent(&slug, 50).await {
        Ok(logs) => logs,
        Err(error) => return page_storage_error("load integration sync history", error),
    };
    let control = match operation_repository.control().await {
        Ok(control) => control,
        Err(error) => return page_storage_error("load integration operation control", error),
    };
    templates::integration_sync_response(IntegrationSyncTemplate {
        title: format!("Trust Deeds - {} Sync", integration.name),
        connection: integration,
        current_section: "sync",
        sync_logs,
        control,
    })
}

pub async fn integration_debug_page(
    Extension(context): Extension<AppContext>,
    Path(slug): Path<String>,
) -> Response {
    let connection = match page_connection(&context, "open integration debug database").await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let repository = IntegrationRepository::new(&connection);
    let integration = match connection_view(&repository, &slug).await {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    let sync_logs = match OperationRepository::new(&connection)
        .list_recent(&slug, 50)
        .await
    {
        Ok(logs) => logs,
        Err(error) => return page_storage_error("load integration debug sync history", error),
    };
    let tmo_import_payments = if integration.slug == "tmo" {
        match repository
            .list_recent_tmo_payments(integration.id, 100)
            .await
        {
            Ok(payments) => payments,
            Err(error) => return page_storage_error("load debug payment imports", error),
        }
    } else {
        Vec::new()
    };
    let normalized_payments = if integration.slug == "tmo" {
        match repository
            .list_normalized_tmo_payments(integration.id, 100)
            .await
        {
            Ok(payments) => payments,
            Err(error) => return page_storage_error("load debug normalized payments", error),
        }
    } else {
        Vec::new()
    };
    let captured_records = match repository
        .list_captured_provider_records(integration.id, 100)
        .await
    {
        Ok(records) => records,
        Err(error) => return page_storage_error("load captured provider records", error),
    };
    templates::integration_debug_response(IntegrationDebugTemplate {
        title: format!("Trust Deeds - {} Debug", integration.name),
        connection: integration,
        current_section: "debug",
        sync_logs,
        tmo_import_payments,
        captured_records,
        normalized_payments,
    })
}

pub async fn legacy_loans_redirect() -> Redirect {
    Redirect::permanent("/integrations/tmo/loans")
}

pub async fn legacy_payments_redirect() -> Redirect {
    Redirect::permanent("/integrations/tmo/payments")
}

async fn page_connection(
    context: &AppContext,
    operation: &'static str,
) -> Result<Connection, Response> {
    context
        .connection()
        .await
        .map_err(|error| page_storage_error(operation, error))
}

async fn connection_view(
    repository: &IntegrationRepository<'_>,
    slug: &str,
) -> Result<IntegrationConnectionView, Response> {
    match repository.connection_view_by_slug(slug).await {
        Ok(Some(connection)) => Ok(connection),
        Ok(None) => Err(templates::not_found_response()),
        Err(error) => Err(page_storage_error("load integration connection", error)),
    }
}

fn page_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "integration page storage failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not load this page.",
    )
        .into_response()
}

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use axum::{body::to_bytes, extract::Extension, http::StatusCode};
    use libsql::Builder;

    use super::*;

    async fn context() -> AppContext {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        AppContext::from_database(database).await.unwrap()
    }

    #[tokio::test]
    async fn unknown_integration_and_loan_return_real_not_found_statuses() {
        let context = context().await;
        let missing_connection =
            integration_overview_page(Extension(context.clone()), Path("missing".to_owned())).await;
        assert_eq!(missing_connection.status(), StatusCode::NOT_FOUND);

        let connection = context.connection().await.unwrap();
        connection
            .execute(
                "INSERT INTO intg_integration_connection (id, slug, name, provider) \
                 VALUES (1, 'tmo', 'The Mortgage Office', 'mortgage_office')",
                (),
            )
            .await
            .unwrap();
        let missing_loan = integration_loan_detail_page(
            Extension(context),
            Path(("tmo".to_owned(), "LN-404".to_owned())),
        )
        .await;
        assert_eq!(missing_loan.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn integrations_page_renders_counts_without_exposing_raw_dates() {
        let context = context().await;
        let connection = context.connection().await.unwrap();
        connection
            .execute_batch(
                "INSERT INTO intg_integration_connection ( \
                    id, slug, name, provider, last_synced_at \
                 ) VALUES (1, 'tmo', 'The Mortgage Office', 'mortgage_office', \
                           '2026-07-14T18:20:30.456Z'); \
                 INSERT INTO intg_tmo_import_loan ( \
                    connection_id, loan_account, is_active \
                 ) VALUES (1, 'LN-1', 1);",
            )
            .await
            .unwrap();
        let response = integrations_page(Extension(context)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("The Mortgage Office"));
        assert!(body.contains("data-local=\"2026-07-14T18:20:30.456Z\""));
    }

    #[tokio::test]
    async fn storage_failures_return_a_sanitized_internal_server_error() {
        let context = context().await;
        context
            .connection()
            .await
            .unwrap()
            .execute("DROP TABLE intg_tmo_import_overview", ())
            .await
            .unwrap();
        let response = integrations_page(Extension(context)).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"Could not load this page.");
    }

    #[tokio::test]
    async fn sync_page_offers_only_canonical_cadences_and_no_destructive_credential_reset() {
        let context = context().await;
        context
            .connection()
            .await
            .unwrap()
            .execute(
                "INSERT INTO intg_integration_connection ( \
                    id, slug, name, provider, sync_cadence \
                 ) VALUES (1, 'tmo', 'The Mortgage Office', 'mortgage_office', 'every_6h')",
                (),
            )
            .await
            .unwrap();
        let response = integration_sync_page(Extension(context), Path("tmo".to_owned())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("/integrations/tmo/sync/cadence"));
        for cadence in ["manual", "hourly", "every_6h", "every_12h", "daily"] {
            assert!(body.contains(&format!("value=\"{cadence}\"")), "{cadence}");
        }
        assert!(body.contains("Save cadence"));
        assert!(!body.to_ascii_lowercase().contains("reset credential"));
    }
}
