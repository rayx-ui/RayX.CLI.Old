//! `[package.metadata.rayx] entry_dependencies`: the generated Android, iOS and web entry crates
//! depend on exactly the app dependencies it lists, declared as the app declares them, and every
//! app feature is forwarded generically. No crate or feature name is special.

mod common;

use std::fs;

use common::Workspace;
use rayx_cli::app::{AppDescriptor, AppFeatureSelection};
use rayx_cli::project::Dependency;

const APP: &str = "apps/demo/Cargo.toml";

/// The fixture with the app manifest replaced by `manifest`.
fn workspace_with_app(manifest: &str) -> Workspace {
    let workspace = Workspace::fixture("app-workspace");
    workspace.write(APP, manifest);
    workspace
}

/// The three generated manifests, parsed.
fn manifests(app: &AppDescriptor, selection: &AppFeatureSelection) -> Vec<(String, toml::Table)> {
    [
        ("android", app.android_rust_manifest(selection)),
        ("ios", app.ios_rust_manifest(selection)),
        ("web", app.web_manifest(selection)),
    ]
    .into_iter()
    .map(|(target, manifest)| {
        let text = fs::read_to_string(manifest.expect("generated")).expect("read");
        let table = text
            .parse::<toml::Table>()
            .unwrap_or_else(|error| panic!("{target} manifest is TOML: {error}\n{text}"));
        (target.to_string(), table)
    })
    .collect()
}

fn dependency_keys(table: &toml::Table) -> Vec<String> {
    let mut keys = table["dependencies"]
        .as_table()
        .expect("[dependencies]")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn feature(table: &toml::Table, name: &str) -> Option<Vec<String>> {
    table["features"].get(name).map(|members| {
        members
            .as_array()
            .expect("feature array")
            .iter()
            .map(|member| member.as_str().expect("string").to_string())
            .collect()
    })
}

fn web_extras(target: &str) -> Vec<String> {
    if target == "web" {
        vec!["wasm-bindgen".into(), "wasm-bindgen-futures".into()]
    } else {
        Vec::new()
    }
}

fn sorted(mut keys: Vec<String>) -> Vec<String> {
    keys.sort();
    keys
}

#[test]
fn listed_path_dependencies_are_declared_in_every_entry_crate() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        let mut expected = vec!["inspector".into(), "rayx_demo".into(), "ui_runtime".into()];
        expected.extend(web_extras(&target));
        assert_eq!(dependency_keys(&table), sorted(expected), "{target}");
        let runtime = &table["dependencies"]["ui_runtime"];
        assert!(
            runtime["path"]
                .as_str()
                .expect("path")
                .ends_with("crates/ui_runtime"),
            "{target}: {runtime}"
        );
        assert!(runtime.get("optional").is_none(), "{target}");
        assert_eq!(
            table["dependencies"]["inspector"]["optional"].as_bool(),
            Some(true),
            "{target}"
        );
    }
}

#[test]
fn without_entry_dependencies_the_entry_crate_depends_only_on_the_app() {
    for listing in ["", "entry_dependencies = []\n"] {
        let workspace = workspace_with_app(&format!(
            "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\napp_name = \"demo\"\n{listing}\n[dependencies]\nui_runtime = {{ path = \"../../crates/ui_runtime\" }}\n"
        ));
        let app = workspace.app("apps/demo");
        for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
            let mut expected = vec!["rayx_demo".to_string()];
            expected.extend(web_extras(&target));
            assert_eq!(dependency_keys(&table), sorted(expected), "{target}");
        }
    }
}

#[test]
fn no_name_is_special_unlisted_rayx_crates_are_left_out() {
    let workspace = Workspace::fixture("app-workspace");
    for name in ["rayx", "rayx_devkit"] {
        workspace.write(
            &format!("crates/{name}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        workspace.write(&format!("crates/{name}/src/lib.rs"), "");
    }
    let root = workspace.read("Cargo.toml").replace(
        "\"apps/demo\",",
        "\"apps/demo\",\n    \"crates/rayx\",\n    \"crates/rayx_devkit\",",
    );
    workspace.write("Cargo.toml", &root);
    workspace.write(
        APP,
        "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [\"ui_runtime\"]\n\n[features]\ndevkit = [\"dep:rayx_devkit\"]\n\n[dependencies]\nrayx = { path = \"../../crates/rayx\" }\nrayx_devkit = { path = \"../../crates/rayx_devkit\", optional = true }\nui_runtime = { path = \"../../crates/ui_runtime\" }\n",
    );
    let app = workspace.app("apps/demo");
    let selection = AppFeatureSelection {
        all_features: true,
        ..AppFeatureSelection::default()
    };
    for (target, table) in manifests(&app, &selection) {
        let keys = dependency_keys(&table);
        assert!(!keys.contains(&"rayx".to_string()), "{target}: {keys:?}");
        assert!(
            !keys.contains(&"rayx_devkit".to_string()),
            "{target}: {keys:?}"
        );
        assert!(keys.contains(&"ui_runtime".to_string()), "{target}");
        // `devkit` is forwarded like any feature, without the unlisted dependency.
        assert_eq!(
            feature(&table, "devkit"),
            Some(vec!["rayx_demo/devkit".to_string()]),
            "{target}"
        );
    }
}

#[test]
fn a_declaration_keeps_its_rename_features_default_features_and_target_table() {
    let workspace = Workspace::fixture("app-workspace");
    workspace.write(
        "crates/ui_runtime/Cargo.toml",
        "[package]\nname = \"ui_runtime\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\ndefault = []\nfast = []\n",
    );
    workspace.write(
        APP,
        "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [\"runtime\", \"store_backend\"]\n\n[features]\nstore = [\"dep:store_backend\"]\n\n[dependencies]\nruntime = { package = \"ui_runtime\", path = \"../../crates/ui_runtime\", default-features = false, features = [\"fast\"] }\n\n[target.'cfg(target_family = \"wasm\")'.dependencies]\nstore_backend = { version = \"0.1\", optional = true }\n",
    );
    let app = workspace.app("apps/demo");
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        let runtime = table["dependencies"]["runtime"]
            .as_table()
            .expect("runtime table");
        assert_eq!(runtime["package"].as_str(), Some("ui_runtime"), "{target}");
        assert_eq!(runtime["default-features"].as_bool(), Some(false));
        assert_eq!(
            runtime["features"].as_array().map(|f| f.len()),
            Some(1),
            "{target}"
        );
        assert!(
            table["dependencies"].get("store_backend").is_none(),
            "{target}: target-specific declarations stay in their table"
        );
        let store =
            &table["target"]["cfg(target_family = \"wasm\")"]["dependencies"]["store_backend"];
        assert_eq!(store["version"].as_str(), Some("^0.1"), "{target}");
        assert_eq!(store["optional"].as_bool(), Some(true), "{target}");
        assert_eq!(
            feature(&table, "store"),
            Some(vec![
                "rayx_demo/store".to_string(),
                "dep:store_backend".to_string()
            ]),
            "{target}"
        );
    }
}

#[test]
fn git_and_registry_sources_are_copied() {
    let workspace = Workspace::fixture("app-workspace");
    let mut app = workspace.app("apps/demo");
    let package = app
        .context
        .project
        .packages
        .iter_mut()
        .find(|package| package.name == "rayx_demo")
        .expect("the app package");
    package.dependencies.retain(|d| d.name != "ui_runtime");
    package.dependencies.push(Dependency {
        name: "ui_runtime".into(),
        source: Some("git+https://example.com/ui/runtime?tag=v1.2.0#0123abcd".into()),
        ..Dependency::default()
    });
    package.dependencies.retain(|d| d.name != "inspector");
    package.dependencies.push(Dependency {
        name: "inspector".into(),
        req: "^1.0".into(),
        source: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
        optional: true,
        ..Dependency::default()
    });
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        let runtime = &table["dependencies"]["ui_runtime"];
        assert_eq!(
            runtime["git"].as_str(),
            Some("https://example.com/ui/runtime")
        );
        assert_eq!(runtime["tag"].as_str(), Some("v1.2.0"), "{target}");
        let inspector = &table["dependencies"]["inspector"];
        assert_eq!(inspector["version"].as_str(), Some("^1.0"), "{target}");
        assert_eq!(inspector["optional"].as_bool(), Some(true), "{target}");
    }
}

#[test]
fn a_listed_key_that_is_not_a_normal_dependency_is_refused() {
    for (listing, extra) in [
        ("\"missing\"", ""),
        (
            "\"tool\"",
            "\n[dev-dependencies]\ntool = { path = \"../../crates/inspector\", package = \"inspector\" }\n",
        ),
        (
            "\"tool\"",
            "\n[build-dependencies]\ntool = { path = \"../../crates/inspector\", package = \"inspector\" }\n",
        ),
    ] {
        let workspace = workspace_with_app(&format!(
            "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [{listing}]\n\n[dependencies]\nui_runtime = {{ path = \"../../crates/ui_runtime\" }}\n{extra}"
        ));
        let app = workspace.app("apps/demo");
        let error = app
            .android_rust_manifest(&AppFeatureSelection::default())
            .expect_err("not a normal dependency");
        let text = format!("{error:#}");
        let key = listing.trim_matches('"');
        assert!(text.contains(&format!("`{key}`")), "{text}");
        assert!(
            text.contains(&app.manifest().display().to_string()),
            "{text}"
        );
    }
}

#[test]
fn entry_dependencies_must_be_an_array_of_strings() {
    for value in ["\"ui_runtime\"", "[1]", "{ ui_runtime = true }"] {
        let workspace = workspace_with_app(&format!(
            "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = {value}\n\n[dependencies]\nui_runtime = {{ path = \"../../crates/ui_runtime\" }}\n"
        ));
        let app = workspace.app("apps/demo");
        let error = app
            .web_manifest(&AppFeatureSelection::default())
            .expect_err("not an array of strings");
        assert!(
            format!("{error:#}").contains("entry_dependencies"),
            "{value}: {error:#}"
        );
    }
}

#[test]
fn features_enable_optional_entry_dependencies_directly_or_through_other_features() {
    let workspace = Workspace::fixture("app-workspace");
    workspace.write(
        "crates/inspector/Cargo.toml",
        "[package]\nname = \"inspector\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nverbose = []\n",
    );
    workspace.write(
        APP,
        "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [\"inspector\", \"ui_runtime\"]\n\n[features]\ninspect = [\"dep:inspector\"]\ntools = [\"inspect\"]\nverbose = [\"inspector/verbose\"]\nweak = [\"inspector?/verbose\"]\nplain = []\n\n[dependencies]\ninspector = { path = \"../../crates/inspector\", optional = true }\nui_runtime = { path = \"../../crates/ui_runtime\" }\n",
    );
    let app = workspace.app("apps/demo");
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        let with_dep = |name: &str| {
            Some(vec![
                format!("rayx_demo/{name}"),
                "dep:inspector".to_string(),
            ])
        };
        let without = |name: &str| Some(vec![format!("rayx_demo/{name}")]);
        assert_eq!(feature(&table, "inspect"), with_dep("inspect"), "{target}");
        assert_eq!(feature(&table, "tools"), with_dep("tools"), "{target}");
        assert_eq!(feature(&table, "verbose"), with_dep("verbose"), "{target}");
        assert_eq!(feature(&table, "weak"), without("weak"), "{target}");
        assert_eq!(feature(&table, "plain"), without("plain"), "{target}");
        assert_eq!(feature(&table, "devkit"), None, "{target}");
    }
}

#[test]
fn the_implicit_feature_of_an_optional_dependency_enables_it() {
    let workspace = workspace_with_app(
        "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [\"inspector\"]\n\n[features]\nextras = [\"inspector\"]\n\n[dependencies]\ninspector = { path = \"../../crates/inspector\", optional = true }\n",
    );
    let app = workspace.app("apps/demo");
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        assert_eq!(
            feature(&table, "extras"),
            Some(vec![
                "rayx_demo/extras".to_string(),
                "dep:inspector".to_string()
            ]),
            "{target}"
        );
    }
}

#[test]
fn an_app_without_features_gets_no_invented_devkit_feature() {
    let workspace = workspace_with_app(
        "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata.rayx]\nentry_dependencies = [\"ui_runtime\"]\n\n[dependencies]\nui_runtime = { path = \"../../crates/ui_runtime\" }\n",
    );
    let app = workspace.app("apps/demo");
    for (target, table) in manifests(&app, &AppFeatureSelection::default()) {
        let features = table["features"].as_table().expect("[features]");
        assert_eq!(features.keys().collect::<Vec<_>>(), ["default"], "{target}");
        assert_eq!(features["default"].as_array().map(Vec::len), Some(0));
    }
}
