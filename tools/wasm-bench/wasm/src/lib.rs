// WASI-HTTP component: same routes, same template as native/src/main.rs.
//
// This builds as a wasm32-wasip2 component exporting `wasi:http/proxy`. It is
// hosted by `wasmtime serve`, which owns the TCP listener and feeds us one
// request at a time via the exported `handle` function.

use askama::Template;

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
        engine: "wasm-wasmtime".into(),
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

use wasi::exports::http::incoming_handler::Guest;
use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

struct Component;

impl Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let path = request.path_with_query().unwrap_or_else(|| "/".into());

        let (status, content_type, body) = if path.starts_with("/health") {
            (200u16, "text/plain; charset=utf-8", b"ok".to_vec())
        } else if path.starts_with("/bench/render") {
            let html = build_bench().render().expect("render");
            (200, "text/html; charset=utf-8", html.into_bytes())
        } else {
            (404, "text/plain; charset=utf-8", b"not found".to_vec())
        };

        let headers = Fields::new();
        headers
            .set(&"content-type".to_string(), &[content_type.as_bytes().to_vec()])
            .ok();
        headers
            .set(
                &"content-length".to_string(),
                &[body.len().to_string().into_bytes()],
            )
            .ok();

        let response = OutgoingResponse::new(headers);
        response.set_status_code(status).expect("set status");

        let response_body = response.body().expect("body");
        ResponseOutparam::set(response_out, Ok(response));

        let out_stream = response_body.write().expect("stream");
        // 4 KiB is the documented safe chunk size for blocking_write_and_flush.
        for chunk in body.chunks(4096) {
            out_stream.blocking_write_and_flush(chunk).expect("write");
        }
        drop(out_stream);
        OutgoingBody::finish(response_body, None).expect("finish body");
    }
}

wasi::http::proxy::export!(Component);
