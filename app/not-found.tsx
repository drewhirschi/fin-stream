import { Button } from "@trust-deeds/client";

export default function NotFound() {
  return <main className="flex min-h-screen flex-col items-center justify-center gap-4 p-6 text-center"><p className="text-sm font-medium text-primary">404</p><h1 className="text-3xl font-semibold">Page not found</h1><p className="text-muted-foreground">The page may have moved or no longer exists.</p><Button asChild><a href="/">Return home</a></Button></main>;
}
