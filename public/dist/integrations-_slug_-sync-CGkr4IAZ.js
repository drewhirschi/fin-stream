import { C as CardTitle, O as dateTime, Q as require_jsx_runtime, S as CardHeader, T as Badge, _t as __toESM, a as Empty, c as Page, g as createLucideIcon, ht as require_react, r as IntegrationBoundary, w as Button, x as CardDescription, y as Card } from "./chunks/src-ZnV_ftAe.js";

//#region node_modules/lucide-react/dist/esm/icons/play.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Play = createLucideIcon("Play", [["polygon", {
	points: "6 3 20 12 6 21 6 3",
	key: "1oa8hb"
}]]);

//#endregion
//#region node_modules/lucide-react/dist/esm/icons/shield-check.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const ShieldCheck = createLucideIcon("ShieldCheck", [["path", {
	d: "M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z",
	key: "oel41y"
}], ["path", {
	d: "m9 12 2 2 4-4",
	key: "dzmm74"
}]]);

//#endregion
//#region ../app/integrations/[slug]/sync/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function Sync() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data, slug) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SyncView, {
		data,
		slug
	}) });
}
function SyncView({ data, slug }) {
	const [running, setRunning] = (0, import_react.useState)(false);
	const run = async () => {
		setRunning(true);
		try {
			await fetch(`/integrations/${encodeURIComponent(slug)}/sync/run`, {
				method: "POST",
				credentials: "same-origin",
				headers: { "Sec-Fetch-Site": "same-origin" }
			});
			window.location.reload();
		} finally {
			setRunning(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: "Sync",
		description: "Provider refresh history and operational controls.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			onClick: run,
			disabled: running || data.control.mode !== "enabled",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "size-4" }), running ? "Starting…" : "Run sync"]
		}),
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "grid gap-4 md:grid-cols-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Write mode" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "capitalize",
						children: data.control.mode.replaceAll("_", " ")
					})] }) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Scheduler" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: data.control.scheduler_enabled ? "Enabled" : "Paused" })] }) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Cadence" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "capitalize",
						children: data.connection.sync_cadence.replaceAll("_", " ")
					})] }) })
				]
			}),
			data.control.mode !== "enabled" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex gap-3 rounded-xl border bg-muted p-4 text-sm text-muted-foreground",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldCheck, { className: "size-5 shrink-0" }), "This imported database is intentionally read-only. Enable writes during the final cutover before running provider syncs."]
			}) : null,
			data.sync_logs.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
				title: "No sync history",
				description: "The first execution will appear here."
			}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
				className: "overflow-x-auto",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
					className: "data-table",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Started" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Status" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Loans" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Events" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Snapshots" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Error" })
					] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: data.sync_logs.map((log) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: dateTime(log.started_at) }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: log.status }) }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: log.loans_upserted.toLocaleString() }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: log.events_upserted.toLocaleString() }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: log.snapshots_created.toLocaleString() }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
							className: "max-w-sm text-xs text-muted-foreground",
							children: log.error_message || "—"
						})
					] }, log.id)) })]
				})
			})
		]
	});
}

//#endregion
export { Sync as default };