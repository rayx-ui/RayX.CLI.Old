//! The app's resolved dependency graph: the packages reachable from the app over normal
//! (non-dev, non-build) dependency edges for a feature selection, the app included, as
//! `cargo metadata` resolves them. Package assets, `[patch]` selection and `android_java_dirs`
//! all read it, so "in the app's graph" means the same thing everywhere.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::app::CargoMetadataContext;
use crate::host::{CommandSpec, Runner};

/// A package of the app's graph.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphPackage {
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
    /// The package's `[package.metadata]` table (`null` when it has none).
    pub metadata: Value,
}

impl GraphPackage {
    /// The directory holding the package's manifest.
    pub fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }
}

/// The resolved graph of one app.
#[derive(Clone, Debug)]
pub struct AppGraph {
    /// The app's own package id.
    pub app_id: String,
    /// The reachable packages in breadth-first order from the app, the app first.
    pub packages: Vec<GraphPackage>,
}

impl AppGraph {
    /// Runs `cargo metadata` for `app_manifest` with the feature selection of `context`.
    pub fn resolve(app_manifest: &Path, context: &CargoMetadataContext) -> Result<Self> {
        let mut spec = CommandSpec::new("cargo")
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(app_manifest.display().to_string())
            .env("RUSTUP_AUTO_INSTALL", "0");
        for arg in context.args()? {
            spec = spec.arg(arg);
        }
        let outcome = Runner::print()
            .query(&spec)
            .map_err(|error| anyhow!("{error}"))?;
        if !outcome.is_success() {
            bail!(
                "cargo metadata failed for {}: {}",
                app_manifest.display(),
                outcome.stderr.trim()
            );
        }
        let metadata = serde_json::from_str::<CargoMetadata>(&outcome.stdout)
            .with_context(|| format!("parsing Cargo metadata for {}", app_manifest.display()))?;
        Self::from_metadata(app_manifest, metadata)
    }

    fn from_metadata(app_manifest: &Path, metadata: CargoMetadata) -> Result<Self> {
        let app_manifest = fs::canonicalize(app_manifest)
            .with_context(|| format!("canonicalizing {}", app_manifest.display()))?;
        let packages = metadata
            .packages
            .into_iter()
            .map(|package| (package.id.clone(), package))
            .collect::<BTreeMap<_, _>>();
        let app_id = packages
            .values()
            .find(|package| {
                fs::canonicalize(&package.manifest_path).ok().as_deref()
                    == Some(app_manifest.as_path())
            })
            .map(|package| package.id.clone())
            .ok_or_else(|| {
                anyhow!(
                    "Cargo metadata did not contain selected app manifest {}",
                    app_manifest.display()
                )
            })?;
        let resolve = metadata
            .resolve
            .ok_or_else(|| anyhow!("Cargo metadata did not include a resolve graph"))?;
        if let Some(root) = resolve.root.as_deref()
            && root != app_id
        {
            bail!(
                "Cargo metadata resolve.root {root} does not match selected app package {app_id}"
            );
        }
        let nodes = resolve
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        if !nodes.contains_key(&app_id) {
            bail!("Cargo metadata resolve graph has no node for selected app {app_id}");
        }

        let mut queue = VecDeque::from([app_id.clone()]);
        let mut visited = BTreeSet::new();
        let mut reachable = Vec::new();
        while let Some(id) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let node = nodes.get(&id).ok_or_else(|| {
                anyhow!("Cargo metadata resolve graph references missing node {id}")
            })?;
            for dependency in &node.deps {
                if dependency.dep_kinds.iter().any(|kind| kind.kind.is_none()) {
                    queue.push_back(dependency.pkg.clone());
                }
            }
            let package = packages.get(&id).ok_or_else(|| {
                anyhow!("Cargo metadata resolve graph references missing package {id}")
            })?;
            reachable.push(GraphPackage {
                id: package.id.clone(),
                name: package.name.clone(),
                manifest_path: package.manifest_path.clone(),
                metadata: package.metadata.clone(),
            });
        }
        Ok(Self {
            app_id,
            packages: reachable,
        })
    }

    /// The packages other than the app.
    pub fn dependencies(&self) -> impl Iterator<Item = &GraphPackage> {
        self.packages
            .iter()
            .filter(move |package| package.id != self.app_id)
    }

    /// Whether a package with this name is in the graph.
    pub fn contains_name(&self, name: &str) -> bool {
        self.packages.iter().any(|package| package.name == name)
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    resolve: Option<CargoResolve>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
struct CargoResolve {
    root: Option<String>,
    nodes: Vec<CargoNode>,
}

#[derive(Debug, Deserialize)]
struct CargoNode {
    id: String,
    #[serde(default)]
    deps: Vec<CargoNodeDependency>,
}

#[derive(Debug, Deserialize)]
struct CargoNodeDependency {
    pkg: String,
    #[serde(default)]
    dep_kinds: Vec<CargoDependencyKind>,
}

#[derive(Debug, Deserialize)]
struct CargoDependencyKind {
    kind: Option<String>,
}
