import { O as dateTime, Q as require_jsx_runtime, a as Empty, c as Page, k as money, r as IntegrationBoundary, y as Card } from "./chunks/src-CJ7ON45K.js";

//#region ../app/integrations/[slug]/debug/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Debug() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Debug",
		description: "Provider staging records and normalization output for troubleshooting.",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
			className: "mb-3 text-sm font-semibold",
			children: "Captured provider records"
		}), data.captured_records.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No captured records",
			description: "Provider payloads will appear after a sync."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "space-y-3",
			children: data.captured_records.map((record, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
				className: "p-4",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("details", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("summary", {
					className: "cursor-pointer list-none",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center justify-between gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-sm font-medium",
							children: String(record.summary || record.external_id || "Provider record")
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
							className: "mt-1 font-mono text-xs text-muted-foreground",
							children: [
								String(record.entity_type || "record"),
								" · ",
								dateTime(record.updated_at)
							]
						})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-sm font-medium",
							children: typeof record.amount === "number" ? money(record.amount) : ""
						})]
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
					className: "mt-4 max-h-96 overflow-auto rounded-lg bg-foreground p-4 text-xs text-background",
					children: formatPayload(record.raw_payload)
				})] })
			}, index))
		})] })
	}) });
}
function formatPayload(value) {
	if (typeof value !== "string") return JSON.stringify(value, null, 2);
	try {
		return JSON.stringify(JSON.parse(value), null, 2);
	} catch {
		return value;
	}
}

//#endregion
export { Debug as default };