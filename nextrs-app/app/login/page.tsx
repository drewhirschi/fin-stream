import { Landmark, LoaderCircle } from "lucide-react";
import { useState } from "react";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input } from "@trust-deeds/client";

export default function Login() {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault(); setBusy(true); setError("");
    const form = new FormData(event.currentTarget);
    try {
      const response = await fetch("/login", { method: "POST", body: new URLSearchParams({ email: String(form.get("email") || ""), password: String(form.get("password") || "") }), credentials: "same-origin", headers: { "Content-Type": "application/x-www-form-urlencoded", "Sec-Fetch-Site": "same-origin" } });
      if (response.ok) window.location.assign("/"); else setError(await response.text());
    } catch { setError("Could not reach the server. Try again."); } finally { setBusy(false); }
  };
  return <main className="grid min-h-screen bg-muted/40 lg:grid-cols-2"><section className="hidden flex-col justify-between bg-foreground p-12 text-background lg:flex"><div className="flex items-center gap-3 font-semibold"><span className="flex size-9 items-center justify-center rounded-lg bg-primary"><Landmark className="size-5" /></span>Trust Deeds</div><div className="max-w-lg"><p className="text-4xl font-semibold leading-tight tracking-tight">A clear view of portfolio income, without the spreadsheet archaeology.</p><p className="mt-5 text-base text-background/65">Connected loan data, payment history, correspondence, and forward cash planning in one private workspace.</p></div><p className="text-xs text-background/45">Private financial workspace</p></section><section className="flex items-center justify-center p-6"><Card className="w-full max-w-md"><CardHeader><div className="mb-4 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground lg:hidden"><Landmark className="size-5" /></div><CardTitle className="text-2xl">Welcome back</CardTitle><CardDescription>Sign in to your Trust Deeds workspace.</CardDescription></CardHeader><CardContent><form onSubmit={submit} className="space-y-4"><label className="grid gap-1.5 text-sm"><span className="font-medium">Email</span><Input name="email" type="email" autoComplete="email" required autoFocus /></label><label className="grid gap-1.5 text-sm"><span className="font-medium">Password</span><Input name="password" type="password" autoComplete="current-password" required /></label>{error ? <p className="rounded-md border border-destructive/25 bg-destructive/5 p-3 text-sm text-destructive">{error}</p> : null}<Button className="w-full" type="submit" disabled={busy}>{busy ? <><LoaderCircle className="size-4 animate-spin" />Signing in…</> : "Sign in"}</Button></form></CardContent></Card></section></main>;
}
