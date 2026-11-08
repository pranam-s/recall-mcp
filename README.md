# Recall-MCP

mcp-name: io.github.pranam-s/recall-mcp

Recall-MCP is a local-first semantic memory broker for AI agents. One SQLite file
and one server hold short records that agents write and read back through MCP
over stdio or streamable HTTP, REST, the CLI, or a web UI. Every recall hit
carries the score breakdown behind its rank: `{bm25, vector, recency,
pinned_boost, total}`. Keyword search works offline with no model. An optional
local ONNX embedder adds semantic matching, and an optional key encrypts memory
text with ChaCha20-Poly1305.

Status: **0.5.0**, tagged 2026-11-08. This is the latest release; `master`
carries fixes that are not in the tag yet.

## Contents

- [Why Recall-MCP](#why-recall-mcp)
- [Install](#install)
- [Quickstart](#quickstart)
- [Connect an agent](#connect-an-agent)
- [Web UI](#web-ui)
- [How recall works](#how-recall-works)
- [Configuration](#configuration)
- [Security and privacy](#security-and-privacy)
- [Limitations](#limitations)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

## Why Recall-MCP

Agents start each session from zero. Recall-MCP keeps observations, decisions,
and preferences in one place and retrieves them with keyword, vector, and
recency signals. Every hit reports the components behind its rank. The hash
embedder scores 98/100 hit-rate@5 on the golden set, and ONNX scores 50/50 on
the paraphrase set. Evaluation records the method and caveats.

## Install

The [latest release](https://github.com/pranam-s/recall-mcp/releases/latest) has
a Windows x86_64 archive, `recall-mcp-windows-x86_64.zip`, with `recall.exe`,
`recall-cli.exe`, `LICENSE`, and `README.md`. Unzip it and add the directory to
`PATH`.

To build from source, install Rust stable and a C compiler for bundled SQLite
(MSVC, gcc, and clang all work). Node 24+ is only needed for the web UI.

```sh
git clone https://github.com/pranam-s/recall-mcp
cd recall-mcp
cargo build --release --locked
```

The binaries land in `target/release/`: `recall`, with `recall-cli` kept as an alias.

## Quickstart

The examples use a new `recall.db` in the current directory.

```console
$ recall remember --db recall.db --namespace work --text "We deploy on Thursdays" --tags ops
{"created_at":1794127372,"id":"19ca950c-d6b5-42e0-af12-1b41f143a998","source":"cli","tags":["ops"],"text":"We deploy on Thursdays"}
$ recall recall --db recall.db --namespace work --query "when do we deploy?" --k 1
[{"breakdown":{"bm25":1.0,"pinned_boost":0.0,"recency":1.0,"total":0.775,"vector":0.5},"created_at":1794127372,"id":"19ca950c-d6b5-42e0-af12-1b41f143a998","source":"cli","tags":["ops"],"text":"We deploy on Thursdays"}]
```

JSON is compact when stdout is a pipe and pretty on a terminal. `--pretty`
forces pretty output, `--full` restores the storage fields (`namespace_id`,
`pinned`, `last_accessed_at`), and `--show-embedding` includes the vector.
Compact is the default because one hash embedding is about 1.2 KB of JSON per
record.

## Connect an agent

```sh
recall serve --bind 127.0.0.1:8787 --db recall.db
```

That starts REST at `/v1/...`, MCP at `/mcp` over streamable HTTP, the OpenAPI
document at `/openapi.json`, and metrics at `/metrics`. For a child-process
client, use `recall serve --stdio --db recall.db`.

The six MCP tools are `remember`, `recall`, `forget`, `update_memory`,
`list_memories`, and `capture`. Responses are compact by default; set
`include_embedding=true` for full records.

Client snippets for Claude Code, Codex, Cursor, OpenCode v2, Warp, and
Antigravity are starting points and have not been verified against every client
version. Capture accepts a transcript at `POST /v1/capture` or through the MCP
`capture` tool and stores it as chunked memories.

## Web UI

The React UI talks only to `/v1/*` on the same origin. Build it with Node 24+
(`cd web && npm ci && npm run build`), then serve it with
`recall serve --bind 127.0.0.1:8787 --db recall.db --web-dir web/dist` and open
`http://127.0.0.1:8787/`. Every control is keyboard-operable, and the Playwright
suite runs axe WCAG 2.1 AA checks with 0 violations.

## How recall works

```text
total = w_bm25 * norm_bm25 + w_vector * cosine + w_recency * exp(-days/tau) + pinned_boost
defaults: 0.45 / 0.45 / 0.10, tau = 30 days, pinned_boost = 0.2 (tunable per request)
```

Recall runs an FTS5 keyword pass, an exact cosine pass over embedding BLOBs in
rowid order, then merges and ranks the candidates. The top `k` return with their
breakdowns. A packed in-memory vector cache serves repeat recalls: it fills on
the first recall after a fresh process, rebuilds after a write or a commit from
another connection, and tag-filtered recalls bypass it.

Beyond roughly 100k memories per namespace, the D-002 ANN revisit applies.

## Configuration

Flags take precedence over environment variables, then the JSON config file,
then defaults, per setting. The API key comes from a flag or environment
variable; the encryption key comes from the environment. Neither is
file-configurable.

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Database file | `--db` | `RECALL_MCP_DB` | `recall.db` |
| Namespace for one-shot commands | `--namespace` | `RECALL_MCP_NAMESPACE` | required for `remember`, `recall`, `capture` |
| Bind address | `--bind` | `RECALL_MCP_BIND` | `127.0.0.1:8787` |
| API key | `--api-key` | `RECALL_MCP_API_KEY` | off |
| Rate limit per key | `--rate-limit` | `RECALL_MCP_RATE_LIMIT` | off |
| Request body limit | `--body-limit` | `RECALL_MCP_BODY_LIMIT` | 1 MiB |
| Embedder | `--embedder` | `RECALL_MCP_EMBEDDER` | `hash` |
| Encryption key | none | `RECALL_MCP_KEY` or `RECALL_MCP_KEY_FILE` | off |

For semantic matching offline, build with
`cargo build --release -p recall-mcp-cli --features onnx` and run with
`--embedder onnx`. It uses quantized all-MiniLM-L6-v2 at 384 dimensions,
downloads the model once into `.fastembed_cache/`, and loads ONNX Runtime
through `ORT_DYLIB_PATH`.

`--embedder openai` needs the `openai` feature, reads
`RECALL_MCP_OPENAI_API_KEY`, and defaults to `text-embedding-3-small` at 1536
dimensions. The full flag list is in `recall serve --help`.

## Security and privacy

The default posture is that your local user session can talk to the server.
Requests are checked against a loopback Host allowlist, foreign Origins are
rejected, and no CORS headers are granted unless you opt in. With an API key,
`/v1/*`, `/metrics`, and `/mcp` require `Authorization: Bearer` or `X-API-Key`.
The rate limiter runs before auth, and `/healthz` and `/openapi.json` stay open.

With `RECALL_MCP_KEY` (64 hex characters) or `RECALL_MCP_KEY_FILE`, memory text
is sealed with ChaCha20-Poly1305 and the keyword index stores keyed HMAC digests.
Namespace names, tags, `source`, embedding vectors, and exports stay plaintext.
Dense neural embeddings can be inverted to text with published attacks, so keyed
mode does not protect text when `onnx` or `openai` is in use; the lexical `hash`
embedder is the at-rest confidentiality choice. Namespaces organize work and are
not a security boundary. Recalled text is data from past sessions, not
instructions.

## Limitations

- The project is pre-1.0. The crates.io packages `recall-mcp-core`,
  `recall-mcp-server`, and `recall-mcp-cli` are not published yet.
- No Docker image is published. The `Dockerfile` is in the repository, but there
  is no pullable image.
- The OpenAI embedder has never been called against the live API.
- Client snippets are starting points and have not been verified against every
  client version.
- Dense embeddings are invertible, so keyed mode does not protect text when
  `onnx` or `openai` is in use.
- Namespaces are not a security boundary, and ANN retrieval beyond roughly 100k
  memories per namespace is deferred (D-002).

## Documentation

| Document | Contents |
|---|---|
| [Architecture](docs/ARCHITECTURE.md) | Layering, data model, recall path, encryption, embedders, surfaces, and testing |
| [Evaluation](docs/EVALUATION.md) | Measured test, coverage, retrieval, latency, token, web, and dependency results |
| [Decisions](docs/adr/DECISIONS.md) | Decision records D-001 to D-021 and dated amendments |
| [Clients](docs/clients.md) | MCP client configuration and troubleshooting |
| [Token efficiency](docs/TOKEN-EFFICIENCY.md) | Compact response formats and measured byte sizes |
| [Contributing](CONTRIBUTING.md) | Build, local gates, and conventions |
| [Security](SECURITY.md) | Supported versions and private vulnerability reports |
| [Changelog](CHANGELOG.md) | Release history |

## Contributing

Bug reports, fixes, tests, docs, and dependency updates are welcome. Contributing
covers the build, local gates, and conventions; Security explains the private
advisory flow.

## License

MIT. See [LICENSE](LICENSE).
