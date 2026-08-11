import {
  AlertTriangle,
  CheckCircle2,
  CircleHelp,
  Clock3,
  Landmark,
  Percent,
  WalletCards,
} from "lucide-react";
import { useEffect, useState } from "react";
import {
  Badge,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  IntegrationBoundary,
  PendingCheckBadge,
  Page,
  cn,
  date,
  dateTime,
  integrationDataIsStale,
  loanPaymentStatus,
  millisecondsUntilNextLocalDay,
  money,
  isPendingCheck,
  pendingCheckSurface,
  type IntegrationData,
} from "@trust-deeds/client";

export default function Overview() {
  return (
    <IntegrationBoundary refetchInterval={OVERVIEW_CLOCK_REFRESH_MS}>
      {(data) => <OverviewView data={data} />}
    </IntegrationBoundary>
  );
}

const OVERVIEW_CLOCK_REFRESH_MS = 60_000;

export function OverviewView({
  data,
  today,
}: {
  data: IntegrationData;
  today?: Date;
}) {
  const [clock, setClock] = useState(() => today ?? new Date());
  const currentTime = today ?? clock;

  useEffect(() => {
    if (today) return;
    const timeout = window.setTimeout(
      () => setClock(new Date()),
      Math.min(
        OVERVIEW_CLOCK_REFRESH_MS,
        millisecondsUntilNextLocalDay(new Date()),
      ),
    );
    return () => window.clearTimeout(timeout);
  }, [clock, today]);

  const overview = data.overviews[0];
  const principal = data.loans.reduce(
    (sum, loan) => sum + (loan.principal_balance ?? 0),
    0,
  );
  const paymentStatus = loanPaymentStatus(data.loans, currentTime);
  const unpaid = paymentStatus.late || paymentStatus.due;
  const noLoans = data.loans.length === 0;
  const stale =
    !["active", "degraded"].includes(data.connection.status) ||
    integrationDataIsStale(
      data.connection.last_synced_at,
      data.connection.sync_cadence,
      currentTime,
    );
  const standingUnavailable = noLoans || stale;
  const statusLabel = paymentStatus.late
    ? "late"
    : `due by ${date(paymentStatus.grace_deadline)}`;
  const StandingIcon = standingUnavailable
    ? CircleHelp
    : paymentStatus.late > 0
      ? AlertTriangle
      : paymentStatus.due > 0
        ? Clock3
        : paymentStatus.unknown > 0
          ? CircleHelp
          : CheckCircle2;

  return (
    <Page
      title={data.connection.name}
      description="Portfolio balance, payment standing, and recent activity."
    >
      {data.connection.last_error ? (
        <div className="flex gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-950">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          {data.connection.last_error}
        </div>
      ) : null}

      <section className="grid gap-4 lg:grid-cols-2">
        <PrimaryMetric
          label="Balance"
          value={money(overview?.trust_balance)}
          icon={<WalletCards className="size-5" />}
          description={
            overview
              ? `Trust balance as of ${date(overview.snapshot_date)}`
              : "Current trust account balance"
          }
        />
        <Card
          className={cn(
            (paymentStatus.late > 0 || stale) &&
              "border-amber-300 bg-amber-50/60",
          )}
        >
          <CardContent className="p-5">
            <div className="flex items-center justify-between text-muted-foreground">
              <span className="text-xs font-medium uppercase tracking-wide">
                Payment standing
              </span>
              <StandingIcon
                className={cn(
                  "size-5",
                  (paymentStatus.late > 0 || stale) && "text-amber-700",
                  !standingUnavailable &&
                    unpaid === 0 &&
                    paymentStatus.unknown === 0 &&
                    "text-primary",
                )}
              />
            </div>
            <div className="mt-4 flex items-end gap-2">
              <p className="text-4xl font-semibold tracking-tight">
                {standingUnavailable ? "—" : paymentStatus.current}
              </p>
              <p className="pb-1 text-sm text-muted-foreground">
                {noLoans
                  ? "No active loans imported"
                  : stale
                    ? "Standing unavailable"
                    : `of ${data.loans.length} active loans current`}
              </p>
            </div>
            <div className="mt-4 flex flex-wrap items-center gap-2 border-t pt-4">
              <Badge
                className={cn(
                  (paymentStatus.late > 0 || stale) &&
                    "border-amber-300 bg-amber-100 text-amber-950",
                )}
              >
                {noLoans
                  ? "No loans"
                  : stale
                    ? "Data needs refresh"
                    : `${unpaid} ${statusLabel}`}
              </Badge>
              {standingUnavailable ? (
                <span className="text-xs text-muted-foreground">
                  {data.connection.last_synced_at
                    ? `Updated ${dateTime(data.connection.last_synced_at)}`
                    : "No update has completed"}
                </span>
              ) : paymentStatus.unknown > 0 ? (
                <span className="text-xs text-muted-foreground">
                  {paymentStatus.unknown} without a payment date
                </span>
              ) : (
                <span className="text-xs text-muted-foreground">
                  Grace period ends {date(paymentStatus.grace_deadline)}
                </span>
              )}
            </div>
          </CardContent>
        </Card>
      </section>

      <section className="grid gap-4 sm:grid-cols-2">
        <Metric
          label="Portfolio value"
          value={money(overview?.portfolio_value ?? principal)}
          icon={<Landmark className="size-4" />}
        />
        <Metric
          label="Portfolio yield"
          value={
            overview?.portfolio_yield == null
              ? "—"
              : `${overview.portfolio_yield.toFixed(2)}%`
          }
          icon={<Percent className="size-4" />}
        />
      </section>

      <section className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Active loans</CardTitle>
            <CardDescription>
              {data.loans.length} loans currently imported.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {data.loans.slice(0, 6).map((loan) => (
              <a
                key={loan.loan_account}
                href={`/integrations/${data.connection.slug}/loans/${encodeURIComponent(loan.loan_account)}`}
                className="flex items-center justify-between rounded-lg border p-3 hover:bg-muted"
              >
                <div>
                  <p className="text-sm font-medium">
                    {loan.borrower_name || loan.loan_account}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {loan.property_address || loan.loan_account}
                  </p>
                </div>
                <span className="text-sm font-medium">
                  {money(loan.principal_balance)}
                </span>
              </a>
            ))}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Recent payments</CardTitle>
            <CardDescription>
              The latest imported provider activity.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {data.payments.slice(0, 6).map((payment) => {
              const pending = isPendingCheck(payment.check_number);
              return (
                <div
                  key={payment.id}
                  className={cn(
                    "flex items-center justify-between gap-3",
                    pending
                      ? `rounded-lg border p-3 ${pendingCheckSurface.bordered}`
                      : "border-b pb-3 last:border-0",
                  )}
                >
                  <div>
                    <p className="text-sm font-medium">
                      {payment.borrower_name || payment.loan_account}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {date(payment.check_date)} · {payment.loan_account}
                    </p>
                    {pending ? (
                      <PendingCheckBadge
                        label="Pending check"
                        className="mt-1"
                      />
                    ) : null}
                  </div>
                  <span className="text-sm font-medium text-primary">
                    {money(payment.amount)}
                  </span>
                </div>
              );
            })}
          </CardContent>
        </Card>
      </section>
    </Page>
  );
}

function Metric({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
}) {
  return (
    <Card>
      <CardContent className="pt-5">
        <div className="flex items-center justify-between text-muted-foreground">
          <span className="text-xs font-medium uppercase tracking-wide">
            {label}
          </span>
          {icon}
        </div>
        <p className="mt-3 text-2xl font-semibold">{value}</p>
      </CardContent>
    </Card>
  );
}

function PrimaryMetric({
  label,
  value,
  icon,
  description,
}: {
  label: string;
  value: string;
  icon: React.ReactNode;
  description: string;
}) {
  return (
    <Card className="border-primary/25 bg-accent/35">
      <CardContent className="p-5">
        <div className="flex items-center justify-between text-muted-foreground">
          <span className="text-xs font-medium uppercase tracking-wide">
            {label}
          </span>
          {icon}
        </div>
        <p className="mt-4 text-4xl font-semibold tracking-tight">{value}</p>
        <p className="mt-2 text-sm text-muted-foreground">{description}</p>
      </CardContent>
    </Card>
  );
}
