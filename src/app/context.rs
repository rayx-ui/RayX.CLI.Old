//! What the app pipeline knows about the project it works on: the Cargo workspace around the
//! selected app, the resolved version pins and the host. Nothing here depends on where `rayx` was
//! built or on the RayX repository layout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::host::{CommandSpec, HostFacts, Runner};
use crate::project::{Dependency, Package, Pins, Project};

/// The workspace, pins and host an app command runs against.
#[derive(Clone, Debug)]
pub struct ProjectContext {
    pub project: Project,
    pub pins: Pins,
    pub host: HostFacts,
}

impl ProjectContext {
    /// A context for an already discovered project.
    pub fn new(project: Project, host: HostFacts) -> Self {
        let pins = Pins::resolve(Some(&project));
        Self {
            project,
            pins,
            host,
        }
    }

    /// Discovers the project of `app_dir` (relative to `cwd`), or of `cwd` itself.
    pub fn discover(cwd: &Path, app_dir: Option<&Path>) -> Result<Self> {
        // Discovery only reads, so it runs for real whatever mode the app command is in.
        let project = Project::discover(&mut Runner::print(), cwd, app_dir)
            .map_err(|error| anyhow!("{error}"))?;
        Ok(Self::new(project, crate::host::facts()))
    }

    /// The Cargo workspace root: `artifacts/`, `artifacts-temp/` and `target/` live under it.
    pub fn workspace_root(&self) -> &Path {
        &self.project.workspace_root
    }

    /// The workspace member whose manifest directory is `dir`.
    pub fn package_in(&self, dir: &Path) -> Option<&Package> {
        let wanted = comparable(dir);
        self.project
            .packages
            .iter()
            .find(|package| comparable(package.dir()) == wanted)
    }

    /// The manifest directory of a package in the app's resolved dependency graph, from
    /// `cargo metadata` of the app (for example the `gpux-fonts` package).
    pub fn resolved_package_dir(&self, package: &str, app_manifest: &Path) -> Result<PathBuf> {
        self.find_resolved_package_dir(package, app_manifest)?
            .ok_or_else(|| {
                anyhow!(
                    "the dependency graph of {} has no `{package}` package",
                    app_manifest.display()
                )
            })
    }

    /// Like [`ProjectContext::resolved_package_dir`], `None` when the graph has no such package.
    pub fn find_resolved_package_dir(
        &self,
        package: &str,
        app_manifest: &Path,
    ) -> Result<Option<PathBuf>> {
        let outcome = Runner::print()
            .query(
                &CommandSpec::new("cargo")
                    .args(["metadata", "--format-version", "1", "--manifest-path"])
                    .arg(app_manifest.display().to_string())
                    .env("RUSTUP_AUTO_INSTALL", "0"),
            )
            .map_err(|error| anyhow!("{error}"))?;
        if !outcome.is_success() {
            bail!(
                "cargo metadata failed for {}: {}",
                app_manifest.display(),
                outcome.stderr.trim()
            );
        }
        let json: serde_json::Value =
            serde_json::from_str(&outcome.stdout).context("unexpected `cargo metadata` output")?;
        Ok(json["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|candidate| candidate["name"] == package)
            .and_then(|candidate| candidate["manifest_path"].as_str())
            .and_then(|manifest| Path::new(manifest).parent().map(Path::to_path_buf)))
    }
}

/// A path in a form that two spellings of one directory share.
fn comparable(path: &Path) -> String {
    let text = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

/// The inline `Cargo.toml` table that declares `dependency` as the app does: the same source (its
/// absolute path, its git repository and revision, or its registry version), `package` for a
/// rename, `default-features = false`, `features` and `optional` (when `optional` is set).
pub fn dependency_toml(dependency: &Dependency, optional: bool) -> Result<String> {
    let mut fields: Vec<String> = Vec::new();
    match (&dependency.path, dependency.source.as_deref()) {
        (Some(path), _) => fields.push(format!("path = {}", toml_string(&cargo_path(path)))),
        (None, Some(source)) if source.starts_with("git+") => {
            let rest = source.trim_start_matches("git+");
            let rest = rest.split('#').next().unwrap_or(rest);
            let (url, query) = rest.split_once('?').unwrap_or((rest, ""));
            fields.push(format!("git = {}", toml_string(url)));
            if let Some((kind, value)) = query.split_once('=')
                && matches!(kind, "rev" | "branch" | "tag")
            {
                fields.push(format!("{kind} = {}", toml_string(value)));
            }
        }
        (None, Some(source)) if source.contains("crates.io") => {
            fields.push(format!("version = {}", toml_string(&dependency.req)));
        }
        (None, Some(source)) => bail!(
            "the dependency `{}` comes from {source}, which generated entry crates cannot copy",
            dependency.name
        ),
        (None, None) => bail!("the dependency `{}` has no source", dependency.name),
    }
    if dependency.rename.is_some() {
        fields.push(format!("package = {}", toml_string(&dependency.name)));
    }
    if !dependency.default_features {
        fields.push("default-features = false".to_string());
    }
    if !dependency.features.is_empty() {
        let features = dependency
            .features
            .iter()
            .map(|feature| toml_string(feature))
            .collect::<Vec<_>>()
            .join(", ");
        fields.push(format!("features = [{features}]"));
    }
    if optional {
        fields.push("optional = true".to_string());
    }
    Ok(format!("{{ {} }}", fields.join(", ")))
}

/// `text` as a TOML basic string.
pub(crate) fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

/// A path as written in a generated `Cargo.toml`: no verbatim prefix, forward slashes.
pub fn cargo_path(path: &Path) -> String {
    crate::app::fs_util::command_path(path)
        .to_string_lossy()
        .replace('\\', "/")
}
