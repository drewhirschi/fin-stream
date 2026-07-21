import { useQueryClient } from "@tanstack/react-query";
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
  getGetIntegrationSyncStatusQueryKey,
  useGetIntegrationSyncStatus,
  useRunIntegrationSync,
} from "@trust-deeds/client";
import { useEffect, useRef } from "react";
import type { IntegrationData } from "@/types";

export default function Sync() {
  return (
    <IntegrationBoundary>
      {(data, slug) => <SyncView data={data} slug={slug} />}
    </IntegrationBoundary>
  );
}

function SyncView({ data, slug }: { data: IntegrationData; slug: string }) {
  const queryClient = useQueryClient();
  const status = useGetIntegrationSyncStatus(slug, {
    fetch: { credentials: "same-origin", headers: { Accept: "application/json" } },
    query: {
      refetchInterval: query =>
        query.state.data?.status === 200 && query.state.data.data.run?.status === "running" ? 10_000 : false,
    },
  });
  const runSync = useRunIntegrationSync({
    fetch: { credentials: "same-origin", headers: { Accept: "application/json" } },
    mutation: {
      onSettled: async () => {
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: getGetIntegrationSyncStatusQueryKey(slug) }),
          queryClient.invalidateQueries({ queryKey: ["integration", slug] }),
        ]);
      },
    },
  });
  const currentRun = status.data?.status === 200 ? status.data.data.run : null;
  const sawRunning = useRef(currentRun?.status === "running");
  const durableRunning = currentRun?.status === "running";
  const busy = durableRunning || runSync.isPending;
  const response = runSync.data;
  const alreadyRunning =
    response?.status === 409 && "outcome" in response.data && response.data.outcome === "already_running";
  const responseMessage =
    response && "message" in response.data
      ? response.data.message
      : response && "run" in response.data
        ? response.data.run.error_message
        : null;
  const error = runSync.isError
    ? "The sync request could not be completed."
    : response && response.status >= 400 && !alreadyRunning
      ? responseMessage || `The sync could not be started (${response.status}).`
      : null;

  useEffect(() => {
    if (durableRunning) {
      sawRunning.current = true;
      return;
    }
    if (sawRunning.current && status.data?.status === 200) {
      sawRunning.current = false;
      void queryClient.invalidateQueries({ queryKey: ["integration", slug] });
    }
  }, [durableRunning, queryClient, slug, status.data?.status]);

  return (
    <Page
      title="Sync"
      description="Provider refresh history and operational controls."
      actions={
        <Button
          onClick={() => runSync.mutate({ slug })}
          disabled={busy || data.control.mode !== "enabled"}
        >
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
            <p className="font-medium">Syncing</p>
            <p className="mt-1 opacity-80">
              Started {dateTime(currentRun.started_at)}. This page checks for updates every ten seconds.
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
