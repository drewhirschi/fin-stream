import { C as CardTitle, O as dateTime, Q as require_jsx_runtime, S as CardHeader, T as Badge, _t as __toESM, a as Empty, c as Page, d as LoaderCircle, g as createLucideIcon, ht as require_react, n as useApi, r as IntegrationBoundary, w as Button, x as CardDescription, y as Card } from "./chunks/src-ZnV_ftAe.js";

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
	const initialRun = data.sync_logs.find((run) => run.status === "running") ?? data.sync_logs[0] ?? null;
	const [submitting, setSubmitting] = (0, import_react.useState)(false);
	const [error, setError] = (0, import_react.useState)(null);
	const [watchingForAutomaticRun, setWatchingForAutomaticRun] = (0, import_react.useState)(true);
	const sawRunning = (0, import_react.useRef)(initialRun?.status === "running");
	const status = useApi(["integration-sync-status", slug], `/integrations/${encodeURIComponent(slug)}/sync/status`, {
		initialData: { run: initialRun },
		refetchInterval: (query) => query.state.data?.run?.status === "running" || watchingForAutomaticRun ? 1500 : false
	});
	const durableRunning = status.data?.run?.status === "running";
	const busy = submitting || durableRunning;
	(0, import_react.useEffect)(() => {
		const timeout = window.setTimeout(() => setWatchingForAutomaticRun(false), 1e4);
		return () => window.clearTimeout(timeout);
	}, []);
	(0, import_react.useEffect)(() => {
		if (durableRunning) {
			sawRunning.current = true;
			return;
		}
		if (sawRunning.current && status.data) {
			sawRunning.current = false;
			window.location.reload();
		}
	}, [durableRunning, status.data]);
	const run = async () => {
		setSubmitting(true);
		setError(null);
		try {
			const response = await fetch(`/integrations/${encodeURIComponent(slug)}/sync/run`, {
				method: "POST",
				credentials: "same-origin",
				headers: { Accept: "application/json" }
			});
			if (!response.ok && response.status !== 409) throw new Error(`The sync could not be started (${response.status}).`);
			if (response.status === 409) {
				if ((await status.refetch()).data?.run?.status !== "running") throw new Error("The sync could not be started. Check the integration configuration and try again.");
				return;
			}
			window.location.reload();
		} catch (runError) {
			setError(runError instanceof Error ? runError.message : "The sync could not be started.");
		} finally {
			setSubmitting(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: "Sync",
		description: "Provider refresh history and operational controls.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			onClick: run,
			disabled: busy || data.control.mode !== "enabled",
			children: [busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "size-4" }), busy ? "Syncing…" : "Run sync"]
		}),
		children: [
			durableRunning ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex gap-3 rounded-xl border border-primary/25 bg-accent p-4 text-sm text-accent-foreground",
				"aria-live": "polite",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-5 shrink-0 animate-spin" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "font-medium",
					children: "Sync in progress"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: "mt-1 opacity-80",
					children: [
						"Started ",
						dateTime(status.data?.run?.started_at),
						". This page will update automatically when it finishes."
					]
				})] })]
			}) : null,
			error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive",
				children: error
			}) : null,
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