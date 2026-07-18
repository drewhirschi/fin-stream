import { LoaderCircle, Play, ShieldCheck } from "lucide-react";
import {
  Badge,
  Button,
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
  Empty,
  IntegrationBoundary,
  Page,
  dateTime,
  useApi,
} from "@trust-deeds/client";
import { useEffect, useRef, useState } from "react";
import type { IntegrationData, SyncRun } from "@/types";

interface SyncStatus {
  run: SyncRun | null;
}

export default function Sync() {
  return (
    <IntegrationBoundary>
      {(data, slug) => <SyncView data={data} slug={slug} />}
    </IntegrationBoundary>
  );
}

function SyncView({ data, slug }: { data: IntegrationData; slug: string }) {
  const initialRun = data.sync_logs.find((run) => run.status === "running") ?? data.sync_logs[0] ?? null;
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [watchingForAutomaticRun, setWatchingForAutomaticRun] = useState(true);
  const sawRunning = useRef(initialRun?.status === "running");
  const status = useApi<SyncStatus>(
    ["integration-sync-status", slug],
    `/integrations/${encodeURIComponent(slug)}/sync/status`,
    {
      initialData: { run: initialRun },
      refetchInterval: (query) =>
        query.state.data?.run?.status === "running" || watchingForAutomaticRun ? 1_500 : false,
    },
  );
  const durableRunning = status.data?.run?.status === "running";
  const busy = submitting || durableRunning;

  useEffect(() => {
    const timeout = window.setTimeout(() => setWatchingForAutomaticRun(false), 10_000);
    return () => window.clearTimeout(timeout);
  }, []);

  useEffect(() => {
    if (durableRunning) {
      sawRunning.current = true;
      return;
    }
    if (sawRunning.current && status.data) {
      sawRunning.current = false;
      window.location.reload();
    }
  }, [durableRunning, status.data]);

  const run = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const response = await fetch(`/integrations/${encodeURIComponent(slug)}/sync/run`, {
        method: "POST",
        credentials: "same-origin",
        headers: { Accept: "application/json" },
      });
      if (!response.ok && response.status !== 409) {
        throw new Error(`The sync could not be started (${response.status}).`);
      }
      if (response.status === 409) {
        const refreshed = await status.refetch();
        if (refreshed.data?.run?.status !== "running") {
          throw new Error("The sync could not be started. Check the integration configuration and try again.");
        }
        return;
      }
      window.location.reload();
    } catch (runError) {
      setError(runError instanceof Error ? runError.message : "The sync could not be started.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Page
      title="Sync"
      description="Provider refresh history and operational controls."
      actions={
        <Button onClick={run} disabled={busy || data.control.mode !== "enabled"}>
          {busy ? <LoaderCircle className="size-4 animate-spin" /> : <Play className="size-4" />}
          {busy ? "Syncing…" : "Run sync"}
        </Button>
      }
    >
      {durableRunning ? (
        <div
          className="flex gap-3 rounded-xl border border-primary/25 bg-accent p-4 text-sm text-accent-foreground"
          aria-live="polite"
        >
          <LoaderCircle className="size-5 shrink-0 animate-spin" />
          <div>
            <p className="font-medium">Sync in progress</p>
            <p className="mt-1 opacity-80">
              Started {dateTime(status.data?.run?.started_at)}. This page will update automatically when it finishes.
            </p>
          </div>
        </div>
      ) : null}
      {error ? (
        <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
          {error}
        </div>
      ) : null}
      <section className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader>
            <CardDescription>Write mode</CardDescription>
            <CardTitle className="capitalize">{data.control.mode.replaceAll("_", " ")}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader>
            <CardDescription>Scheduler</CardDescription>
            <CardTitle>{data.control.scheduler_enabled ? "Enabled" : "Paused"}</CardTitle>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader>
            <CardDescription>Cadence</CardDescription>
            <CardTitle className="capitalize">{data.connection.sync_cadence.replaceAll("_", " ")}</CardTitle>
          </CardHeader>
        </Card>
      </section>
      {data.control.mode !== "enabled" ? (
        <div className="flex gap-3 rounded-xl border bg-muted p-4 text-sm text-muted-foreground">
          <ShieldCheck className="size-5 shrink-0" />
          This imported database is intentionally read-only. Enable writes during the final cutover before running
          provider syncs.
        </div>
      ) : null}
      {data.sync_logs.length === 0 ? (
        <Empty title="No sync history" description="The first execution will appear here." />
      ) : (
        <Card className="overflow-x-auto">
          <table className="data-table">
            <thead>
              <tr>
                <th>Started</th>
                <th>Status</th>
                <th>Loans</th>
                <th>Events</th>
                <th>Snapshots</th>
                <th>Error</th>
              </tr>
            </thead>
            <tbody>
              {data.sync_logs.map((log) => (
                <tr key={log.id}>
                  <td>{dateTime(log.started_at)}</td>
                  <td>
                    <Badge>{log.status}</Badge>
                  </td>
                  <td>{log.loans_upserted.toLocaleString()}</td>
                  <td>{log.events_upserted.toLocaleString()}</td>
                  <td>{log.snapshots_created.toLocaleString()}</td>
                  <td className="max-w-sm text-xs text-muted-foreground">{log.error_message || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </Page>
  );
}
