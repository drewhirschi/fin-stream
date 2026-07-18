import type { QueryClient } from "@tanstack/react-query";

interface SeedEntry {
  key: unknown[];
  data: unknown;
}

export function readSeeds(): SeedEntry[] {
  const tag = document.getElementById("__nx_seeds__");
  if (!tag?.textContent) return [];
  try {
    return JSON.parse(tag.textContent) as SeedEntry[];
  } catch {
    return [];
  }
}

export function seedQueryClient(queryClient: QueryClient): void {
  for (const entry of readSeeds()) queryClient.setQueryData(entry.key, entry.data);
}
