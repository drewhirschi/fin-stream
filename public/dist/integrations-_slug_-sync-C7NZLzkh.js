import { $ as useQueryClient, C as CardTitle, O as dateTime, S as CardHeader, T as Badge, Z as useQuery, a as Empty, at as notifyManager, c as Page, d as LoaderCircle, dt as noop, et as require_jsx_runtime, g as createLucideIcon, ht as shouldThrowError, mt as shallowEqualObjects, r as IntegrationBoundary, st as hashKey, vt as Subscribable, w as Button, x as CardDescription, xt as __toESM, y as Card, yt as require_react } from "./chunks/src-DJBa3cvh.js";
import { n as getDefaultState } from "./chunks/mutation-DQYa5_Cn.js";

//#region node_modules/@tanstack/query-core/build/modern/mutationObserver.js
var MutationObserver = class extends Subscribable {
	#client;
	#currentResult = void 0;
	#currentMutation;
	#mutateOptions;
	constructor(client, options) {
		super();
		this.#client = client;
		this.setOptions(options);
		this.bindMethods();
		this.#updateResult();
	}
	bindMethods() {
		this.mutate = this.mutate.bind(this);
		this.reset = this.reset.bind(this);
	}
	setOptions(options) {
		const prevOptions = this.options;
		this.options = this.#client.defaultMutationOptions(options);
		if (!shallowEqualObjects(this.options, prevOptions)) this.#client.getMutationCache().notify({
			type: "observerOptionsUpdated",
			mutation: this.#currentMutation,
			observer: this
		});
		if (prevOptions?.mutationKey && this.options.mutationKey && hashKey(prevOptions.mutationKey) !== hashKey(this.options.mutationKey)) this.reset();
		else if (this.#currentMutation?.state.status === "pending") this.#currentMutation.setOptions(this.options);
	}
	onUnsubscribe() {
		if (!this.hasListeners()) this.#currentMutation?.removeObserver(this);
	}
	onMutationUpdate(action) {
		this.#updateResult();
		this.#notify(action);
	}
	getCurrentResult() {
		return this.#currentResult;
	}
	reset() {
		this.#currentMutation?.removeObserver(this);
		this.#currentMutation = void 0;
		this.#updateResult();
		this.#notify();
	}
	mutate(variables, options) {
		this.#mutateOptions = options;
		this.#currentMutation?.removeObserver(this);
		this.#currentMutation = this.#client.getMutationCache().build(this.#client, this.options);
		this.#currentMutation.addObserver(this);
		return this.#currentMutation.execute(variables);
	}
	#updateResult() {
		const state = this.#currentMutation?.state ?? getDefaultState();
		this.#currentResult = {
			...state,
			isPending: state.status === "pending",
			isSuccess: state.status === "success",
			isError: state.status === "error",
			isIdle: state.status === "idle",
			mutate: this.mutate,
			reset: this.reset
		};
	}
	#notify(action) {
		notifyManager.batch(() => {
			if (this.#mutateOptions && this.hasListeners()) {
				const variables = this.#currentResult.variables;
				const onMutateResult = this.#currentResult.context;
				const context = {
					client: this.#client,
					meta: this.options.meta,
					mutationKey: this.options.mutationKey
				};
				if (action?.type === "success") {
					try {
						this.#mutateOptions.onSuccess?.(action.data, variables, onMutateResult, context);
					} catch (e) {
						Promise.reject(e);
					}
					try {
						this.#mutateOptions.onSettled?.(action.data, null, variables, onMutateResult, context);
					} catch (e) {
						Promise.reject(e);
					}
				} else if (action?.type === "error") {
					try {
						this.#mutateOptions.onError?.(action.error, variables, onMutateResult, context);
					} catch (e) {
						Promise.reject(e);
					}
					try {
						this.#mutateOptions.onSettled?.(void 0, action.error, variables, onMutateResult, context);
					} catch (e) {
						Promise.reject(e);
					}
				}
			}
			this.listeners.forEach((listener) => {
				listener(this.#currentResult);
			});
		});
	}
};

//#endregion
//#region node_modules/@tanstack/react-query/build/modern/useMutation.js
var import_react = /* @__PURE__ */ __toESM(require_react(), 1);
function useMutation(options, queryClient) {
	const client = useQueryClient(queryClient);
	const [observer] = import_react.useState(() => new MutationObserver(client, options));
	import_react.useEffect(() => {
		observer.setOptions(options);
	}, [observer, options]);
	const result = import_react.useSyncExternalStore(import_react.useCallback((onStoreChange) => observer.subscribe(notifyManager.batchCalls(onStoreChange)), [observer]), () => observer.getCurrentResult(), () => observer.getCurrentResult());
	const mutate = import_react.useCallback((variables, mutateOptions) => {
		observer.mutate(variables, mutateOptions).catch(noop);
	}, [observer]);
	if (result.error && shouldThrowError(observer.options.throwOnError, [result.error])) throw result.error;
	return {
		...result,
		mutate,
		mutateAsync: result.mutate
	};
}

//#endregion
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
//#region src/generated/integration-sync/integration-sync.ts
/**
* Generated by orval v7.21.0 🍺
* Do not edit manually.
* trust-deeds
* OpenAPI spec version: 0.1.0
*/
const getRunIntegrationSyncUrl = (slug) => {
	return `/integrations/${slug}/sync/run`;
};
const runIntegrationSync = async (slug, options) => {
	const res = await fetch(getRunIntegrationSyncUrl(slug), {
		...options,
		method: "POST"
	});
	const body = [
		204,
		205,
		304
	].includes(res.status) ? null : await res.text();
	return {
		data: body ? JSON.parse(body) : {},
		status: res.status,
		headers: res.headers
	};
};
const getRunIntegrationSyncMutationOptions = (options) => {
	const mutationKey = ["runIntegrationSync"];
	const { mutation: mutationOptions, fetch: fetchOptions } = options ? options.mutation && "mutationKey" in options.mutation && options.mutation.mutationKey ? options : {
		...options,
		mutation: {
			...options.mutation,
			mutationKey
		}
	} : {
		mutation: { mutationKey },
		fetch: void 0
	};
	const mutationFn = (props) => {
		const { slug } = props ?? {};
		return runIntegrationSync(slug, fetchOptions);
	};
	return {
		mutationFn,
		...mutationOptions
	};
};
const useRunIntegrationSync = (options, queryClient) => {
	return useMutation(getRunIntegrationSyncMutationOptions(options), queryClient);
};
const getGetIntegrationSyncStatusUrl = (slug) => {
	return `/integrations/${slug}/sync/status`;
};
const getIntegrationSyncStatus = async (slug, options) => {
	const res = await fetch(getGetIntegrationSyncStatusUrl(slug), {
		...options,
		method: "GET"
	});
	const body = [
		204,
		205,
		304
	].includes(res.status) ? null : await res.text();
	return {
		data: body ? JSON.parse(body) : {},
		status: res.status,
		headers: res.headers
	};
};
const getGetIntegrationSyncStatusQueryKey = (slug) => {
	return [`/integrations/${slug}/sync/status`];
};
const getGetIntegrationSyncStatusQueryOptions = (slug, options) => {
	const { query: queryOptions, fetch: fetchOptions } = options ?? {};
	const queryKey = queryOptions?.queryKey ?? getGetIntegrationSyncStatusQueryKey(slug);
	const queryFn = ({ signal }) => getIntegrationSyncStatus(slug, {
		signal,
		...fetchOptions
	});
	return {
		queryKey,
		queryFn,
		enabled: !!slug,
		...queryOptions
	};
};
function useGetIntegrationSyncStatus(slug, options, queryClient) {
	const queryOptions = getGetIntegrationSyncStatusQueryOptions(slug, options);
	const query = useQuery(queryOptions, queryClient);
	query.queryKey = queryOptions.queryKey;
	return query;
}

//#endregion
//#region ../app/integrations/[slug]/sync/page.tsx
var import_jsx_runtime = require_jsx_runtime();
function Sync() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(IntegrationBoundary, { children: (data, slug) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SyncView, {
		data,
		slug
	}) });
}
function SyncView({ data, slug }) {
	const queryClient = useQueryClient();
	const initialRun = data.sync_logs.find((run) => run.status === "running") ?? data.sync_logs[0] ?? null;
	const sawRunning = (0, import_react.useRef)(initialRun?.status === "running");
	const status = useGetIntegrationSyncStatus(slug, {
		fetch: {
			credentials: "same-origin",
			headers: { Accept: "application/json" }
		},
		query: {
			initialData: {
				data: { run: initialRun },
				status: 200,
				headers: new Headers()
			},
			refetchInterval: 1e4
		}
	});
	const runSync = useRunIntegrationSync({
		fetch: {
			credentials: "same-origin",
			headers: { Accept: "application/json" }
		},
		mutation: { onSettled: async () => {
			await Promise.all([queryClient.invalidateQueries({ queryKey: getGetIntegrationSyncStatusQueryKey(slug) }), queryClient.invalidateQueries({ queryKey: ["integration", slug] })]);
		} }
	});
	const currentRun = status.data?.status === 200 ? status.data.data.run : null;
	const durableRunning = currentRun?.status === "running";
	const busy = durableRunning || runSync.isPending;
	const response = runSync.data;
	const alreadyRunning = response?.status === 409 && "outcome" in response.data && response.data.outcome === "already_running";
	const responseMessage = response && "message" in response.data ? response.data.message : response && "run" in response.data ? response.data.run.error_message : null;
	const error = runSync.isError ? "The sync request could not be completed." : response && response.status >= 400 && !alreadyRunning ? responseMessage || `The sync could not be started (${response.status}).` : null;
	(0, import_react.useEffect)(() => {
		if (durableRunning) {
			sawRunning.current = true;
			return;
		}
		if (sawRunning.current && status.data?.status === 200) {
			sawRunning.current = false;
			queryClient.invalidateQueries({ queryKey: ["integration", slug] });
		}
	}, [
		durableRunning,
		queryClient,
		slug,
		status.data?.status
	]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Page, {
		title: "Sync",
		description: "Provider refresh history and operational controls.",
		actions: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
			onClick: () => runSync.mutate({ slug }),
			disabled: busy || data.control.mode !== "enabled",
			children: [busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-4 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "size-4" }), busy ? "Syncing…" : "Run sync"]
		}),
		children: [
			durableRunning ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex gap-3 rounded-xl border border-primary/25 bg-accent p-4 text-sm text-accent-foreground",
				"aria-live": "polite",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "size-5 shrink-0 animate-spin" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "font-medium",
					children: "Syncing"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
					className: "mt-1 opacity-80",
					children: [
						"Started ",
						dateTime(currentRun.started_at),
						". This page checks for updates every ten seconds."
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