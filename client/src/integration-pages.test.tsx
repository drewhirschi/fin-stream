import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { test } from "vitest";

import { LoanPayments } from "../../app/integrations/[slug]/loans/[loan_account]/page";
import { OverviewView } from "../../app/integrations/[slug]/page";
import { PaymentsView } from "../../app/integrations/[slug]/payments/page";
import type { IntegrationData, LoanData, Payment } from "./types";

const processedPayment: Payment = {
  id: 1,
  loan_account: "LN-001",
  borrower_name: "Current Borrower",
  property_name: "Current Property",
  check_number: "1042",
  check_date: "2026-08-05",
  amount: 1_250,
  service_fee: 25,
  interest: 900,
  principal: 325,
  charges: 0,
  late_charges: 0,
  other: 0,
  processing_state: "normalized",
};

const pendingPayment: Payment = {
  ...processedPayment,
  id: 2,
  loan_account: "LN-002",
  borrower_name: "Pending Borrower",
  check_number: null,
};

function integrationData(): IntegrationData {
  return {
    connection: {
      id: 1,
      slug: "tmo",
      name: "The Mortgage Office",
      provider: "mortgage_office",
      status: "active",
      sync_cadence: "daily",
      last_synced_at: "2026-08-10T12:00:00Z",
      record_count: 20,
      normalized_count: 2,
      pending_count: 1,
    },
    loans: [
      {
        loan_account: "LN-001",
        borrower_name: "Current Borrower",
        next_payment_date: "2026-09-01",
        principal_balance: 100_000,
      },
      {
        loan_account: "LN-002",
        borrower_name: "Pending Borrower",
        next_payment_date: "2026-08-01",
        principal_balance: 80_000,
      },
    ],
    payments: [pendingPayment, processedPayment],
    normalized_payments: [],
    overviews: [
      {
        snapshot_date: "2026-08-11",
        trust_balance: 14_500,
        portfolio_value: 180_000,
        portfolio_yield: 8.25,
      },
    ],
    captured_records: [],
    sync_logs: [],
    control: {
      mode: "enabled",
      scheduler_enabled: true,
      updated_at: "2026-08-11T12:00:00Z",
    },
  };
}

test("overview leads with balance and payment standing, then value and yield", () => {
  const html = renderToStaticMarkup(
    <OverviewView data={integrationData()} today={new Date(2026, 7, 10)} />,
  );

  const labels = [
    "Balance",
    "Payment standing",
    "Portfolio value",
    "Portfolio yield",
  ];
  for (let index = 1; index < labels.length; index += 1) {
    assert.ok(html.indexOf(labels[index - 1]) < html.indexOf(labels[index]));
  }
  assert.match(html, /\$14,500\.00/);
  assert.match(html, />1<\/p><p[^>]*>of 2 active loans current<\/p>/);
  assert.doesNotMatch(html, /Sync settings|YTD interest/);
  assert.match(html, /grid gap-4 sm:grid-cols-2/);
});

test("overview keeps unpaid loans due through the 10th and marks them late on the 11th", () => {
  const data = integrationData();
  data.payments = [processedPayment];
  const dueHtml = renderToStaticMarkup(
    <OverviewView data={data} today={new Date(2026, 7, 10)} />,
  );
  const lateHtml = renderToStaticMarkup(
    <OverviewView data={data} today={new Date(2026, 7, 11)} />,
  );

  assert.match(dueHtml, /1 due by Aug 10, 2026/);
  assert.doesNotMatch(dueHtml, /bg-amber-50\/60/);
  assert.match(dueHtml, /lucide-clock3/);
  assert.match(lateHtml, /1 late/);
  assert.match(lateHtml, /border-amber-300 bg-amber-50\/60/);
  assert.match(lateHtml, /lucide-triangle-alert/);
});

test("overview handles zero and missing balances and unknown payment dates", () => {
  const zero = integrationData();
  zero.overviews[0].trust_balance = 0;
  zero.loans = [{ loan_account: "LN-003", next_payment_date: null }];
  const missing = integrationData();
  missing.overviews[0].trust_balance = null;

  assert.match(
    renderToStaticMarkup(
      <OverviewView data={zero} today={new Date(2026, 7, 11)} />,
    ),
    /\$0.*1 without a payment date/,
  );
  assert.match(
    renderToStaticMarkup(
      <OverviewView data={missing} today={new Date(2026, 7, 11)} />,
    ),
    /Trust balance as of Aug 11, 2026/,
  );
  assert.ok(
    renderToStaticMarkup(
      <OverviewView data={missing} today={new Date(2026, 7, 11)} />,
    ).includes("—"),
  );
});

test("overview withholds payment standing when provider data is stale", () => {
  const data = integrationData();
  data.connection.last_synced_at = "2026-08-01T12:00:00Z";
  const html = renderToStaticMarkup(
    <OverviewView data={data} today={new Date(2026, 7, 11)} />,
  );

  assert.match(html, /Standing unavailable/);
  assert.match(html, /Data needs refresh/);
  assert.match(html, /Updated Aug 1, 2026/);
  assert.match(html, /lucide-circle-help/);
  assert.doesNotMatch(html, /of 2 active loans current/);
});

test("overview keeps fresh degraded summary data available with its warning", () => {
  const data = integrationData();
  data.connection.status = "degraded";
  data.connection.last_error = "One loan detail request failed.";
  const html = renderToStaticMarkup(
    <OverviewView data={data} today={new Date(2026, 7, 11)} />,
  );

  assert.match(html, /One loan detail request failed/);
  assert.match(html, /of 2 active loans current/);
  assert.match(html, /1 late/);
  assert.doesNotMatch(html, /Standing unavailable/);
});

test("overview presents an empty portfolio without a false success state", () => {
  const data = integrationData();
  data.loans = [];
  const html = renderToStaticMarkup(
    <OverviewView data={data} today={new Date(2026, 7, 11)} />,
  );

  assert.match(html, /No active loans imported/);
  assert.match(html, />No loans</);
  assert.match(html, /lucide-circle-help/);
  assert.doesNotMatch(html, /lucide-circle-check/);
});

test("overview and responsive payments distinguish pending checks in amber", () => {
  const overview = renderToStaticMarkup(
    <OverviewView data={integrationData()} today={new Date(2026, 7, 11)} />,
  );
  const payments = renderToStaticMarkup(
    <PaymentsView data={integrationData()} />,
  );

  assert.match(overview, /Pending check/);
  assert.match(overview, /bg-amber-50\/70/);
  const mobileStart = payments.indexOf('data-layout="mobile"');
  const desktopStart = payments.indexOf('data-layout="desktop"');
  assert.ok(mobileStart >= 0 && desktopStart > mobileStart);
  const mobilePayments = payments.slice(mobileStart, desktopStart);
  const desktopPayments = payments.slice(desktopStart);

  assert.match(mobilePayments, /data-payment-state="pending"/);
  assert.match(mobilePayments, /border-amber-300 bg-amber-50\/70/);
  assert.match(desktopPayments, /data-payment-state="pending"/);
  assert.match(desktopPayments, /<tr[^>]*bg-amber-50\/70/);
  assert.match(
    desktopPayments,
    /<span class="[^"]*bg-amber-100[^"]*">Pending<\/span>/,
  );
  assert.match(payments, />1042<\/td>/);
});

test("loan payment history uses the same pending and processed treatment", () => {
  const data = integrationData();
  const loanData: LoanData = {
    connection: data.connection,
    loan: { id: 1, connection_id: 1, ...data.loans[0] },
    workspace: {
      redfin_url: "",
      zillow_url: "",
      decision_status: "",
      notes: "",
    },
    photos: [],
    payments: data.payments,
    emails: [],
  };
  const html = renderToStaticMarkup(<LoanPayments data={loanData} />);

  assert.match(html, /bg-amber-50\/70/);
  assert.match(html, />Pending<\/span>/);
  assert.match(html, />1042<\/td>/);
});
