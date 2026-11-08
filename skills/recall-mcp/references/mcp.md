# MCP tools reference

Transport: stdio or streamable-HTTP (`POST /mcp`). The server targets the
`2026-07-28` spec. A legacy `initialize` naming `2026-07-28` is answered with
`2025-11-25`, the newest version with a handshake. The newer lifecycle sends
`server/discover`, then per-request metadata in `_meta` plus the
`MCP-Protocol-Version`, `Mcp-Method`, and (for tool calls) `Mcp-Name` headers.

All tool results are compact by default. Pass `include_embedding: true` to get
the full record with its vector.

## `remember`

Arguments: `namespace`, `text`, optional `tags`, `pinned`,
`include_embedding`. Creates the namespace if missing. Returns the compact
memory (`id`, `text`, `tags`, `source`, `created_at`).

## `recall`

Arguments: `namespace`, `query`, optional `k` (1-100, default 5), `tags`,
`tau_days` (`null` disables decay), `pinned_boost`, `format` (`json` default,
`text`), `include_embedding`. Returns hits with a `breakdown`
(`bm25`, `vector`, `recency`, `pinned_boost`, `total`). `format: "text"` cannot
be combined with embeddings.

## `update_memory`

Arguments: `id`, any of `text`, `tags`, `pinned`, plus `include_embedding`.
Changing text re-embeds it. Unknown ids are invalid-params errors, matching
REST's 404.

## `forget`

Argument: `id`. Permanently deletes one memory. Unknown ids are
invalid-params errors.

## `list_memories`

Arguments: `namespace`, optional `limit` (default 20, hard cap 100), `offset`,
`include_embedding`. Returns a compact page:
`{items, total, limit, offset, has_more}`.

## `capture`

Arguments: `namespace`, `transcript` (array of `{role, content, at}`), optional
`tags`, `max_chunk_chars`, `include_memories`, `include_embedding`. Messages are
chunked deterministically, embedded, and stored in one transaction. The default
response is `{namespace, captured, first_id, last_id}`.
