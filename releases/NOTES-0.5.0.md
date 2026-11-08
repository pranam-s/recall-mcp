# Recall-MCP 0.5.0

Release date: 2026-11-08. Status: **untagged until the owner tags and publishes
it**. 0.4.0 was never tagged or published; 0.5.0 is the first public release.

## Highlights

- Compact responses across MCP, REST, and CLI, with full records one flag away.
- A shorter `recall` binary plus a complete one-shot CLI surface.
- Encryption upgrades now run a forensic VACUUM and WAL truncate on every
  plaintext-to-keyed transition, with a retryable marker.
- Bounded, validated rate limiting and input limits.
- Stateless MCP 2026-07-28 conformance over both transports.
- A keyed-server web test, API-key connection UI, pin/delete, and embedder
  identity in Stats.
- A 100-query golden eval with MRR@10 and a 50-query ONNX semantic set.

## Migration from 0.4.x

0.4.0 was never published, so this table is mainly for anyone tracking the
repository.

| Surface | 0.4.x | 0.5.0 |
|---|---|---|
| MCP/REST memory record | full record by default | compact by default; add `include_embedding=true` |
| MCP/REST recall hit | full memory + breakdown | compact memory + breakdown; `include_embedding=true` for full |
| REST `GET /v1/memories` | bare array + `X-Total-Count` | `{items,total,limit,offset,has_more}` + header |
| REST capture | summary plus all memories | compact summary; `include_memories=true` |
| MCP `list_memories` | default 50, cap 10,000 | default 20, cap 100, `offset` supported |
| MCP `recall` | JSON only | `format=json\|text`; text rejects embeddings |
| CLI binary | `recall-cli` | `recall` plus the `recall-cli` alias |
| CLI JSON | pretty, embeddings omitted | compact when piped, pretty on a terminal; `--pretty`, `--full`, `--show-embedding` |
| CLI commands | remember, recall, update, export, import, backup, vacuum, serve | adds forget, list, capture, stats, keygen |
| CLI database | `--db` only | `--db` or `RECALL_MCP_DB`; namespace env fallback |
| Config keys | `allow_origins`, `allow_hosts` | same canonical plural keys, with singular aliases |
| Embedder identity | none stored | fingerprint guard in `store_meta`; explicit override |
| MCP protocol | initialize handshake | 2026-07-28 stateless lifecycle; legacy handshake still answers 2025-11-25 |
| Errors | mixed JSON/plain framework errors | JSON for every API rejection |

No database migration is required beyond the automatic schema version 2 stamp.
A store written by an older embedder is adopted only when dimensions match;
otherwise the guard names both sides and requires
`--allow-embedder-mismatch`.

## Measured verification

Local, windows-msvc, rustc 1.99.1, 2026-11-08. Exact output is in
`docs/EVALUATION.md`.

- `cargo test --workspace --locked`: 206 passed, 0 failed, 3 ignored.
- `cargo test --workspace --all-features --locked` with onnxruntime 1.28.0:
  213 passed, 0 failed, 4 ignored.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings`: 0 warnings.
- `cargo llvm-cov -p recall-mcp-core --fail-under-lines 90
  --fail-under-regions 90`: 95.39% lines, 94.19% regions.
- Golden eval: 98/100 hit-rate@5 (0.980), MRR@10 0.975.
- Semantic eval with ONNX: 50/50 hit-rate@5 (1.000), MRR@10 0.740. Hash
  diagnostic: 31/50 (0.620).
- `cargo audit`: 0 vulnerabilities, one allowed unmaintained `paste` warning.
- `cargo deny check`: pass.
- Web: build clean, 8 Playwright tests passed, axe WCAG 2.1 AA 0 violations.
- tools/list: 3,370 bytes (budget 4,600).

## Known gaps

- **p95 at 10k is not met on this host.** Four serial runs measured 29.583,
  32.745, 32.768, and 33.058 ms against the 25 ms PRD budget. The ignored manual
  gate remains at 25 ms and fails honestly.
- **CI has not run.** GitHub Actions is disabled for billing. Workflows are
  committed and were not executed remotely. Do not treat them as passing.
- The OpenAI embedder has never been called live (no key).
- Client snippets are expected starts, not live-verified.
- Dense embeddings are invertible; keyed mode does not protect text with ONNX
  or OpenAI vectors.

## Install

```sh
cargo install recall-mcp-cli --version 0.5.0
recall --version
recall serve --bind 127.0.0.1:8787 --db recall.db
```

Crate order if publishing manually: `recall-mcp-core`, then
`recall-mcp-server`, then `recall-mcp-cli`. See `RELEASING.md`.
