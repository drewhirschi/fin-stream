import { Q as QueryClientProvider, et as require_jsx_runtime, w as Button } from "./chunks/src-DJBa3cvh.js";
import { i as require_client, n as seedQueryClient, r as QueryClient, t as Layout } from "./chunks/layout-BJ3kUKXl.js";

//#region ../app/not-found.tsx
var import_client = require_client();
var import_jsx_runtime = require_jsx_runtime();
function NotFound() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("main", {
		className: "flex min-h-screen flex-col items-center justify-center gap-4 p-6 text-center",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-sm font-medium text-primary",
				children: "404"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
				className: "text-3xl font-semibold",
				children: "Page not found"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "text-muted-foreground",
				children: "The page may have moved or no longer exists."
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				asChild: true,
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
					href: "/",
					children: "Return home"
				})
			})
		]
	});
}

//#endregion
//#region ../target/debug/build/trust-deeds-fa710ddc270ef531/out/nextrs_tsx/not-found.tsx
const qc = new QueryClient({ defaultOptions: { queries: { staleTime: 3e4 } } });
seedQueryClient(qc);
const paramsEl = document.getElementById("__nx_params__");
const params = paramsEl?.textContent ? JSON.parse(paramsEl.textContent) : {};
(0, import_client.createRoot)(document.getElementById("__nx_root__")).render(/* @__PURE__ */ (0, import_jsx_runtime.jsx)(QueryClientProvider, {
	client: qc,
	children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Layout, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(NotFound, { params }) })
}));

//#endregion