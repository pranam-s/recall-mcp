# Ideas, deferred with reasons

Nothing here is promised. Each item records why it is parked so a future
session can pick it up (or drop it) with the original reasoning intact.
Retrieval and storage decisions live in `docs/adr/DECISIONS.md`; this file
is for the not-yet-decided.

## Retrieval / performance

- **ANN or packed-vector-file retrieval beyond ~100k memories/namespace**
  (D-002, mandatory revisit beyond that scale). Deferred: the 10k worst case
  does not meet the 25 ms p95 budget (see EVALUATION), but the residual is the
  FTS keyword pass, not the vector scan; building ANN now would add an index
  format without addressing that floor. LanceDB is the recorded first choice
  when the scale trigger fires.
- **FTS candidate-pass optimization** (remediation path (b) in D-002):
  keyword-only floor ~15-17 ms/call in the measured diagnostics. This is the
  remaining cause of the p95 budget failure and is the next performance item.
- **Cold-open first-recall latency**: the first recall after a fresh process
  is slower because the packed vector cache fills and first page faults land in
  the tail. A warm-up-on-serve could remove that extra cost; it would not fix
  the steady-state p95 gap above.
- **Eviction / recency-of-use policy**: the `last_accessed_at` column is
  reserved for it (write-time only since AR-018; recalls no longer touch
  it). Needs a use case before it earns an UPDATE back on the hot path.

## Trust and encryption

- **Encrypt embeddings, namespace names, tags at rest** (v0.2-era deferral,
  D-014/D-021): today only `memories.text` is sealed; dense embeddings are
  invertible to text, so keyed mode with `onnx`/`openai` does not protect
  text. Encrypting (or dropping) plaintext-side metadata is the v2
  candidate; it changes the storage format and the FTS story.
- **True branch coverage gate**: cargo-llvm-cov has no branch fail-under on
  stable Rust (`--branch` needs nightly; D-010 amendment). Bring the branch
  gate back when stable rustc ships branch instrumentation.

## Surface

- **Generated TypeScript client from the OpenAPI document** (D-006):
  `web/src/api.ts` is hand-typed. OpenAPI codegen was rejected for the v1
  UI (generated clients lag the doc and bloat the bundle); revisit if a
  second API consumer appears.
- **Live API-key UX for a keyed server**: the web console now saves the key in
  sessionStorage and sends `X-API-Key`; a full login/session story is still
  out of scope.
- **OpenAI embedder live verification**: request/parse/dims logic is unit-
  tested but the client has never called the live API (no key available
  during development). One recorded live call would close the last untested-by-execution
  path.
- **Docker image publish**: the Dockerfile and server.json are committed, but
  no workflow pushes the advertised `ghcr.io` image. The owner must add a
  registry login and image push before the OCI package can resolve.

## Not pursuing (decided, do not re-litigate)

- Multi-user auth/OAuth, TLS termination, cloud sync (PRD non-goals;
  local-first single-user tool; putting it on the network is a different
  product with a different threat model).
- Entity graphs and memory consolidation flows (no demonstrated need at the
  scales this tool targets).
- Team Server (multi-user shared memory is a different product; see the
  PRD non-goals).
