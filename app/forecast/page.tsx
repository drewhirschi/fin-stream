import { ArrowDownRight, ArrowUpRight, CalendarRange, Wallet } from "lucide-react";
import { useMemo, useState } from "react";
import {
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Empty,
  ErrorState,
  Input,
  Loading,
  Page,
  api,
  date,
  money,
  useApi,
  type FinanceData,
} from "@trust-deeds/client";

interface ForecastRow {
  event_id: number;
  date: string;
  label?: string | null;
  stream_name?: string | null;
  amount: number;
  running_balance: number;
  status: string;
  direction: string;
  is_late: boolean;
}

interface Forecast {
  starting_balance: number;
  balance_as_of_date: string;
  opening_balance: number;
  ending_balance: number;
  rows: ForecastRow[];
}

function iso(offset: number) {
  const value = new Date();
  value.setDate(value.getDate() + offset);
  return value.toISOString().slice(0, 10);
}

export default function Timeline() {
  const finance = useApi<FinanceData>(["finance"], "/api/ui/finance");
  const defaultView = finance.data?.views.find(view => view.is_default) ?? finance.data?.views[0];
  const path = `/api/forecast?from=${iso(-30)}&through=${iso(365)}${defaultView ? `&view_id=${defaultView.id}` : ""}`;
  const forecast = useApi<Forecast>(["forecast", path], path, { enabled: Boolean(finance.data) });
  const [cash, setCash] = useState("");
  const rows = forecast.data?.rows ?? [];
  const next = useMemo(() => rows.filter(row => row.date >= iso(0)).slice(0, 30), [rows]);
  const saveCash = async () => {
    await api("/api/settings/cash", { method: "POST", body: JSON.stringify({ amount: Number(cash), as_of_date: iso(0) }) });
    await forecast.refetch();
    setCash("");
  };

  return (
    <Page title="Timeline" description="Projected cash position from imported and manually scheduled income events.">
      {finance.isLoading || forecast.isLoading ? <Loading label="Building timeline" /> : finance.error ? <ErrorState error={finance.error} /> : forecast.error ? (
        <Card>
          <CardHeader><CardTitle>Set your starting cash</CardTitle><CardDescription>A current cash anchor is required before the forecast can be calculated.</CardDescription></CardHeader>
          <CardContent className="flex max-w-md flex-col gap-2 sm:flex-row"><Input type="number" step="0.01" value={cash} onChange={event => setCash(event.target.value)} placeholder="Current cash balance" /><Button onClick={saveCash} disabled={!cash}>Save</Button></CardContent>
        </Card>
      ) : forecast.data ? (
        <>
          <section className="grid grid-cols-1 gap-3 sm:grid-cols-3 sm:gap-4">
            <Metric label="Starting cash" value={money(forecast.data.starting_balance)} icon={<Wallet className="size-4" />} />
            <Metric label="Projected ending" value={money(forecast.data.ending_balance)} icon={<CalendarRange className="size-4" />} />
            <Metric label="Net change" value={money(forecast.data.ending_balance - forecast.data.starting_balance)} icon={forecast.data.ending_balance >= forecast.data.starting_balance ? <ArrowUpRight className="size-4" /> : <ArrowDownRight className="size-4" />} />
          </section>
          {next.length === 0 ? <Empty title="No upcoming events" description="Add a stream or run an integration sync to populate the timeline." /> : <ForecastEvents rows={next} />}
        </>
      ) : null}
    </Page>
  );
}

function ForecastEvents({ rows }: { rows: ForecastRow[] }) {
  return (
    <>
      <div className="space-y-3 md:hidden">
        {rows.map(row => (
          <Card key={row.event_id}>
            <CardContent className="p-4">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0"><p className="text-xs text-muted-foreground">{date(row.date)}</p><h2 className="mt-1 break-words font-medium">{row.label || "Scheduled event"}</h2><p className="mt-1 text-xs text-muted-foreground">{row.stream_name || "No stream"}</p></div>
                <p className={row.direction === "inflow" ? "shrink-0 font-semibold text-primary" : "shrink-0 font-semibold text-destructive"}>{row.direction === "inflow" ? "+" : "−"}{money(Math.abs(row.amount))}</p>
              </div>
              <div className="mt-4 flex items-center justify-between border-t pt-3"><div className="flex items-center gap-2"><Badge>{row.status}</Badge>{row.is_late ? <Badge className="border-amber-300 bg-amber-50 text-amber-900">Late</Badge> : null}</div><div className="text-right"><p className="text-xs text-muted-foreground">Running balance</p><p className="mt-0.5 text-sm font-medium">{money(row.running_balance)}</p></div></div>
            </CardContent>
          </Card>
        ))}
      </div>
      <Card className="hidden overflow-x-auto md:block">
        <table className="data-table"><thead><tr><th>Date</th><th>Event</th><th>Stream</th><th>Status</th><th>Amount</th><th>Running balance</th></tr></thead><tbody>{rows.map(row => <tr key={row.event_id}><td>{date(row.date)}</td><td className="font-medium">{row.label || "Scheduled event"}{row.is_late ? <Badge className="ml-2 border-amber-300 bg-amber-50 text-amber-900">Late</Badge> : null}</td><td>{row.stream_name || "—"}</td><td><Badge>{row.status}</Badge></td><td className={row.direction === "inflow" ? "text-primary" : "text-destructive"}>{row.direction === "inflow" ? "+" : "−"}{money(Math.abs(row.amount))}</td><td className="font-medium">{money(row.running_balance)}</td></tr>)}</tbody></table>
      </Card>
    </>
  );
}

function Metric({ label, value, icon }: { label: string; value: string; icon: React.ReactNode }) {
  return <Card><CardContent className="p-4 sm:p-5"><div className="flex items-center justify-between text-muted-foreground"><span className="text-xs font-medium uppercase tracking-wide">{label}</span>{icon}</div><p className="mt-3 break-words text-2xl font-semibold">{value}</p></CardContent></Card>;
}
