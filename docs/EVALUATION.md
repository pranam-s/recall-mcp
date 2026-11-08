# Recall-MCP evaluation: 0.5.0

Everything here was executed locally on 2026-11-08 in this worktree, on
windows-msvc with rustc 1.99.1, unless a row names another toolchain. Nothing
is copied from CI: Actions has not run (billing). No external benchmark is
claimed.

## Test suite

`cargo test --workspace --locked`: **206 passed, 0 failed, 3 ignored**.

| Suite | Passed | Covers |
|---|---:|---|
| recall-core unit | 117 | models, validation, views, crypto, embedders, scorer, store CRUD, update, recall ranking and ties, tag filters, packed cache, `data_version`, encryption upgrade and forensic cleanup, embedder guard, schema version, export/import, capture chunking, OpenAI request/dimension/redaction |
| recall-core golden eval | 2 | 100-query golden gate and hash semantic diagnostic |
| recall-core proptests | 4 | crypto roundtrip/AAD, FTS syntax, chunking invariants, scorer linearity and bounds |
| recall-core ignored | 3 | FTS query-plan probe, p95 harness, cost breakdown (manual gates) |
| recall-server unit | 39 | error/status mapping, JSON error contract, auth, rate limiting and bucket bound, Host/Origin scheme, security headers, slow-embedder responsiveness, MCP info, list cap/offset, capture, OpenAPI drift, metrics |
| recall-server API | 12 | REST CRUD, auth matrix, pagination, compact/full views, 400/404/405/413/415/429 JSON errors, security headers, origin scheme, capture rollback |
| recall-server MCP HTTP | 6 | initialize/tools/list/tools/call, stateless 2026-07-28 discover/list/call, API key, tools/list byte budget, compact vs full vs text sizes |
| recall-server metadata | 2 | version pins across server.json/CITATION/README/release notes; six MCP tools in source and OpenAPI; auth schemes |
| recall-server proptest | 1 | token-bucket partition and burst bound |
| recall-cli unit | 9 | config precedence, embedder selection, origin validation, BOM tolerance |
| recall-cli e2e | 12 | remember/recall/export/import, update, encryption, backup/vacuum, compact defaults, text format, list page, stats, capture, keygen, forget, `recall` alias, env fallbacks |
| recall-cli stdio | 2 | legacy initialize conformance and stateless 2026-07-28 discover/list/call |

Doc tests: `cargo test --workspace --doc --locked` passed (0 doc tests defined).

Optional features: `cargo test -p recall-mcp-core --features 'onnx openai' --lib`
with onnxruntime 1.28.0 and a model cache: **125 passed, 2 ignored** (the
feature-gated ONNX and OpenAI tests plus the default suite). The full
`cargo test --workspace --all-features` run measured **213 passed, 0 failed,
4 ignored**.

Formatting and lints:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  0 warnings.

## Coverage

`cargo llvm-cov -p recall-mcp-core --fail-under-lines 90 --fail-under-regions 90
--summary-only --locked`, windows-msvc, 2026-11-08:

- **recall-core: 95.39% lines, 94.19% regions** (gate 90/90, exit 0).
- Branch coverage is unmeasured: cargo-llvm-cov 0.9.1 has no branch fail-under
  on stable (D-010 amendment).

Per-file lines: crypto 99.16%, embed 100%, error 100%, export 100%, scorer
100%, util 100%, validation 98.35%, views 99.27%, sqlite 93.73%, capture
94.21%, models 93.24%, lib 78.57%.

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

## Recall latency at 10k (manual gate, measured failure)

Command:

```sh
cargo test -p recall-mcp-core --release --test p95_latency -- --ignored --nocapture --test-threads=1
```

The harness seeds 10,000 embedded memories, runs 100 untimed warm-up recalls,
then times 1,000 recalls. Run it with `--test-threads=1`: the cost-breakdown
test seeds its own 10k store and distorts p95 when both ignored tests run in
parallel. Runs 1-2 are from implementation; runs 3-4 are review runs with the
serial command above:

| Run | mean | p50 | p95 | p99 | max | 25 ms gate |
|---|---:|---:|---:|---:|---:|---|
| 1 | 21.674 | 20.738 | **33.058** | 42.021 | 51.700 | FAIL |
| 2 | 22.706 | 21.974 | **32.768** | 40.462 | 48.432 | FAIL |
| 3 | 22.707 | 21.572 | **32.745** | 40.174 | 51.696 | FAIL |
| 4 | 20.247 | 19.006 | **29.583** | 40.357 | 48.731 | FAIL |

Cost breakdown from the final clean review run: hybrid recall 19.912 ms/call,
keyword-only recall 19.233 ms/call, full-row materialization 447.898 ms/call.
The keyword floor moving with the same host load explains the p95 spread; the
vector scan is no longer the dominant cost.

**The PRD 25 ms p95 budget is not met on this host.** The threshold was not
lowered and the harness remains an ignored manual gate. The previous 0.4.x
steady-state numbers (p95 20.1 to 20.9 ms) came from a different host state and
are history, not a current claim. This is the main performance gap in 0.5.0.

## Token budgets

Measured by `cargo test -p recall-mcp-server --test mcp_http -- --nocapture`:

| Surface | Bytes |
|---|---:|
| tools/list (six tools) | 3,370 |
| remember compact / full | 200 / 1,403 |
| recall one compact / full hit | 598 / 3,004 |
| recall one text hit | 384 |

The tools/list budget is 4,600 bytes; headroom is 1,230 bytes. Method and the
trim history are in [TOKEN-EFFICIENCY.md](TOKEN-EFFICIENCY.md).

## Web UI

`cd web && npm run build && npx playwright test`: **8 passed**, including a
keyed server on a second port. `@axe-core/playwright` runs WCAG 2.1 A/AA on the
loaded page and the expanded metrics state: **0 violations**. The suite covers
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
- Web dependency refresh: Vite 8.3.6, React 19.3.1, Playwright 1.65, axe 4.14
  were the update targets; the committed lockfile is the source of truth for
  what actually resolved.

## Toolchains

- Primary: windows-msvc through `scripts/with-msvc.ps1` (vcvars64).
- Secondary: `cargo +stable-x86_64-pc-windows-gnu test --workspace --target
  x86_64-pc-windows-gnu --locked`.
- MSRV: 1.89 default features, 1.92 all features. Both are CI jobs; local
  toolchain availability is noted in the PR if a version could not be installed.

## What is not measured

- CI results: Actions is disabled for billing.
- The Docker image was not built because the local Docker daemon is not
  running. The Dockerfile is committed but unverified in this environment, and
  no image has been pushed to the `ghcr.io` tag in `server.json`.
- Branch coverage: no stable fail-under flag.
- OpenAI calls: no key in this environment.
- Client integrations: transport tests exist; the six client snippets are
  expected, not live-verified.
