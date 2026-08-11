import { Badge, Button, Card, CardContent, ErrorState, Loading, useApi } from "@trust-deeds/client";
import type { IntegrationData } from "@/types";
import { dateTime } from "@/lib/utils";

export function useIntegration(refetchInterval?: number) {
  const parts = window.location.pathname.split("/").filter(Boolean);
  const slug = parts[1] ?? "tmo";
  const section = parts[2] ?? "overview";
  return { slug, query: useApi<IntegrationData>(["integration", slug, section], `/api/ui/integrations/${encodeURIComponent(slug)}?section=${encodeURIComponent(section)}`, refetchInterval ? { refetchInterval } : undefined) };
}

export function IntegrationBoundary({ children, refetchInterval }: { children: (data: IntegrationData, slug: string) => React.ReactNode; refetchInterval?: number }) {
  const { slug, query } = useIntegration(refetchInterval);
  if (query.isLoading) return <Loading label="Loading integration" />;
  if (!query.data) return <ErrorState error={query.error} />;
  return <>{children(query.data, slug)}</>;
}

export function IntegrationSummary({ data }: { data: IntegrationData }) {
  return <Card><CardContent className="flex flex-col gap-4 pt-5 sm:flex-row sm:items-center sm:justify-between"><div><div className="flex items-center gap-2"><h2 className="text-lg font-semibold">{data.connection.name}</h2><Badge className={data.connection.status === "active" ? "border-primary/30 bg-accent text-accent-foreground" : ""}>{data.connection.status}</Badge></div><p className="mt-1 text-sm text-muted-foreground">Last synced {dateTime(data.connection.last_synced_at)} · {data.connection.record_count.toLocaleString()} imported records</p></div><Button asChild variant="outline"><a href={`/integrations/${data.connection.slug}/sync`}>Sync settings</a></Button></CardContent></Card>;
}
