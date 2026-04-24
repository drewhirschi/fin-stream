use askama::Template;
use axum::{Router, http::header, response::IntoResponse, routing::get};

// Keep the shape identical between native and wasm builds — see wasm/src/lib.rs.
struct Loan {
    account: String,
    borrower: String,
    address: String,
    city: String,
    state: String,
    property_type: String,
    note_rate: String,
    principal_balance: String,
    regular_payment: String,
    maturity_date: String,
    next_payment_date: String,
    delinquent: bool,
}

struct Payment {
    id: u32,
    label: String,
    expected_date: String,
    actual_date: String,
    amount: String,
    status: String,
    source_type: String,
    loan_account: String,
}

#[derive(Template)]
#[template(path = "bench.html")]
struct BenchTemplate {
    title: String,
    current_section: String,
    engine: String,
    sections: Vec<String>,
    portfolio_value: String,
    portfolio_yield: String,
    ytd_interest: String,
    trust_balance: String,
    outstanding_checks: String,
    loans: Vec<Loan>,
    payments: Vec<Payment>,
}

fn build_bench() -> BenchTemplate {
    let loans: Vec<Loan> = (0..15)
        .map(|i| Loan {
            account: format!("LOAN-{i:04}"),
            borrower: format!("Borrower {i}"),
            address: format!("{} Oak Street", 100 + i),
            city: "Salt Lake City".into(),
            state: "UT".into(),
            property_type: "Single Family".into(),
            note_rate: format!("{:.2}", 8.5 + (i as f64) * 0.25),
            principal_balance: format!("{:.2}", 150_000.0 + (i as f64) * 10_000.0),
            regular_payment: format!("{:.2}", 1_200.0 + (i as f64) * 50.0),
            maturity_date: "2029-06-01".into(),
            next_payment_date: "2026-05-01".into(),
            delinquent: false,
        })
        .collect();

    let payments: Vec<Payment> = (0..8)
        .map(|i| Payment {
            id: i + 1,
            label: format!("Payment from Borrower {i}"),
            expected_date: format!("2026-04-{:02}", 1 + i),
            actual_date: format!("2026-04-{:02}", 1 + i),
            amount: format!("{:.2}", 1_200.0 + (i as f64) * 50.0),
            status: "received".into(),
            source_type: "tmo_history".into(),
            loan_account: format!("LOAN-{i:04}"),
        })
        .collect();

    BenchTemplate {
        title: "Trust Deeds - The Mortgage Office".into(),
        current_section: "overview".into(),
        engine: "native-axum".into(),
        sections: vec!["overview".into(), "loans".into(), "payments".into(), "forecast".into()],
        portfolio_value: "2,450,000.00".into(),
        portfolio_yield: "9.20".into(),
        ytd_interest: "85,000.00".into(),
        trust_balance: "125,000.00".into(),
        outstanding_checks: "3,500.00".into(),
        loans,
        payments,
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn bench_render() -> impl IntoResponse {
    let body = build_bench().render().expect("render");
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body)
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health))
        .route("/bench/render", get(bench_render));

    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    eprintln!("native bench listening on {addr}");
    axum::serve(listener, app).await.expect("serve");
}
