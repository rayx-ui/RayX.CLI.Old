//! The app pipeline against workspaces that do not look like RayX's own: nothing is found from
//! where `rayx` was built, and every output lands under the selected workspace.

mod common;

use std::fs;
use std::path::Path;

use common::Workspace;
use rayx_cli::app::{
    AppDescriptor, AppFeatureSelection, BuildProfile, ProjectContext, check_wasm_bindgen_version,
    dependency_toml, desktop_target_triple, wasm_cargo_command,
};
use rayx_cli::host::{Arch, HostFacts, Os};
use rayx_cli::project::Dependency;

fn canonical(path: &Path) -> std::path::PathBuf {
    fs::canonicalize(path).expect("canonical path")
}

fn host(os: Os, arch: Arch) -> HostFacts {
    HostFacts {
        os,
        arch,
        emulated: false,
        wsl: false,
        distro: None,
    }
}

#[test]
fn the_app_directory_resolves_against_the_current_directory() {
    let workspace = Workspace::fixture("app-workspace");
    let expected = canonical(&workspace.path("apps/demo"));

    // From the workspace root, from a sibling directory with `..`, and from inside the app.
    for (cwd, app_dir) in [
        (workspace.root.clone(), "apps/demo"),
        (workspace.path("crates"), "../apps/demo"),
        (workspace.path("apps/demo"), "."),
    ] {
        let context = ProjectContext::discover(&cwd, Some(Path::new(app_dir))).expect("project");
        let app = AppDescriptor::resolve_from(&context, &cwd, app_dir).expect("app");
        assert_eq!(canonical(&app.root), expected, "{app_dir} from {cwd:?}");
        assert_eq!(
            canonical(context.workspace_root()),
            canonical(&workspace.root),
            "the workspace of {app_dir}"
        );
    }
}

#[test]
fn desktop_pack_writes_under_the_selected_workspace() {
    let workspace = Workspace::fixture("app-workspace");
    let app_dir = workspace.path("apps/demo").display().to_string();

    rayx_cli::app::run_unchecked(vec![app_dir, "pack".into(), "host".into()])
        .expect("the fixture app packs");

    let exe = if cfg!(windows) {
        "rayx_demo.exe"
    } else {
        "rayx_demo"
    };
    let packed = workspace.path("artifacts/apps/demo/host/release");
    assert!(packed.join(exe).is_file(), "{packed:?}");
    assert!(packed.join("assets/index.json").is_file());
    assert!(workspace.path("target/release").join(exe).is_file());
    assert!(
        !workspace.path("apps/demo/target").exists()
            && !workspace.path("apps/demo/artifacts").exists(),
        "nothing is written next to the app"
    );
}

#[test]
fn entry_manifests_copy_the_apps_own_entry_dependency() {
    let workspace = Workspace::fixture("app-workspace");
    let mut app = workspace.app("apps/demo");
    let manifest_for = |app: &AppDescriptor| {
        let manifest = app
            .android_rust_manifest(&AppFeatureSelection::default())
            .expect("manifest");
        let text = fs::read_to_string(manifest).expect("read");
        text.lines()
            .find(|line| line.starts_with("ui_runtime = "))
            .expect("the ui_runtime dependency line")
            .to_string()
    };

    // A path dependency: the absolute directory of the app's own `ui_runtime`.
    let line = manifest_for(&app);
    assert!(line.contains("path = "), "{line}");
    assert!(line.contains("crates/ui_runtime\""), "{line}");
    assert!(!line.contains("version"), "{line}");

    let set_runtime = |app: &mut AppDescriptor, dependency: Dependency| {
        let package = app
            .context
            .project
            .packages
            .iter_mut()
            .find(|package| package.name == "rayx_demo")
            .expect("the app package");
        package.dependencies.retain(|d| d.name != "ui_runtime");
        package.dependencies.push(dependency);
    };

    // A git dependency keeps its revision.
    set_runtime(
        &mut app,
        Dependency {
            name: "ui_runtime".into(),
            source: Some("git+https://example.com/ui/runtime?rev=abc1234#abc1234def".into()),
            ..Dependency::default()
        },
    );
    assert_eq!(
        manifest_for(&app),
        "ui_runtime = { git = \"https://example.com/ui/runtime\", rev = \"abc1234\" }"
    );

    // A registry dependency keeps its version requirement.
    set_runtime(
        &mut app,
        Dependency {
            name: "ui_runtime".into(),
            req: "^0.4".into(),
            source: Some("registry+https://github.com/rust-lang/crates.io-index".into()),
            ..Dependency::default()
        },
    );
    assert_eq!(manifest_for(&app), "ui_runtime = { version = \"^0.4\" }");
}

#[test]
fn dependency_sources_that_cannot_be_copied_are_refused() {
    let private_registry = Dependency {
        name: "ui_runtime".into(),
        source: Some("registry+https://registry.example.com/index".into()),
        ..Dependency::default()
    };
    let error = dependency_toml(&private_registry, false).expect_err("not copyable");
    assert!(format!("{error:#}").contains("registry.example.com"));

    let optional = Dependency {
        name: "inspector".into(),
        path: Some("/work/ui/crates/inspector".into()),
        optional: true,
        ..Dependency::default()
    };
    let text = dependency_toml(&optional, true).expect("path dependency");
    assert_eq!(
        text,
        "{ path = \"/work/ui/crates/inspector\", optional = true }"
    );
}

#[test]
fn root_patches_come_from_the_selected_workspace() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let selection = AppFeatureSelection {
        features: vec!["store".into()],
        no_default_features: true,
        ..AppFeatureSelection::default()
    };

    let manifest = app.web_manifest(&selection).expect("web manifest");

    let text = fs::read_to_string(manifest).expect("read");
    let patches = text
        .split("[patch.crates-io]")
        .nth(1)
        .expect("the patch table");
    assert!(patches.contains("store_backend = { path = "), "{patches}");
    assert!(patches.contains("patches/store_backend"), "{patches}");
    assert!(!patches.contains("unused_backend"), "{patches}");
    assert!(
        !patches.contains("RayX.CLI"),
        "no path from where rayx was built: {patches}"
    );
    let root = workspace.root.display().to_string().replace('\\', "/");
    let root = root.trim_start_matches("//?/");
    assert!(
        patches.replace('\\', "/").contains(root),
        "patch paths are absolute under {root}: {patches}"
    );
}

#[test]
fn the_threaded_build_uses_the_pinned_nightly_and_the_workspace_target() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let manifest = app
        .web_manifest(&AppFeatureSelection::default())
        .expect("web manifest");

    let command = wasm_cargo_command(&app, &manifest, BuildProfile::Release).expect("command");

    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args[0],
        format!("+{}", app.context.pins.web_toolchain.value)
    );
    assert!(args[0].starts_with("+nightly-"), "{}", args[0]);
    for expected in [
        "build-std=std,panic_abort",
        "wasm32-unknown-unknown",
        "--release",
    ] {
        assert!(
            args.iter().any(|arg| arg == expected),
            "{expected} in {args:?}"
        );
    }
    let target_dir = args
        .iter()
        .position(|arg| arg == "--target-dir")
        .and_then(|index| args.get(index + 1))
        .expect("a target directory");
    let target_dir = Path::new(target_dir);
    assert_eq!(
        target_dir.file_name().and_then(|n| n.to_str()),
        Some("target")
    );
    assert_eq!(
        canonical(target_dir.parent().expect("the workspace root")),
        canonical(&workspace.root)
    );
    let rustflags = command
        .get_envs()
        .find(|(key, _)| key.to_str() == Some("RUSTFLAGS"))
        .and_then(|(_, value)| value)
        .expect("RUSTFLAGS")
        .to_string_lossy()
        .into_owned();
    assert!(rustflags.contains("+atomics"), "{rustflags}");
}

#[test]
fn the_wasm_bindgen_cli_must_be_the_version_the_project_locks() {
    let workspace = Workspace::fixture("app-workspace");
    let app = workspace.app("apps/demo");
    let locked = app
        .context
        .pins
        .wasm_bindgen
        .as_ref()
        .expect("the fixture's Cargo.lock names wasm-bindgen");
    assert_eq!(locked.value, "0.2.129");

    check_wasm_bindgen_version(&locked.value, &locked.value).expect("same version");
    let error = check_wasm_bindgen_version("0.2.100", &locked.value).expect_err("other version");
    let text = format!("{error:#}");
    assert!(
        text.contains("0.2.100") && text.contains("0.2.129"),
        "{text}"
    );
    assert!(text.contains("rayx setup --web"), "{text}");
}

#[test]
fn desktop_triples_follow_the_native_host_architecture() {
    let triple = |name: &str, os: Os, arch: Arch, extra: &[&str]| {
        let mut args: Vec<String> = extra.iter().map(ToString::to_string).collect();
        desktop_target_triple(name, &host(os, arch), &mut args).expect("triple")
    };

    assert_eq!(
        triple("windows", Os::Windows, Arch::Arm64, &[]).as_deref(),
        Some("aarch64-pc-windows-msvc")
    );
    assert_eq!(
        triple("windows", Os::Windows, Arch::X64, &[]).as_deref(),
        Some("x86_64-pc-windows-msvc")
    );
    assert_eq!(
        triple("linux", Os::Linux, Arch::Arm64, &[]).as_deref(),
        Some("aarch64-unknown-linux-gnu")
    );
    assert_eq!(
        triple("macos", Os::MacOs, Arch::Arm64, &[]).as_deref(),
        Some("aarch64-apple-darwin")
    );
    // `host` builds for the machine without a `--target`; an explicit one wins.
    assert_eq!(triple("host", Os::Windows, Arch::Arm64, &[]), None);
    assert_eq!(
        triple(
            "windows",
            Os::Windows,
            Arch::Arm64,
            &["--target", "x86_64-pc-windows-msvc"]
        )
        .as_deref(),
        Some("x86_64-pc-windows-msvc")
    );
}
