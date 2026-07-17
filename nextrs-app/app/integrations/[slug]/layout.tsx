import type { ReactNode } from "react";
import { useRouterState } from "@tanstack/react-router";
import { cn } from "@/lib/utils";

export default function IntegrationLayout({ children }: { children: ReactNode }) {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const parts = pathname.split("/").filter(Boolean);
  const slug = parts[1] ?? "tmo";
  const current = parts[2] ?? "overview";
  const tabs = [
    ["overview", "Overview", `/integrations/${slug}`],
    ["loans", "Loans", `/integrations/${slug}/loans`],
    ["payments", "Payments", `/integrations/${slug}/payments`],
    ["sync", "Sync", `/integrations/${slug}/sync`],
    ["debug", "Debug", `/integrations/${slug}/debug`],
  ];
  return <div><div className="sticky top-14 z-30 border-b bg-background/95 px-4 backdrop-blur lg:top-0 lg:px-8"><nav className="mx-auto flex max-w-[1500px] gap-1 overflow-x-auto py-2">{tabs.map(([key, label, href]) => <a key={key} href={href} className={cn("rounded-md px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-muted hover:text-foreground", current === key && "bg-accent text-accent-foreground")}>{label}</a>)}</nav></div>{children}</div>;
}
