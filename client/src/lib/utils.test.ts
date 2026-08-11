import assert from "node:assert/strict";
import { test } from "vitest";

import {
  integrationDataIsStale,
  loanPaymentStatus,
  millisecondsUntilNextLocalDay,
} from "./utils";

test("counts loans paid through the current month as current", () => {
  const status = loanPaymentStatus(
    [
      { next_payment_date: "2026-09-01" },
      { next_payment_date: "2026-08-10" },
      { next_payment_date: "2026-07-01" },
    ],
    new Date(2026, 7, 10),
  );

  assert.deepEqual(status, {
    current: 1,
    due: 2,
    late: 0,
    unknown: 0,
    grace_deadline: "2026-08-10",
  });
});

test("keeps unpaid loans in grace through the 10th and marks them late on the 11th", () => {
  const loans = [{ next_payment_date: "2026-08-01" }];

  assert.equal(loanPaymentStatus(loans, new Date(2026, 7, 10)).due, 1);
  assert.deepEqual(loanPaymentStatus(loans, new Date(2026, 7, 11)), {
    current: 0,
    due: 0,
    late: 1,
    unknown: 0,
    grace_deadline: "2026-08-10",
  });
});

test("uses payment month rather than the provider's day of month", () => {
  const status = loanPaymentStatus(
    [{ next_payment_date: "2027-01-10" }, { next_payment_date: "2026-12-31" }],
    new Date(2026, 11, 15),
  );

  assert.equal(status.current, 1);
  assert.equal(status.late, 1);
});

test("separates missing and invalid dates instead of calling them current", () => {
  const status = loanPaymentStatus(
    [{ next_payment_date: null }, { next_payment_date: "2026-02-30" }],
    new Date(2026, 1, 11),
  );

  assert.equal(status.unknown, 2);
  assert.equal(status.current, 0);
  assert.equal(status.late, 0);
});

test("rejects malformed date shapes and handles an empty portfolio", () => {
  const malformed = loanPaymentStatus(
    [{ next_payment_date: "2026-08-10T00:00:00Z" }],
    new Date(2026, 7, 10),
  );
  const empty = loanPaymentStatus([], new Date(2026, 7, 10));

  assert.equal(malformed.unknown, 1);
  assert.deepEqual(empty, {
    current: 0,
    due: 0,
    late: 0,
    unknown: 0,
    grace_deadline: "2026-08-10",
  });
});

test("evaluates provider data freshness using the configured cadence", () => {
  const now = new Date("2026-08-11T12:00:00Z");

  assert.equal(integrationDataIsStale(null, "daily", now), true);
  assert.equal(integrationDataIsStale("invalid", "daily", now), true);
  assert.equal(
    integrationDataIsStale("2026-08-11T01:00:00Z", "every_6h", now),
    false,
  );
  assert.equal(
    integrationDataIsStale("2026-08-10T23:00:00Z", "every_6h", now),
    true,
  );
  assert.equal(
    integrationDataIsStale("2026-08-10T00:00:00Z", "daily", now),
    false,
  );
});

test("schedules the standing refresh just after local midnight", () => {
  const now = new Date(2026, 11, 31, 23, 59, 59, 900);

  assert.equal(millisecondsUntilNextLocalDay(now), 150);
});
