import type { ReactNode } from "react";
import { AlertCircle, LoaderCircle } from "lucide-react";
import { Button } from "@/components/ui/button";

export function Page({ title, description, actions, children }: { title: string; description?: string; actions?: ReactNode; children: ReactNode }) {
  return <div className="mx-auto flex min-w-0 w-full max-w-[1500px] flex-col gap-5 p-4 sm:gap-6 sm:p-6 lg:p-8"><header className="flex flex-col gap-3 border-b pb-4 sm:flex-row sm:items-end sm:justify-between sm:pb-5"><div className="min-w-0"><h1 className="break-words text-2xl font-semibold tracking-tight sm:text-3xl">{title}</h1>{description ? <p className="mt-1.5 max-w-3xl text-sm text-muted-foreground">{description}</p> : null}</div>{actions ? <div className="flex w-full flex-wrap gap-2 sm:w-auto">{actions}</div> : null}</header>{children}</div>;
}

export function Loading({ label = "Loading" }: { label?: string }) {
  return <div className="flex min-h-64 items-center justify-center gap-2 text-sm text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{label}</div>;
}

export function ErrorState({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : "Something went wrong.";
  return <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-xl border border-destructive/25 bg-destructive/5 p-6 text-center"><AlertCircle className="size-6 text-destructive" /><div><p className="font-medium">Couldn’t load this view</p><p className="mt-1 text-sm text-muted-foreground">{message}</p></div><Button variant="outline" onClick={() => window.location.reload()}>Try again</Button></div>;
}

export function Empty({ title, description }: { title: string; description: string }) {
  return <div className="flex min-h-48 flex-col items-center justify-center rounded-xl border border-dashed p-8 text-center"><p className="font-medium">{title}</p><p className="mt-1 max-w-md text-sm text-muted-foreground">{description}</p></div>;
}
