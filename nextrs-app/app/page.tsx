import { ArrowRight, CalendarRange, Inbox, Network, Sparkles } from "lucide-react";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Page } from "@trust-deeds/client";

const cards = [
  { href: "/integrations", title: "Integrations", description: "Review connected financial sources, loans, payments, and sync health.", icon: Network },
  { href: "/forecast", title: "Cash timeline", description: "See projected balances and inspect upcoming income events.", icon: CalendarRange },
  { href: "/inbox", title: "Loan inbox", description: "Link inbound documents and correspondence to the right loan.", icon: Inbox },
];

export default function Dashboard() {
  return <Page title="Good evening" description="Your trust deed portfolio, imported activity, and income outlook in one place." actions={<Button asChild><a href="/integrations"><Sparkles className="size-4" />Review portfolio</a></Button>}><section className="grid gap-4 md:grid-cols-3">{cards.map(({ href, title, description, icon: Icon }) => <a key={href} href={href} className="group"><Card className="h-full transition-all group-hover:-translate-y-0.5 group-hover:border-primary/35 group-hover:shadow-md"><CardHeader><span className="mb-2 flex size-10 items-center justify-center rounded-lg bg-accent text-accent-foreground"><Icon className="size-5" /></span><CardTitle>{title}</CardTitle><CardDescription>{description}</CardDescription></CardHeader><CardContent><span className="inline-flex items-center gap-1 text-sm font-medium text-primary">Open <ArrowRight className="size-4 transition-transform group-hover:translate-x-0.5" /></span></CardContent></Card></a>)}</section><Card><CardHeader><CardTitle>How this workspace fits together</CardTitle><CardDescription>Imported provider activity becomes normalized income events. The timeline projects those events against cash, while the loan workspace keeps the underlying property and correspondence context nearby.</CardDescription></CardHeader></Card></Page>;
}
