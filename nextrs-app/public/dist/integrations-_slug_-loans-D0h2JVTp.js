import { D as date, Q as require_jsx_runtime, T as Badge, a as Empty, b as CardContent, c as Page, g as createLucideIcon, k as money, r as IntegrationBoundary, y as Card } from "./chunks/src-CJ7ON45K.js";

//#region node_modules/lucide-react/dist/esm/icons/map-pin.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const MapPin = createLucideIcon("MapPin", [["path", {
	d: "M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0",
	key: "1r0f0z"
}], ["circle", {
	cx: "12",
	cy: "10",
	r: "3",
	key: "ilqhr7"
}]]);

//#endregion
//#region ../app/integrations/[slug]/loans/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Loans() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Loans",
		description: `${data.loans.length} active loans imported from ${data.connection.name}.`,
		children: data.loans.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No active loans",
			description: "Run a sync to import current loan details."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "grid gap-4 md:grid-cols-2 xl:grid-cols-3",
			children: data.loans.map((loan) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href: `/integrations/${data.connection.slug}/loans/${encodeURIComponent(loan.loan_account)}`,
				className: "group",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "h-full overflow-hidden transition-all group-hover:border-primary/35 group-hover:shadow-md",
					children: [loan.featured_image_url ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
						src: loan.featured_image_url,
						alt: "",
						className: "h-36 w-full object-cover"
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "h-2 bg-accent" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "pt-5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-start justify-between gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
								className: "font-semibold",
								children: loan.borrower_name || loan.loan_account
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "mt-1 flex items-center gap-1 text-xs text-muted-foreground",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MapPin, { className: "size-3" }), [
									loan.property_address,
									loan.property_city,
									loan.property_state
								].filter(Boolean).join(", ") || "No property address"]
							})] }), loan.is_delinquent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								className: "border-amber-300 bg-amber-50 text-amber-900",
								children: "Attention"
							}) : null]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("dl", {
							className: "mt-5 grid grid-cols-2 gap-4 text-sm",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
									className: "text-xs text-muted-foreground",
									children: "Principal"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
									className: "mt-1 font-medium",
									children: money(loan.principal_balance)
								})] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
									className: "text-xs text-muted-foreground",
									children: "Payment"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
									className: "mt-1 font-medium",
									children: money(loan.regular_payment)
								})] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
									className: "text-xs text-muted-foreground",
									children: "Rate"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
									className: "mt-1 font-medium",
									children: loan.note_rate == null ? "—" : `${loan.note_rate.toFixed(3)}%`
								})] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
									className: "text-xs text-muted-foreground",
									children: "Maturity"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
									className: "mt-1 font-medium",
									children: date(loan.maturity_date)
								})] })
							]
						})]
					})]
				})
			}, loan.loan_account))
		})
	}) });
}

//#endregion
export { Loans as default };