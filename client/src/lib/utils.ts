import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function money(value: number | null | undefined) {
  if (value == null) return "—";
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: value === 0 ? 0 : 2,
  }).format(value);
}

export function date(value: string | null | undefined) {
  if (!value) return "—";
  const parsed = new Date(value.length === 10 ? `${value}T00:00:00` : value);
  return Number.isNaN(parsed.valueOf())
    ? value
    : new Intl.DateTimeFormat("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
      }).format(parsed);
}

export function dateTime(value: string | null | undefined) {
  if (!value) return "Never";
  const parsed = new Date(value);
  return Number.isNaN(parsed.valueOf())
    ? value
    : new Intl.DateTimeFormat("en-US", {
        month: "short",
        day: "numeric",
        year: "numeric",
        hour: "numeric",
        minute: "2-digit",
      }).format(parsed);
}

type LoanWithNextPayment = {
  next_payment_date?: string | null;
};

const PAYMENT_GRACE_DAY = 10;
const DEFAULT_DATA_FRESHNESS_HOURS = 48;
const DATA_FRESHNESS_HOURS: Record<string, number> = {
  hourly: 3,
  every_6h: 12,
  every_12h: 24,
  daily: 48,
};

export interface LoanPaymentStatus {
  current: number;
  due: number;
  late: number;
  unknown: number;
  grace_deadline: string;
}

/**
 * Treat every active mortgage as current once its next payment has advanced
 * beyond the viewer's current month. Unpaid loans remain due through the
 * configured grace day, then become late the following day.
 */
export function loanPaymentStatus(
  loans: readonly LoanWithNextPayment[],
  today = new Date(),
): LoanPaymentStatus {
  const year = today.getFullYear();
  const month = today.getMonth() + 1;
  const currentMonth = `${year.toString().padStart(4, "0")}-${month.toString().padStart(2, "0")}`;
  let current = 0;
  let unpaid = 0;
  let unknown = 0;

  for (const loan of loans) {
    const nextPayment = loan.next_payment_date;
    if (!nextPayment || !isCalendarDate(nextPayment)) {
      unknown += 1;
    } else if (nextPayment.slice(0, 7) > currentMonth) {
      current += 1;
    } else {
      unpaid += 1;
    }
  }

  const afterGracePeriod = today.getDate() > PAYMENT_GRACE_DAY;
  return {
    current,
    due: afterGracePeriod ? 0 : unpaid,
    late: afterGracePeriod ? unpaid : 0,
    unknown,
    grace_deadline: `${currentMonth}-${PAYMENT_GRACE_DAY.toString().padStart(2, "0")}`,
  };
}

/**
 * Payment standing depends on provider data being recent enough for the
 * configured sync cadence. Unknown cadences get a conservative two-day limit.
 */
export function integrationDataIsStale(
  lastSyncedAt: string | null | undefined,
  cadence: string,
  now = new Date(),
) {
  if (!lastSyncedAt) return true;
  const syncedAt = new Date(lastSyncedAt);
  if (Number.isNaN(syncedAt.valueOf())) return true;

  const freshnessHours =
    DATA_FRESHNESS_HOURS[cadence] ?? DEFAULT_DATA_FRESHNESS_HOURS;
  return now.valueOf() - syncedAt.valueOf() > freshnessHours * 60 * 60 * 1_000;
}

/** Return the delay until just after the viewer's next local midnight. */
export function millisecondsUntilNextLocalDay(now = new Date()) {
  const nextDay = new Date(now);
  nextDay.setHours(24, 0, 0, 50);
  return Math.max(0, nextDay.valueOf() - now.valueOf());
}

function isCalendarDate(value: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const [year, month, day] = value.split("-").map(Number);
  const parsed = new Date(year, month - 1, day);
  return (
    parsed.getFullYear() === year &&
    parsed.getMonth() === month - 1 &&
    parsed.getDate() === day
  );
}
