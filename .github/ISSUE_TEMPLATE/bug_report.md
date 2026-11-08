---
name: Bug report
about: Something is broken or a gate fails
title: "bug: "
labels: bug
---

## What happened

A short description of the failure.

## Reproduction

Exact commands, with placeholder paths and keys:

```sh
recall serve --db /tmp/recall.db
```

## Expected

What you expected instead.

## Environment

- Recall-MCP version or commit:
- OS and toolchain (MSVC or GNU):
- Transport: MCP stdio / MCP HTTP / REST / CLI / web
- Encryption on? API key on?

## Verification

- [ ] I ran `cargo test --workspace` (or the relevant package)
- [ ] I checked `docs/STATUS.md` and `docs/EVALUATION.md`
- [ ] I removed real keys, usernames, and personal paths from this report
