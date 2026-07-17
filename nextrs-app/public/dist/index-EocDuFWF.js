import { C as CardTitle, Q as require_jsx_runtime, S as CardHeader, b as CardContent, c as Page, g as createLucideIcon, h as CalendarRange, p as Inbox, u as Network, w as Button, x as CardDescription, y as Card } from "./chunks/src-CJ7ON45K.js";
import { t as ArrowRight } from "./chunks/arrow-right-CWIqpc4O.js";

//#region node_modules/lucide-react/dist/esm/icons/sparkles.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Sparkles = createLucideIcon("Sparkles", [
	["path", {
		d: "M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z",
		key: "4pj2yx"
	}],
	["path", {
		d: "M20 3v4",
		key: "1olli1"
	}],
	["path", {
		d: "M22 5h-4",
		key: "1gvqau"
	}],
	["path", {
		d: "M4 17v2",
		key: "vumght"
	}],
	["path", {
		d: "M5 18H3",
		key: "zchphs"
	}]
]);

//#endregion
//#region ../app/page.tsx
var import_jsx_runtime = require_jsx_runtime();
const cards = [
	{
		href: "/integrations",
		title: "Integrations",
		description: "Review connected financial sources, loans, payments, and sync health.",
		icon: Network
	},
	{
		href: "/forecast",
		title: "Cash timeline",
		description: "See projected balances and inspect upcoming income events.",
		icon: CalendarRange
	},
	{
		href: "/inbox",
		title: "Loan inbox",
		description: "Link inbound documents and correspondence to the right loan.",
		icon: Inbox
	}
];
function Dashboard() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: "Good evening",
		description: "Your trust deed portfolio, imported activity, and income outlook in one place.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			asChild: true,
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
				href: "/integrations",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "size-4" }), "Review portfolio"]
			})
		}),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
			className: "grid gap-4 md:grid-cols-3",
			children: cards.map(({ href, title, description, icon: Icon }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href,
				className: "group",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "h-full transition-all group-hover:-translate-y-0.5 group-hover:border-primary/35 group-hover:shadow-md",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "mb-2 flex size-10 items-center justify-center rounded-lg bg-accent text-accent-foreground",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "size-5" })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: title }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: description })
					] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "inline-flex items-center gap-1 text-sm font-medium text-primary",
						children: ["Open ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "size-4 transition-transform group-hover:translate-x-0.5" })]
					}) })]
				})
			}, href))
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "How this workspace fits together" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Imported provider activity becomes normalized income events. The timeline projects those events against cash, while the loan workspace keeps the underlying property and correspondence context nearby." })] }) })]
	});
}

//#endregion
export { Dashboard as default };