import { D as date, T as Badge, _ as Input, a as Empty, b as CardContent, c as Page, et as require_jsx_runtime, g as createLucideIcon, k as money, n as useApi, o as ErrorState, s as Loading, t as api, w as Button, xt as __toESM, y as Card, yt as require_react } from "./chunks/src-DJBa3cvh.js";
import { t as RefreshCw } from "./chunks/refresh-cw-CAquR-xU.js";

//#region node_modules/lucide-react/dist/esm/icons/plus.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Plus = createLucideIcon("Plus", [["path", {
	d: "M5 12h14",
	key: "1ays0h"
}], ["path", {
	d: "M12 5v14",
	key: "s699le"
}]]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/trash-2.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Trash2 = createLucideIcon("Trash2", [
	["path", {
		d: "M3 6h18",
		key: "d0wm0j"
	}],
	["path", {
		d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6",
		key: "4alrt4"
	}],
	["path", {
		d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2",
		key: "v07s0e"
	}],
	["line", {
		x1: "10",
		x2: "10",
		y1: "11",
		y2: "17",
		key: "1uufr5"
	}],
	["line", {
		x1: "14",
		x2: "14",
		y1: "11",
		y2: "17",
		key: "xtxkd"
	}]
]);

//#endregion
//#region ../app/streams/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function Streams() {
	const query = useApi(["finance"], "/api/ui/finance");
	const [adding, setAdding] = (0, import_react.useState)(false);
	const [busy, setBusy] = (0, import_react.useState)(false);
	const add = async (event) => {
		event.preventDefault();
		setBusy(true);
		const form = new FormData(event.currentTarget);
		try {
			await api("/api/streams", {
				method: "POST",
				body: JSON.stringify({
					name: form.get("name"),
					kind: form.get("kind"),
					schedule_amount: Number(form.get("amount")),
					schedule_frequency: form.get("frequency"),
					due_day: form.get("due_day") ? Number(form.get("due_day")) : null,
					start_date: (/* @__PURE__ */ new Date()).toISOString().slice(0, 10)
				})
			});
			setAdding(false);
			await query.refetch();
		} finally {
			setBusy(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: "Streams",
		description: "Recurring and one-off income sources that drive the cash timeline.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			variant: "outline",
			size: "icon",
			onClick: () => query.refetch(),
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-4" })
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			onClick: () => setAdding(!adding),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "size-4" }), "New stream"]
		})] }),
		children: [adding ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
			className: "pt-5",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
				onSubmit: add,
				className: "grid gap-3 md:grid-cols-5",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						name: "name",
						placeholder: "Stream name",
						required: true
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
						name: "kind",
						className: "h-9 rounded-md border bg-background px-3 text-sm",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
							value: "manual_income",
							children: "Income"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
							value: "manual_expense",
							children: "Expense"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						name: "amount",
						type: "number",
						step: "0.01",
						placeholder: "Amount",
						required: true
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
						name: "frequency",
						className: "h-9 rounded-md border bg-background px-3 text-sm",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
								value: "monthly",
								children: "Monthly"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
								value: "weekly",
								children: "Weekly"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
								value: "one_time",
								children: "One time"
							})
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							name: "due_day",
							type: "number",
							min: "1",
							max: "31",
							placeholder: "Day"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							disabled: busy,
							children: busy ? "Saving…" : "Add"
						})]
					})
				]
			})
		}) }) : null, query.isLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, {}) : query.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error }) : !query.data?.streams.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No streams",
			description: "Create an income or expense stream to start forecasting."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "space-y-3",
			children: query.data.streams.map((stream) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "grid gap-4 pt-5 md:grid-cols-[minmax(0,1fr)_repeat(3,140px)_auto] md:items-center",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
							className: "font-medium",
							children: stream.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: stream.kind.replaceAll("_", " ") })]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-1 text-xs text-muted-foreground",
						children: stream.description || `${stream.schedules.length} active schedule${stream.schedules.length === 1 ? "" : "s"}`
					})] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Datum, {
						label: "Amount",
						value: money(stream.schedule_amount)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Datum, {
						label: "Frequency",
						value: stream.schedule_frequency?.replaceAll("_", " ") || "—"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Datum, {
						label: "Starts",
						value: date(stream.schedules[0]?.start_date)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "icon",
						variant: "ghost",
						"aria-label": "Delete stream",
						onClick: async () => {
							if (confirm(`Delete ${stream.name}?`)) {
								await api(`/api/streams/${stream.id}`, { method: "DELETE" });
								query.refetch();
							}
						},
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "size-4 text-destructive" })
					})
				]
			}) }, stream.id))
		})]
	});
}
function Datum({ label, value }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "text-xs text-muted-foreground",
		children: label
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "mt-1 text-sm font-medium capitalize",
		children: value
	})] });
}

//#endregion
export { Streams as default };