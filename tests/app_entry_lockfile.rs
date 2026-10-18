//! Every generated entry crate (Android, iOS, web) starts from the project's `Cargo.lock`, and
//! the web entry crate pins nothing a library needs: only the app, `wasm-bindgen` at the locked
//! version, `wasm-bindgen-futures` and the listed entry dependencies.

mod common;

use std::fs;
use std::path::PathBuf;

use common::Workspace;
use rayx_cli::app::{AppDescriptor, AppFeatureSelection};

type Generate = fn(&AppDescriptor, &AppFeatureSelection) -> anyhow::Result<PathBuf>;

const TARGETS: [(&str, Generate); 3] = [
    ("android", AppDescriptor::android_rust_manifest),
    ("ios", AppDescriptor::ios_rust_manifest),
    ("wasm", AppDescriptor::web_manifest),
];

#[test]
fn every_entry_crate_gets_the_workspace_lock_byte_for_byte() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, generate) in TARGETS {
        let manifest = generate(&app, &AppFeatureSelection::default()).expect("generated");
        let lock = manifest.with_file_name("Cargo.lock");
        assert_eq!(
            fs::read(&lock).expect("seeded lock"),
            fs::read(workspace.path("Cargo.lock")).expect("workspace lock"),
            "{target}"
        );
    }
}

#[test]
fn a_stale_entry_lock_is_replaced_on_every_generation() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, generate) in TARGETS {
        let manifest = generate(&app, &AppFeatureSelection::default()).expect("generated");
        let lock = manifest.with_file_name("Cargo.lock");
        fs::write(&lock, "stale lock").expect("write stale");
        generate(&app, &AppFeatureSelection::default()).expect("regenerated");
        assert_eq!(
            fs::read(&lock).expect("lock"),
            fs::read(workspace.path("Cargo.lock")).expect("workspace lock"),
            "{target}"
        );
    }
}

#[test]
fn without_a_workspace_lock_a_stale_entry_lock_is_removed() {
    // With the `[patch]` table and a feature that reaches a patched package, generation resolves
    // the app's graph, which may write a workspace lock; the entry crates still get none.
    let store = AppFeatureSelection {
        features: vec!["store".into()],
        no_default_features: true,
        ..AppFeatureSelection::default()
    };
    for (target, generate) in TARGETS {
        let workspace = Workspace::fixture("app-workspace");
        let app = workspace.app("apps/demo");
        let manifest = generate(&app, &store).expect("generated");
        let lock = manifest.with_file_name("Cargo.lock");
        fs::write(&lock, "stale lock").expect("write stale");
        fs::remove_file(workspace.path("Cargo.lock")).expect("remove workspace lock");
        let manifest = generate(&app, &store).expect("regenerated");
        assert!(
            fs::read_to_string(manifest)
                .expect("manifest")
                .contains("[patch.crates-io]"),
            "{target}: the graph was resolved"
        );
        assert!(
            !lock.exists(),
            "{target}: no entry lock without a workspace lock"
        );
    }
}

#[test]
fn the_web_entry_crate_depends_only_on_the_app_wasm_bindgen_and_its_entry_dependencies() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let manifest = app
        .web_manifest(&AppFeatureSelection::default())
        .expect("web manifest");
    let text = fs::read_to_string(manifest).expect("read");
    let table: toml::Table = text.parse().expect("TOML");
    let dependencies = table["dependencies"].as_table().expect("[dependencies]");
    let mut keys = dependencies.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(
        keys,
        [
            "inspector",
            "rayx_demo",
            "ui_runtime",
            "wasm-bindgen",
            "wasm-bindgen-futures"
        ]
    );
    assert_eq!(dependencies["wasm-bindgen"].as_str(), Some("=0.2.129"));
    assert_eq!(dependencies["wasm-bindgen-futures"].as_str(), Some("0.4"));
    assert!(table.get("target").is_none(), "{text}");
    for gone in ["zune-core", "branches", "rclite"] {
        assert!(!text.contains(gone), "{gone} in {text}");
    }
}

#[test]
fn a_web_entry_crate_without_a_locked_wasm_bindgen_still_fails() {
    let workspace = Workspace::fixture("app-workspace");
    let lock = workspace
        .read("Cargo.lock")
        .replace("name = \"wasm-bindgen\"", "name = \"not-wasm-bindgen\"");
    workspace.write("Cargo.lock", &lock);
    let app = workspace.app("apps/demo");
    let error = app
        .web_manifest(&AppFeatureSelection::default())
        .expect_err("no locked wasm-bindgen");
    let text = format!("{error:#}");
    assert!(text.contains("wasm-bindgen is not locked in"), "{text}");
}
