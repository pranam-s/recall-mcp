import { useCallback, useEffect, useState } from "react";
import type { FormEvent } from "react";
import { api } from "./api";
import type { Namespace, RecallHit, Stats } from "./api";

function formatScore(value: number): string {
  return value.toFixed(3);
}

export default function App() {
  // Namespace list is shared: creating one in the namespaces section must
  // immediately be selectable in the search section.
  const [namespaces, setNamespaces] = useState<Namespace[] | null>(null);

  const refreshNamespaces = useCallback(async () => {
    try {
      setNamespaces(await api.listNamespaces());
    } catch {
      // The per-section error regions render details; keep the shared list here.
    }
  }, []);

  useEffect(() => {
    void refreshNamespaces();
  }, [refreshNamespaces]);

  return (
    <div className="mx-auto max-w-4xl px-4 py-8">
      <header>
        <h1 className="text-2xl font-bold">Recall-MCP memory console</h1>
        <nav aria-label="Section navigation" className="mt-2">
          <ul className="flex gap-4">
            <li>
              <a className="text-blue-700 underline" href="#namespaces">Namespaces</a>
            </li>
            <li>
              <a className="text-blue-700 underline" href="#search">Search</a>
            </li>
            <li>
              <a className="text-blue-700 underline" href="#stats">Stats</a>
            </li>
          </ul>
        </nav>
      </header>

      <NamespacesSection namespaces={namespaces} onRefresh={refreshNamespaces} />
      <SearchSection namespaces={namespaces} onRefresh={refreshNamespaces} />
      <StatsSection />

      <footer className="mt-10 text-sm text-gray-600">
        Local-first memory broker · REST + MCP · scores explained on every result.
      </footer>
    </div>
  );
}

interface NamespacesSectionProps {
  namespaces: Namespace[] | null;
  onRefresh: () => Promise<void>;
}

function NamespacesSection({ namespaces, onRefresh }: NamespacesSectionProps) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    try {
      await api.createNamespace(name);
      setNotice(`Namespace “${name}” created.`);
      setName("");
      await onRefresh();
    } catch (e) {
      setNotice(null);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section id="namespaces" aria-labelledby="namespaces-heading" className="mt-8">
      <h2 id="namespaces-heading" className="text-xl font-semibold">Namespaces</h2>
      <p className="mt-1 text-sm text-gray-700">Workspaces never share memories with each other.</p>

      <form onSubmit={submit} className="mt-3 flex flex-wrap items-end gap-2">
        <div>
          <label htmlFor="namespace-name" className="block text-sm font-medium">New namespace name</label>
          <input
            id="namespace-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            className="mt-1 rounded border border-gray-400 px-2 py-1"
          />
        </div>
        <button type="submit" className="rounded bg-blue-700 px-3 py-1.5 font-medium text-white">
          Create namespace
        </button>
      </form>

      <p role="status" aria-live="polite" className={`mt-2 min-h-5 text-sm ${error ? "text-red-700" : "text-green-800"}`}>
        {error ?? notice ?? ""}
      </p>

      <table className="mt-2 w-full border-collapse text-left text-sm">
        <caption className="sr-only">All namespaces with creation time</caption>
        <thead>
          <tr className="border-b border-gray-300">
            <th scope="col" className="py-1 pr-4">Name</th>
            <th scope="col" className="py-1 pr-4">Created (unix)</th>
          </tr>
        </thead>
        <tbody>
          {(namespaces ?? []).map((ns) => (
            <tr key={ns.id} className="border-b border-gray-200">
              <td className="py-1 pr-4">{ns.name}</td>
              <td className="py-1 pr-4">{ns.created_at}</td>
            </tr>
          ))}
          {namespaces && namespaces.length === 0 && (
            <tr>
              <td colSpan={2} className="py-1 text-gray-600">No namespaces yet.</td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}

interface SearchSectionProps {
  namespaces: Namespace[] | null;
  onRefresh: () => Promise<void>;
}

function SearchSection({ namespaces, onRefresh }: SearchSectionProps) {
  const [namespace, setNamespace] = useState("");
  const [memoryText, setMemoryText] = useState("");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<RecallHit[] | null>(null);
  const [status, setStatus] = useState<{ kind: "info" | "error"; text: string } | null>(null);

  // Follow the shared list: default to the first namespace until the user picks one.
  useEffect(() => {
    const first = namespaces?.[0];
    if (first && !namespaces.some((ns) => ns.name === namespace)) {
      setNamespace(first.name);
    }
  }, [namespaces, namespace]);

  const remember = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    try {
      await api.remember(namespace, memoryText, []);
      setStatus({ kind: "info", text: "Memory stored." });
      setMemoryText("");
      await onRefresh();
    } catch (e) {
      setStatus({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    }
  };

  const search = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    try {
      const results = await api.recall(namespace, query, 5);
      setHits(results);
      setStatus({ kind: "info", text: `${results.length} result(s).` });
    } catch (e) {
      setHits(null);
      setStatus({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <section id="search" aria-labelledby="search-heading" className="mt-10">
      <h2 id="search-heading" className="text-xl font-semibold">Search memories</h2>

      <form onSubmit={remember} className="mt-3 flex flex-wrap items-end gap-2">
        <div>
          <label htmlFor="remember-namespace" className="block text-sm font-medium">Namespace</label>
          <select
            id="remember-namespace"
            value={namespace}
            onChange={(e) => setNamespace(e.target.value)}
            className="mt-1 rounded border border-gray-400 px-2 py-1"
          >
            {(namespaces ?? []).map((ns) => (
              <option key={ns.id} value={ns.name}>{ns.name}</option>
            ))}
          </select>
        </div>
        <div className="grow">
          <label htmlFor="remember-text" className="block text-sm font-medium">New memory text</label>
          <input
            id="remember-text"
            value={memoryText}
            onChange={(e) => setMemoryText(e.target.value)}
            required
            className="mt-1 w-full rounded border border-gray-400 px-2 py-1"
          />
        </div>
        <button type="submit" className="rounded bg-blue-700 px-3 py-1.5 font-medium text-white">
          Remember
        </button>
      </form>

      <form onSubmit={search} className="mt-4 flex flex-wrap items-end gap-2">
        <div className="grow">
          <label htmlFor="search-query" className="block text-sm font-medium">Query</label>
          <input
            id="search-query"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            required
            className="mt-1 w-full rounded border border-gray-400 px-2 py-1"
          />
        </div>
        <button type="submit" className="rounded bg-blue-700 px-3 py-1.5 font-medium text-white">
          Recall
        </button>
      </form>

      <p
        role="status"
        aria-live="polite"
        className={`mt-2 min-h-5 text-sm ${status?.kind === "error" ? "text-red-700" : "text-green-800"}`}
      >
        {status?.text ?? ""}
      </p>

      {hits && (
        <div className="mt-2 overflow-x-auto">
          <table className="w-full border-collapse text-left text-sm">
            <caption className="sr-only">Recalled memories with score breakdowns</caption>
            <thead>
              <tr className="border-b border-gray-300">
                <th scope="col" className="py-1 pr-3">Memory</th>
                <th scope="col" className="py-1 pr-3">BM25</th>
                <th scope="col" className="py-1 pr-3">Vector</th>
                <th scope="col" className="py-1 pr-3">Recency</th>
                <th scope="col" className="py-1 pr-3">Pinned</th>
                <th scope="col" className="py-1 pr-3">Total</th>
              </tr>
            </thead>
            <tbody>
              {hits.map((hit) => (
                <tr key={hit.id} className="border-b border-gray-200 align-top">
                  <td className="py-1 pr-3">
                    {hit.text}
                    {hit.tags.length > 0 && (
                      <span className="ml-2 text-gray-600">[{hit.tags.join(", ")}]</span>
                    )}
                    {hit.pinned && <span className="ml-2 font-medium">(pinned)</span>}
                  </td>
                  <td className="py-1 pr-3 tabular-nums">{formatScore(hit.breakdown.bm25)}</td>
                  <td className="py-1 pr-3 tabular-nums">{formatScore(hit.breakdown.vector)}</td>
                  <td className="py-1 pr-3 tabular-nums">{formatScore(hit.breakdown.recency)}</td>
                  <td className="py-1 pr-3 tabular-nums">{formatScore(hit.breakdown.pinned_boost)}</td>
                  <td className="py-1 pr-3 font-medium tabular-nums">{formatScore(hit.breakdown.total)}</td>
                </tr>
              ))}
              {hits.length === 0 && (
                <tr>
                  <td colSpan={6} className="py-1 text-gray-600">No matching memories.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function StatsSection() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .stats()
      .then(setStats)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

  return (
    <section id="stats" aria-labelledby="stats-heading" className="mt-10">
      <h2 id="stats-heading" className="text-xl font-semibold">Server stats</h2>
      {error && <p role="alert" className="mt-2 text-sm text-red-700">{error}</p>}
      {stats && (
        <>
          <dl className="mt-3 grid grid-cols-3 gap-4 text-sm">
            <div>
              <dt className="font-medium">Namespaces</dt>
              <dd className="tabular-nums">{stats.total_namespaces}</dd>
            </div>
            <div>
              <dt className="font-medium">Memories</dt>
              <dd className="tabular-nums">{stats.total_memories}</dd>
            </div>
            <div>
              <dt className="font-medium">Pinned</dt>
              <dd className="tabular-nums">{stats.pinned_memories}</dd>
            </div>
          </dl>
          <table className="mt-4 w-full border-collapse text-left text-sm">
            <caption className="sr-only">Memories per namespace</caption>
            <thead>
              <tr className="border-b border-gray-300">
                <th scope="col" className="py-1 pr-4">Namespace</th>
                <th scope="col" className="py-1 pr-4">Memories</th>
              </tr>
            </thead>
            <tbody>
              {stats.per_namespace.map((row) => (
                <tr key={row.namespace} className="border-b border-gray-200">
                  <td className="py-1 pr-4">{row.namespace}</td>
                  <td className="py-1 pr-4 tabular-nums">{row.count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}
