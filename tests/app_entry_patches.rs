//! Root `[patch]` entries follow the app's resolved graph: an entry is repeated in every
//! generated entry crate exactly when its patched package is in the app's graph for the selected
//! features. No package name decides it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::Workspace;
use rayx_cli::app::graph::{AppGraph, GraphPackage};
use rayx_cli::app::{AppDescriptor, AppFeatureSelection, patch_tables_for_graph};

type Generate = fn(&AppDescriptor, &AppFeatureSelection) -> anyhow::Result<PathBuf>;

const TARGETS: [(&str, Generate); 3] = [
    ("android", AppDescriptor::android_rust_manifest),
    ("ios", AppDescriptor::ios_rust_manifest),
    ("web", AppDescriptor::web_manifest),
];

fn store() -> AppFeatureSelection {
    AppFeatureSelection {
        features: vec!["store".into()],
        no_default_features: true,
        ..AppFeatureSelection::default()
    }
}

fn parsed(path: PathBuf) -> (String, toml::Table) {
    let text = fs::read_to_string(path).expect("read");
    let table = text
        .parse::<toml::Table>()
        .unwrap_or_else(|error| panic!("generated manifest is TOML: {error}\n{text}"));
    (text, table)
}

fn slash(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_string()
}

#[test]
fn a_patch_reached_through_a_selected_feature_is_repeated_in_every_entry_crate() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, generate) in TARGETS {
        let (text, table) = parsed(generate(&app, &store()).expect("generated"));
        let crates_io = table["patch"]["crates-io"]
            .as_table()
            .unwrap_or_else(|| panic!("{target}: [patch.crates-io] in {text}"));
        assert_eq!(
            crates_io.keys().collect::<Vec<_>>(),
            ["store_backend"],
            "{target}"
        );
        let path = crates_io["store_backend"]["path"].as_str().expect("path");
        assert_eq!(
            path,
            slash(&workspace.path("patches/store_backend")),
            "{target}: absolute under the selected workspace"
        );
        assert!(!path.contains('\\') && !path.starts_with("//?/"));
    }
}

#[test]
fn a_patch_behind_an_unselected_feature_or_outside_the_graph_is_left_out() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, generate) in TARGETS {
        let (text, table) =
            parsed(generate(&app, &AppFeatureSelection::default()).expect("generated"));
        assert!(table.get("patch").is_none(), "{target}: {text}");
        assert!(!text.contains("unused_backend"), "{target}");
    }
}

#[test]
fn no_patch_table_is_written_when_the_root_manifest_has_none() {
    let workspace = Workspace::fixture("app-workspace");
    let root = workspace.read("Cargo.toml");
    let root = root.split("[patch.crates-io]").next().expect("members");
    workspace.write("Cargo.toml", root);
    let app = workspace.app("apps/demo");
    for (target, generate) in TARGETS {
        let (text, table) = parsed(generate(&app, &store()).expect("generated"));
        assert!(table.get("patch").is_none(), "{target}: {text}");
    }
}

fn graph(names: &[&str]) -> AppGraph {
    AppGraph {
        app_id: "app".into(),
        packages: names
            .iter()
            .map(|name| GraphPackage {
                id: (*name).into(),
                name: (*name).into(),
                manifest_path: PathBuf::from(format!("/work/{name}/Cargo.toml")),
                metadata: serde_json::Value::Null,
            })
            .collect(),
    }
}

#[test]
fn every_field_is_kept_url_keys_are_quoted_and_relative_paths_become_absolute() {
    let root: toml::Table = r#"
[patch.crates-io]
local_widgets = { path = "vendor/widgets" }
never_used = { path = "vendor/never" }

[patch."https://example.com/ui/runtime"]
runtime = { git = "https://example.com/forks/runtime", rev = "abc1234", package = "ui_runtime" }
other = { git = "https://example.com/forks/other", branch = "main" }
"#
    .parse()
    .expect("root manifest");
    let workspace_root = Path::new("/work/project");
    let text = patch_tables_for_graph(
        &root,
        workspace_root,
        &graph(&["app", "local_widgets", "ui_runtime"]),
    )
    .expect("rendered");

    let table: toml::Table = text.parse().expect("valid TOML");
    let patch = table["patch"].as_table().expect("[patch]");
    assert_eq!(
        patch["crates-io"]["local_widgets"]["path"].as_str(),
        Some(slash(&workspace_root.join("vendor/widgets")).as_str())
    );
    assert!(patch["crates-io"].get("never_used").is_none(), "{text}");
    let url = patch["https://example.com/ui/runtime"]
        .as_table()
        .expect("URL-keyed table");
    assert_eq!(
        url.keys().collect::<Vec<_>>(),
        ["runtime"],
        "selected by `package`, not the key: {text}"
    );
    assert_eq!(
        url["runtime"]["git"].as_str(),
        Some("https://example.com/forks/runtime")
    );
    assert_eq!(url["runtime"]["rev"].as_str(), Some("abc1234"));
    assert_eq!(url["runtime"]["package"].as_str(), Some("ui_runtime"));
    assert!(
        text.contains("[patch.\"https://example.com/ui/runtime\"]"),
        "{text}"
    );

    let none = patch_tables_for_graph(&root, workspace_root, &graph(&["app"])).expect("rendered");
    assert!(none.is_empty(), "{none}");
}
