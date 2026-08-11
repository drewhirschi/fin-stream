import {
  Badge,
  Card,
  CardContent,
  Empty,
  IntegrationBoundary,
  Page,
  PendingCheckBadge,
  date,
  isPendingCheck,
  money,
  pendingCheckSurface,
  type IntegrationData,
} from "@trust-deeds/client";

export default function Payments() {
  return (
    <IntegrationBoundary>
      {(data) => <PaymentsView data={data} />}
    </IntegrationBoundary>
  );
}

export function PaymentsView({ data }: { data: IntegrationData }) {
  return (
    <Page
      title="Payments"
      description={`The 100 most recent imported payments from ${data.connection.name}.`}
    >
      {data.payments.length === 0 ? (
        <Empty
          title="No payments"
          description="Payments will appear after the provider has been synced."
        />
      ) : (
        <>
          <div className="space-y-3 md:hidden" data-layout="mobile">
            {data.payments.map((payment) => {
              const pending = isPendingCheck(payment.check_number);
              return (
                <Card
                  key={payment.id}
                  className={pending ? pendingCheckSurface.bordered : undefined}
                  data-payment-state={pending ? "pending" : "processed"}
                >
                  <CardContent className="p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="text-xs text-muted-foreground">
                          {date(payment.check_date)}
                        </p>
                        <h2 className="mt-1 truncate font-medium">
                          {payment.borrower_name}
                        </h2>
                        <p className="mt-1 font-mono text-xs text-muted-foreground">
                          {payment.loan_account}
                        </p>
                      </div>
                      <p className="shrink-0 font-semibold text-primary">
                        {money(payment.amount)}
                      </p>
                    </div>
                    <div className="mt-4 grid grid-cols-3 gap-2 border-t pt-3 text-sm">
                      <Datum label="Interest" value={money(payment.interest)} />
                      <Datum
                        label="Principal"
                        value={money(payment.principal)}
                      />
                      <div>
                        <p className="text-xs text-muted-foreground">Check</p>
                        <div className="mt-1">
                          {pending ? (
                            <PendingCheckBadge />
                          ) : (
                            <Badge>{payment.check_number}</Badge>
                          )}
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              );
            })}
          </div>

          <Card
            className="hidden overflow-x-auto md:block"
            data-layout="desktop"
          >
            <table className="data-table">
              <thead>
                <tr>
                  <th>Date</th>
                  <th>Borrower</th>
                  <th>Loan</th>
                  <th>Check</th>
                  <th>Interest</th>
                  <th>Principal</th>
                  <th>Total</th>
                </tr>
              </thead>
              <tbody>
                {data.payments.map((payment) => {
                  const pending = isPendingCheck(payment.check_number);
                  return (
                    <tr
                      key={payment.id}
                      className={pending ? pendingCheckSurface.row : undefined}
                      data-payment-state={pending ? "pending" : "processed"}
                    >
                      <td>{date(payment.check_date)}</td>
                      <td className="font-medium">{payment.borrower_name}</td>
                      <td className="font-mono text-xs">
                        {payment.loan_account}
                      </td>
                      <td>
                        {pending ? <PendingCheckBadge /> : payment.check_number}
                      </td>
                      <td>{money(payment.interest)}</td>
                      <td>{money(payment.principal)}</td>
                      <td className="font-medium text-primary">
                        {money(payment.amount)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </Card>
        </>
      )}
    </Page>
  );
}

function Datum({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="mt-1 font-medium">{value}</p>
    </div>
  );
}
