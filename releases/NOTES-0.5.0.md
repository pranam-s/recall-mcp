# Recall-MCP 0.5.0

Released 2026-11-08. This is the first public release; 0.4.0 was never tagged
or published.

## Highlights

- Compact responses across MCP, REST, and the CLI, with full records available
  through `include_embedding`.
- A short `recall` binary and a complete one-shot CLI: remember, recall,
  update, forget, list, capture, stats, keygen, export, import, backup, vacuum,
  and serve.
- Encryption upgrades run a forensic VACUUM and WAL truncate on every
  plaintext-to-keyed transition, with a retryable marker.
- Bounded rate limiting and input validation.
- Stateless MCP 2026-07-28 conformance over stdio and HTTP.
- A keyed-server web test, API-key connection UI, pin and delete controls, and
  embedder identity in Stats.
- A 100-query golden eval with MRR@10 and a 50-query ONNX semantic set.

## Migration from 0.4.x

0.4.0 was never published, so this table is mainly for anyone who tracked the
repository.

| Surface | 0.4.x | 0.5.0 |
|---|---|---|
| MCP and REST memory record | full record by default | compact by default; add `include_embedding=true` |
| MCP and REST recall hit | full memory plus breakdown | compact memory plus breakdown; `include_embedding=true` for full |
| REST `GET /v1/memories` | bare array plus `X-Total-Count` | `{items,total,limit,offset,has_more}` plus the header |
| REST capture | summary plus all memories | compact summary; `include_memories=true` |
| MCP `list_memories` | default 50, cap 10,000 | default 20, cap 100, `offset` supported |
| MCP `recall` | JSON only | `format=json\|text`; text rejects embeddings |
| CLI binary | `recall-cli` | `recall` plus the `recall-cli` alias |
| CLI JSON | pretty, embeddings omitted | compact when piped, pretty on a terminal; `--pretty`, `--full`, `--show-embedding` |
| CLI commands | remember, recall, update, export, import, backup, vacuum, serve | adds forget, list, capture, stats, keygen |
| CLI database | `--db` only | `--db` or `RECALL_MCP_DB`; namespace environment fallback |
| Config keys | `allow_origins`, `allow_hosts` | same plural keys, with singular aliases |
| Embedder identity | none stored | fingerprint guard in `store_meta`; explicit override |
| MCP protocol | initialize handshake | 2026-07-28 stateless lifecycle; the legacy handshake still answers 2025-11-25 |
| Errors | mixed JSON and plain framework errors | JSON for every API rejection |

No manual database migration is needed beyond the automatic schema version 2
stamp. A store written by an older embedder is adopted only when dimensions
match; otherwise the guard names both sides and requires
`--allow-embedder-mismatch`.

## Verification

These results were measured in a Windows MSVC build with rustc 1.99.1 on
2026-11-08. The exact output is in `docs/EVALUATION.md`.

- `cargo test --workspace --locked`: 207 passed, 0 failed, 3 ignored.
- `cargo test --workspace --all-features --locked` with onnxruntime 1.28.0:
  214 passed, 0 failed, 4 ignored.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings`: 0 warnings.
- `cargo llvm-cov -p recall-mcp-core --fail-under-lines 90
  --fail-under-regions 90`: 95.39% lines, 94.19% regions.
- Golden eval: 98/100 hit-rate@5 (0.980), MRR@10 0.975.
- Semantic eval with ONNX: 50/50 hit-rate@5 (1.000), MRR@10 0.740. The hash
  diagnostic scored 31/50 (0.620).
- `cargo audit`: 0 vulnerabilities, with one allowed unmaintained `paste`
  warning.
- `cargo deny check`: pass.
- Web: build clean, 8 Playwright tests passed, axe WCAG 2.1 AA at 0
  violations.
- tools/list: 3,370 bytes against a 4,600-byte budget.

## Limitations

- p95 recall at 10k memories has a 25 ms design target. The harness is an
  ignored manual test, so run it locally; the command is in
  `docs/EVALUATION.md`.
- The OpenAI embedder has never been called against the live API because no
  key was available.
- The client snippets in `docs/clients.md` are starting points, not verified
  against every client version.
- Dense embeddings are invertible, so keyed mode does not protect memory text
  when ONNX or OpenAI vectors are stored.

## Install

Download the Windows x86_64 archive attached to this release. It contains
`recall.exe`, `recall-cli.exe`, `LICENSE`, and `README.md`. SHA-256:

```text
2a8e53dcaaad18ef16094841cc53908efab3a1a920205067abd1000bf6bd410c
```

To build from source:

```sh
cargo install --path crates/recall-cli --locked
recall --version
recall serve --bind 127.0.0.1:8787 --db recall.db
```

The crates.io packages are not published yet. When they are, the install
command will be `cargo install recall-mcp-cli --version 0.5.0`. Publish order
is `recall-mcp-core`, then `recall-mcp-server`, then `recall-mcp-cli`. See
`RELEASING.md`.
