import { D as date, Q as require_jsx_runtime, a as Empty, c as Page, k as money, r as IntegrationBoundary, y as Card } from "./chunks/src-CJ7ON45K.js";

//#region ../app/integrations/[slug]/payments/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Payments() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Payments",
		description: `The 100 most recent imported payments from ${data.connection.name}.`,
		children: data.payments.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No payments",
			description: "Payments will appear after the provider has been synced."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
			className: "overflow-x-auto",
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
		})
	}) });
}

//#endregion
export { Payments as default };