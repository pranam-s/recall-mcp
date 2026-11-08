# CLI reference

The short binary is `recall`; `recall-cli` is kept as an alias. Global flags:

- `--db <path>` (env `RECALL_MCP_DB`, default `recall.db`)
- `--embedder hash|onnx|openai` (env `RECALL_MCP_EMBEDDER`)
- `--allow-embedder-mismatch` (env `RECALL_MCP_ALLOW_EMBEDDER_MISMATCH`)

Commands that take `--namespace` also read `RECALL_MCP_NAMESPACE`.

## Commands

```sh
recall remember --namespace work --text "We deploy on Thursdays" --tags ops
recall recall --namespace work --query "when do we deploy?" --k 3
recall recall --namespace work --query "deploy" --format text
recall update --id <id> --text "We deploy on Wednesdays"
recall forget --id <id>
recall list --namespace work --limit 20 --offset 0
recall capture --namespace session-42 --file transcript.json --tags session:42
recall stats
recall keygen --out recall.key
recall export --out backup.json
recall import --file backup.json
recall backup --out snapshot.db
recall vacuum
recall serve --bind 127.0.0.1:8787 --db recall.db --web-dir web/dist
recall serve --stdio --db recall.db
```

`capture --file` expects a JSON array of `{"role","content","at"}` messages.

## Output

JSON is compact when stdout is a pipe and pretty on a terminal. `--pretty`
forces pretty output, `--full` restores the complete record shape, and
`--show-embedding` includes the vector. `recall --format text` prints one block
per hit with rank, total, id, text, and the full breakdown.

## Encryption

Set `RECALL_MCP_KEY` (64 hex characters) or `RECALL_MCP_KEY_FILE`. A plaintext
store is re-encrypted in place on the first keyed open, with a forensic
VACUUM + WAL checkpoint. Backups of keyed stores stay encrypted.

## Exit codes

Non-zero on validation failures, unknown ids, a wrong key, an embedder
fingerprint mismatch, and I/O errors. Scripts can rely on that; no partial
capture or import is left behind.
