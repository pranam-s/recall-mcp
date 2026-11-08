/** Typed API client for the Recall-MCP HTTP API. Relative paths: the UI is
 * served by the same server in production; vite proxies in dev. */

export interface Namespace {
  id: string;
  name: string;
  created_at: number;
}

export interface Memory {
  id: string;
  namespace_id: string;
  text: string;
  tags: string[];
  source: string;
  pinned: boolean;
  created_at: number;
  last_accessed_at: number;
  embedding?: number[] | null;
}

export interface Page<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
  has_more: boolean;
}

/** One server-driven page of memories plus the unpaginated total. */
export interface MemoryPage {
  memories: Memory[];
  total: number;
  limit: number;
  offset: number;
  hasMore: boolean;
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

export interface EmbedderInfo {
  name: string;
  dimensions: number;
  fingerprint: string;
}

export interface Stats {
  total_namespaces: number;
  total_memories: number;
  pinned_memories: number;
  per_namespace: { namespace: string; count: number }[];
  embedder: EmbedderInfo;
}

export interface CaptureMessage {
  role?: string;
  content: string;
}

export interface CapturePayload {
  namespace: string;
  tags: string[];
  transcript: CaptureMessage[];
}

export interface CaptureResult {
  namespace: string;
  captured: number;
  first_id?: string;
  last_id?: string;
  memories?: Memory[];
}

export interface ApiErrorBody {
  error: string;
}

/** Error carrying the HTTP status and any rate-limit hint, so the UI can
 * explain 401/429 differently from a generic failure. */
export class ApiRequestError extends Error {
  readonly status: number;
  readonly retryAfterSeconds: number | null;

  constructor(message: string, status: number, retryAfterSeconds: number | null) {
    super(message);
    this.name = "ApiRequestError";
    this.status = status;
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

const API_KEY_STORAGE = "recall-mcp-api-key";

/** The key lives in sessionStorage only: closing the tab clears it, and it is
 * never written to localStorage or a URL. */
export function getApiKey(): string {
  try {
    return window.sessionStorage.getItem(API_KEY_STORAGE) ?? "";
  } catch {
    return "";
  }
}

export function setApiKey(key: string): void {
  try {
    if (key.trim()) {
      window.sessionStorage.setItem(API_KEY_STORAGE, key.trim());
    } else {
      window.sessionStorage.removeItem(API_KEY_STORAGE);
    }
  } catch {
    // Private/storage-disabled modes degrade to no key for this page.
  }
}

export function errorMessage(error: unknown): string {
  if (error instanceof ApiRequestError) {
    if (error.status === 401) {
      return "API key missing or invalid. Save the server's key in the Connection section.";
    }
    if (error.status === 429) {
      const wait = error.retryAfterSeconds;
      return wait === null
        ? "Rate limit exceeded. Wait a moment and try again."
        : `Rate limit exceeded. Retry after ${wait} second${wait === 1 ? "" : "s"}.`;
    }
    return error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

/** Fetch that throws the API's `{"error": ...}` message on failure and returns
 * the Response on success — callers may need headers (`X-Total-Count`) or
 * non-JSON bodies (`/metrics` text). */
async function requestResponse(path: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers);
  if (!headers.has("content-type")) headers.set("content-type", "application/json");
  const key = getApiKey();
  if (key) headers.set("x-api-key", key);
  let res: Response;
  try {
    res = await fetch(path, { ...init, headers });
  } catch (e) {
    throw new ApiRequestError(
      `Cannot reach the server: ${e instanceof Error ? e.message : String(e)}`,
      0,
      null,
    );
  }
  if (!res.ok) {
    const body: unknown = await res.json().catch(() => null);
    const detail =
      body && typeof body === "object" && "error" in body
        ? String((body as ApiErrorBody).error)
        : `${res.status} ${res.statusText}`;
    const retryHeader = res.headers.get("retry-after");
    const retry = retryHeader === null ? null : Number(retryHeader);
    throw new ApiRequestError(
      detail,
      res.status,
      retry !== null && Number.isFinite(retry) ? retry : null,
    );
  }
  return res;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await requestResponse(path, init);
  return (await res.json()) as T;
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
    // Full records keep the UI's pinned/namespace columns meaningful; the
    // compact views remain the default for API and MCP clients.
    request<RecallHit[]>("/v1/recall?include_embedding=true", {
      method: "POST",
      body: JSON.stringify({ namespace, query, k }),
    }),
  stats: () => request<Stats>("/v1/stats"),
  /** `GET /v1/memories` with server-side pagination. */
  listMemories: async (
    namespace: string | undefined,
    limit: number,
    offset: number,
  ): Promise<MemoryPage> => {
    const params = new URLSearchParams({
      limit: String(limit),
      offset: String(offset),
      include_embedding: "true",
    });
    if (namespace) params.set("namespace", namespace);
    const page = await request<Page<Memory>>(`/v1/memories?${params.toString()}`);
    return {
      memories: page.items,
      total: page.total,
      limit: page.limit,
      offset: page.offset,
      hasMore: page.has_more,
    };
  },
  updatePinned: (id: string, pinned: boolean) =>
    request<Memory>(`/v1/memories/${encodeURIComponent(id)}?include_embedding=true`, {
      method: "PATCH",
      body: JSON.stringify({ pinned }),
    }),
  deleteMemory: async (id: string): Promise<void> => {
    await requestResponse(`/v1/memories/${encodeURIComponent(id)}`, { method: "DELETE" });
  },
  capture: (payload: CapturePayload) =>
    request<CaptureResult>("/v1/capture", { method: "POST", body: JSON.stringify(payload) }),
  /** Prometheus text exposition (0.0.4), parsed for display by `parseMetrics`. */
  metrics: async (): Promise<string> => (await requestResponse("/metrics")).text(),
};
