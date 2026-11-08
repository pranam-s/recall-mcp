# Token efficiency

Agent context is limited, so response size matters. Version 0.5.0 has one
compact serialization layer (`recall-core::views`) shared by MCP, REST, and the
CLI, plus a text format for recall. The full record is still one flag away.

## Measured bytes

Measured on 2026-11-08 in a Windows MSVC build by
`cargo test -p recall-mcp-server --test mcp_http -- --nocapture`. The numbers
are serialized JSON-RPC response sizes (`body.to_string().len()`), not token
counts, because token counts depend on the client's tokenizer. The tests
reproduce them byte for byte.

| Surface | Bytes | Test |
|---|---:|---|
| `tools/list` (six tools, trimmed descriptions and schemas) | 3,370 | `tools_list_stays_within_the_token_budget` |
| `remember` compact record | 200 | `compact_and_text_responses_stay_smaller_than_full_records` |
| `remember` full record (`include_embedding`) | 1,403 | same |
| `recall` one compact hit with breakdown | 598 | same |
| `recall` one full hit with embedding | 3,004 | same |
| `recall --format text`, one hit | 384 | same |

The tools/list budget is 4,600 bytes; the measured 3,370 leaves 1,230 bytes of
headroom for future optional fields. A full record is dominated by the
256-float hash embedding (about 1.2 KB as JSON). Semantic embedders are larger,
which is why embeddings are opt-in.

## What the compact layer includes

The compact memory record carries `id`, `text`, `tags` (omitted when empty),
`source`, and `created_at`. A compact recall hit is that record plus the
existing `breakdown`. List pages serialize as
`{items,total,limit,offset,has_more}` and capture summaries as
`{namespace,captured,first_id,last_id}`.

MCP `list_memories` defaults to 20 items, caps at 100, and supports `offset`.
Passing `include_embedding=true` restores the full record on MCP, REST, and CLI.
`recall` accepts `format: "json"|"text"`; the text format keeps ranking and the
complete breakdown and rejects embeddings.

Trimming tool descriptions and input-schema descriptions reduced the measured
`tools/list` response from 6,411 bytes to 3,370.

## Using it well

Ask for the smallest surface you need. `format: "text"` is the cheapest to read
but drops machine-readable field names. Paginate instead of raising `k` or
`limit`; the server returns at most 100 items per MCP call. Store one claim per
memory, since a short precise memory retrieves better and costs less to return
than a transcript. Keep embeddings out of agent context unless a client needs
the vector.
