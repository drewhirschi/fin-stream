import { D as date, Q as require_jsx_runtime, T as Badge, a as Empty, b as CardContent, c as Page, k as money, r as IntegrationBoundary, y as Card } from "./chunks/src-ZnV_ftAe.js";

//#region ../app/integrations/[slug]/payments/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Payments() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Payments",
		description: `The 100 most recent imported payments from ${data.connection.name}.`,
		children: data.payments.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No payments",
			description: "Payments will appear after the provider has been synced."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "space-y-3 md:hidden",
			children: data.payments.map((payment) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "p-4",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-xs text-muted-foreground",
								children: date(payment.check_date)
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
								className: "mt-1 truncate font-medium",
								children: payment.borrower_name
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-1 font-mono text-xs text-muted-foreground",
								children: payment.loan_account
							})
						]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "shrink-0 font-semibold text-primary",
						children: money(payment.amount)
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "mt-4 grid grid-cols-3 gap-2 border-t pt-3 text-sm",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Datum, {
							label: "Interest",
							value: money(payment.interest)
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Datum, {
							label: "Principal",
							value: money(payment.principal)
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: "Check"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-1",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: payment.check_number || "Pending" })
						})] })
					]
				})]
			}) }, payment.id))
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
			className: "hidden overflow-x-auto md:block",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
				className: "data-table",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Date" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Borrower" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Loan" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Check" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Interest" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Principal" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Total" })
				] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: data.payments.map((payment) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: date(payment.check_date) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
						className: "font-medium",
						children: payment.borrower_name
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
						className: "font-mono text-xs",
						children: payment.loan_account
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: payment.check_number || "—" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: money(payment.interest) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: money(payment.principal) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
						className: "font-medium text-primary",
						children: money(payment.amount)
					})
				] }, payment.id)) })]
			})
		})] })
	}) });
}
function Datum({ label, value }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "text-xs text-muted-foreground",
		children: label
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "mt-1 font-medium",
		children: value
	})] });
}

//#endregion
export { Payments as default };