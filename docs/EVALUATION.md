# Recall-MCP evaluation: 0.5.0

The measured results on this page come from runs on 2026-11-08 on the Windows
MSVC development machine with rustc 1.99.1, unless a row names another
toolchain. GitHub Actions runs the same test, lint, coverage, web, and
dependency gates on push and pull requests. No external benchmark is claimed.

## Test suite

`cargo test --workspace --locked`: 209 passed, 0 failed, 3 ignored.

| Suite | Passed | Covers |
|---|---:|---|
| recall-core unit | 119 | models, validation, views, crypto, embedders, scorer, store CRUD, update, recall ranking and ties, tag filters, packed cache, `data_version`, encryption upgrade and forensic cleanup, embedder guard, schema version, export/import, capture chunking, OpenAI request/dimension/redaction |
| recall-core golden eval | 2 | 100-query golden gate and hash semantic diagnostic |
| recall-core proptests | 4 | crypto roundtrip/AAD, FTS syntax, chunking invariants, scorer linearity and bounds |
| recall-core ignored | 3 | FTS query-plan probe, p95 harness, cost breakdown (manual gates) |
| recall-server unit | 40 | error/status mapping, JSON error contract, auth, rate limiting and bucket bound, Host/Origin scheme, security headers, slow-embedder responsiveness, MCP info, list cap/offset, capture, OpenAPI drift, metrics |
| recall-server API | 12 | REST CRUD, auth matrix, pagination, compact/full views, 400/404/405/413/415/429 JSON errors, security headers, origin scheme, capture rollback |
| recall-server MCP HTTP | 6 | initialize/tools/list/tools/call, stateless 2026-07-28 discover/list/call, API key, tools/list byte budget, compact vs full vs text sizes |
| recall-server metadata | 2 | version pins across server.json/CITATION/README/release notes; six MCP tools in source and OpenAPI; auth schemes |
| recall-server proptest | 1 | token-bucket partition and burst bound |
| recall-cli unit | 9 | config precedence, embedder selection, origin validation, BOM tolerance |
| recall-cli e2e | 12 | remember/recall/export/import, update, encryption, backup/vacuum, compact defaults, text format, list page, stats, capture, keygen, forget, `recall` alias, env fallbacks |
| recall-cli stdio | 2 | legacy initialize conformance and stateless 2026-07-28 discover/list/call |

Doc tests: `cargo test --workspace --doc --locked` passed (0 doc tests defined).

Optional features: `cargo test -p recall-mcp-core --features 'onnx openai' --lib`
with onnxruntime 1.28.0 and a model cache: 127 passed, 2 ignored (the
feature-gated ONNX and OpenAI tests plus the default suite). The full
`cargo test --workspace --all-features` run measured 216 passed, 0 failed,
4 ignored.

Formatting and lints:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  0 warnings.

## Coverage

`cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90
--summary-only --locked`, on the Windows MSVC machine, 2026-11-08:

- recall-core: 95.39% lines, 94.19% regions, above the 90% gate.
- Branch coverage is unmeasured: cargo-llvm-cov 0.9.1 has no branch fail-under
  on stable (D-010 amendment).

Per-file lines: crypto 99.16%, embed 100%, error 100%, export 100%, scorer
100%, util 100%, validation 98.35%, views 99.27%, sqlite 93.73%, capture
94.21%, models 93.24%, lib 78.57%.

The GitHub Actions coverage job runs on Linux. `llvm-cov` cannot run on
windows-gnu (D-010), so the local measurement uses Windows MSVC.

## Retrieval quality

Corpus: 100 memories across `product`, `ops`, and `research`. Golden set: 100
queries (50 lexical, 30 tag-scoped, 20 temporal), gate hit-rate@5 >= 0.85 and
MRR@10 >= 0.80.

```text
golden: 98/100 hit-rate@5 = 0.980 (gate >= 0.85), MRR@10 = 0.975
  embedder: hash-256
semantic (diagnostic): 31/50 hit-rate@5 = 0.620 (gate >= 0.85), MRR@10 = 0.508
  embedder: hash-256
semantic: 50/50 hit-rate@5 = 1.000 (gate >= 0.85), MRR@10 = 0.740
  embedder: onnx:all-minilm-l6-v2-q
```

The ONNX run used fastembed 7.6.1 and onnxruntime 1.28.0 through
`ORT_DYLIB_PATH`, one test thread, and a fresh model cache. The semantic gate
is hit-rate@5; MRR is reported for the record. The hash diagnostic is expected
to be weak on paraphrases and does not gate.

Labelling rules and file descriptions live in [evals/README.md](../evals/README.md).

## Recall latency at 10k

The design target is 25 ms p95 at 10,000 memories. The harness seeds 10,000
embedded memories, runs 100 untimed warm-up recalls, then times 1,000 recalls.
p95 is the 95th percentile of those timed recalls: 95% finish at or below that
value.

Command:

```sh
cargo test -p recall-mcp-core --release --test p95_latency -- --ignored --nocapture --test-threads=1
```

Run it with `--test-threads=1`: the cost-breakdown test seeds its own 10k store
and distorts p95 when both ignored tests run in parallel. The harness is manual
and does not run in CI.

## Token budgets

Measured by `cargo test -p recall-mcp-server --test mcp_http -- --nocapture`:

| Surface | Bytes |
|---|---:|
| tools/list (six tools) | 3,370 |
| remember compact / full | 200 / 1,403 |
| recall one compact / full hit | 598 / 3,004 |
| recall one text hit | 384 |

The tools/list budget is 4,600 bytes; headroom is 1,230 bytes. Method and the
trimming results are in [TOKEN-EFFICIENCY.md](TOKEN-EFFICIENCY.md).

## Web UI

`cd web && npm run build && npx playwright test`: 8 passed, including a keyed
server on a second port. `@axe-core/playwright` runs WCAG 2.1 A/AA on the
loaded page and the expanded metrics state: 0 violations. The suite covers
create/recall with breakdowns, keyboard paging, capture, metrics, stats with the
embedder identity, pin/unpin and delete with a native dialog and focus return,
and API-key save/clear with sessionStorage persistence.

## Dependency and security gates

- `cargo audit`: 0 vulnerabilities. One allowed unmaintained warning for
  `paste` (transitive through fastembed's tokenizers), recorded in `audit.toml`
  with a 2026-11-08 review date. It is a build-time dependency with no known
  vulnerability and no upstream fix.
- `cargo deny check`: advisories, bans, licenses, and sources pass. Licenses are
  permissive and MIT-compatible.
- npm: `npm audit --audit-level=high` reports 0 vulnerabilities on the refreshed
  lockfile.
- The web dependency refresh targeted Vite 8.3.6, React 19.3.1, Playwright 1.65,
  and axe 4.14; the committed lockfile records what resolved.

## Toolchains

- Primary: windows-msvc through `scripts/with-msvc.ps1` (vcvars64).
- Secondary: `cargo +stable-x86_64-pc-windows-gnu test --workspace --target
  x86_64-pc-windows-gnu --locked`.
- MSRV: 1.89 default features, 1.92 all features. CI runs both checks.

## Gaps and unmeasured areas

- No Docker image is published. The Dockerfile is in the repository but has
  not been built in CI, and `server.json` does not list an OCI package yet.
- Branch coverage: no stable fail-under flag.
- OpenAI calls: the embedder has never been called live; no API key was
  available.
- Client integrations: transport tests exist, but the six client snippets are
  starting points and have not been verified against every client.
