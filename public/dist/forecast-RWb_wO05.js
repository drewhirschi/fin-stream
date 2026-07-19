import { C as CardTitle, D as date, S as CardHeader, T as Badge, _ as Input, a as Empty, b as CardContent, c as Page, et as require_jsx_runtime, g as createLucideIcon, h as CalendarRange, k as money, n as useApi, o as ErrorState, s as Loading, t as api, w as Button, x as CardDescription, xt as __toESM, y as Card, yt as require_react } from "./chunks/src-DJBa3cvh.js";

//#region node_modules/lucide-react/dist/esm/icons/arrow-down-right.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const ArrowDownRight = createLucideIcon("ArrowDownRight", [["path", {
	d: "m7 7 10 10",
	key: "1fmybs"
}], ["path", {
	d: "M17 7v10H7",
	key: "6fjiku"
}]]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/arrow-up-right.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const ArrowUpRight = createLucideIcon("ArrowUpRight", [["path", {
	d: "M7 7h10v10",
	key: "1tivn9"
}], ["path", {
	d: "M7 17 17 7",
	key: "1vkiza"
}]]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/wallet.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Wallet = createLucideIcon("Wallet", [["path", {
	d: "M19 7V4a1 1 0 0 0-1-1H5a2 2 0 0 0 0 4h15a1 1 0 0 1 1 1v4h-3a2 2 0 0 0 0 4h3a1 1 0 0 0 1-1v-2a1 1 0 0 0-1-1",
	key: "18etb6"
}], ["path", {
	d: "M3 5v14a2 2 0 0 0 2 2h15a1 1 0 0 0 1-1v-4",
	key: "xoc0q4"
}]]);

//#endregion
//#region ../app/forecast/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function iso(offset) {
	const value = /* @__PURE__ */ new Date();
	value.setDate(value.getDate() + offset);
	return value.toISOString().slice(0, 10);
}
function Timeline() {
	const finance = useApi(["finance"], "/api/ui/finance");
	const defaultView = finance.data?.views.find((view) => view.is_default) ?? finance.data?.views[0];
	const path = `/api/forecast?from=${iso(-30)}&through=${iso(365)}${defaultView ? `&view_id=${defaultView.id}` : ""}`;
	const forecast = useApi(["forecast", path], path, { enabled: Boolean(finance.data) });
	const [cash, setCash] = (0, import_react.useState)("");
	const rows = forecast.data?.rows ?? [];
	const next = (0, import_react.useMemo)(() => rows.filter((row) => row.date >= iso(0)).slice(0, 30), [rows]);
	const saveCash = async () => {
		await api("/api/settings/cash", {
			method: "POST",
			body: JSON.stringify({
				amount: Number(cash),
				as_of_date: iso(0)
			})
		});
		await forecast.refetch();
		setCash("");
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Timeline",
		description: "Projected cash position from imported and manually scheduled income events.",
		children: finance.isLoading || forecast.isLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, { label: "Building timeline" }) : finance.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: finance.error }) : forecast.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Set your starting cash" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "A current cash anchor is required before the forecast can be calculated." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "flex max-w-md flex-col gap-2 sm:flex-row",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				type: "number",
				step: "0.01",
				value: cash,
				onChange: (event) => setCash(event.target.value),
				placeholder: "Current cash balance"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				onClick: saveCash,
				disabled: !cash,
				children: "Save"
			})]
		})] }) : forecast.data ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
			className: "grid grid-cols-1 gap-3 sm:grid-cols-3 sm:gap-4",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
					label: "Starting cash",
					value: money(forecast.data.starting_balance),
					icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Wallet, { className: "size-4" })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
					label: "Projected ending",
					value: money(forecast.data.ending_balance),
					icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CalendarRange, { className: "size-4" })
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Metric, {
					label: "Net change",
					value: money(forecast.data.ending_balance - forecast.data.starting_balance),
					icon: forecast.data.ending_balance >= forecast.data.starting_balance ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowUpRight, { className: "size-4" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowDownRight, { className: "size-4" })
				})
			]
		}), next.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No upcoming events",
			description: "Add a stream or run an integration sync to populate the timeline."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ForecastEvents, { rows: next })] }) : null
	});
}
function ForecastEvents({ rows }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "space-y-3 md:hidden",
		children: rows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "p-4",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-start justify-between gap-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-xs text-muted-foreground",
							children: date(row.date)
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
							className: "mt-1 break-words font-medium",
							children: row.label || "Scheduled event"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "mt-1 text-xs text-muted-foreground",
							children: row.stream_name || "No stream"
						})
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: row.direction === "inflow" ? "shrink-0 font-semibold text-primary" : "shrink-0 font-semibold text-destructive",
					children: [row.direction === "inflow" ? "+" : "−", money(Math.abs(row.amount))]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mt-4 flex items-center justify-between border-t pt-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: row.status }), row.is_late ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						className: "border-amber-300 bg-amber-50 text-amber-900",
						children: "Late"
					}) : null]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "text-right",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-muted-foreground",
						children: "Running balance"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-0.5 text-sm font-medium",
						children: money(row.running_balance)
					})]
				})]
			})]
		}) }, row.event_id))
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
		className: "hidden overflow-x-auto md:block",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
			className: "data-table",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Date" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Event" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Stream" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Status" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Amount" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Running balance" })
			] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: rows.map((row) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: date(row.date) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", {
					className: "font-medium",
					children: [row.label || "Scheduled event", row.is_late ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						className: "ml-2 border-amber-300 bg-amber-50 text-amber-900",
						children: "Late"
					}) : null]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: row.stream_name || "—" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: row.status }) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("td", {
					className: row.direction === "inflow" ? "text-primary" : "text-destructive",
					children: [row.direction === "inflow" ? "+" : "−", money(Math.abs(row.amount))]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
					className: "font-medium",
					children: money(row.running_balance)
				})
			] }, row.event_id)) })]
		})
	})] });
}
function Metric({ label, value, icon }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
		className: "p-4 sm:p-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex items-center justify-between text-muted-foreground",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-xs font-medium uppercase tracking-wide",
				children: label
			}), icon]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "mt-3 break-words text-2xl font-semibold",
			children: value
		})]
	}) });
}

//#endregion
export { Timeline as default };