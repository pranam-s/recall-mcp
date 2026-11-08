# Security policy

## Supported versions

Security fixes land on the current `0.5.x` line. Earlier pre-1.0 lines are not
maintained.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on the repository: open the Security
tab and choose Report a vulnerability. Do not open a public issue with an exploit
or with a real store, key, or API key attached.

Include the version or commit, the transport (stdio, HTTP, CLI, web), whether
encryption or an API key was enabled, and a minimal reproduction with placeholder
data.

You will get an acknowledgement and a status update through the advisory thread.
Please leave time for a fix and release before public disclosure.

## Scope

In scope: the Rust workspace, the web UI, the MCP and REST transports, encryption,
authentication, rate limiting, request validation, and the release artifacts on
GitHub.

Out of scope: vulnerabilities that require the attacker to already control the
user's machine or database file, and the documented limitations of the design.
Namespaces are not a security boundary, dense embeddings are invertible, and
plaintext exports are plaintext.

## Hardening in place

HTTP requests are checked against a loopback Host allowlist. The default is
same-origin with no CORS grants. API keys are compared in constant time, and the
rate limiter runs before auth. Request bodies, queries, text, tags, ids, and
captures have bounded sizes.

Encryption at rest uses AEAD over memory text with keyed FTS token digests. Every
plaintext-to-keyed upgrade runs a forensic VACUUM and WAL truncate.

Responses carry CSP, nosniff, frame denial, a referrer policy, COOP/CORP, a
permissions policy, and `Cache-Control: no-store` on API responses.

The CI workflow runs `cargo audit`, `cargo deny`, and `npm audit`. A scheduled
security workflow repeats the Rust and npm advisory checks every week and runs
the live ONNX eval.
