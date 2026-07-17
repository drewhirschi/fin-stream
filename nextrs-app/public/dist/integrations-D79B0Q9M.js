import { O as dateTime, Q as require_jsx_runtime, T as Badge, a as Empty, b as CardContent, c as Page, g as createLucideIcon, n as useApi, o as ErrorState, s as Loading, y as Card } from "./chunks/src-CJ7ON45K.js";
import { t as ArrowRight } from "./chunks/arrow-right-CWIqpc4O.js";
import { t as RefreshCw } from "./chunks/refresh-cw-BCNHM6Ss.js";

//#region node_modules/lucide-react/dist/esm/icons/database.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Database = createLucideIcon("Database", [
	["ellipse", {
		cx: "12",
		cy: "5",
		rx: "9",
		ry: "3",
		key: "msslwz"
	}],
	["path", {
		d: "M3 5V19A9 3 0 0 0 21 19V5",
		key: "1wlel7"
	}],
	["path", {
		d: "M3 12A9 3 0 0 0 21 12",
		key: "mv7ke4"
	}]
]);

//#endregion
//#region ../app/integrations/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Integrations() {
	const query = useApi(["integrations"], "/api/ui/integrations");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Integrations",
		description: "Connected sources that feed portfolio details and income activity.",
		children: query.isLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, {}) : query.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error }) : !query.data?.connections.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No integrations",
			description: "Connect a provider to start importing portfolio data."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "grid gap-4 lg:grid-cols-2",
			children: query.data.connections.map((connection) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href: `/integrations/${connection.slug}`,
				className: "group",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
					className: "h-full transition-all group-hover:border-primary/35 group-hover:shadow-md",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "flex items-start gap-4 pt-5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "flex size-10 shrink-0 items-center justify-center rounded-lg bg-accent text-accent-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Database, { className: "size-5" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "flex flex-wrap items-center gap-2",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
										className: "font-semibold",
										children: connection.name
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: connection.status })]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
									className: "mt-1 text-sm text-muted-foreground",
									children: [
										connection.provider.replaceAll("_", " "),
										" · ",
										connection.record_count.toLocaleString(),
										" records"
									]
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "mt-4 flex items-center justify-between text-xs text-muted-foreground",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
										className: "inline-flex items-center gap-1",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "size-3.5" }), dateTime(connection.last_synced_at)]
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-4 transition-transform group-hover:translate-x-1" })]
								})
							]
						})]
					})
				})
			}, connection.id))
		})
	});
}

//#endregion
export { Integrations as default };