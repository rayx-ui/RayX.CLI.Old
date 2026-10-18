//! The web page is the app's: `platform/web/index.html` is copied as `index.html` with
//! `__RAYX_MODULE__`, `__RAYX_TITLE__` and `__RAYX_DIAGNOSTICS__` replaced, an unknown
//! `__RAYX_*__` token is refused before anything is built, and without a page `rayx` writes a
//! neutral fallback with no diagnostics or library code.

mod common;

use common::Workspace;
use rayx_cli::app::{AppFeatureSelection, FALLBACK_WEB_PAGE, render_web_page};

const PAGE: &str = "apps/demo/platform/web/index.html";

fn diagnostics() -> AppFeatureSelection {
    AppFeatureSelection {
        features: vec!["rayx_diagnostics".into()],
        ..AppFeatureSelection::default()
    }
}

/// The fixture app with a `rayx_diagnostics` feature, so diagnostics builds validate.
fn workspace() -> Workspace {
    let workspace = Workspace::fixture("app-workspace");
    let manifest = workspace
        .read("apps/demo/Cargo.toml")
        .replace("[features]\n", "[features]\nrayx_diagnostics = []\n");
    workspace.write("apps/demo/Cargo.toml", &manifest);
    workspace
}

const APP_PAGE: &str = "<!doctype html>\n<html>\n<head><title>__RAYX_TITLE__ | __RAYX_TITLE__</title></head>\n<body data-x=\"__rayx_lower__\">\n  <script type=\"module\">\n    globalThis.appDiagnostics = __RAYX_DIAGNOSTICS__;\n    import init, { run } from '__RAYX_MODULE__';\n    // again: __RAYX_MODULE__ and __RAYX_DIAGNOSTICS__\n    await init(); await run();\n  </script>\n</body>\n</html>\n";

#[test]
fn the_apps_page_is_copied_with_the_three_placeholders_replaced() {
    let workspace = workspace();
    workspace.write(PAGE, APP_PAGE);
    let app = workspace.app("apps/demo");

    let production = render_web_page(&app, &AppFeatureSelection::default()).expect("page");
    let expected = APP_PAGE
        .replace("__RAYX_TITLE__", "demo")
        .replace("__RAYX_MODULE__", "/rayx_demo_web.js")
        .replace("__RAYX_DIAGNOSTICS__", "false");
    assert_eq!(production, expected, "everything else is byte for byte");
    assert!(production.contains("<title>demo | demo</title>"));
    assert_eq!(production.matches("/rayx_demo_web.js").count(), 2);

    let diagnostic = render_web_page(&app, &diagnostics()).expect("page");
    assert_eq!(
        diagnostic,
        expected.replace("false", "true"),
        "only the diagnostics value differs"
    );
    assert!(!diagnostic.contains("__RAYX_"));
}

#[test]
fn all_features_turn_the_diagnostics_value_on() {
    let workspace = workspace();
    workspace.write(PAGE, "<p>__RAYX_DIAGNOSTICS__</p>\n");
    let app = workspace.app("apps/demo");
    let all = AppFeatureSelection {
        all_features: true,
        ..AppFeatureSelection::default()
    };
    assert_eq!(render_web_page(&app, &all).expect("page"), "<p>true</p>\n");
}

#[test]
fn an_unknown_placeholder_is_refused_with_its_name_the_page_and_the_line() {
    let workspace = workspace();
    workspace.write(
        PAGE,
        "<!doctype html>\n<title>__RAYX_TITLE__</title>\n<script type=\"module\">import init from '__RAYX_MODUEL__';</script>\n",
    );
    let app = workspace.app("apps/demo");
    let error = render_web_page(&app, &AppFeatureSelection::default()).expect_err("typo");
    let text = format!("{error:#}");
    assert!(text.contains("__RAYX_MODUEL__"), "{text}");
    assert!(
        text.contains("platform/web/index.html") || text.contains("platform\\web\\index.html"),
        "{text}"
    );
    assert!(text.contains("line 3"), "{text}");
}

#[test]
fn a_build_with_an_unknown_placeholder_fails_before_anything_is_built_or_written() {
    let workspace = workspace();
    workspace.write(PAGE, "<p>__RAYX_NOPE__</p>\n");
    let error = rayx_cli::app::run_unchecked(vec![
        workspace.path("apps/demo").display().to_string(),
        "build".into(),
        "wasm".into(),
    ])
    .expect_err("refused");
    assert!(format!("{error:#}").contains("__RAYX_NOPE__"), "{error:#}");
    assert!(
        !workspace
            .path("artifacts/apps/demo/wasm/release/index.html")
            .exists()
    );
    assert!(
        !workspace.path("target/wasm32-unknown-unknown").exists(),
        "no cargo build ran"
    );
}

#[test]
fn without_an_app_page_the_neutral_fallback_is_written_for_every_build() {
    let workspace = workspace();
    let app = workspace.app("apps/demo");
    let production = render_web_page(&app, &AppFeatureSelection::default()).expect("page");
    let diagnostic = render_web_page(&app, &diagnostics()).expect("page");
    assert_eq!(production, diagnostic, "the same page for both settings");

    for required in [
        "<base href=\"/\" />",
        "<meta charset=\"utf-8\" />",
        "name=\"viewport\"",
        "<link rel=\"icon\"",
        "<title>demo WASM</title>",
        "import init, { run } from '/rayx_demo_web.js';",
        "await init();",
        "await run();",
        "document.body.dataset.rayxWasm = 'started'",
    ] {
        assert!(production.contains(required), "{required} in {production}");
    }
    assert_eq!(production.matches("<script").count(), 1);
    for absent in [
        "__rayx",
        "HtmlIsland",
        "html-island",
        "webgpu-status",
        "wasm-thread-status",
        "navigator.gpu",
        "crossOriginIsolated",
        "SharedArrayBuffer",
        "__RAYX_",
    ] {
        assert!(!production.contains(absent), "{absent} in {production}");
    }
    assert!(FALLBACK_WEB_PAGE.contains("__RAYX_MODULE__"));
}
