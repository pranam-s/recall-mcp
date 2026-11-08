# Token efficiency

Agent context is the scarce resource. Version 0.5.0 adds one compact
serialization layer (`recall-core::views`) used by MCP, REST, and the CLI, plus
a text format for recall. The full record is still one flag away.

## Measured bytes

Measured on 2026-11-08 on windows-msvc, at this commit, by
`cargo test -p recall-mcp-server --test mcp_http -- --nocapture`. The numbers
are serialized JSON-RPC response sizes (`body.to_string().len()`), not token
counts: token counts depend on the client's tokenizer. They are byte-for-byte
reproducible from the tests.

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
256-float hash embedding (about 1.2 KB as JSON); semantic embedders are larger,
which is why embeddings are opt-in.

## What changed

- Compact memory: `id`, `text`, `tags` (omitted when empty), `source`,
  `created_at`.
- Compact recall hit: compact memory plus the existing `breakdown`.
- Compact list page: `{items,total,limit,offset,has_more}`.
- Compact capture summary: `{namespace,captured,first_id,last_id}`.
- MCP `list_memories`: default 20, hard cap 100, `offset` supported.
- `include_embedding=true` restores the full record on MCP, REST, and CLI.
- `recall` accepts `format: "json"|"text"`; text keeps ranking and the complete
  breakdown and rejects embeddings.
- Tool descriptions and input-schema descriptions were shortened; the measured
  tools/list response dropped from 6,411 bytes before trimming to 3,370.

## Using it well

- Ask for the smallest surface you need. `format: "text"` is cheapest to read
  but loses machine-readable field names.
- Paginate instead of raising `k` or `limit`; the server refuses to dump a
  namespace past 100 items per MCP call.
- Store one claim per memory. A short precise memory retrieves better and
  costs less to return than a transcript.
- Keep embeddings out of agent context unless a client genuinely needs the
  vector.
