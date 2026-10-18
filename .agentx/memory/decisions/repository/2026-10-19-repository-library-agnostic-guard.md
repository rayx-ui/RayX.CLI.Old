---
agentx:
  kind: decision
  scope: decisions/repository
  status: current
  created: 2026-10-19T02:00:00+02:00
  updated: 2026-10-19T02:00:00+02:00
  promoted_from: rayx-cli-library-agnostic
  topics:
    - dependencies
    - guard
    - library-agnostic
  supersedes:
    - 2026-10-14-repository-dependency-sources-public-git.md
  superseded_by: []
---

## Decision: One package, public dependencies, and no gpux or RayX library at all

### Context: Epic `rayx-cli-library-agnostic` (design decision `d-library-guard`), 2026-10-18.

### Decision text

Everything in [the 2026-10-14 decision](2026-10-14-repository-dependency-sources-public-git.md) holds (one package `rayx-cli`; crates.io, public `https://` git pinned to a `rev`, upstream-linked `[patch]`; no path, branch, tag, alternative registry, `[replace]` or private repository), with one narrowing: the CLI depends on no gpux or RayX library whatever its source, so a pinned public git dependency on Gpux is no longer allowed. `tests/library_agnostic.rs` denies the library package names (exact names and families, `-` = `_`, case-insensitive) in non-test `src/**/*.rs` (test modules and test-only files left out) and in the CLI's own `Cargo.toml`/`Cargo.lock`, plus the app web-page glue names in source; an allowlist of `(file, token, reason)` holds CLI vocabulary and prose about the work `rayx setup` prepares, and a stale entry fails.

### Reasoning: The owner rule of 2026-10-18 makes rayx library-agnostic; a guard keeps the decoupling from regressing.

### Supersedes: [Ship rayx as one Cargo package and allow dependencies anyone can fetch](2026-10-14-repository-dependency-sources-public-git.md).
