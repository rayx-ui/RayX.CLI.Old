//! `rayx` is library-agnostic: its own code and its own dependency graph name no gpux or RayX
//! library package. Libraries and apps declare what they need through `[package.metadata.rayx]`;
//! the CLI never names them. This group fails, listing `file:line: token`, when non-test source
//! under `src/` or the CLI's `Cargo.toml`/`Cargo.lock` names one, and when an allowlist entry
//! matches nothing any more.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Library package names, compared case-insensitively with `-` and `_` equal (as of 2026-10-18:
/// Gpux, RayX.Dtcg, RayX.Reactive, RayX.SignalR, RayX, and the crates the CLI used to
/// special-case). A new library crate outside these families is added when it appears.
const DENIED_NAMES: &[&str] = &[
    "gpux",
    "gpui",
    "branches",
    "rclite",
    "rxrust",
    "indxdb",
    "rexie",
    "surrealdb",
    "zune",
    "rayx_runtime",
    "rayx_theme",
    "rayx_macros",
    "rayx_blocks",
    "rayx_collab",
    "rayx_lab",
    "rayx_test_suite",
    "rayx_code_theme_converter",
    "rayx_mobile_smoke",
];

/// Library package families.
const DENIED_PREFIXES: &[&str] = &[
    "gpux_",
    "gpui_",
    "rayx_devkit",
    "rayx_storage",
    "rayx_components",
    "rayx_reactive",
    "rayx_signalr",
    "dtcg_runtime",
    "zune_",
];

/// Web page glue that belongs to apps (source scan only): HTML-island, thread and WebGPU
/// diagnostics live in each app's own `platform/web/index.html`.
const DENIED_PAGE_SUBSTRINGS: &[&str] = &["htmlisland", "html_island"];
const DENIED_PAGE_PREFIXES: &[&str] = &["__rayxthreaddiagnostics"];
const DENIED_PAGE_NAMES: &[&str] = &["webgpu_status", "wasm_thread_status"];

/// CLI vocabulary and non-package uses that look like a denied name: `(file, token, reason)`.
/// Every entry must still match something.
const ALLOWLIST: &[(&str, &str, &str)] = &[
    (
        "src/cli.rs",
        "gpux",
        "help text naming the gpux and RayX work `rayx setup` prepares a machine for",
    ),
    (
        "src/setup/mod.rs",
        "gpux",
        "the base set's documentation: building gpux and RayX natively",
    ),
    (
        "src/setup/linux.rs",
        "gpux",
        "the apt packages for building gpux and RayX natively",
    ),
    (
        "src/app/android.rs",
        "rayx-devkit-android",
        "the `--android-devkit` bridge's default registry directory (CLI vocabulary)",
    ),
];

fn normalized(token: &str) -> String {
    token.to_ascii_lowercase().replace('-', "_")
}

/// Whether `token` is a library package name.
fn is_package_name(token: &str) -> bool {
    let token = normalized(token);
    DENIED_NAMES.contains(&token.as_str())
        || DENIED_PREFIXES
            .iter()
            .any(|prefix| token.starts_with(prefix))
}

/// Whether `token` is a denied name in source: a package name or app page glue.
fn is_denied_in_source(token: &str) -> bool {
    let normal = normalized(token);
    is_package_name(token)
        || DENIED_PAGE_SUBSTRINGS
            .iter()
            .any(|part| normal.contains(part))
        || DENIED_PAGE_PREFIXES
            .iter()
            .any(|prefix| normal.starts_with(prefix))
        || DENIED_PAGE_NAMES.contains(&normal.as_str())
}

/// `text` without its `#[cfg(test)] mod <name> { ... }` blocks (string literals, raw strings,
/// char literals and comments do not count as braces).
fn strip_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find_test_module(rest) {
        out.push_str(&rest[..start]);
        let open = start + rest[start..].find('{').expect("the module's brace");
        let end = matching_brace(rest, open);
        // Keep the line count so reported lines stay true.
        out.extend(rest[start..end].chars().filter(|ch| *ch == '\n'));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The offset of the next `#[cfg(test)]` that is followed by `mod <name> {`.
fn find_test_module(text: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(found) = text[offset..].find("#[cfg(test)]") {
        let start = offset + found;
        let after = text[start + "#[cfg(test)]".len()..].trim_start();
        let after = after.strip_prefix("pub(crate) ").unwrap_or(after);
        let after = after.strip_prefix("pub ").unwrap_or(after);
        if let Some(module) = after.strip_prefix("mod ") {
            let name_end = module
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .unwrap_or(module.len());
            if module[name_end..].trim_start().starts_with('{') {
                return Some(start);
            }
        }
        offset = start + 1;
    }
    None
}

/// The offset just past the brace that closes the one at `open`.
fn matching_brace(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'#' | b'"'))
                && !bytes
                    .get(i.wrapping_sub(1))
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') =>
            {
                let hashes = bytes[i + 1..].iter().take_while(|b| **b == b'#').count();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let closing = format!("\"{}", "#".repeat(hashes));
                    let body = i + 2 + hashes;
                    i = body
                        + text[body..].find(&closing).expect("closed raw string")
                        + closing.len()
                        - 1;
                }
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // A char literal (`'{'`, `'\''`); a lifetime has no closing quote nearby.
                if bytes.get(i + 1) == Some(&b'\\') {
                    if let Some(close) = text[i + 2..].find('\'') {
                        i += 2 + close;
                    }
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    i += 2;
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("unbalanced braces after offset {open}");
}

/// The denied `(line, token)` pairs of a source text, test modules left out.
fn scan(text: &str) -> Vec<(usize, String)> {
    let text = strip_test_modules(text);
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for token in line
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
            .filter(|token| !token.is_empty())
        {
            if is_denied_in_source(token) {
                found.push((index + 1, token.to_string()));
            }
        }
    }
    found
}

/// Files compiled only for tests: declared under `#[cfg(test)] mod <name>;`.
fn test_only_files() -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for file in source_files() {
        let text = fs::read_to_string(&file).expect("read");
        let dir = file.parent().expect("parent");
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            if let Some(next) = lines.peek()
                && let Some(name) = next
                    .trim()
                    .strip_prefix("mod ")
                    .and_then(|rest| rest.strip_suffix(';'))
            {
                files.insert(dir.join(format!("{name}.rs")));
                files.insert(dir.join(name).join("mod.rs"));
            }
        }
    }
    files
}

fn source_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("src"), &mut files);
    files.sort();
    files
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .expect("under the repository")
        .to_string_lossy()
        .replace('\\', "/")
}

fn allowed(file: &str, token: &str) -> bool {
    ALLOWLIST
        .iter()
        .any(|(path, allowed, _)| *path == file && *allowed == token)
}

/// Every dependency key and `package` name of a `Cargo.toml`, in every dependency table.
fn manifest_dependency_names(manifest: &toml::Table) -> Vec<String> {
    let mut tables = Vec::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(kind).and_then(toml::Value::as_table) {
            tables.push(table);
        }
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            tables.extend(
                targets
                    .values()
                    .filter_map(|target| target.get(kind).and_then(toml::Value::as_table)),
            );
        }
    }
    if let Some(workspace) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
    {
        tables.push(workspace);
    }
    let mut names = Vec::new();
    for table in tables {
        for (key, value) in table {
            names.push(key.clone());
            if let Some(package) = value.get("package").and_then(toml::Value::as_str) {
                names.push(package.to_string());
            }
        }
    }
    names
}

/// The package names a `Cargo.lock` locks.
fn lock_package_names(lock: &toml::Table) -> Vec<String> {
    lock.get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| package.get("name").and_then(toml::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn denied_packages(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| is_package_name(name))
        .cloned()
        .collect()
}

#[test]
fn the_scanner_reports_planted_names_outside_test_modules_only() {
    let sample = r#"
fn fonts() -> &'static str {
    "gpux-fonts"
}
fn mobile() { let gpui_mobile = 1; }
const PAGE: &str = "globalThis.__rayxHtmlIslandEnabled = true; <div id='webgpu-status'>";
// The rayx binary and its metadata namespace are CLI vocabulary.
const VOCABULARY: &str = "[package.metadata.rayx] rayx.assets.toml";

#[cfg(test)]
mod tests {
    #[test]
    fn inside() {
        let _ = "gpux-fonts gpui_mobile __rayxHtmlIslandEnabled webgpu-status { '{' }";
    }
}
fn after() { let _ = "rayx_devkit"; }
"#;
    let found = scan(sample);
    let tokens = found
        .iter()
        .map(|(_, token)| token.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        [
            "gpux-fonts",
            "gpui_mobile",
            "__rayxHtmlIslandEnabled",
            "webgpu-status",
            "rayx_devkit"
        ],
        "{found:?}"
    );
    assert_eq!(found[0].0, 3, "line numbers are kept: {found:?}");
    assert_eq!(
        found[4].0, 17,
        "lines after a test module stay true: {found:?}"
    );
    assert!(allowed("src/cli.rs", "gpux"));
    assert!(!allowed("src/cli.rs", "gpux-fonts"));
    assert!(!allowed("src/app/wasm.rs", "gpux"));
}

#[test]
fn non_test_source_names_no_library_and_every_allowlist_entry_still_matches() {
    let skipped = test_only_files();
    assert!(
        skipped.contains(&root().join("src/app/test_support.rs")),
        "{skipped:?}"
    );
    let mut violations = Vec::new();
    let mut used = BTreeSet::new();
    for file in source_files() {
        if skipped.contains(&file) {
            continue;
        }
        let path = relative(&file);
        let text = fs::read_to_string(&file).expect("read source");
        for (line, token) in scan(&text) {
            if allowed(&path, &token) {
                used.insert((path.clone(), token));
            } else {
                violations.push(format!("{path}:{line}: {token}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "rayx names library packages:\n{}",
        violations.join("\n")
    );
    let stale = ALLOWLIST
        .iter()
        .filter(|(path, token, _)| !used.contains(&(path.to_string(), token.to_string())))
        .map(|(path, token, reason)| format!("{path}: {token} ({reason})"))
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "allowlist entries that match nothing:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_rule_flags_library_dependencies_in_sample_manifests_and_lockfiles() {
    let manifest: toml::Table = r#"
[dependencies]
anyhow = "1"
fonts = { package = "gpux-fonts", git = "https://github.com/rayx-ui/Gpux", rev = "abc" }

[dev-dependencies]
rayx_devkit = { git = "https://github.com/rayx-ui/RayX", rev = "abc" }

[target."cfg(windows)".dependencies]
gpui = "0.2"
"#
    .parse()
    .expect("sample manifest");
    let mut denied = denied_packages(&manifest_dependency_names(&manifest));
    denied.sort();
    assert_eq!(denied, ["gpui", "gpux-fonts", "rayx_devkit"]);
    let lock: toml::Table = r#"
version = 4
[[package]]
name = "serde"
version = "1.0.0"
[[package]]
name = "dtcg_runtime_core"
version = "0.1.0"
[[package]]
name = "rayx-cli"
version = "1.4.0"
"#
    .parse()
    .expect("sample lock");
    assert_eq!(
        denied_packages(&lock_package_names(&lock)),
        ["dtcg_runtime_core"]
    );
    // Page glue is a source rule only; the CLI's own package name is not a library.
    assert!(!is_package_name("rayx-cli") && !is_package_name("rayx"));
}

#[test]
fn the_clis_own_manifest_and_lockfile_name_no_library() {
    let manifest: toml::Table = fs::read_to_string(root().join("Cargo.toml"))
        .expect("Cargo.toml")
        .parse()
        .expect("TOML");
    let names = manifest_dependency_names(&manifest);
    assert!(names.contains(&"toml".to_string()), "{names:?}");
    assert_eq!(denied_packages(&names), Vec::<String>::new());

    let lock: toml::Table = fs::read_to_string(root().join("Cargo.lock"))
        .expect("Cargo.lock")
        .parse()
        .expect("TOML");
    let names = lock_package_names(&lock);
    assert!(names.contains(&"rayx-cli".to_string()));
    assert_eq!(denied_packages(&names), Vec::<String>::new());
}
