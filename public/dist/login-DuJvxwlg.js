import { C as CardTitle, Q as require_jsx_runtime, S as CardHeader, _ as Input, _t as __toESM, b as CardContent, d as LoaderCircle, f as Landmark, ht as require_react, w as Button, x as CardDescription, y as Card } from "./chunks/src-ZnV_ftAe.js";

//#region ../app/login/page.tsx
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function Login() {
	const [error, setError] = (0, import_react.useState)("");
	const [busy, setBusy] = (0, import_react.useState)(false);
	const submit = async (event) => {
		event.preventDefault();
		setBusy(true);
		setError("");
		const form = new FormData(event.currentTarget);
		try {
			const response = await fetch("/login", {
				method: "POST",
				body: new URLSearchParams({
					email: String(form.get("email") || ""),
					password: String(form.get("password") || "")
				}),
				credentials: "same-origin",
				headers: {
					"Content-Type": "application/x-www-form-urlencoded",
					"Sec-Fetch-Site": "same-origin"
				}
			});
			if (response.ok) window.location.assign("/");
			else setError(await response.text());
		} catch {
			setError("Could not reach the server. Try again.");
		} finally {
			setBusy(false);
		}
	};
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("main", {
		className: "grid min-h-screen bg-muted/40 lg:grid-cols-2",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
			className: "hidden flex-col justify-between bg-foreground p-12 text-background lg:flex",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center gap-3 font-semibold",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "flex size-9 items-center justify-center rounded-lg bg-primary",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Landmark, { className: "size-5" })
					}), "Trust Deeds"]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "max-w-lg",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-4xl font-semibold leading-tight tracking-tight",
						children: "A clear view of portfolio income, without the spreadsheet archaeology."
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "mt-5 text-base text-background/65",
						children: "Connected loan data, payment history, correspondence, and forward cash planning in one private workspace."
					})]
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-xs text-background/45",
					children: "Private financial workspace"
				})
			]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("section", {
			className: "flex items-center justify-center p-6",
			children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "w-full max-w-md",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "mb-4 flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground lg:hidden",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Landmark, { className: "size-5" })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-2xl",
						children: "Welcome back"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Sign in to your Trust Deeds workspace." })
				] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
					onSubmit: submit,
					className: "space-y-4",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
							className: "grid gap-1.5 text-sm",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "font-medium",
								children: "Email"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								name: "email",
								type: "email",
								autoComplete: "email",
								required: true,
								autoFocus: true
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
							className: "grid gap-1.5 text-sm",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "font-medium",
								children: "Password"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								name: "password",
								type: "password",
								autoComplete: "current-password",
								required: true
							})]
						}),
						error ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "rounded-md border border-destructive/25 bg-destructive/5 p-3 text-sm text-destructive",
							children: error
						}) : null,
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							className: "w-full",
							type: "submit",
							disabled: busy,
							children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }), "Signing in…"] }) : "Sign in"
						})
					]
				}) })]
			})
		})]
	});
}

//#endregion
export { Login as default };