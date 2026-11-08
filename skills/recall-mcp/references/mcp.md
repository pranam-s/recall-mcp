# MCP tools reference

Transport: stdio or streamable-HTTP (`POST /mcp`). The server targets the
`2026-07-28` spec. A legacy `initialize` naming `2026-07-28` is answered with
`2025-11-25`, the newest version with a handshake. The newer lifecycle starts
with `server/discover`, then sends per-request metadata in `_meta` plus the
`MCP-Protocol-Version`, `Mcp-Method`, and (for tool calls) `Mcp-Name` headers.

Tools that return memories use compact records by default.
`include_embedding: true` returns each memory with its vector. For `capture`,
records are returned only when `include_memories: true`.

## `remember`

Arguments: `namespace`, `text`, optional `tags`, `pinned`,
`include_embedding`. Creates the namespace if missing. Returns the compact
memory (`id`, `text`, `tags`, `source`, `created_at`).

## `recall`

Arguments: `namespace`, `query`, optional `k` (1-100, default 5), `tags`
(ALL-of filter), `tau_days` (recency decay in days, default 30; `null` disables
decay), `pinned_boost` (default 0.2), `format` (`json` default, `text`), and
`include_embedding`.
Returns hits with a `breakdown` (`bm25`, `vector`, `recency`, `pinned_boost`,
`total`). `format: "text"` cannot be combined with embeddings.

## `update_memory`

Arguments: `id`, any of `text`, `tags`, `pinned`, plus `include_embedding`.
Only the fields you pass change. Changing text re-embeds it. Unknown ids are
invalid-params errors, matching REST's 404.

## `forget`

Argument: `id`. Permanently deletes one memory. Unknown ids are invalid-params
errors.

## `list_memories`

Arguments: `namespace`, optional `limit` (default 20, hard cap 100), `offset`,
and `include_embedding`. Returns a compact page:
`{items, total, limit, offset, has_more}`.

## `capture`

Arguments: `namespace`, `transcript` (array of `{role, content, at}`), optional
`tags`, `max_chunk_chars`, `include_memories`, and `include_embedding`. Messages
are chunked deterministically, embedded, and stored in one transaction. The
default response is `{namespace, captured, first_id, last_id}`. With
`include_memories`, the response also carries the stored `memories`.
