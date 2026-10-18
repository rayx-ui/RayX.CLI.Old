//! The 1.x release policy: no commit is marked breaking, a break is a `feat`/`fix` commit with a
//! `Migration:` footer the changelog renders, and releases stay 1.x until a second consumer appears.

use std::path::PathBuf;

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn release_plz() -> toml::Table {
    read("release-plz.toml").parse().expect("release-plz.toml")
}

#[test]
fn semver_checks_stay_off_and_only_feat_fix_perf_start_a_release() {
    let config = release_plz();
    let workspace = config["workspace"].as_table().expect("[workspace]");
    assert_eq!(workspace["semver_check"].as_bool(), Some(false));
    assert_eq!(
        workspace["release_commits"].as_str(),
        Some("^(feat|fix|perf)")
    );
}

#[test]
fn the_changelog_body_prints_migration_footers_under_their_commit() {
    let config = release_plz();
    let body = config["changelog"]["body"].as_str().expect("body");
    let commit_loop = body
        .split("{% for commit in commits %}")
        .nth(1)
        .expect("the commit loop");
    let footers = commit_loop
        .find("{%- for footer in commit.footers")
        .expect("a loop over the commit's footers");
    let message = commit_loop
        .rfind("{{ commit.message }}")
        .expect("the commit line");
    assert!(message < footers, "footers come after the commit line");
    let footer_loop = &commit_loop[footers..];
    assert!(
        footer_loop.contains("footer.token == \"Migration\""),
        "{footer_loop}"
    );
    assert!(
        footer_loop.contains("  Migration: {{ footer.value }}"),
        "an indented `Migration: <value>` line: {footer_loop}"
    );
    assert!(
        footer_loop.find("{%- endfor -%}") < footer_loop.find("{% endfor -%}"),
        "the footer loop closes inside the commit loop: {footer_loop}"
    );
}

#[test]
fn releasing_md_states_the_1x_policy_and_drops_the_breaking_advice() {
    let releasing = read("docs/releasing.md");
    let text = flat(&releasing);
    for needle in [
        "## The 1.x policy",
        "No commit is marked breaking",
        "`BREAKING CHANGE:` footer",
        "`Migration:` footer",
        "releases stay 1.x",
        "proposes `2.0.0` means a stray marker",
        "reword the commit",
        "--force-with-lease",
        "The policy ends when a second consumer appears",
    ] {
        assert!(text.contains(needle), "docs/releasing.md lacks `{needle}`");
    }
    assert!(
        !text.contains("| major (`1.0.0` → `2.0.0`)"),
        "the commit table has no major row"
    );
    assert!(
        !text.contains("Write a breaking change as `feat!`"),
        "the feat! advice is gone"
    );
    assert!(!read("release-plz.toml").contains("A breaking change is written `feat!`"));
}

#[test]
fn claude_and_agents_state_the_policy_in_a_releases_paragraph() {
    let claude = read("CLAUDE.md");
    assert_eq!(claude, read("AGENTS.md"));
    let releases = claude
        .split("\n## Releases\n")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .map(flat)
        .expect("a Releases section");
    for needle in [
        "stays on 1.x",
        "no `!`",
        "no `BREAKING CHANGE:` footer",
        "`Migration:` footer",
        "proposes 2.0.0 means a stray marker",
        "second consumer",
    ] {
        assert!(releases.contains(needle), "Releases lacks `{needle}`");
    }
}
