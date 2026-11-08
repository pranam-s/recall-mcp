# Client setup

Recall-MCP speaks MCP over stdio and streamable-HTTP. Every example below uses
the same six tools and the same memory file; only the client's configuration
shape differs.

**Verification status: expected, not live-verified.** The HTTP and stdio
transports are covered by conformance tests, but no external client was
installed and driven in this environment. Treat the snippets as starting
points and check the client's current documentation if a field has moved.

Start a server first:

```sh
recall serve --bind 127.0.0.1:8787 --db recall.db
# or, for local child-process clients:
recall serve --stdio --db recall.db
```

If `RECALL_MCP_API_KEY` is set, HTTP clients must send it as
`X-API-Key: <key>` or `Authorization: Bearer <key>`.

## Claude Code

Expected HTTP form (project `.mcp.json`):

```json
{
  "mcpServers": {
    "recall": {
      "type": "http",
      "url": "http://127.0.0.1:8787/mcp",
      "headers": { "X-API-Key": "replace-me" }
    }
  }
}
```

Expected stdio form:

```json
{
  "mcpServers": {
    "recall": {
      "command": "recall",
      "args": ["serve", "--stdio", "--db", "recall.db"]
    }
  }
}
```

## Codex

Expected `~/.codex/config.toml` entry:

```toml
[mcp_servers.recall]
command = "recall"
args = ["serve", "--stdio", "--db", "recall.db"]
```

Use the `recall` binary installed on `PATH`, or an absolute path to the built
binary.

## Cursor

Expected `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "recall": {
      "url": "http://127.0.0.1:8787/mcp",
      "headers": { "X-API-Key": "replace-me" }
    }
  }
}
```

Cursor's stdio shape uses `command` and `args` as in the Claude Code example.

## OpenCode v2

Expected `opencode.json` entry (remote):

```json
{
  "mcp": {
    "recall": {
      "type": "remote",
      "url": "http://127.0.0.1:8787/mcp",
      "headers": { "X-API-Key": "replace-me" }
    }
  }
}
```

Expected local entry:

```json
{
  "mcp": {
    "recall": {
      "type": "local",
      "command": ["recall", "serve", "--stdio", "--db", "recall.db"]
    }
  }
}
```

## Warp

Warp's MCP settings accept a command for stdio servers. Expected values:

- Name: `recall`
- Command: `recall`
- Arguments: `serve --stdio --db recall.db`

For the HTTP transport, use `http://127.0.0.1:8787/mcp` and add the API key as
a header if the UI exposes one.

## Antigravity

Expected MCP configuration (stdio):

```json
{
  "mcpServers": {
    "recall": {
      "command": "recall",
      "args": ["serve", "--stdio", "--db", "recall.db"]
    }
  }
}
```

For HTTP, point the server URL at `http://127.0.0.1:8787/mcp` and add the
`X-API-Key` header when auth is on.

## Troubleshooting

- `401` or `missing API key`: set the same key the server was started with.
- `421` or a rejected Host: the default guard accepts loopback hosts only. Bind
  loopback locally or add the external host with `--allow-host`.
- Empty recall results: the default embedder is lexical. Re-run with
  `--embedder onnx` and the `onnx` feature if you need paraphrase matching; use
  the same embedder that wrote the store.
- `embedder mismatch`: keep the original embedder, or pass
  `--allow-embedder-mismatch` knowingly. Mixing vector spaces lowers quality.
