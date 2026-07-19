import { C as CardTitle, S as CardHeader, b as CardContent, c as Page, et as require_jsx_runtime, f as Landmark, g as createLucideIcon, i as IntegrationSummary, k as money, r as IntegrationBoundary, x as CardDescription, y as Card } from "./chunks/src-DJBa3cvh.js";

//#region node_modules/lucide-react/dist/esm/icons/banknote.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Banknote = createLucideIcon("Banknote", [
	["rect", {
		width: "20",
		height: "12",
		x: "2",
		y: "6",
		rx: "2",
		key: "9lu3g6"
	}],
	["circle", {
		cx: "12",
		cy: "12",
		r: "2",
		key: "1c9p78"
	}],
	["path", {
		d: "M6 12h.01M18 12h.01",
		key: "113zkx"
	}]
]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/percent.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Percent = createLucideIcon("Percent", [
	["line", {
		x1: "19",
		x2: "5",
		y1: "5",
		y2: "19",
		key: "1x9vlm"
	}],
	["circle", {
		cx: "6.5",
		cy: "6.5",
		r: "2.5",
		key: "4mh3h7"
	}],
	["circle", {
		cx: "17.5",
		cy: "17.5",
		r: "2.5",
		key: "1mdrzq"
	}]
]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/triangle-alert.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const TriangleAlert = createLucideIcon("TriangleAlert", [
	["path", {
		d: "m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3",
		key: "wmoenq"
	}],
	["path", {
		d: "M12 9v4",
		key: "juzpu7"
	}],
	["path", {
		d: "M12 17h.01",
		key: "p32p05"
	}]
]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/wallet-cards.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const WalletCards = createLucideIcon("WalletCards", [
	["rect", {
		width: "18",
		height: "18",
		x: "3",
		y: "3",
		rx: "2",
		key: "afitv7"
	}],
	["path", {
		d: "M3 9a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2",
		key: "4125el"
	}],
	["path", {
		d: "M3 11h3c.8 0 1.6.3 2.1.9l1.1.9c1.6 1.6 4.1 1.6 5.7 0l1.1-.9c.5-.5 1.3-.9 2.1-.9H21",
		key: "1dpki6"
	}]
]);

//#endregion
//#region ../app/integrations/[slug]/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Overview() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data) => {
		const overview = data.overviews[0];
		const principal = data.loans.reduce((sum, loan) => sum + (loan.principal_balance ?? 0), 0);
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
			title: data.connection.name,
			description: "Portfolio position, current income, and the latest imported activity.",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationSummary, { data }),
				data.connection.last_error ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-3 rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-950",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TriangleAlert, { className: "mt-0.5 size-4 shrink-0" }), data.connection.last_error]
				}) : null,
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
					className: "grid gap-4 sm:grid-cols-2 xl:grid-cols-4",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
							label: "Portfolio value",
							value: money(overview?.portfolio_value ?? principal),
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Landmark, { className: "size-4" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
							label: "Trust balance",
							value: money(overview?.trust_balance),
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(WalletCards, { className: "size-4" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
							label: "Portfolio yield",
							value: overview?.portfolio_yield == null ? "—" : `${overview.portfolio_yield.toFixed(2)}%`,
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Percent, { className: "size-4" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
							label: "YTD interest",
							value: money(overview?.ytd_interest),
							icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Banknote, { className: "size-4" })
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
					className: "grid gap-4 lg:grid-cols-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Active loans" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [data.loans.length, " loans currently imported."] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
						className: "space-y-3",
						children: data.loans.slice(0, 6).map((loan) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
							href: `/integrations/${data.connection.slug}/loans/${encodeURIComponent(loan.loan_account)}`,
							className: "flex items-center justify-between rounded-lg border p-3 hover:bg-muted",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm font-medium",
								children: loan.borrower_name || loan.loan_account
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-xs text-muted-foreground",
								children: loan.property_address || loan.loan_account
							})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-sm font-medium",
								children: money(loan.principal_balance)
							})]
						}, loan.loan_account))
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Recent payments" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "The latest imported provider activity." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
						className: "space-y-3",
						children: data.payments.slice(0, 6).map((payment) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-between border-b pb-3 last:border-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm font-medium",
								children: payment.borrower_name || payment.loan_account
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "text-xs text-muted-foreground",
								children: [
									payment.check_date,
									" · ",
									payment.loan_account
								]
							})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-sm font-medium text-primary",
								children: money(payment.amount)
							})]
						}, payment.id))
					})] })]
				})
			]
		});
	} });
}
function Metric({ label, value, icon }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
		className: "pt-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center justify-between text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-xs font-medium uppercase tracking-wide",
				children: label
			}), icon]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "mt-3 text-2xl font-semibold",
			children: value
		})]
	}) });
}

//#endregion
export { Overview as default };