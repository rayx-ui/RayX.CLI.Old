# Using `rayx app` in a project

`rayx app` builds, runs, packages, publishes, deploys and tests an app. It works in any Cargo
workspace: nothing is read from another repository or from where `rayx` was built, and `rayx` names
no library. Libraries and apps declare what they need through the keys and files below. This page
is the contract between a project and the tool.

```text
rayx app [<app-dir>] <action> <target> [options]
```

`<app-dir>` is a path absolute or relative to the current directory and defaults to `.`.

| Action | Targets | What it does |
| --- | --- | --- |
| `build` | `host`, `windows`, `linux`, `macos`, `mobile`, `ios`, `android`, `wasm` (alias `web`) | Builds the app for the target. |
| `run` | the same | Builds and runs the app (WASM: serves it). |
| `pack` | the same | Builds and copies the app and its content to `artifacts/`. |
| `publish` | the same | Stages like `pack`; store upload and signing are not automated. |
| `deploy` | `android` | Installs the APK on a device or emulator. |
| `test` | `host`, `windows`, `linux`, `macos`, `wasm` | Runs the app's tests (below). |
| `assets` | `generate`, `validate`, `list` | Manages the asset catalog. |

Builds are release builds; `--debug` (or `--development`) selects a debug build. `--features`,
`--all-features` and `--no-default-features` select the app's Cargo features, and everything after
`--` goes to the app. `--no-install` makes the prerequisite step report what is missing and stop instead of installing it. On a host `rayx setup` cannot install for, such as a Linux distribution other than Ubuntu or Debian, the step says so and the build goes on.

## What the project provides

- **A Cargo workspace.** The workspace root and its members come from `cargo metadata` of the
  selected app. `artifacts/` (packaged output), `artifacts-temp/` (generated files) and `target/`
  are created under that root for every target.
- **The crates its `main.rs` uses, in `entry_dependencies`.** The generated Android, iOS and
  WebAssembly entry crates compile the app's `src/main.rs` as a module, so they depend on the app's
  own library plus the dependencies `[package.metadata.rayx] entry_dependencies` lists (below).
- **A `Cargo.lock`.** Every generated entry crate is a workspace of its own and starts from a copy
  of the workspace's `Cargo.lock`, so it builds with the versions the project builds with. Without
  a workspace lock the entry crates resolve on their own.
- **The versions it wants.** `Cargo.lock` (`wasm-bindgen`), `rust-toolchain.toml` (the Rust
  channel) and `[workspace.metadata.rayx]` pin the tools; anything not pinned has a built-in
  default. See `rayx doctor` for the resolved values.

### `[workspace.metadata.rayx]`

| Key | Meaning |
| --- | --- |
| `tools-node` | The directory, relative to the workspace root, of the shared Playwright package. |
| `web-toolchain`, `node`, `jdk`, `playwright` | Tool versions. |
| `android-platform`, `android-ndk`, `android-ndk-api`, `android-build-tools`, `android-system-image` | Android SDK pins. |
| `xcode` | An exact Xcode version. |

`[patch]` applies only from the root of the workspace being built, so each generated entry crate
repeats the root manifest's `[patch.<registry or URL>]` entries whose patched package (the entry's
`package`, else its key) is in the app's resolved dependency graph for the selected features. A
relative `path` is made absolute under the workspace root; every other field is kept. No package
name decides it.

### `[package.metadata.rayx]`

| Key | Meaning |
| --- | --- |
| `app_name`, `app_slug` | The slug used in `artifacts/apps/<slug>`; defaults to the package name. |
| `native_library`, `android_native_library` | The native library name when it differs from the crate's. |
| `app_content_root` | The directory holding bootstrap content such as `app_settings.json`. |
| `asset_roots`, `assets_roots`, `assets_root`, `wasm_manifest_path` | Asset catalog locations. |
| `android_application_id`, `android_activity` | Android launch component. |
| `ios_project`, `ios_scheme`, `ios_bundle_id`, `ios_app_name` | iOS project names. |
| `diagnostic_harness` | Marks an app that may use the `rayx_diagnostics` feature (never for `pack`, `publish` or `deploy`). |
| `playwright_specs` | The spec files or directories `test wasm` runs. |
| `playwright_projects` | The Playwright projects `test wasm` runs. |
| `entry_dependencies` | The app dependencies the generated entry crates also depend on (below). |
| `android_java_dirs` | Android Java source directories of this package (below); any package in the app's graph may declare it. |
| `assets.manifest` | `[package.metadata.rayx.assets] manifest = "rayx.assets.toml"`: the package ships assets. |

#### `entry_dependencies`

```toml
[package.metadata.rayx]
entry_dependencies = ["ui_runtime", "inspector"]

[features]
inspect = ["dep:inspector"]

[dependencies]
ui_runtime = { path = "../../crates/ui_runtime" }
inspector = { version = "0.3", optional = true }
```

- It lists keys of the app's own normal dependencies (`[dependencies]` or
  `[target.'<cfg>'.dependencies]`; a renamed dependency by its key). The default is none: the entry
  crates then depend only on the app's library (and the web crate on `wasm-bindgen` and
  `wasm-bindgen-futures`).
- Each listed dependency is declared in the entry crate as the app declares it: the same source
  (absolute path, git URL with its `rev`/`branch`/`tag`, or registry version), `package` for a
  rename, `default-features = false`, `features`, `optional`, and the same target table.
- Every app feature `f` except `default` is forwarded as `f = ["<app lib>/f", ...]`, plus
  `dep:<key>` for each optional entry dependency `f` enables, directly or through other features:
  a member `dep:<key>`, `<key>/<feature>` (not `<key>?/<feature>`), or the implicit feature
  `<key>`. No feature or crate name is special.
- The entry crate's `default` lists the selected features by their entry-crate names (the app's
  `default` members, every feature with `--all-features`, none of them with
  `--no-default-features`, plus `--features`), so a selected feature also turns on the optional
  entry dependencies it enables and the `#[cfg(feature = "...")]` code in `main.rs`. A default
  member the app's `[features]` table does not declare (an implicit optional-dependency feature)
  is listed as `<app lib>/<name>`.
- A listed key that is not a normal dependency of the app, or a value that is not an array of
  strings, fails with an error naming the key or `entry_dependencies`.

#### `android_java_dirs`

```toml
[package.metadata.rayx]
android_java_dirs = ["java"]
```

Any package in the app's resolved graph for the selected features, the app included, may declare
directories of Android Java sources, relative to its own manifest directory. `rayx app ...
build|run|deploy|pack android` collects them as absolute, deduplicated, sorted directories and
passes them to Gradle as `-PrayxAndroidJavaDirs=<dirs joined by the platform path separator>`;
without declarations no such property is passed. An entry that is not a string, is absolute, leaves
the package's directory or does not exist fails before Gradle runs, naming the package and the
entry. A package outside the graph (an unrelated workspace member, a dependency behind an
unselected feature) contributes nothing.

## Files the app owns

`rayx` generates the Rust entry crates; the platform projects around them belong to the app, so a
future `rayx new` can generate them.

- **`rayx.assets.toml`** next to the app's `Cargo.toml` (and in every package that ships assets):
  `version = 1`, `[assets] roots = [...]` (a path, or `{ path, mount }`), optional
  `[package_assets] mode = "explicit"` with `include = [<asset package ids>]` to pull in the assets
  of dependencies, and `[app_content] files = [...]` for bootstrap files such as
  `app_settings.json` that sit beside the catalog rather than inside it. A package that ships assets
  declares `[asset_package] id, kind, namespace` and names the file in
  `[package.metadata.rayx.assets] manifest = "rayx.assets.toml"`. Fonts and every other library
  asset arrive this way only, on every target; for example a fonts package:

  ```toml
  # rayx.assets.toml of the fonts package
  version = 1

  [asset_package]
  id = "demo-fonts"
  kind = "fonts"
  namespace = ""

  [assets]
  roots = ["assets"]
  ```

  In `mode = "auto"` (the default) every asset package of the graph is included; in
  `mode = "explicit"` the app adds `"demo-fonts"` to `include`.
- **`platform/android/gradle/`**: a Gradle project with `gradlew` and an `app/build.gradle.kts` that
  reads the native libraries from `build/generated/rustJniLibs` and the `-PrayxAndroid*` properties
  `rayx` passes (`rayxAndroidMinify`, `rayxAndroidShrinkResources`, `rayxAndroidCrunchPngs`,
  `rayxAndroidCompressNativeLibs` and `rayxAndroidJavaDirs`). It adds the Java directories with
  `(findProperty("rayxAndroidJavaDirs") as String?)?.split(File.pathSeparator)` as Java source
  directories.
- **`platform/ios/project.yml`** (and the generated `.xcodeproj` it names): an XcodeGen project that
  builds the static library from `RAYX_IOS_RUST_MANIFEST` and copies the generated `assets` folder
  into the bundle.
- **`platform/web/index.html`**: the web host page. `rayx app ... build|run|pack|publish|test wasm`
  copies it to the output as `index.html`, replacing these placeholders (each any number of times)
  and nothing else:

  | Placeholder | Replaced with |
  | --- | --- |
  | `__RAYX_MODULE__` | The wasm-bindgen JavaScript module URL, `/<module base>.js`. |
  | `__RAYX_TITLE__` | The app's title (its slug). |
  | `__RAYX_DIAGNOSTICS__` | `true` or `false`: whether this build enables the app's diagnostics feature (`rayx_diagnostics`, or `--all-features`). |

  Any other `__RAYX_<NAME>__` token is an error naming the token, the page and its line; the page
  is checked before the build starts, so nothing is built or written. What the page does in
  diagnostics mode is the app's business. Without the file, `rayx` writes this neutral page, the
  same for every build; copy it as the starting template:

  ```html
  <!doctype html>
  <html lang="en">
  <head>
    <base href="/" />
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <link rel="icon" href="data:," />
    <title>__RAYX_TITLE__ WASM</title>
    <style>
      *, *::before, *::after { box-sizing: border-box; }
      html, body { width: 100vw; height: 100vh; margin: 0; padding: 0; overflow: hidden; background: #f8fafc; color: #111827; }
      body { font-family: system-ui, sans-serif; }
    </style>
  </head>
  <body>
    <script type="module">
      import init, { run } from '__RAYX_MODULE__';
      await init();
      await run();
      document.body.dataset.rayxWasm = 'started';
    </script>
  </body>
  </html>
  ```

## Android DevKit bridge

`deploy|run android --android-devkit` builds the entry crate with `--features devkit` and
`RAYX_ANDROID_DEVKIT_PORT` (default port 47123), forwards that port, waits for the `devkit.metadata`
JSON-RPC probe and writes a registration file (by default in `<temp>/rayx-devkit-android`, or
`--android-devkit-registry-dir`), then prints the file and its registry directory. The app must
define a Cargo feature named `devkit`; without it `build`, `run`, `deploy` and `pack android
--android-devkit` fail before any toolchain step.

## The CLI vocabulary

`rayx` names no library. What it does know is its own vocabulary:

- the `[package.metadata.rayx]` and `[workspace.metadata.rayx]` tables (including
  `entry_dependencies` and `android_java_dirs`) and the `rayx.assets.toml` schema;
- the app features `rayx_diagnostics` (with `diagnostic_harness`) and `devkit` (the bridge above,
  with `RAYX_ANDROID_DEVKIT_PORT`);
- the default Android activity `dev.rayx.mobile.RayXActivity` and the `-PrayxAndroid*` Gradle
  properties;
- the `RAYX_*` environment variables (`RAYX_APP_URL`, `RAYX_WEBGPU_REQUIRED`,
  `RAYX_IOS_RUST_MANIFEST`, ...);
- the web page placeholders `__RAYX_MODULE__`, `__RAYX_TITLE__`, `__RAYX_DIAGNOSTICS__` and the
  fallback page's `data-rayx-wasm` marker (`document.body.dataset.rayxWasm`);
- the threaded WebAssembly build's requirement that the module export `wasm_thread_entry_point`;
- the binary and Lima VM name `rayx`.

`tests/library_agnostic.rs` in this repository keeps the code that way.

## Migrating to rayx 1.4

rayx 1.4 stops special-casing libraries. What used to happen implicitly is now declared.

An **app** adds:

- `entry_dependencies` listing the crates its `main.rs` uses directly (the entry crates no longer
  add any crate on their own);
- the id of each fonts (or other asset) package to `include` in its `[package_assets]` table, if it
  uses `mode = "explicit"`;
- `rayxAndroidJavaDirs` in its `build.gradle.kts` (split by `File.pathSeparator`) instead of a
  fixed path to a library's Java sources;
- a `devkit` feature, if it uses `--android-devkit`;
- `platform/web/index.html` with the three placeholders, if it relied on the generated page's
  diagnostics (start from the fallback page above and add what its specs read).

A **library** adds:

- a package-asset declaration (`[package.metadata.rayx.assets]` and its `rayx.assets.toml`) for
  the fonts or other assets it ships;
- `android_java_dirs` for its Android Java sources;
- any feature its dependencies need on a target, in the crate that needs it (for example a
  `branches/std` feature enabled on wasm), instead of relying on the generated web entry crate.

A **workspace** that relied on the Android pins `rayx` used to infer from one of its member
names declares them in `[workspace.metadata.rayx]` (`android-platform`, `android-build-tools`,
`android-ndk`).

Gone: the implicit dependencies and the always-present `devkit` feature of the entry crates, the
extra font asset root, the web entry crate's version pins, the name-based `[patch]` gate, the
inferred Android pin profile, the generated page's diagnostics, and the library name in the DevKit
bridge's message.

## Artifacts

| Command | Output |
| --- | --- |
| `build wasm` | `artifacts/apps/<slug>/wasm/<profile>/`: `index.html`, the wasm-bindgen output and `assets/`. |
| `pack <desktop target>` | `artifacts/apps/<slug>/<target>/<profile>/`: the executable, `assets/` and the app content. |
| `build android` | `platform/android/gradle/app/build/outputs/apk/<profile>/`: the APK, whose path `build` prints. |
| `pack android` | `artifacts/apps/<slug>/android/<profile>/`: a copy of the APK. |
| generated files | `artifacts-temp/apps/<slug>/`. |

`run wasm` serves the build with `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`, which the threaded build needs.

## Prerequisites

Before it builds, an app command checks what the target needs: the base set for every target, plus
the web set for `wasm` (and the test set for `test wasm`), the Android set for `android`, and the
iOS set for `ios`. What can be installed without privileges (rustup toolchains, components and
targets, `wasm-bindgen-cli`, `cargo-ndk`, the Playwright package and its browser, SDK packages) is
installed on the spot with a one-line notice. What needs `sudo`, administrator rights or a license
answer is offered in an interactive terminal; otherwise the command stops and prints the exact
`rayx setup --<set>` command. `--no-install` reports what is missing and stops.

## Tests

`rayx app <dir> test <target>` works for any app, with no configuration.

- **`test host|windows|linux|macos`** runs `cargo test -p <app package>` from the workspace root
  with the selected features and profile. Arguments after `--` go to the test binary.
- **`test wasm`** builds the app, serves it with cross-origin isolation and runs Playwright against
  it. The address reaches the specs in `RAYX_APP_URL`; `--webgpu` sets `RAYX_WEBGPU_REQUIRED=1`,
  which makes the template's fixture fail unless the app initialized graphics with the browser's
  WebGPU; `--headed` shows the browser.
- **`test android` and `test ios`** are not supported yet.

### Where the Playwright package lives

The first that exists wins:

1. `[workspace.metadata.rayx] tools-node`;
2. `tools-node/` under the workspace root, shared by the workspace's apps;
3. the app's own `tests/e2e/`.

When none exists, `rayx` creates `<app>/tests/e2e/` from its embedded template and tells you to
commit it. The template is a complete package: `package.json` pinning `@playwright/test` to the
Playwright pin with pnpm as the package manager, `playwright.config.ts` with Chromium projects at
device scale factor 1 and 2 and the verified per-OS WebGPU launch flags, a fixture that waits for
the app's canvas, an example smoke spec and a `.gitignore`. Files that exist are never overwritten.

### Which specs and projects run

- Specs: the files named with `--playwright-test <spec>...` (a `spec.ts:<line>` selector runs the
  test containing that line); else `playwright_specs`; else, in a shared package, the directory
  named like the app (`apps/test_suite` runs `test_suite/`); else, in an app-local package, every
  spec.
- Projects: `playwright_projects`, else every project of the config.

## Formatting

`rayx fmt [--check]` formats the selected workspace one member at a time (`cargo fmt -p <member>`)
so the command line stays under the Windows limit, and lists the members that fail.
