import type { ReactNode } from "react";
import { AppShell } from "@trust-deeds/client";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <AppShell>
      <title>Trust Deeds</title>
      <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
      <link rel="icon" href="/static/favicon.ico" />
      <link rel="apple-touch-icon" sizes="180x180" href="/static/apple-touch-icon.png" />
      <link rel="manifest" href="/static/manifest.webmanifest" />
      <meta name="theme-color" content="#FBFAF6" />
      <meta name="apple-mobile-web-app-capable" content="yes" />
      <meta name="apple-mobile-web-app-status-bar-style" content="default" />
      <meta name="apple-mobile-web-app-title" content="Trust Deeds" />
      {children}
    </AppShell>
  );
}
