//! Pins come from the project or the built-in defaults, nothing else: no member name selects a
//! profile, and doctor reports only the `project` and `default` sources.

use std::fs;
use std::path::Path;

use rayx_cli::doctor::report_for;
use rayx_cli::host::path_env::{MemoryPathStore, UserPath};
use rayx_cli::host::{Arch, Distro, HostFacts, Os, Outcome, Runner};
use rayx_cli::project::pins::DEFAULTS;
use rayx_cli::project::{Discovery, PinSource, Pins, Project};
use rayx_cli::setup::{FakeMachine, PlanEnv};

/// A workspace with a `tooling/testkit` member called `member_name` and the given root metadata.
fn workspace(member_name: &str, metadata: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(dir.path().join("tooling/testkit/src")).expect("dirs");
    fs::write(
        dir.path().join("Cargo.toml"),
        format!("[workspace]\nresolver = \"2\"\nmembers = [\"tooling/testkit\"]\n{metadata}"),
    )
    .expect("manifest");
    fs::write(
        dir.path().join("tooling/testkit/Cargo.toml"),
        format!("[package]\nname = \"{member_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )
    .expect("member manifest");
    fs::write(dir.path().join("tooling/testkit/src/lib.rs"), "").expect("lib");
    dir
}

const ANDROID_PINS: &str = "\n[workspace.metadata.rayx]\nandroid-platform = \"android-36\"\nandroid-build-tools = \"36.0.0\"\nandroid-ndk = \"28.0.12674087\"\n";

fn discover_with_cargo(dir: &Path) -> Project {
    Project::discover(&mut Runner::execute(), dir, None).expect("discovers")
}

fn discover_from_files(dir: &Path) -> Project {
    let mut runner = Runner::record().respond(|_| true, Outcome::failure(127));
    let project = Project::discover(&mut runner, dir, None).expect("from files");
    assert_eq!(project.discovery, Discovery::Files);
    project
}

fn all_sources(pins: &Pins) -> Vec<PinSource> {
    let mut sources = vec![
        pins.web_toolchain.source,
        pins.node.source,
        pins.jdk.source,
        pins.playwright.source,
        pins.android_ndk.source,
        pins.android_platform.source,
        pins.android_ndk_api.source,
        pins.android_system_image.source,
        pins.ubuntu.source,
    ];
    sources.extend(pins.android_build_tools.as_ref().map(|p| p.source));
    sources
}

#[test]
fn a_member_named_gpux_testkit_selects_nothing_and_the_defaults_apply() {
    let dir = workspace("gpux-testkit", "");
    for project in [
        discover_with_cargo(dir.path()),
        discover_from_files(dir.path()),
    ] {
        let pins = Pins::resolve(Some(&project));
        assert_eq!(pins.android_platform.value, "android-34");
        assert_eq!(pins.android_platform.value, DEFAULTS.android_platform);
        assert!(pins.android_build_tools.is_none());
        assert_eq!(pins.android_ndk.value, "28.0.12674087");
        assert_eq!(pins.android_system_image.value, "android-34;google_apis");
        assert!(
            all_sources(&pins)
                .iter()
                .all(|source| *source == PinSource::Default),
            "{pins:?}"
        );
    }
}

#[test]
fn declared_android_pins_come_from_the_project_whatever_the_members_are_called() {
    for member in ["gpux-testkit", "testkit"] {
        let dir = workspace(member, ANDROID_PINS);
        for project in [
            discover_with_cargo(dir.path()),
            discover_from_files(dir.path()),
        ] {
            let pins = Pins::resolve(Some(&project));
            assert_eq!(pins.android_platform.value, "android-36");
            assert_eq!(pins.android_platform.source, PinSource::Project);
            let build_tools = pins.android_build_tools.as_ref().expect("build tools");
            assert_eq!(build_tools.value, "36.0.0");
            assert_eq!(build_tools.source, PinSource::Project);
            assert_eq!(pins.android_ndk.value, "28.0.12674087");
            assert_eq!(pins.android_ndk.source, PinSource::Project);
            assert_eq!(pins.android_system_image.value, "android-36;google_apis");
            assert_eq!(pins.android_system_image.source, PinSource::Project);
            assert_eq!(pins.node.source, PinSource::Default);
        }
    }
}

#[test]
fn a_pin_source_is_project_or_default() {
    assert_eq!(PinSource::Project.as_str(), "project");
    assert_eq!(PinSource::Default.as_str(), "default");
    // Exhaustive: a third source would not compile here.
    for source in [PinSource::Project, PinSource::Default] {
        match source {
            PinSource::Project | PinSource::Default => {}
        }
    }
}

#[test]
fn doctor_shows_only_project_and_default_sources() {
    let dir = workspace("gpux-testkit", ANDROID_PINS);
    let project = discover_with_cargo(dir.path());
    let env = PlanEnv {
        host: HostFacts {
            os: Os::Linux,
            arch: Arch::X64,
            emulated: false,
            wsl: false,
            distro: Some(Distro::parse(
                "ID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n",
            )),
        },
        pins: Pins::resolve(Some(&project)),
        project_root: None,
        tools_node: None,
        yes: false,
    };
    let mut path = UserPath::Windows(Box::new(MemoryPathStore::new(None)));
    let report = report_for(
        &env,
        &mut Runner::record().with_os(Os::Linux),
        &FakeMachine::new().with_home("/home/dev"),
        &mut path,
        None,
    );

    let json = report.to_json();
    let pins = json["pins"].as_object().expect("pins object");
    assert!(!pins.is_empty());
    for (name, pin) in pins {
        let source = pin["source"].as_str().unwrap_or_default();
        assert!(
            source == "project" || source == "default",
            "{name}: {source}"
        );
    }
    assert_eq!(json["pins"]["android-platform"]["source"], "project");
    assert_eq!(json["pins"]["node"]["source"], "default");

    let text = report.to_text();
    assert!(
        text.contains("android-platform: android-36 (project)"),
        "{text}"
    );
    assert!(!text.contains("checkout)"), "{text}");
}
