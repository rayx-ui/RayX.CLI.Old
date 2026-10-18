---
agentx:
  kind: decision
  scope: decisions/app-pipeline
  status: current
  created: 2026-10-19T02:00:00+02:00
  updated: 2026-10-19T02:00:00+02:00
  promoted_from: rayx-cli-library-agnostic
  topics:
    - app
    - contract
    - library-agnostic
  supersedes:
    - 2026-10-08-app-pipeline-move-first-decouple-second.md
  superseded_by: []
---

## Decision: rayx app names no library; projects declare what they need

### Context: Epic `rayx-cli-library-agnostic` (design decisions `d-library-agnostic-contract`, `d-app-graph`, `d-entry-dependencies`, `d-entry-lockfile`, `d-entry-patches`, `d-fonts-package-assets`, `d-android-devkit`, `d-android-java-dirs`, `d-web-host-page`), owner rule 2026-10-18, released as rayx 1.4.0.

### Decision text

`rayx app` takes everything about an app from the selected workspace and from declarations in the CLI's own vocabulary; it names no gpux or RayX library.

- Generated Android, iOS and web entry crates depend on the app's library plus `[package.metadata.rayx] entry_dependencies`, declared as the app declares them; app features are forwarded generically (no invented `devkit`), and `--android-devkit` requires the app's own `devkit` feature.
- Every entry crate starts from the workspace `Cargo.lock`; the web crate pins nothing a library needs.
- One feature-aware graph walk (`app::graph`) decides package assets, the root `[patch]` entries repeated in entry crates, and `android_java_dirs` (Gradle `-PrayxAndroidJavaDirs`); a devkit build uses the selection plus `devkit` for all of them.
- Fonts and other library assets arrive only as package assets.
- The web host page is the app's `platform/web/index.html` (`__RAYX_MODULE__`, `__RAYX_TITLE__`, `__RAYX_DIAGNOSTICS__`), with a neutral fallback; diagnostics glue belongs to apps.

The contract lives in `docs/app-project.md`, including the CLI vocabulary list and the 1.4 migration.

### Reasoning: rayx is environment setup, the app pipeline and later templates; knowing libraries coupled its releases to theirs (the gpux-fonts root collided with package assets, the hard-coded `rayx`/`rayx_devkit` and `zune-core`/`branches` lines broke other projects). Declarations keep one rayx usable for any project and library version.

### Invariants

- No crate, feature or package name is special in `src/app`; `tests/library_agnostic.rs` enforces it.
- Artifact locations stay under the selected workspace root.

### Supersedes: [Move the xtask app pipeline first, then decouple it from RayX](2026-10-08-app-pipeline-move-first-decouple-second.md) (its second step is now complete and its RayX-specific couplings are gone).
