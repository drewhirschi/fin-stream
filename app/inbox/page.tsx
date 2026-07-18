import { Mail, Paperclip } from "lucide-react";
import {
  Badge,
  Button,
  Card,
  CardContent,
  Empty,
  ErrorState,
  Loading,
  Page,
  dateTime,
  useApi,
  type InboxData,
} from "@trust-deeds/client";

export default function Inbox() {
  const showLinked = new URLSearchParams(window.location.search).get("show_linked") === "true";
  const query = useApi<InboxData>(["inbox", showLinked], `/api/ui/inbox?show_linked=${showLinked}`);
  const emails = query.data?.emails ?? [];

  return (
    <Page
      title="Inbox"
      description="Inbound loan documents and correspondence waiting to be organized."
      actions={<Button asChild variant="outline"><a href={showLinked ? "/inbox" : "/inbox?show_linked=true"}>{showLinked ? "Hide linked" : "Show linked"}</a></Button>}
    >
      {query.isLoading ? <Loading /> : query.error ? <ErrorState error={query.error} /> : emails.length === 0 ? (
        <Empty title="Inbox is clear" description={showLinked ? "No messages have been received." : "Every received message has been linked to a loan."} />
      ) : (
        <>
          <div className="space-y-3 md:hidden">
            {emails.map(({ email, attachment_count }) => (
              <a key={email.id} href={`/inbox/${email.id}`} className="block">
                <Card className="transition-colors active:bg-muted/60">
                  <CardContent className="p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="text-xs text-muted-foreground">{dateTime(email.received_at)}</p>
                        <h2 className="mt-1 break-words font-medium">{email.subject || "(no subject)"}</h2>
                        <p className="mt-1 truncate text-sm text-muted-foreground">{email.from_address}</p>
                      </div>
                      <Badge className="shrink-0">{email.processing_state}</Badge>
                    </div>
                    <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 border-t pt-3 text-xs text-muted-foreground">
                      <span className="inline-flex items-center gap-1"><Paperclip className="size-3.5" />{attachment_count} attachment{attachment_count === 1 ? "" : "s"}</span>
                      <span className="inline-flex items-center gap-1"><Mail className="size-3.5" />{email.loan_account || "Unlinked"}</span>
                    </div>
                  </CardContent>
                </Card>
              </a>
            ))}
          </div>

          <Card className="hidden overflow-x-auto md:block">
            <table className="data-table">
              <thead><tr><th>Received</th><th>From</th><th>Subject</th><th>Attachments</th><th>Loan</th><th>State</th></tr></thead>
              <tbody>{emails.map(({ email, attachment_count }) => (
                <tr key={email.id}>
                  <td className="whitespace-nowrap">{dateTime(email.received_at)}</td>
                  <td>{email.from_address}</td>
                  <td><a href={`/inbox/${email.id}`} className="inline-flex items-center gap-2 font-medium hover:text-primary"><Mail className="size-4 text-muted-foreground" />{email.subject || "(no subject)"}</a></td>
                  <td><span className="inline-flex items-center gap-1"><Paperclip className="size-3.5" />{attachment_count}</span></td>
                  <td>{email.loan_account || <span className="text-muted-foreground">Unlinked</span>}</td>
                  <td><Badge>{email.processing_state}</Badge></td>
                </tr>
              ))}</tbody>
            </table>
          </Card>
        </>
      )}
    </Page>
  );
}
