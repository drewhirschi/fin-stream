import type { ReactNode } from "react";
import {
  Activity,
  CalendarRange,
  CircleDollarSign,
  Inbox,
  Landmark,
  LayoutDashboard,
  LogOut,
  Menu,
  Network,
  X,
} from "lucide-react";
import { useRouterState } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";

const navigation = [
  { href: "/", label: "Overview", icon: LayoutDashboard, exact: true },
  { href: "/integrations", label: "Integrations", icon: Network },
  { href: "/inbox", label: "Inbox", icon: Inbox },
  { href: "/forecast", label: "Timeline", icon: CalendarRange },
  { href: "/canvas", label: "Canvas", icon: CircleDollarSign },
  { href: "/streams", label: "Streams", icon: Activity },
];

export function AppShell({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  useEffect(() => {
    if (!open) return;
    const previousOverflow = document.body.style.overflow;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.body.style.overflow = "hidden";
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);
  if (pathname === "/login") return <>{children}</>;

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="sticky top-0 z-40 flex min-h-14 items-center justify-between border-b bg-background/90 px-4 backdrop-blur lg:hidden">
        <Brand />
        <button className="rounded-md p-2 hover:bg-muted" onClick={() => setOpen(!open)} aria-label="Toggle navigation">
          {open ? <X className="size-5" /> : <Menu className="size-5" />}
        </button>
      </header>
      {open ? <button className="fixed inset-0 z-40 bg-black/20 lg:hidden" onClick={() => setOpen(false)} aria-label="Close navigation" /> : null}
      <aside className={cn("fixed inset-y-0 left-0 z-50 flex w-[min(18rem,88vw)] -translate-x-full flex-col overflow-hidden border-r bg-card transition-transform lg:w-64 lg:translate-x-0", open && "translate-x-0")}>
        <div className="flex h-16 items-center border-b px-5"><Brand /></div>
        <nav className="flex-1 space-y-1 overflow-y-auto p-3">
          {navigation.map(({ href, label, icon: Icon, exact }) => {
            const active = exact ? pathname === href : pathname.startsWith(href);
            return (
              <a key={href} href={href} onClick={() => setOpen(false)} className={cn("flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground", active && "bg-accent text-accent-foreground")}>
                <Icon className="size-4" />{label}
              </a>
            );
          })}
        </nav>
        <form method="post" action="/logout" className="border-t p-3 [padding-bottom:max(0.75rem,env(safe-area-inset-bottom))]">
          <button className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground" type="submit">
            <LogOut className="size-4" />Sign out
          </button>
        </form>
      </aside>
      <main className="min-w-0 lg:pl-64">{children}</main>
    </div>
  );
}

function Brand() {
  return <a href="/" className="flex items-center gap-2 font-semibold tracking-tight"><span className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground"><Landmark className="size-4" /></span><span>Trust Deeds</span></a>;
}
