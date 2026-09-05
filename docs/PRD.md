# PRD — Recall-MCP

**Status:** Draft v1 (2026-09-06) · **Owner:** Pranam · **Stack:** Rust (Axum, Tokio, SQLite) + MCP + React 19/Vite UI

## Problem

AI agents (ZCode, AutoClaw, Claude-family tools, custom builds) are amnesiac. Every session starts
from zero; context is re-pasted, decisions are re-explained, and user preferences are re-learned.
Existing "memory" features are vendor-locked. What's missing is a **local-first, vendor-neutral
memory layer that any agent can use through an open protocol**.

## Product

**Recall-MCP** is a local semantic memory broker that any MCP-capable agent can connect to.
Agents store observations/decisions/preferences as short memory records; Recall-MCP retrieves the
right memories at query time via hybrid search (BM25 keyword + vector similarity) with recency
decay and source pinning.

## Goals (v1)

1. Implement an **MCP server** (stdio + streamable-HTTP transports) exposing tools:
   `remember`, `recall`, `forget`, `list-memories` — per the current MCP specification.
2. Hybrid retrieval: keyword (BM25-ish scoring) blended with embedding similarity; recency decay
   knob; deterministic, explainable ranking with score breakdowns in API responses.
3. Multi-workspace namespacing (`workspace_id` scoping) so different projects never leak memories.
4. First-class HTTP/JSON API (same engine as MCP) + **OpenAPI document**.
5. Web UI (screen-reader friendly): create/search memories, inspect score breakdowns, manage
   namespaces, view server stats.
6. CLI (`recall-cli`) for scripting: `remember`, `recall`, `serve`, `export`, `import`.

## Non-goals (v1)

- Multi-user auth (single local user; API-key auth stub only), cloud sync, entity graphs,
  automatic memory extraction from conversations (v2 candidate).

## Innovations (resume differentiators)

1. **MCP-native memory**: among the first local memory brokers exposing itself as an MCP server
   rather than a REST SaaS — plugs into ZCode/AutoClaw/any MCP client with zero glue code.
2. **Explainable hybrid retrieval**: every result ships its score decomposition
   (keyword/vector/recency), not a black-box top-k.
3. **Local-first with zero dependencies**: one SQLite file; no daemon services, no cloud round-trip.

## User stories

- As an agent, I call `remember{workspace, text, tags}` after a decision so it persists.
- As an agent, I call `recall{workspace, query, k}` and receive ranked memories with explanations.
- As a user, I search the UI for a memory and see why it ranked.
- As a user, I export/import my memory store as JSON for backup/migration.

## Success metrics

- p95 `recall` latency < 25 ms on 10k memories (local).
- ≥ 90% line and branch coverage on core crates; MCP conformance smoke test passes.
- Retrieval quality: ≥ 0.85 hit-rate@5 on a 50-query golden set (kept in `evals/`).

## Risks / mitigations

- Embedding model choice (local vs API): abstract behind a trait; ship a deterministic
  hash-based embedder for tests/offline, optional API embedder behind a flag.
- MCP spec drift: pin to spec version in docs; conformance smoke tests.
- SQLite vector limits at scale: document 1M-row ceiling; chunked scans with WAL.
