# Ideas, deferred with reasons

Nothing here is promised. Each item records why it is parked so a future
contributor can pick it up or drop it with the original reasoning intact.
Retrieval and storage decisions live in
[docs/adr/DECISIONS.md](docs/adr/DECISIONS.md); this file is for ideas that are
not decided yet.

## Retrieval and performance

ANN or packed-vector-file retrieval beyond roughly 100k memories per namespace
(D-002, a mandatory revisit beyond that scale). The 10k design target is 25 ms
p95 (see [docs/EVALUATION.md](docs/EVALUATION.md)). Building ANN now would add an
index format without addressing the keyword-pass cost. LanceDB is the recorded
first choice when the scale trigger fires.

FTS candidate-pass optimization (remediation path (b) in D-002). This is the next
performance item.

Cold-open first-recall latency. The first recall after a fresh process is slower
because the packed vector cache is empty. A warm-up on serve could remove that
cost; it would not change the steady-state p95.

Eviction and recency-of-use policy. The `last_accessed_at` column is reserved for
it and is write-time only; recalls no longer touch it. The feature needs a use
case before it earns an UPDATE back on the hot path.

## Trust and encryption

Encrypt embeddings, namespace names, and tags at rest (D-014, D-021). Only
`memories.text` is sealed today. Dense embeddings are invertible to text, so
keyed mode with `onnx` or `openai` does not protect text. Encrypting or dropping
the plaintext-side metadata is a possible v2 change; it changes the storage
format and the FTS story.

True branch coverage gate. cargo-llvm-cov has no branch fail-under on stable Rust;
`--branch` needs nightly (D-010 amendment). Bring the gate back when stable rustc
ships branch instrumentation.

## Surface

Generated TypeScript client from the OpenAPI document (D-006). `web/src/api.ts` is
hand-typed. OpenAPI codegen was rejected for the v1 UI because generated clients
lag the document and bloat the bundle; revisit if a second API consumer appears.

Live API-key UX for a keyed server. The web console saves the key in
`sessionStorage` and sends `X-API-Key`; a full login and session story is still
out of scope.

OpenAI embedder live verification. Request, parse, and dimension logic has unit
tests, but the client has never called the live API (no key was available). One
recorded live call would close the last path that tests do not execute.

Docker image publish. The `Dockerfile` is committed, but no image is published and
`server.json` does not list an OCI package yet. Publishing an image needs a
registry login and an image push step. When one is published, add the OCI package
to `server.json` with the release tag and the Dockerfile's
`io.modelcontextprotocol.server.name` label.

## Out of scope

Multi-user auth and OAuth, TLS termination, and cloud sync. Recall-MCP is a
local-first single-user tool; putting it on a network is a different product with
a different threat model.

Entity graphs and memory consolidation flows. No demonstrated need at the scales
this tool targets.

A team server with shared memory. That is a different product.
