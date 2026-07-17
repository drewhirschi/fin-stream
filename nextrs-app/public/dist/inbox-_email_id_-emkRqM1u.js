import { C as CardTitle, O as dateTime, Q as require_jsx_runtime, S as CardHeader, T as Badge, a as Empty, b as CardContent, c as Page, g as createLucideIcon, n as useApi, o as ErrorState, s as Loading, w as Button, x as CardDescription, y as Card } from "./chunks/src-CJ7ON45K.js";
import { n as Mail, t as Paperclip } from "./chunks/paperclip-DMmrdsTI.js";

//#region node_modules/lucide-react/dist/esm/icons/download.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const Download = createLucideIcon("Download", [
	["path", {
		d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4",
		key: "ih7n3h"
	}],
	["polyline", {
		points: "7 10 12 15 17 10",
		key: "2ggqvy"
	}],
	["line", {
		x1: "12",
		x2: "12",
		y1: "15",
		y2: "3",
		key: "1vk2je"
	}]
]);

//#endregion
//#region ../app/inbox/[email_id]/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function EmailDetail() {
	const id = Number(window.location.pathname.split("/").filter(Boolean)[1]);
	const query = useApi(["email", id], `/api/ui/inbox/${id}`);
	if (query.isLoading) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, { label: "Loading message" });
	if (query.error || !query.data) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error });
	const { email, attachments, recipients, loans } = query.data;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Page, {
		title: email.subject || "(no subject)",
		description: `Received ${dateTime(email.received_at)} from ${email.from_address}`,
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: email.processing_state }),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
			className: "grid gap-5 xl:grid-cols-[minmax(0,1fr)_360px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Mail, { className: "size-4" }), "Message"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: ["To ", recipients.join(", ")] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, { children: [email.body_s3_key ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					asChild: true,
					variant: "outline",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: `/media/emails/${email.body_s3_key.replace(/^emails\//, "")}`,
						target: "_blank",
						rel: "noreferrer",
						children: ["Open stored body ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, { className: "size-4" })]
					})
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm text-muted-foreground",
					children: "No stored message body is available."
				}), email.error_message ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "mt-4 rounded-lg border border-destructive/25 bg-destructive/5 p-3 text-sm text-destructive",
					children: email.error_message
				}) : null] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Paperclip, { className: "size-4" }), "Attachments"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
					attachments.length,
					" stored attachment",
					attachments.length === 1 ? "" : "s"
				] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: attachments.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
					title: "No attachments",
					description: "This message did not contain files."
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "divide-y rounded-lg border",
					children: attachments.map((attachment) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-between gap-3 p-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "truncate text-sm font-medium",
								children: attachment.filename
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "text-xs text-muted-foreground",
								children: [attachment.content_type, attachment.size_bytes ? ` · ${Math.ceil(attachment.size_bytes / 1024).toLocaleString()} KB` : ""]
							})]
						}), attachment.s3_key ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							asChild: true,
							size: "sm",
							variant: "ghost",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
								href: `/media/emails/${attachment.s3_key.replace(/^emails\//, "")}`,
								children: "Download"
							})
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: attachment.processing_state })]
					}, attachment.id))
				}) })] })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "h-fit",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Loan link" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Associate this message with an imported TMO loan." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: email.loan_account ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
						className: "text-sm",
						children: ["Linked to ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
							className: "font-medium text-primary",
							href: `/integrations/tmo/loans/${encodeURIComponent(email.loan_account)}`,
							children: email.loan_account
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						method: "post",
						action: `/inbox/${email.id}/unlink`,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "hidden",
							name: "return_to",
							value: "detail"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							variant: "outline",
							size: "sm",
							children: "Unlink"
						})]
					})]
				}) : loans.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm text-muted-foreground",
					children: "No active loans are available."
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
					method: "post",
					action: `/inbox/${email.id}/link`,
					className: "space-y-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							type: "hidden",
							name: "return_to",
							value: "detail"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
							name: "loan_account",
							required: true,
							className: "h-9 w-full rounded-md border bg-background px-3 text-sm",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
								value: "",
								children: "Select a loan"
							}), loans.map((loan) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("option", {
								value: loan.loan_account,
								children: [
									loan.borrower_name || loan.loan_account,
									" · ",
									loan.loan_account
								]
							}, loan.loan_account))]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							size: "sm",
							children: "Link message"
						})
					]
				}) })]
			})]
		})
	});
}

//#endregion
export { EmailDetail as default };