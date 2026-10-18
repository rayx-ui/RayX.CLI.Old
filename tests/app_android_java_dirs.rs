//! `[package.metadata.rayx] android_java_dirs`: any package in the app's graph for the selected
//! features (the app included) declares Android Java source directories, which `rayx` collects as
//! absolute, deduplicated, sorted directories for the Gradle property `rayxAndroidJavaDirs`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::Workspace;
use rayx_cli::app::{AppFeatureSelection, android_java_dirs_property};

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("canonical")
}

fn canonical_all(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    dirs.iter().map(|dir| canonical(dir)).collect()
}

fn inspect() -> AppFeatureSelection {
    AppFeatureSelection {
        features: vec!["inspect".into()],
        ..AppFeatureSelection::default()
    }
}

/// Appends an `android_java_dirs` declaration to a package of the fixture and creates the dirs.
fn declare(workspace: &Workspace, package_dir: &str, dirs: &[&str]) {
    for dir in dirs {
        let path = workspace.path(&format!("{package_dir}/{dir}"));
        fs::create_dir_all(&path).expect("java dir");
    }
    let manifest = format!("{package_dir}/Cargo.toml");
    let text = workspace.read(&manifest);
    let list = dirs
        .iter()
        .map(|dir| format!("\"{dir}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let text = if text.contains("[package.metadata.rayx]") {
        text.replace(
            "[package.metadata.rayx]\n",
            &format!("[package.metadata.rayx]\nandroid_java_dirs = [{list}]\n"),
        )
    } else {
        format!("{text}\n[package.metadata.rayx]\nandroid_java_dirs = [{list}]\n")
    };
    workspace.write(&manifest, &text);
}

#[test]
fn declarations_from_the_graph_are_absolute_deduplicated_and_sorted() {
    let workspace = Workspace::fixture("app-workspace");
    // The app's own declaration, a package with two directories (one listed twice).
    declare(&workspace, "apps/demo", &["android/java"]);
    let runtime = workspace.read("crates/ui_runtime/Cargo.toml").replace(
        "android_java_dirs = [\"java\"]",
        "android_java_dirs = [\"java\", \"extra/java\", \"java\"]",
    );
    workspace.write("crates/ui_runtime/Cargo.toml", &runtime);
    fs::create_dir_all(workspace.path("crates/ui_runtime/extra/java")).expect("dir");
    let app = workspace.app("apps/demo");

    let dirs = app.android_java_dirs(&inspect()).expect("dirs");
    assert!(dirs.iter().all(|dir| dir.is_absolute()), "{dirs:?}");
    let mut expected = vec![
        canonical(&workspace.path("apps/demo/android/java")),
        canonical(&workspace.path("crates/inspector/java")),
        canonical(&workspace.path("crates/ui_runtime/extra/java")),
        canonical(&workspace.path("crates/ui_runtime/java")),
    ];
    expected.sort();
    assert_eq!(canonical_all(dirs.clone()), expected);
    let mut sorted = dirs.clone();
    sorted.sort();
    assert_eq!(dirs, sorted, "sorted");
}

#[test]
fn the_property_joins_the_directories_with_the_platform_separator() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let dirs = app.android_java_dirs(&inspect()).expect("dirs");
    assert_eq!(dirs.len(), 2, "{dirs:?}");
    let property = android_java_dirs_property(&dirs)
        .expect("joined")
        .expect("a property");
    let value = property
        .strip_prefix("-PrayxAndroidJavaDirs=")
        .expect("the property name");
    let split = std::env::split_paths(value).collect::<Vec<_>>();
    assert_eq!(split, dirs);
    let separator = if cfg!(windows) { ';' } else { ':' };
    assert_eq!(value.matches(separator).count(), 1, "{value}");
}

#[test]
fn packages_outside_the_graph_and_unselected_features_contribute_nothing() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    // `inspector` is behind the unselected `inspect` feature; `unused_backend` is a member the
    // app does not depend on.
    let dirs = canonical_all(
        app.android_java_dirs(&AppFeatureSelection::default())
            .expect("dirs"),
    );
    assert_eq!(dirs, [canonical(&workspace.path("crates/ui_runtime/java"))]);
    let all = AppFeatureSelection {
        all_features: true,
        ..AppFeatureSelection::default()
    };
    let dirs = canonical_all(app.android_java_dirs(&all).expect("dirs"));
    assert!(!dirs.contains(&canonical(&workspace.path("patches/unused_backend/java"))));
    assert!(dirs.contains(&canonical(&workspace.path("crates/inspector/java"))));
}

#[test]
fn without_declarations_there_are_no_directories_and_no_property() {
    let workspace = Workspace::fixture("app-workspace");
    let runtime = workspace
        .read("crates/ui_runtime/Cargo.toml")
        .replace("android_java_dirs = [\"java\"]\n", "");
    workspace.write("crates/ui_runtime/Cargo.toml", &runtime);
    let app = workspace.app("apps/demo");
    let dirs = app
        .android_java_dirs(&AppFeatureSelection::default())
        .expect("dirs");
    assert!(dirs.is_empty(), "{dirs:?}");
    assert_eq!(android_java_dirs_property(&dirs).expect("none"), None);
}

fn error_for(value: &str, setup: impl Fn(&Workspace)) -> String {
    let workspace = Workspace::fixture("app-workspace");
    setup(&workspace);
    let runtime = workspace.read("crates/ui_runtime/Cargo.toml").replace(
        "android_java_dirs = [\"java\"]",
        &format!("android_java_dirs = {value}"),
    );
    workspace.write("crates/ui_runtime/Cargo.toml", &runtime);
    let app = workspace.app("apps/demo");
    let error = app
        .android_java_dirs(&AppFeatureSelection::default())
        .expect_err("invalid declaration");
    format!("{error:#}")
}

#[test]
fn a_value_that_is_not_an_array_of_strings_is_refused() {
    for value in ["\"java\"", "[1]"] {
        let text = error_for(value, |_| {});
        assert!(text.contains("ui_runtime"), "{text}");
        assert!(text.contains("android_java_dirs"), "{text}");
    }
}

#[test]
fn absolute_escaping_and_missing_entries_are_refused_with_the_package_and_entry() {
    let absolute = if cfg!(windows) {
        "C:/java"
    } else {
        "/tmp/java"
    };
    for (entry, reason, setup) in [
        (absolute, "absolute", None),
        ("missing", "does not exist", None),
        (
            "../outside",
            "leaves the package directory",
            Some("crates/outside"),
        ),
    ] {
        let text = error_for(&format!("[\"{entry}\"]"), |workspace| {
            if let Some(dir) = setup {
                fs::create_dir_all(workspace.path(dir)).expect("outside dir");
            }
        });
        assert!(text.contains("ui_runtime"), "{entry}: {text}");
        assert!(text.contains(&format!("`{entry}`")), "{entry}: {text}");
        assert!(text.contains(reason), "{entry}: {text}");
    }
}
