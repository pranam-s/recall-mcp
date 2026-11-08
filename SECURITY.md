# Security policy

## Supported versions

Security fixes land on the current `0.5.x` line. Earlier pre-1.0 lines are not
maintained.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on the repository
(Security → Report a vulnerability). Do not open a public issue with an exploit
or with a real store, key, or API key attached.

Include:

- the version or commit,
- the transport (stdio, HTTP, CLI, web),
- whether encryption or an API key was enabled,
- a minimal reproduction with placeholder data.

You will get an acknowledgement and a status update through the advisory
thread. Please leave time for a fix and release before public disclosure.

## Scope

In scope: the Rust workspace, the web UI, MCP/REST transports, encryption,
authentication, rate limiting, request validation, and the published release
artifacts.

Out of scope: vulnerabilities that require the attacker to already control the
user's machine or database file, and the documented limitations of the design
(namespaces are not a security boundary; dense embeddings are invertible;
plaintext exports are plaintext).

## Hardening already in place

- Loopback-only Host allowlist and a same-origin default with no CORS grants.
- Constant-time API-key comparison, with the rate limiter in front of auth.
- Bounded request bodies, query/text/tag/id lengths, and capture sizes.
- AEAD encryption at rest with keyed FTS digests; a forensic VACUUM and WAL
  truncate on every plaintext-to-keyed upgrade.
- Security headers (CSP, nosniff, frame denial, referrer policy, COOP/CORP,
  permissions policy) and `Cache-Control: no-store` on API responses.
- Dependency audits in CI (`cargo audit`, `cargo deny`, `npm audit`) and a
  weekly scheduled scan.
