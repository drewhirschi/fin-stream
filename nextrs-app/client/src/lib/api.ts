import { useQuery, type UseQueryOptions } from "@tanstack/react-query";

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: "same-origin",
    ...init,
    headers: {
      Accept: "application/json",
      ...(init?.body && !(init.body instanceof FormData) ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  });
  if (response.status === 401) {
    window.location.assign("/login");
    throw new Error("Your session expired.");
  }
  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Request failed (${response.status})`);
  }
  const contentType = response.headers.get("content-type") ?? "";
  return (contentType.includes("application/json") ? response.json() : response.text()) as Promise<T>;
}

export function useApi<T>(key: readonly unknown[], path: string, options?: Partial<UseQueryOptions<T>>) {
  return useQuery<T>({ queryKey: key, queryFn: () => api<T>(path), ...options });
}

export async function submitForm(path: string, values: Record<string, string | number | boolean | null | undefined>) {
  const body = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) if (value != null) body.set(key, String(value));
  return api<string>(path, {
    method: "POST",
    body,
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    redirect: "manual",
  });
}
