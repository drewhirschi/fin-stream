import { O as dateTime, Q as require_jsx_runtime, T as Badge, a as Empty, c as Page, n as useApi, o as ErrorState, s as Loading, w as Button, y as Card } from "./chunks/src-CJ7ON45K.js";
import { n as Mail, t as Paperclip } from "./chunks/paperclip-DMmrdsTI.js";

//#region ../app/inbox/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Inbox() {
	const showLinked = new URLSearchParams(window.location.search).get("show_linked") === "true";
	const query = useApi(["inbox", showLinked], `/api/ui/inbox?show_linked=${showLinked}`);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: "Inbox",
		description: "Inbound loan documents and correspondence waiting to be organized.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
			asChild: true,
			variant: "outline",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
				href: showLinked ? "/inbox" : "/inbox?show_linked=true",
				children: showLinked ? "Hide linked" : "Show linked"
			})
		}),
		children: query.isLoading ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, {}) : query.error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error }) : !query.data?.emails.length ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "Inbox is clear",
			description: showLinked ? "No messages have been received." : "Every received message has been linked to a loan."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
			className: "overflow-x-auto",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
				className: "data-table",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Received" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "From" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Subject" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Attachments" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Loan" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "State" })
				] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: query.data.emails.map(({ email, attachment_count }) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
						className: "whitespace-nowrap",
						children: dateTime(email.received_at)
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: email.from_address }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: `/inbox/${email.id}`,
						className: "inline-flex items-center gap-2 font-medium hover:text-primary",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Mail, { className: "size-4 text-muted-foreground" }), email.subject || "(no subject)"]
					}) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "inline-flex items-center gap-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Paperclip, { className: "size-3.5" }), attachment_count]
					}) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: email.loan_account || /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-muted-foreground",
						children: "Unlinked"
					}) }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: email.processing_state }) })
				] }, email.id)) })]
			})
		})
	});
}

//#endregion
export { Inbox as default };