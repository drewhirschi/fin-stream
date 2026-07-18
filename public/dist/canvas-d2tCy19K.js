import { Q as require_jsx_runtime, a as Empty, b as CardContent, c as Page, m as CircleDollarSign, n as useApi, o as ErrorState, s as Loading, y as Card } from "./chunks/src-ZnV_ftAe.js";

//#region ../app/canvas/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Canvas() {
	const query = useApi(["finance"], "/api/ui/finance");
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Canvas",
		description: "A flexible visual map of the streams feeding your income plan.",
		children: query.isLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, {}) : query.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error }) : !query.data?.canvas_streams.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "Nothing to map yet",
			description: "Create a stream to add it to the canvas."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "relative min-h-[65vh] overflow-hidden rounded-2xl border bg-[radial-gradient(circle_at_center,var(--border)_1px,transparent_1px)] [background-size:24px_24px] p-8",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-6 md:grid-cols-2 xl:grid-cols-3",
				children: query.data.canvas_streams.map((stream, index) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
					className: "relative",
					style: { transform: `translateY(${index % 3 * 18}px)` },
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "flex items-center gap-4 pt-5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "flex size-11 items-center justify-center rounded-full bg-accent text-accent-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CircleDollarSign, { className: "size-5" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
							className: "font-medium",
							children: stream.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "mt-1 text-xs capitalize text-muted-foreground",
							children: stream.kind.replaceAll("_", " ")
						})] })]
					})
				}, stream.id))
			})
		})
	});
}

//#endregion
export { Canvas as default };