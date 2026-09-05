/** Typed API client for the Recall-MCP HTTP API. Relative paths: the UI is
 * served by the same server in production; vite proxies in dev. */

export interface Namespace {
  id: string;
  name: string;
  created_at: number;
}

export interface ScoreBreakdown {
  bm25: number;
  vector: number;
  recency: number;
  pinned_boost: number;
  total: number;
}

export interface RecallHit {
  id: string;
  namespace_id: string;
  text: string;
  tags: string[];
  source: string;
  pinned: boolean;
  created_at: number;
  last_accessed_at: number;
  breakdown: ScoreBreakdown;
}

export interface Stats {
  total_namespaces: number;
  total_memories: number;
  pinned_memories: number;
  per_namespace: { namespace: string; count: number }[];
}

export interface ApiError {
  error: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...init,
  });
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const detail =
      body && typeof body === "object" && "error" in body
        ? String((body as ApiError).error)
        : `${res.status} ${res.statusText}`;
    throw new Error(detail);
  }
  return body as T;
}

export const api = {
  listNamespaces: () => request<Namespace[]>("/v1/namespaces"),
  createNamespace: (name: string) =>
    request<Namespace>("/v1/namespaces", { method: "POST", body: JSON.stringify({ name }) }),
  remember: (namespace: string, text: string, tags: string[]) =>
    request<unknown>("/v1/memories", {
      method: "POST",
      body: JSON.stringify({ namespace, text, tags }),
    }),
  recall: (namespace: string, query: string, k: number) =>
    request<RecallHit[]>("/v1/recall", {
      method: "POST",
      body: JSON.stringify({ namespace, query, k }),
    }),
  stats: () => request<Stats>("/v1/stats"),
};
