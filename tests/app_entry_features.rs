//! The generated entry crates enable the selected app features through their own features, so a
//! selected feature also turns on the optional entry dependencies it enables and the
//! `#[cfg(feature = "...")]` code in the app's `main.rs`, which compiles inside the entry crate.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::Workspace;
use rayx_cli::app::{AppDescriptor, AppFeatureSelection};

const APP: &str = "apps/demo/Cargo.toml";

/// The fixture app, `inspect = ["dep:inspector"]`, with no default features and without the
/// registry dependency, so Cargo resolves the entry crate offline.
fn workspace() -> Workspace {
    let workspace = Workspace::fixture("app-workspace");
    let manifest = workspace
        .read(APP)
        .replace("default = [\"profile-all\"]\n", "")
        .replace("store = [\"dep:store_backend\"]\n", "")
        .replace(
            "store_backend = { version = \"0.1\", optional = true }\n",
            "",
        );
    workspace.write(APP, &manifest);
    workspace
}

fn selection(features: &[&str]) -> AppFeatureSelection {
    AppFeatureSelection {
        features: features.iter().map(|feature| feature.to_string()).collect(),
        ..AppFeatureSelection::default()
    }
}

/// The three generated manifests, with their paths.
fn manifests(
    app: &AppDescriptor,
    selection: &AppFeatureSelection,
) -> Vec<(&'static str, std::path::PathBuf)> {
    vec![
        (
            "android",
            app.android_rust_manifest(selection).expect("android"),
        ),
        ("ios", app.ios_rust_manifest(selection).expect("ios")),
        ("web", app.web_manifest(selection).expect("web")),
    ]
}

fn default_features(manifest: &Path) -> Vec<String> {
    let table = fs::read_to_string(manifest)
        .expect("read")
        .parse::<toml::Table>()
        .expect("TOML");
    table["features"]["default"]
        .as_array()
        .expect("default array")
        .iter()
        .map(|member| member.as_str().expect("string").to_string())
        .collect()
}

/// The entry package's resolved features and dependency names, as Cargo resolves the generated
/// manifest with its default features.
fn resolved(manifest: &Path) -> (Vec<String>, Vec<String>) {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata emits JSON");
    let root = metadata["resolve"]["root"].as_str().expect("root package");
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"].as_str() == Some(root))
        .expect("root node");
    let strings = |value: &serde_json::Value, key: Option<&str>| {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|item| {
                key.map_or(item, |key| &item[key])
                    .as_str()
                    .expect("string")
                    .to_string()
            })
            .collect::<Vec<_>>()
    };
    (
        strings(&node["features"], None),
        strings(&node["deps"], Some("name")),
    )
}

#[test]
fn a_selected_feature_enables_the_entry_feature_and_its_optional_dependency() {
    let workspace = workspace();
    let app = workspace.app("apps/demo");
    for (target, manifest) in manifests(&app, &selection(&["inspect"])) {
        assert_eq!(default_features(&manifest), vec!["inspect"], "{target}");
    }
    let manifest = app
        .android_rust_manifest(&selection(&["inspect"]))
        .expect("android");
    let (features, deps) = resolved(&manifest);
    assert!(features.contains(&"inspect".to_string()), "{features:?}");
    assert!(deps.contains(&"inspector".to_string()), "{deps:?}");
}

#[test]
fn no_selection_enables_no_entry_feature_or_optional_dependency() {
    let workspace = workspace();
    let app = workspace.app("apps/demo");
    for (target, manifest) in manifests(&app, &AppFeatureSelection::default()) {
        assert!(default_features(&manifest).is_empty(), "{target}");
    }
    let manifest = app
        .android_rust_manifest(&AppFeatureSelection::default())
        .expect("android");
    let (features, deps) = resolved(&manifest);
    assert_eq!(features, vec!["default"], "{features:?}");
    assert!(!deps.contains(&"inspector".to_string()), "{deps:?}");
}

#[test]
fn the_app_default_features_become_entry_features() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, manifest) in manifests(&app, &AppFeatureSelection::default()) {
        assert_eq!(default_features(&manifest), vec!["profile-all"], "{target}");
    }
}

#[test]
fn no_default_features_drops_the_app_defaults() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let selection = AppFeatureSelection {
        features: vec!["inspect".to_string()],
        no_default_features: true,
        ..AppFeatureSelection::default()
    };
    for (target, manifest) in manifests(&app, &selection) {
        assert_eq!(default_features(&manifest), vec!["inspect"], "{target}");
    }
}

#[test]
fn all_features_enables_every_entry_feature() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let selection = AppFeatureSelection {
        all_features: true,
        ..AppFeatureSelection::default()
    };
    for (target, manifest) in manifests(&app, &selection) {
        let mut expected = vec!["inspect", "profile-a", "profile-all", "profile-b", "store"];
        expected.sort();
        assert_eq!(default_features(&manifest), expected, "{target}");
    }
}

#[test]
fn an_implicit_optional_dependency_feature_is_forwarded_to_the_app() {
    let workspace = workspace();
    workspace.write(
        "crates/extra/Cargo.toml",
        "[package]\nname = \"extra\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    workspace.write("crates/extra/src/lib.rs", "");
    let manifest = workspace
        .read(APP)
        .replace("[features]\n", "[features]\ndefault = [\"extra\"]\n")
        + "extra = { path = \"../../crates/extra\", optional = true }\n";
    workspace.write(APP, &manifest);
    let app = workspace.app("apps/demo");
    for (target, manifest) in manifests(&app, &AppFeatureSelection::default()) {
        assert_eq!(
            default_features(&manifest),
            vec!["rayx_demo/extra"],
            "{target}"
        );
    }
}
