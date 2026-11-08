# Client setup

Recall-MCP speaks MCP over stdio and streamable-HTTP. Every example below uses
the same six tools and the same memory file; only the client's configuration
shape differs.

The HTTP and stdio transports are covered by conformance tests. The six client
configurations below are starting points; they have not been verified against
every client, so check the client's current documentation if a field has moved.

Start a server first:

```sh
recall serve --bind 127.0.0.1:8787 --db recall.db
# or, for local child-process clients:
recall serve --stdio --db recall.db
```

If `RECALL_MCP_API_KEY` is set, HTTP clients must send it as
`X-API-Key: <key>` or `Authorization: Bearer <key>`.

## Claude Code

HTTP configuration in a project `.mcp.json`:

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

Stdio configuration:

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

Configuration in `~/.codex/config.toml`:

```toml
[mcp_servers.recall]
command = "recall"
args = ["serve", "--stdio", "--db", "recall.db"]
```

Use the `recall` binary installed on `PATH`, or an absolute path to the built
binary.

## Cursor

Configuration in `.cursor/mcp.json`:

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

Remote server entry in `opencode.json`:

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

Local server entry:

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

Warp's MCP settings accept a command for stdio servers. Use these values:

- Name: `recall`
- Command: `recall`
- Arguments: `serve --stdio --db recall.db`

For the HTTP transport, use `http://127.0.0.1:8787/mcp` and add the API key as
a header if the UI exposes one.

## Antigravity

Stdio configuration:

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

## Agent skill

The repository ships a portable skill in `skills/recall-mcp/`. It contains a
short `SKILL.md` plus CLI and MCP references. Copy the directory into the
client's skills location, or point the client at it if it supports external
skill paths. The skill assumes the `recall` binary is on `PATH` and that a
server or database is available.

## Troubleshooting

- `401` or `missing API key`: set the same key the server was started with.
- `421` or a rejected Host: the default guard accepts loopback hosts only. Bind
  loopback locally or add the external host with `--allow-host`.
- Empty recall results: the default hash embedder scores token overlap, so
  paraphrases can miss. Re-run with `--embedder onnx` and the `onnx` feature if
  you need paraphrase matching; use the same embedder that wrote the store.
- `embedder mismatch`: keep the original embedder, or pass
  `--allow-embedder-mismatch` knowingly. Mixing vector spaces lowers quality.
