import type { ReactNode } from "react";
import { AppShell } from "@trust-deeds/client";

export default function Layout({ children }: { children: ReactNode }) {
  return <AppShell><title>Trust Deeds</title><link rel="icon" href="/static/favicon.ico" />{children}</AppShell>;
}
