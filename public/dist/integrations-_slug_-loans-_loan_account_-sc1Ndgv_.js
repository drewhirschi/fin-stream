import { C as CardTitle, D as date, E as cn, O as dateTime, S as CardHeader, T as Badge, _ as Input, a as Empty, b as CardContent, c as Page, et as require_jsx_runtime, g as createLucideIcon, k as money, n as useApi, o as ErrorState, s as Loading, v as Textarea, w as Button, x as CardDescription, xt as __toESM, y as Card, yt as require_react } from "./chunks/src-DJBa3cvh.js";

//#region node_modules/lucide-react/dist/esm/icons/external-link.js
/**
* @license lucide-react v0.468.0 - ISC
*
* This source code is licensed under the ISC license.
* See the LICENSE file in the root directory of this source tree.
*/
const ExternalLink = createLucideIcon("ExternalLink", [
	["path", {
		d: "M15 3h6v6",
		key: "1q9fwt"
	}],
	["path", {
		d: "M10 14 21 3",
		key: "gplh6r"
	}],
	["path", {
		d: "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
		key: "a6xqqp"
	}]
]);

//#endregion
//#region ../app/integrations/[slug]/loans/[loan_account]/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function Loan() {
	const parts = window.location.pathname.split("/").filter(Boolean);
	const slug = parts[1] ?? "tmo";
	const account = decodeURIComponent(parts[3] ?? "");
	const query = useApi([
		"loan",
		slug,
		account
	], `/api/ui/integrations/${encodeURIComponent(slug)}/loans/${encodeURIComponent(account)}`);
	if (query.isLoading) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Loading, { label: "Loading loan" });
	if (query.error || !query.data) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ErrorState, { error: query.error });
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoanView, { data: query.data });
}
function LoanView({ data }) {
	const [tab, setTab] = (0, import_react.useState)(window.location.hash === "#workspace" ? "workspace" : "overview");
	const loan = data.loan;
	const base = `/integrations/${encodeURIComponent(data.connection.slug)}/loans/${encodeURIComponent(loan.loan_account)}`;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: loan.borrower_name || loan.loan_account,
		description: [
			loan.property_address,
			loan.property_city,
			loan.property_state,
			loan.property_zip
		].filter(Boolean).join(", "),
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: loan.loan_account }), loan.is_delinquent ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
			className: "border-amber-300 bg-amber-50 text-amber-900",
			children: "Delinquent"
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
			className: "border-primary/30 bg-accent text-accent-foreground",
			children: "Current"
		})] }),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "flex gap-1 overflow-x-auto rounded-lg bg-muted p-1",
			children: [
				["overview", "Overview"],
				["payments", "Payments"],
				["workspace", "Workspace"],
				["mail", `Mail (${data.emails.length})`]
			].map(([key, label]) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				onClick: () => setTab(key),
				className: cn("rounded-md px-3 py-2 text-sm font-medium text-muted-foreground", tab === key && "bg-background text-foreground shadow-sm"),
				children: label
			}, key))
		}), tab === "overview" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Overview, { data }) : tab === "payments" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Payments, { data }) : tab === "workspace" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Workspace, {
			data,
			base
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MailView, { data })]
	});
}
function Overview({ data }) {
	const loan = data.loan;
	const fields = [
		["Principal balance", money(loan.principal_balance)],
		["Original balance", money(loan.original_balance)],
		["Regular payment", money(loan.regular_payment)],
		["Note rate", loan.note_rate == null ? "—" : `${loan.note_rate.toFixed(3)}%`],
		["Next payment", date(loan.next_payment_date)],
		["Maturity", date(loan.maturity_date)],
		["Interest paid to", date(loan.interest_paid_to)],
		["Billed through", date(loan.billed_through)],
		["Property type", loan.property_type || "—"],
		["Occupancy", loan.occupancy || "—"],
		["Ownership", loan.percent_owned == null ? "—" : `${loan.percent_owned.toFixed(1)}%`],
		["LTV", loan.ltv == null ? "—" : `${loan.ltv.toFixed(1)}%`]
	];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Loan details" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Current provider values for this investment." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dl", {
		className: "grid gap-5 sm:grid-cols-2 xl:grid-cols-4",
		children: fields.map(([label, value]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("dt", {
			className: "text-xs text-muted-foreground",
			children: label
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("dd", {
			className: "mt-1 text-sm font-medium",
			children: value
		})] }, label))
	}), loan.property_description ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mt-6 border-t pt-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "text-xs text-muted-foreground",
			children: "Property description"
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
			className: "mt-1 text-sm",
			children: loan.property_description
		})]
	}) : null] })] });
}
function Payments({ data }) {
	return data.payments.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
		title: "No payment history",
		description: "No imported payments were found for this loan."
	}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
		className: "overflow-x-auto",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
			className: "data-table",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Date" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Check" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Total" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Fee" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Interest" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Principal" })
			] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: data.payments.map((payment) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: date(payment.check_date) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: payment.check_number || "Pending" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
					className: "font-medium",
					children: money(payment.amount)
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: money(payment.service_fee) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: money(payment.interest) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: money(payment.principal) })
			] }, payment.id)) })]
		})
	});
}
function Workspace({ data, base }) {
	const w = data.workspace;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Investment workspace" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Decision context and property references stored alongside the imported loan." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex gap-2",
				children: [w.redfin_link ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					asChild: true,
					size: "sm",
					variant: "outline",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: w.redfin_link,
						target: "_blank",
						rel: "noreferrer",
						children: ["Redfin ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" })]
					})
				}) : null, w.zillow_link ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					asChild: true,
					size: "sm",
					variant: "outline",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: w.zillow_link,
						target: "_blank",
						rel: "noreferrer",
						children: ["Zillow ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "size-3.5" })]
					})
				}) : null]
			})]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
			method: "post",
			action: `${base}/workspace`,
			className: "grid gap-4",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "grid gap-4 md:grid-cols-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
							label: "Redfin URL",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								type: "url",
								name: "redfin_url",
								defaultValue: w.redfin_url
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
							label: "Zillow URL",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								type: "url",
								name: "zillow_url",
								defaultValue: w.zillow_url
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
							label: "Decision status",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("select", {
								name: "decision_status",
								defaultValue: w.decision_status,
								className: "h-9 w-full rounded-md border bg-background px-3 text-sm",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
									value: "",
									children: "No status"
								}), [
									"new",
									"reviewing",
									"committed",
									"funded",
									"passed"
								].map((v) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
									value: v,
									children: v[0].toUpperCase() + v.slice(1)
								}, v))]
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "grid grid-cols-2 gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
								label: "Target contribution",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
									type: "number",
									min: "0",
									step: "0.01",
									name: "target_contribution",
									defaultValue: w.target_contribution ?? ""
								})
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
								label: "Actual contribution",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
									type: "number",
									min: "0",
									step: "0.01",
									name: "actual_contribution",
									defaultValue: w.actual_contribution ?? ""
								})
							})]
						})
					]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Field, {
					label: "Notes",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Textarea, {
						name: "notes",
						defaultValue: w.notes
					})
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-xs text-muted-foreground",
						children: w.updated_at ? `Saved ${dateTime(w.updated_at)}` : "Not saved yet"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "submit",
						children: "Save workspace"
					})]
				})
			]
		}) })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
			className: "mb-3 text-sm font-semibold",
			children: "Property photos"
		}), data.photos.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
			title: "No property photos",
			description: "Photos can be uploaded after writes are enabled."
		}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "grid gap-4 md:grid-cols-2 xl:grid-cols-3",
			children: data.photos.map((photo) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "overflow-hidden",
				children: [photo.image_url ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
					src: photo.image_url,
					alt: photo.caption || "Property",
					className: "h-52 w-full object-cover"
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "flex h-52 items-center justify-center bg-muted text-sm text-muted-foreground",
					children: "External image unavailable"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
					className: "flex items-center justify-between pt-4",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-sm capitalize",
						children: photo.provider
					}), photo.is_featured ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: "Featured" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("form", {
						method: "post",
						action: `${base}/workspace/photos/${photo.id}/feature`,
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							size: "sm",
							variant: "outline",
							children: "Feature"
						})
					})]
				})]
			}, photo.id))
		})] })]
	});
}
function MailView({ data }) {
	return data.emails.length === 0 ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Empty, {
		title: "No linked mail",
		description: "Link inbox messages to this loan to see them here."
	}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, {
		className: "overflow-x-auto",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("table", {
			className: "data-table",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("thead", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Received" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "From" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "Subject" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", { children: "State" })
			] }) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("tbody", { children: data.emails.map((email) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("tr", { children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: dateTime(email.received_at) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: email.from_address }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
					className: "font-medium hover:text-primary",
					href: `/inbox/${email.id}`,
					children: email.subject || "(no subject)"
				}) }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: email.processing_state }) })
			] }, email.id)) })]
		})
	});
}
function Field({ label, children }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
		className: "grid gap-1.5 text-sm",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
			className: "text-xs font-medium text-muted-foreground",
			children: label
		}), children]
	});
}

//#endregion
export { Loan as default };