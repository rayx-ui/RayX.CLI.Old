//! The documented app-project contract matches the implemented one: `docs/app-project.md` names the
//! new keys, the Gradle property, the lockfile and `[patch]` rules, the app-owned web page, the CLI
//! vocabulary and the rayx 1.4 migration, and names no library; README states the two pin sources;
//! `CLAUDE.md` and `AGENTS.md` are identical and state the library-agnostic rule.

use std::path::PathBuf;

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Whitespace-normalized text, so wrapped lines compare as one.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text from the heading containing `title` to the next heading of the same or a higher level.
fn section(text: &str, title: &str) -> String {
    let mut lines = text
        .lines()
        .skip_while(|line| !(line.starts_with('#') && line.contains(title)));
    let first = lines.next().unwrap_or_else(|| panic!("no section {title}"));
    let level = first.chars().take_while(|ch| *ch == '#').count();
    let body = lines
        .take_while(|line| {
            !(line.starts_with('#') && line.chars().take_while(|ch| *ch == '#').count() <= level)
        })
        .collect::<Vec<_>>()
        .join("\n");
    flat(&body)
}

fn assert_contains(text: &str, what: &str, needles: &[&str]) {
    for needle in needles {
        assert!(text.contains(needle), "{what} lacks `{needle}`");
    }
}

#[test]
fn the_contract_documents_entry_dependencies_and_feature_forwarding() {
    let page = read("docs/app-project.md");
    let keys = section(&page, "`[package.metadata.rayx]`");
    assert_contains(
        &keys,
        "the [package.metadata.rayx] section",
        &["`entry_dependencies`", "`android_java_dirs`"],
    );
    let entry = section(&page, "`entry_dependencies`");
    assert_contains(
        &entry,
        "the entry_dependencies section",
        &[
            "normal dependencies",
            "default is none",
            "`package` for a rename",
            "`default-features = false`",
            "`dep:<key>`",
            "`<key>?/<feature>`",
            "No feature or crate name is special",
        ],
    );
}

#[test]
fn the_contract_documents_android_java_dirs_and_its_gradle_property() {
    let page = flat(&read("docs/app-project.md"));
    let java = section(&read("docs/app-project.md"), "`android_java_dirs`");
    assert_contains(
        &java,
        "the android_java_dirs section",
        &[
            "the app included",
            "relative to its own manifest directory",
            "-PrayxAndroidJavaDirs=",
            "does not exist",
            "leaves the package's directory",
        ],
    );
    assert_contains(
        &page,
        "the Gradle project bullet",
        &["`rayxAndroidJavaDirs`", "File.pathSeparator"],
    );
}

#[test]
fn the_contract_documents_lockfile_seeding_patches_fonts_and_the_devkit_feature() {
    let page = flat(&read("docs/app-project.md"));
    assert_contains(
        &page,
        "the contract",
        &[
            "starts from a copy of the workspace's `Cargo.lock`",
            "patched package",
            "resolved dependency graph for the selected features",
            "Fonts and every other library asset arrive this way only",
            "kind = \"fonts\"",
            "define a Cargo feature named `devkit`",
        ],
    );
}

#[test]
fn the_contract_documents_the_app_owned_web_page_and_its_fallback() {
    let page = read("docs/app-project.md");
    let owned = section(&page, "Files the app owns");
    assert_contains(
        &owned,
        "Files the app owns",
        &[
            "`platform/web/index.html`",
            "`__RAYX_MODULE__`",
            "`__RAYX_TITLE__`",
            "`__RAYX_DIAGNOSTICS__`",
            "Any other `__RAYX_<NAME>__` token is an error",
        ],
    );
    // The fallback is shown in full, exactly as `rayx` writes it.
    let indented = rayx_cli::app::FALLBACK_WEB_PAGE
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        page.contains(&indented),
        "the fallback page is shown in full"
    );
}

#[test]
fn the_contract_lists_the_cli_vocabulary_and_the_migration() {
    let page = read("docs/app-project.md");
    let vocabulary = section(&page, "The CLI vocabulary");
    assert_contains(
        &vocabulary,
        "The CLI vocabulary",
        &[
            "`rayx.assets.toml`",
            "`rayx_diagnostics`",
            "`diagnostic_harness`",
            "`devkit`",
            "`RAYX_ANDROID_DEVKIT_PORT`",
            "dev.rayx.mobile.RayXActivity",
            "`RAYX_*`",
            "`data-rayx-wasm`",
            "`wasm_thread_entry_point`",
            "tests/library_agnostic.rs",
        ],
    );
    let migration = section(&page, "Migrating to rayx 1.4");
    assert_contains(
        &migration,
        "Migrating to rayx 1.4",
        &[
            "`entry_dependencies`",
            "`include`",
            "`rayxAndroidJavaDirs`",
            "package-asset declaration",
            "`android_java_dirs`",
            "`branches/std`",
            "`[workspace.metadata.rayx]`",
            "`platform/web/index.html` with the three placeholders",
            "Gone:",
        ],
    );
}

#[test]
fn the_contract_names_no_library_and_no_removed_behaviour() {
    let page = read("docs/app-project.md");
    for absent in [
        "gpux-checkout",
        "rayx_devkit",
        "gpux-fonts",
        "IndexedDB",
        "HtmlIsland",
        "webgpu-status",
        "zune",
        "branches = ",
        "gpux",
        "Gpux",
    ] {
        assert!(!page.contains(absent), "docs/app-project.md names {absent}");
    }
}

#[test]
fn the_readme_states_two_pin_sources_and_points_at_the_migration() {
    let readme = read("README.md");
    let versions = section(&readme, "Versions and where they come from");
    assert_contains(
        &versions,
        "the versions section",
        &["then the built-in default", "`project` or `default`"],
    );
    assert!(!versions.contains("gpux checkout"), "{versions}");
    assert!(versions.contains("| Android platform | `android-34` | `android-platform` |"));
    assert!(versions.contains("| Android build tools | none | `android-build-tools` |"));
    let app = section(&readme, "`rayx app` and `rayx fmt`");
    assert_contains(
        &app,
        "the rayx app section",
        &[
            "`entry_dependencies`",
            "`android_java_dirs`",
            "docs/app-project.md#migrating-to-rayx-14",
        ],
    );
}

#[test]
fn claude_and_agents_are_identical_and_state_the_library_agnostic_rule() {
    let claude = read("CLAUDE.md");
    assert_eq!(claude, read("AGENTS.md"), "CLAUDE.md and AGENTS.md differ");
    let layout = section(&claude, "Layout");
    assert_contains(
        &layout,
        "the Layout section",
        &[
            "names and depends on no gpux or RayX library",
            "tests/library_agnostic.rs",
        ],
    );
    assert!(!layout.contains("(Gpux included)"), "{layout}");
}
