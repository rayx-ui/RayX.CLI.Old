---
agentx:
  kind: decision
  scope: decisions/project
  status: current
  created: 2026-10-19T02:00:00+02:00
  updated: 2026-10-19T02:00:00+02:00
  promoted_from: rayx-cli-library-agnostic
  topics:
    - project
    - pins
  supersedes:
    - 2026-10-08-project-discovery-and-pin-precedence.md
  superseded_by: []
---

## Decision: Pins come from the project or the built-in default

### Context: Epic `rayx-cli-library-agnostic` (design decision `d-pin-sources`), 2026-10-18.

### Decision text

`project::Project::discover` is unchanged (Cargo through `host::Runner`, files-only fallback). `Pins::resolve` has one rule for every key: the project's value (`Cargo.lock`, `rust-toolchain.toml`, `[workspace.metadata.rayx]`) with source `project`, else `DEFAULTS` with source `default`. The gpux-checkout profile (selected by a `gpux-testkit` member) is removed; a workspace that wants Android 36 pins declares them.

### Reasoning: A profile chosen by a member's name is a library the CLI knows. Gpux declares its own `[workspace.metadata.rayx]` pins, like any project.

### Invariants

- One `Pins` value flows to setup, doctor and the app pipeline; built-in defaults live in one table in `project::pins`.
- `PinSource` is `Project` or `Default`.

### Supersedes: [Find the project through Cargo and let the project own its version pins](2026-10-08-project-discovery-and-pin-precedence.md).
