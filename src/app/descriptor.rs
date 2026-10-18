use crate::app::args::{take_bool_flag, take_flag_value};
use crate::app::context::{ProjectContext, dependency_toml, toml_string};
use crate::app::fs_util::command_path;
use crate::app::graph::AppGraph;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    pub fn cargo_arg(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some("--release"),
        }
    }

    pub fn gradle_task(self) -> &'static str {
        match self {
            Self::Debug => "assembleDebug",
            Self::Release => "assembleRelease",
        }
    }

    pub fn xcode_configuration(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Release => "Release",
        }
    }

    pub fn is_release(self) -> bool {
        self == Self::Release
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CargoMetadataContext {
    pub target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
}

impl CargoMetadataContext {
    pub fn apply_to(&self, command: &mut Command) -> Result<()> {
        command.args(self.args()?);
        Ok(())
    }

    /// The `cargo` arguments of this selection.
    pub fn args(&self) -> Result<Vec<String>> {
        if self.all_features && self.no_default_features {
            bail!("Cargo metadata context cannot combine all-features with no-default-features");
        }
        let mut args = Vec::new();
        if let Some(target) = self.target.as_deref() {
            args.extend(["--filter-platform".to_string(), target.to_string()]);
        }
        let features = self
            .features
            .iter()
            .map(|feature| feature.trim())
            .filter(|feature| !feature.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        if !features.is_empty() {
            args.push("--features".to_string());
            args.push(features.into_iter().collect::<Vec<_>>().join(","));
        }
        if self.all_features {
            args.push("--all-features".to_string());
        }
        if self.no_default_features {
            args.push("--no-default-features".to_string());
        }
        Ok(args)
    }
}

/// Normalized Cargo feature switches shared by every xtask app action.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppFeatureSelection {
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
}

impl AppFeatureSelection {
    pub fn take_from_args(args: &mut Vec<String>) -> Result<Self> {
        let mut app_args = args
            .iter()
            .position(|arg| arg == "--")
            .map(|separator| args.split_off(separator))
            .unwrap_or_default();
        let all_features = take_bool_flag(args, "--all-features");
        let no_default_features = take_bool_flag(args, "--no-default-features");
        if all_features && no_default_features {
            bail!("--all-features cannot be combined with --no-default-features");
        }

        let mut features = Vec::new();
        while let Some(feature_set) = take_flag_value(args, "--features")? {
            let normalized = feature_set
                .split([',', ' ', '\t'])
                .map(str::trim)
                .filter(|feature| !feature.is_empty())
                .collect::<Vec<_>>();
            if normalized.is_empty() {
                bail!("--features expects at least one feature name");
            }
            features.extend(normalized.into_iter().map(str::to_owned));
        }
        let selection = Self {
            features,
            all_features,
            no_default_features,
        };
        args.append(&mut app_args);
        Ok(selection)
    }

    pub fn default_features_enabled(&self) -> bool {
        !self.no_default_features
    }

    pub fn normalized_features(&self) -> Vec<String> {
        self.features
            .iter()
            .flat_map(|value| value.split([',', ' ', '\t']))
            .map(str::trim)
            .filter(|feature| !feature.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    pub fn validate_for(&self, manifest: &Path) -> Result<()> {
        let value = read_manifest(manifest)?;
        let known = value
            .get("features")
            .and_then(Value::as_table)
            .map(|features| features.keys().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for feature in self.normalized_features() {
            if !known.contains(&feature) {
                bail!(
                    "app manifest {} does not define feature {feature}",
                    manifest.display()
                );
            }
        }
        Ok(())
    }

    pub fn metadata_context(&self) -> CargoMetadataContext {
        CargoMetadataContext {
            features: self.normalized_features(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            ..CargoMetadataContext::default()
        }
    }

    pub fn apply_to_cargo(&self, command: &mut Command) -> Result<()> {
        self.metadata_context().apply_to(command)
    }
}

#[derive(Clone, Debug)]
pub struct AppDescriptor {
    pub root: PathBuf,
    pub slug: String,
    pub lib_name: String,
    pub rust_lib_name: String,
    pub android_application_id: Option<String>,
    pub android_activity: Option<String>,
    pub ios_project: Option<String>,
    pub ios_scheme: Option<String>,
    pub ios_bundle_id: Option<String>,
    pub ios_app_name: Option<String>,
    pub diagnostic_harness: bool,
    /// The workspace, pins and host this app is built in.
    pub context: ProjectContext,
}

impl AppDescriptor {
    /// The app in `app_dir`, a path absolute or relative to the current directory.
    pub fn resolve(context: &ProjectContext, app_dir: &str) -> Result<Self> {
        let base = std::env::current_dir().context("reading the current directory")?;
        Self::resolve_from(context, &base, app_dir)
    }

    /// The app in `app_dir`, a path absolute or relative to `base`.
    pub fn resolve_from(context: &ProjectContext, base: &Path, app_dir: &str) -> Result<Self> {
        let requested = PathBuf::from(app_dir);
        let root = if requested.is_absolute() {
            requested
        } else {
            base.join(requested)
        };
        let root = fs::canonicalize(&root)
            .with_context(|| format!("app directory {} was not found", root.display()))?;
        if !root.is_dir() {
            bail!("{} is not an app directory", root.display());
        }

        let manifest = root.join("Cargo.toml");
        if !manifest.is_file() {
            bail!("app directory {} has no Cargo.toml", root.display());
        }
        let manifest_value = read_manifest(&manifest)?;
        let package_name = package_name_from_value(&manifest_value, &manifest)?;
        let slug = metadata_string(&manifest_value, "app_name")
            .or_else(|| metadata_string(&manifest_value, "app_slug"))
            .unwrap_or_else(|| package_name.clone());
        let rust_lib_name = lib_name_from_value(&manifest_value)
            .unwrap_or_else(|| rust_crate_file_stem(&package_name));
        let lib_name = metadata_string(&manifest_value, "native_library")
            .or_else(|| metadata_string(&manifest_value, "android_native_library"))
            .unwrap_or_else(|| rust_lib_name.clone());

        Ok(Self {
            root,
            slug: sanitize_slug(&slug),
            lib_name,
            rust_lib_name,
            android_application_id: metadata_string(&manifest_value, "android_application_id"),
            android_activity: metadata_string(&manifest_value, "android_activity"),
            ios_project: metadata_string(&manifest_value, "ios_project"),
            ios_scheme: metadata_string(&manifest_value, "ios_scheme"),
            ios_bundle_id: metadata_string(&manifest_value, "ios_bundle_id"),
            ios_app_name: metadata_string(&manifest_value, "ios_app_name"),
            diagnostic_harness: metadata_bool(&manifest_value, "diagnostic_harness"),
            context: context.clone(),
        })
    }

    pub fn manifest(&self) -> PathBuf {
        self.root.join("Cargo.toml")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// `[package.metadata.rayx] playwright_specs`: the spec files or directories, relative to the
    /// Playwright package, that `test wasm` runs for this app.
    pub fn playwright_specs(&self) -> Result<Option<Vec<String>>> {
        Ok(metadata_string_array(
            &read_manifest(&self.manifest())?,
            "playwright_specs",
        ))
    }

    /// `[package.metadata.rayx] playwright_projects`: the Playwright projects `test wasm` runs.
    pub fn playwright_projects(&self) -> Result<Option<Vec<String>>> {
        Ok(metadata_string_array(
            &read_manifest(&self.manifest())?,
            "playwright_projects",
        ))
    }

    /// Directory holding bootstrap app content such as `app_settings.json`:
    /// `[package.metadata.rayx] app_content_root`, or the app root itself.
    pub fn app_content_root(&self) -> Result<PathBuf> {
        let manifest_value = read_manifest(&self.manifest())?;
        let root = match metadata_string(&manifest_value, "app_content_root") {
            Some(root) => resolve_app_path(&self.root, root),
            None => self.root.clone(),
        };
        Ok(root)
    }

    pub fn asset_roots(&self) -> Result<Vec<PathBuf>> {
        let manifest_value = read_manifest(&self.manifest())?;
        let roots = metadata_string_array(&manifest_value, "asset_roots")
            .or_else(|| metadata_string_array(&manifest_value, "assets_roots"))
            .map(|roots| {
                roots
                    .into_iter()
                    .map(|root| resolve_app_path(&self.root, root))
                    .collect::<Vec<_>>()
            })
            .or_else(|| {
                metadata_string(&manifest_value, "assets_root")
                    .map(|root| vec![resolve_app_path(&self.root, root)])
            })
            .unwrap_or_else(|| vec![self.assets_dir()]);
        Ok(roots)
    }

    pub fn asset_manifest_path(&self) -> Result<PathBuf> {
        let manifest_value = read_manifest(&self.manifest())?;
        let manifest_path = metadata_string(&manifest_value, "wasm_manifest_path")
            .unwrap_or_else(|| "index.json".to_string());
        let manifest_path = PathBuf::from(manifest_path);
        if manifest_path.is_absolute() {
            Ok(manifest_path)
        } else {
            let root = self
                .asset_roots()?
                .into_iter()
                .next()
                .unwrap_or_else(|| self.assets_dir());
            Ok(root.join(manifest_path))
        }
    }

    #[cfg(test)]
    pub fn platform_manifest(&self, target: &str) -> PathBuf {
        self.root.join("platform").join(target).join("Cargo.toml")
    }

    pub fn android_rust_manifest(&self, features: &AppFeatureSelection) -> Result<PathBuf> {
        self.write_generated_android_rust_manifest(features)
    }

    pub fn ios_rust_manifest(&self, features: &AppFeatureSelection) -> Result<PathBuf> {
        self.write_generated_ios_rust_manifest(features)
    }

    pub fn android_gradle_dir(&self) -> PathBuf {
        self.root.join("platform").join("android").join("gradle")
    }

    pub fn require_android_gradle_dir(&self) -> Result<PathBuf> {
        let dir = self.android_gradle_dir();
        if dir.is_dir() {
            Ok(dir)
        } else {
            bail!(
                "app {} has no Android Gradle wrapper at {}",
                self.root.display(),
                dir.display()
            )
        }
    }

    pub fn ios_dir(&self) -> PathBuf {
        self.root.join("platform").join("ios")
    }

    pub fn require_ios_dir(&self) -> Result<PathBuf> {
        let dir = self.ios_dir();
        if dir.is_dir() {
            Ok(dir)
        } else {
            bail!(
                "app {} has no iOS wrapper at {}",
                self.root.display(),
                dir.display()
            )
        }
    }

    pub fn web_manifest(&self, features: &AppFeatureSelection) -> Result<PathBuf> {
        self.write_generated_web_rust_manifest(features)
    }

    pub fn web_module_base(&self) -> String {
        self.generated_web_lib_name()
    }

    /// Where packaged output goes: `artifacts/apps/<slug>` under the workspace root.
    pub fn artifact_root(&self) -> Result<PathBuf> {
        Ok(self
            .context
            .workspace_root()
            .join("artifacts")
            .join("apps")
            .join(&self.slug))
    }

    /// Generated files: `artifacts-temp/apps/<slug>` under the workspace root.
    pub fn temp_root(&self) -> Result<PathBuf> {
        Ok(self
            .context
            .workspace_root()
            .join("artifacts-temp")
            .join("apps")
            .join(&self.slug))
    }

    /// The Cargo target directory of the workspace.
    pub fn target_dir(&self) -> Result<PathBuf> {
        Ok(self.context.workspace_root().join("target"))
    }

    /// The workspace package of this app.
    pub fn package(&self) -> Result<&crate::project::Package> {
        self.context.package_in(&self.root).ok_or_else(|| {
            anyhow!(
                "{} is not a package of the workspace at {}",
                self.root.display(),
                self.context.workspace_root().display()
            )
        })
    }

    pub fn android_generated_jni_dir(&self) -> Result<PathBuf> {
        Ok(self
            .require_android_gradle_dir()?
            .join("app")
            .join("build")
            .join("generated")
            .join("rustJniLibs"))
    }

    fn generated_android_rust_dir(&self) -> Result<PathBuf> {
        Ok(self.temp_root()?.join("android").join("rust"))
    }

    fn write_generated_android_rust_manifest(
        &self,
        features: &AppFeatureSelection,
    ) -> Result<PathBuf> {
        self.write_generated_rust_manifest(
            self.generated_android_rust_dir()?,
            "android",
            &self.lib_name,
            &["cdylib"],
            false,
            features,
        )
    }

    fn generated_ios_rust_dir(&self) -> Result<PathBuf> {
        Ok(self.temp_root()?.join("ios").join("rust"))
    }

    fn write_generated_ios_rust_manifest(&self, features: &AppFeatureSelection) -> Result<PathBuf> {
        self.write_generated_rust_manifest(
            self.generated_ios_rust_dir()?,
            "ios",
            &self.lib_name,
            &["staticlib"],
            false,
            features,
        )
    }

    fn generated_web_rust_dir(&self) -> Result<PathBuf> {
        Ok(self.temp_root()?.join("wasm").join("rust"))
    }

    fn generated_web_lib_name(&self) -> String {
        format!("{}_web", self.rust_lib_name)
    }

    fn write_generated_web_rust_manifest(&self, features: &AppFeatureSelection) -> Result<PathBuf> {
        self.write_generated_rust_manifest(
            self.generated_web_rust_dir()?,
            "web",
            &self.generated_web_lib_name(),
            &["cdylib", "rlib"],
            false,
            features,
        )
    }

    fn write_generated_rust_manifest(
        &self,
        root: PathBuf,
        target: &str,
        lib_name: &str,
        crate_types: &[&str],
        app_default_features: bool,
        selected_features: &AppFeatureSelection,
    ) -> Result<PathBuf> {
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).with_context(|| format!("creating {}", src_dir.display()))?;

        let manifest = root.join("Cargo.toml");
        let app_manifest = self.manifest();
        let package_name = package_name_from_manifest(&app_manifest)?;
        let generated_default_features = if app_default_features {
            Vec::new()
        } else {
            selected_features.validate_for(&app_manifest)?;
            let mut features = if selected_features.all_features {
                manifest_feature_names(&app_manifest)?
            } else if selected_features.default_features_enabled() {
                manifest_default_features(&app_manifest)?
            } else {
                Vec::new()
            };
            features.extend(selected_features.normalized_features());
            let enabled_app_features = features
                .into_iter()
                .filter(|feature| !feature.starts_with("dep:") && !feature.contains('/'))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            enabled_app_features
                .iter()
                .map(|feature| format!("{}/{feature}", self.rust_lib_name))
                .collect::<Vec<_>>()
        };
        // Whether the project has a lock, before resolving the graph (which may write one).
        let workspace_lock = self.context.workspace_root().join("Cargo.lock");
        let workspace_has_lock = workspace_lock.is_file();
        let app_value = read_manifest(&app_manifest)?;
        let entry_dependencies = self.entry_dependencies(&app_value, &app_manifest)?;
        let app_features = app_value.get("features").and_then(Value::as_table);
        // Every app feature is forwarded; one that enables an optional entry dependency also
        // enables it in the entry crate. No feature name is special.
        let feature_lines = manifest_feature_names(&app_manifest)?
            .into_iter()
            .map(|feature| {
                let mut members = vec![format!("{}/{feature}", self.rust_lib_name)];
                let mut enabled = BTreeSet::new();
                for dependency in &entry_dependencies {
                    if dependency.optional
                        && enabled.insert(dependency.key().to_string())
                        && feature_enables_dependency(app_features, &feature, dependency.key())
                    {
                        members.push(format!("dep:{}", dependency.key()));
                    }
                }
                let members = members
                    .iter()
                    .map(|member| toml_string(member))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} = [{members}]", toml_key(&feature))
            })
            .collect::<Vec<_>>();
        // The crates `main.rs` uses directly, declared as the app declares them.
        let mut dependency_lines = String::new();
        let mut target_lines = std::collections::BTreeMap::<String, String>::new();
        for dependency in &entry_dependencies {
            let line = format!(
                "{} = {}\n",
                toml_key(dependency.key()),
                dependency_toml(dependency, dependency.optional)?
            );
            match &dependency.target {
                Some(target) => target_lines
                    .entry(target.clone())
                    .or_default()
                    .push_str(&line),
                None => dependency_lines.push_str(&line),
            }
        }
        let target_tables = target_lines
            .iter()
            .map(|(target, lines)| {
                format!("\n[target.{}.dependencies]\n{lines}", toml_string(target))
            })
            .collect::<String>();
        let app_path = cargo_toml_path(&self.root);
        let generated_web_dependency_lines = if target == "web" {
            let bindgen = self.context.pins.wasm_bindgen.as_ref().ok_or_else(|| {
                anyhow!(
                    "wasm-bindgen is not locked in {}: build once with `cargo build` so Cargo.lock names it",
                    self.context.workspace_root().join("Cargo.lock").display()
                )
            })?;
            format!(
                "wasm-bindgen = \"={}\"\nwasm-bindgen-futures = \"0.4\"\n",
                bindgen.value
            )
        } else {
            String::new()
        };
        let generated_patch_lines = self.generated_patch_tables(selected_features)?;
        let crate_type_line = crate_types
            .iter()
            .map(|crate_type| format!("\"{crate_type}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let app_dependency_line = if app_default_features {
            format!(
                "{} = {{ package = \"{}\", path = \"{}\" }}",
                self.rust_lib_name, package_name, app_path
            )
        } else {
            format!(
                "{} = {{ package = \"{}\", path = \"{}\", default-features = false }}",
                self.rust_lib_name, package_name, app_path
            )
        };
        let manifest_text = format!(
            r#"[package]
name = "{}-{}-entry"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
name = "{}"
crate-type = [{}]
path = "src/lib.rs"

[workspace]

[features]
default = [{}]
{}

[dependencies]
{}{}{}
{}{}"#,
            self.slug,
            target,
            lib_name,
            crate_type_line,
            generated_default_features
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", "),
            feature_lines.join("\n"),
            dependency_lines,
            generated_web_dependency_lines,
            app_dependency_line,
            target_tables,
            generated_patch_lines
        );
        fs::write(&manifest, manifest_text)
            .with_context(|| format!("writing {}", manifest.display()))?;
        // Each entry crate is a workspace of its own and would resolve the newest crates, which
        // can break a build the project's lockfile avoids; it starts from the project's lock.
        let lockfile = root.join("Cargo.lock");
        if lockfile.is_file() {
            fs::remove_file(&lockfile)
                .with_context(|| format!("removing stale {}", lockfile.display()))?;
        }
        if workspace_has_lock {
            fs::copy(&workspace_lock, &lockfile).with_context(|| {
                format!(
                    "seeding {} from {}",
                    lockfile.display(),
                    workspace_lock.display()
                )
            })?;
        }

        let main_path = cargo_toml_path(&self.root.join("src").join("main.rs"));
        let source = format!(
            r#"#![allow(dead_code)]

#[path = "{}"]
mod rayx_app_main;
"#,
            main_path
        );
        fs::write(src_dir.join("lib.rs"), source)
            .with_context(|| format!("writing {}", src_dir.join("lib.rs").display()))?;
        Ok(manifest)
    }

    /// `[package.metadata.rayx] entry_dependencies`: the app's normal dependency declarations the
    /// generated entry crates repeat, one per table that declares each listed key.
    fn entry_dependencies(
        &self,
        app_value: &Value,
        app_manifest: &Path,
    ) -> Result<Vec<crate::project::Dependency>> {
        let keys = match rayx_metadata(app_value).and_then(|rayx| rayx.get("entry_dependencies")) {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| {
                    anyhow!(
                        "package.metadata.rayx.entry_dependencies in {} must be an array of dependency keys",
                        app_manifest.display()
                    )
                })?,
        };
        let package = self.package()?;
        let mut dependencies = Vec::new();
        for key in keys.iter().collect::<BTreeSet<_>>() {
            let declarations = package.dependencies_by_key(key);
            if declarations.is_empty() {
                bail!(
                    "package.metadata.rayx.entry_dependencies lists `{key}`, which is not a normal dependency of {}",
                    app_manifest.display()
                );
            }
            dependencies.extend(declarations.into_iter().cloned());
        }
        Ok(dependencies)
    }

    /// The root `[patch]` tables the app's graph uses, for a generated entry crate.
    fn generated_patch_tables(&self, features: &AppFeatureSelection) -> Result<String> {
        let workspace_root = self.context.workspace_root();
        let manifest_path = workspace_root.join("Cargo.toml");
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let root_manifest: toml::Table = toml::from_str(&text)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        if root_manifest.get("patch").is_none() {
            return Ok(String::new());
        }
        let graph = AppGraph::resolve(&self.manifest(), &features.metadata_context())?;
        patch_tables_for_graph(&root_manifest, workspace_root, &graph)
    }

    /// Whether the app's `[features]` table defines `feature`.
    pub(crate) fn has_feature(&self, feature: &str) -> Result<bool> {
        manifest_has_feature(&self.manifest(), feature)
    }

    /// `[package.metadata.rayx] android_java_dirs` of every package in the app's graph for
    /// `features` (the app included): absolute, deduplicated and sorted. Each entry is relative to
    /// the declaring package's directory and must be an existing directory inside it.
    pub fn android_java_dirs(&self, features: &AppFeatureSelection) -> Result<Vec<PathBuf>> {
        let graph = AppGraph::resolve(&self.manifest(), &features.metadata_context())?;
        let mut dirs = BTreeSet::new();
        for package in &graph.packages {
            let Some(value) = package
                .metadata
                .get("rayx")
                .and_then(|rayx| rayx.get("android_java_dirs"))
            else {
                continue;
            };
            let entries = value
                .as_array()
                .and_then(|items| items.iter().map(|item| item.as_str()).collect::<Option<Vec<_>>>())
                .ok_or_else(|| {
                    anyhow!(
                        "package {} (package.metadata.rayx.android_java_dirs in {}) must be an array of relative directories",
                        package.name,
                        package.manifest_path.display()
                    )
                })?;
            let package_dir = fs::canonicalize(package.dir())
                .with_context(|| format!("resolving {}", package.dir().display()))?;
            for entry in entries {
                let invalid = |reason: &str| {
                    anyhow!(
                        "package {} declares android_java_dirs entry `{entry}`, which {reason} ({})",
                        package.name,
                        package.manifest_path.display()
                    )
                };
                if Path::new(entry).is_absolute()
                    || entry.starts_with('/')
                    || entry.starts_with('\\')
                {
                    return Err(invalid("is absolute; entries are relative to the package"));
                }
                let dir = fs::canonicalize(package_dir.join(entry))
                    .map_err(|_| invalid("does not exist"))?;
                if !dir.starts_with(&package_dir) {
                    return Err(invalid("leaves the package directory"));
                }
                if !dir.is_dir() {
                    return Err(invalid("is not a directory"));
                }
                dirs.insert(command_path(&dir));
            }
        }
        Ok(dirs.into_iter().collect())
    }

    pub fn android_launch_component(&self) -> Result<String> {
        let application_id = self.android_application_id.as_deref().ok_or_else(|| {
            anyhow!(
                "app {} must set package.metadata.rayx.android_application_id for Android launch",
                self.root.display()
            )
        })?;
        let activity = self
            .android_activity
            .as_deref()
            .unwrap_or("dev.rayx.mobile.RayXActivity");
        Ok(format!("{application_id}/{activity}"))
    }

    pub fn ios_scheme(&self) -> Result<&str> {
        self.ios_scheme.as_deref().ok_or_else(|| {
            anyhow!(
                "app {} must set package.metadata.rayx.ios_scheme for iOS builds",
                self.root.display()
            )
        })
    }

    pub fn ios_project(&self) -> Result<String> {
        if let Some(project) = self.ios_project.as_deref() {
            Ok(project.to_string())
        } else {
            Ok(format!("{}.xcodeproj", self.ios_scheme()?))
        }
    }

    pub fn ios_app_name(&self) -> Result<String> {
        Ok(self
            .ios_app_name
            .clone()
            .unwrap_or_else(|| self.ios_scheme().unwrap_or(&self.slug).to_string()))
    }

    pub fn ios_bundle_id(&self) -> Result<&str> {
        self.ios_bundle_id.as_deref().ok_or_else(|| {
            anyhow!(
                "app {} must set package.metadata.rayx.ios_bundle_id for iOS simulator launch",
                self.root.display()
            )
        })
    }
}

pub fn package_name_from_manifest(manifest: &Path) -> Result<String> {
    let value = read_manifest(manifest)?;
    package_name_from_value(&value, manifest)
}

pub fn rust_crate_file_stem(package_name: &str) -> String {
    package_name.replace('-', "_")
}

fn read_manifest(manifest: &Path) -> Result<Value> {
    let text = fs::read_to_string(manifest)
        .with_context(|| format!("reading manifest {}", manifest.display()))?;
    toml::from_str::<Value>(&text)
        .with_context(|| format!("parsing manifest {}", manifest.display()))
}

fn package_name_from_value(value: &Value, manifest: &Path) -> Result<String> {
    value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("manifest {} has no package.name", manifest.display()))
}

fn lib_name_from_value(value: &Value) -> Option<String> {
    value
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn metadata_string(value: &Value, key: &str) -> Option<String> {
    rayx_metadata(value)
        .and_then(|rayx| rayx.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn metadata_string_array(value: &Value, key: &str) -> Option<Vec<String>> {
    rayx_metadata(value)
        .and_then(|rayx| rayx.get(key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
}

fn metadata_bool(value: &Value, key: &str) -> bool {
    rayx_metadata(value)
        .and_then(|rayx| rayx.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn manifest_has_feature(manifest: &Path, feature: &str) -> Result<bool> {
    let value = read_manifest(manifest)?;
    Ok(value
        .get("features")
        .and_then(|features| features.get(feature))
        .is_some())
}

/// Whether the app feature `feature` enables the optional dependency `key`, directly or through
/// other features of the app's `[features]` table: a member `dep:<key>`, `<key>/<feature>` (not
/// `<key>?/<feature>`), or the implicit feature `<key>` of an optional dependency the table never
/// names with `dep:`.
fn feature_enables_dependency(features: Option<&toml::Table>, feature: &str, key: &str) -> bool {
    let Some(features) = features else {
        return false;
    };
    let dep_member = format!("dep:{key}");
    let implicit = !features.values().any(|members| {
        members
            .as_array()
            .is_some_and(|members| members.iter().any(|m| m.as_str() == Some(&dep_member)))
    });
    let mut pending = vec![feature.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        let Some(members) = features.get(&current).and_then(Value::as_array) else {
            continue;
        };
        for member in members.iter().filter_map(Value::as_str) {
            if member == dep_member
                || member.split_once('/').is_some_and(|(name, _)| name == key)
                || (implicit && member == key && !features.contains_key(key))
            {
                return true;
            }
            if features.contains_key(member) {
                pending.push(member.to_string());
            }
        }
    }
    false
}

/// A `Cargo.toml` key: bare when it can be, quoted otherwise.
fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        key.to_string()
    } else {
        toml_string(key)
    }
}

fn manifest_default_features(manifest: &Path) -> Result<Vec<String>> {
    let value = read_manifest(manifest)?;
    Ok(value
        .get("features")
        .and_then(|features| features.get("default"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect())
}

fn manifest_feature_names(manifest: &Path) -> Result<Vec<String>> {
    let value = read_manifest(manifest)?;
    Ok(value
        .get("features")
        .and_then(Value::as_table)
        .into_iter()
        .flat_map(|features| features.keys())
        .filter(|feature| feature.as_str() != "default")
        .cloned()
        .collect())
}

fn rayx_metadata(value: &Value) -> Option<&Value> {
    value
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("rayx"))
}

/// The Gradle property for the Android Java directories: `-PrayxAndroidJavaDirs=<dirs joined by
/// the platform path separator>`, `None` without directories.
pub fn android_java_dirs_property(dirs: &[PathBuf]) -> Result<Option<String>> {
    if dirs.is_empty() {
        return Ok(None);
    }
    let joined = std::env::join_paths(dirs).context("joining the Android Java directories")?;
    Ok(Some(format!(
        "-PrayxAndroidJavaDirs={}",
        joined.to_string_lossy()
    )))
}

/// The root manifest's `[patch.<registry or URL>]` entries whose patched package (`package`,
/// else the key) is in `graph`, with relative paths made absolute under `workspace_root`, as
/// `Cargo.toml` text; empty when none applies. Patches only apply from the root of the workspace
/// being built, so each generated entry crate repeats the ones the app uses.
pub fn patch_tables_for_graph(
    root_manifest: &toml::Table,
    workspace_root: &Path,
    graph: &AppGraph,
) -> Result<String> {
    let Some(patch) = root_manifest.get("patch").and_then(Value::as_table) else {
        return Ok(String::new());
    };
    let mut rendered = String::new();
    for (registry, entries) in patch {
        let Some(entries) = entries.as_table() else {
            continue;
        };
        let mut lines = String::new();
        for (key, entry) in entries {
            let mut entry = entry.clone();
            let package = entry
                .get("package")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_string();
            if !graph.contains_name(&package) {
                continue;
            }
            if let Some(table) = entry.as_table_mut()
                && let Some(path) = table.get("path").and_then(Value::as_str)
            {
                let path = Path::new(path);
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    workspace_root.join(path)
                };
                table.insert(
                    "path".to_string(),
                    Value::String(cargo_toml_path(&absolute)),
                );
            }
            lines.push_str(&format!("{} = {entry}\n", toml_key(key)));
        }
        if !lines.is_empty() {
            rendered.push_str(&format!("\n[patch.{}]\n{lines}", toml_key(registry)));
        }
    }
    Ok(rendered)
}

fn cargo_toml_path(path: &Path) -> String {
    command_path(path).to_string_lossy().replace('\\', "/")
}

fn resolve_app_path(root: &Path, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn sanitize_slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "app".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::FixtureWorkspace;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    /// Replaces the fixture app's manifest in the temporary copy of the workspace.
    fn rewrite_app_manifest(workspace: &FixtureWorkspace, text: &str) -> std::io::Result<()> {
        std::fs::write(
            workspace.root.join("apps").join("demo").join("Cargo.toml"),
            text,
        )
    }

    fn root_patch(app: &AppDescriptor, relative: &str) -> String {
        cargo_toml_path(&app.context.workspace_root().join(relative))
    }

    #[test]
    fn resolve_reads_the_descriptor_from_the_manifest_metadata() {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();

        assert_eq!(app.slug, "demo");
        assert_eq!(app.rust_lib_name, "rayx_demo");
        assert_eq!(app.lib_name, "rayx_demo");
        assert_eq!(
            app.android_application_id.as_deref(),
            Some("dev.example.demo")
        );
        assert!(!app.diagnostic_harness);
        assert!(app.package().is_ok());
    }

    #[test]
    fn resolve_rejects_a_directory_without_a_manifest() {
        let workspace = FixtureWorkspace::new();
        let context = workspace.app().context;
        let error =
            AppDescriptor::resolve_from(&context, &workspace.root, "apps/missing").unwrap_err();
        assert!(error.to_string().contains("was not found"), "{error:#}");

        let error = AppDescriptor::resolve_from(&context, &workspace.root, "crates").unwrap_err();
        assert!(error.to_string().contains("no Cargo.toml"), "{error:#}");
    }

    #[test]
    fn output_directories_live_under_the_workspace_root() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let root = app.context.workspace_root().to_path_buf();

        assert_eq!(app.artifact_root()?, root.join("artifacts/apps/demo"));
        assert_eq!(app.temp_root()?, root.join("artifacts-temp/apps/demo"));
        assert_eq!(app.target_dir()?, root.join("target"));
        Ok(())
    }

    #[test]
    fn android_entry_manifest_is_generated_from_the_apps_entry_dependencies() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();

        let manifest = app.android_rust_manifest(&AppFeatureSelection::default())?;
        assert!(manifest.starts_with(app.temp_root()?.join("android").join("rust")));
        let text = std::fs::read_to_string(&manifest)?;
        assert!(text.contains("[workspace]"));
        assert!(text.contains("crate-type = [\"cdylib\"]"));
        assert!(text.contains("ui_runtime = { path = "));
        assert!(text.contains("crates/ui_runtime\" }"));
        assert!(text.contains("rayx_demo = { package = \"rayx_demo\", path = "));
        assert!(text.contains("default = [\"rayx_demo/profile-all\"]"));

        let source = std::fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs"))?;
        assert!(source.contains("apps/demo/src/main.rs"));
        Ok(())
    }

    #[test]
    fn android_entry_manifest_ignores_a_checked_in_wrapper_manifest() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let wrapper = app.root.join("platform").join("android");
        std::fs::write(
            wrapper.join("Cargo.toml"),
            "[package]\nname = \"wrapper\"\n",
        )?;

        let manifest = app.android_rust_manifest(&AppFeatureSelection::default())?;
        assert_ne!(manifest, wrapper.join("Cargo.toml"));
        assert!(manifest.starts_with(app.temp_root()?.join("android").join("rust")));
        Ok(())
    }

    #[test]
    fn ios_entry_manifest_builds_a_static_library() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();

        assert!(!app.platform_manifest("ios").is_file());
        let manifest = app.ios_rust_manifest(&AppFeatureSelection::default())?;
        assert!(manifest.starts_with(app.temp_root()?.join("ios").join("rust")));
        let text = std::fs::read_to_string(&manifest)?;
        assert!(text.contains("crate-type = [\"staticlib\"]"));
        assert!(text.contains("rayx_demo = { package = \"rayx_demo\""));
        Ok(())
    }

    #[test]
    fn platform_names_come_from_the_manifest_metadata() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();

        assert_eq!(
            app.android_launch_component()?,
            "dev.example.demo/dev.rayx.mobile.RayXActivity"
        );
        assert_eq!(app.ios_project()?, "Demo.xcodeproj");
        assert_eq!(app.ios_scheme()?, "Demo");
        assert_eq!(app.ios_bundle_id()?, "dev.example.demo");
        assert_eq!(app.ios_app_name()?, "Demo");
        Ok(())
    }

    #[test]
    fn every_entry_crate_starts_from_the_workspace_lockfile() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let workspace_lock = app.context.workspace_root().join("Cargo.lock");
        for (target, manifest) in [
            (
                "android",
                AppDescriptor::android_rust_manifest as fn(&_, &_) -> _,
            ),
            ("ios", AppDescriptor::ios_rust_manifest),
            ("wasm", AppDescriptor::web_manifest),
        ] {
            let generated_root = app.temp_root()?.join(target).join("rust");
            std::fs::create_dir_all(&generated_root)?;
            std::fs::write(generated_root.join("Cargo.lock"), "stale lock")?;

            manifest(&app, &AppFeatureSelection::default())?;
            assert_eq!(
                std::fs::read(generated_root.join("Cargo.lock"))?,
                std::fs::read(&workspace_lock)?,
                "the stale {target} lock is replaced by the project's"
            );
        }
        Ok(())
    }

    #[test]
    fn web_manifest_is_generated_with_the_locked_wasm_bindgen() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let generated_root = app.temp_root()?.join("wasm").join("rust");

        let manifest = app.web_manifest(&AppFeatureSelection::default())?;
        assert!(manifest.starts_with(&generated_root));
        assert_eq!(app.web_module_base(), "rayx_demo_web");

        let text = std::fs::read_to_string(&manifest)?;
        assert!(text.contains("name = \"rayx_demo_web\""));
        assert!(text.contains("crate-type = [\"cdylib\", \"rlib\"]"));
        assert!(text.contains("default-features = false"));
        assert!(text.contains("wasm-bindgen = \"=0.2.129\""));
        assert!(!text.contains("zune-core"));
        assert!(!text.contains("branches"));
        assert!(!text.contains("[patch"));
        Ok(())
    }

    #[test]
    fn root_patches_follow_the_apps_graph_for_the_selected_features() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();

        let text = std::fs::read_to_string(app.web_manifest(&AppFeatureSelection::default())?)?;
        assert!(!text.contains("[patch.crates-io]"), "{text}");

        let selection = AppFeatureSelection {
            features: vec!["store".to_string()],
            no_default_features: true,
            ..AppFeatureSelection::default()
        };
        let text = std::fs::read_to_string(app.web_manifest(&selection)?)?;
        assert!(text.contains("[patch.crates-io]"), "{text}");
        assert!(
            text.contains(&root_patch(&app, "patches/store_backend")),
            "{text}"
        );
        assert!(!text.contains("unused_backend"), "{text}");
        Ok(())
    }

    #[test]
    fn every_entry_manifest_forwards_one_explicit_feature_selection() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let selection = AppFeatureSelection {
            features: vec!["profile-a, profile-b".to_string()],
            all_features: false,
            no_default_features: true,
        };

        for manifest in [
            app.android_rust_manifest(&selection)?,
            app.ios_rust_manifest(&selection)?,
            app.web_manifest(&selection)?,
        ] {
            let text = std::fs::read_to_string(manifest)?;
            assert!(text.contains("default = [\"rayx_demo/profile-a\", \"rayx_demo/profile-b\"]"));
            assert!(text.contains("default-features = false"));
        }
        Ok(())
    }

    #[test]
    fn all_features_forwards_every_feature_and_the_optional_entry_dependency() -> TestResult {
        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let selection = AppFeatureSelection {
            all_features: true,
            ..AppFeatureSelection::default()
        };

        let text = std::fs::read_to_string(app.android_rust_manifest(&selection)?)?;
        assert!(text.contains("profile-a = [\"rayx_demo/profile-a\"]"));
        assert!(text.contains("inspect = [\"rayx_demo/inspect\", \"dep:inspector\"]"));
        assert!(text.contains("inspector = { path = "));
        assert!(text.contains(", optional = true }"));
        assert!(!text.contains("devkit"), "{text}");
        Ok(())
    }

    #[test]
    fn an_app_without_entry_dependencies_gets_an_entry_crate_with_only_its_library() -> TestResult {
        let workspace = FixtureWorkspace::new();
        rewrite_app_manifest(
            &workspace,
            "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nui_runtime = { path = \"../../crates/ui_runtime\" }\n",
        )?;
        let app = workspace.app();

        let manifest = app.web_manifest(&AppFeatureSelection::default())?;
        let text = std::fs::read_to_string(manifest)?;
        assert!(!text.contains("ui_runtime"), "{text}");
        assert!(text.contains("rayx_demo = { package = \"rayx_demo\""));
        Ok(())
    }

    #[test]
    fn app_feature_selection_rejects_conflicting_empty_and_unknown_features() {
        let mut conflicting = vec![
            "--all-features".to_string(),
            "--no-default-features".to_string(),
        ];
        assert!(AppFeatureSelection::take_from_args(&mut conflicting).is_err());

        let mut empty = vec!["--features".to_string(), " , ".to_string()];
        assert!(AppFeatureSelection::take_from_args(&mut empty).is_err());

        let workspace = FixtureWorkspace::new();
        let app = workspace.app();
        let unknown = AppFeatureSelection {
            features: vec!["not-a-profile".to_string()],
            ..AppFeatureSelection::default()
        };
        assert!(unknown.validate_for(&app.manifest()).is_err());
    }

    #[test]
    fn feature_arguments_before_the_separator_are_normalized() -> TestResult {
        let mut args: Vec<String> = [
            "--features",
            "profile-a,profile-b profile-a",
            "--no-default-features",
            "--",
            "--app-flag",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();

        let selection = AppFeatureSelection::take_from_args(&mut args)?;
        assert_eq!(selection.normalized_features(), ["profile-a", "profile-b"]);
        assert!(!selection.default_features_enabled());
        assert_eq!(args, ["--", "--app-flag"]);
        Ok(())
    }

    #[test]
    fn playwright_metadata_is_read_from_the_package_table() -> TestResult {
        let workspace = FixtureWorkspace::new();
        assert_eq!(workspace.app().playwright_specs()?, None);

        rewrite_app_manifest(
            &workspace,
            "[package]\nname = \"rayx_demo\"\nversion = \"0.1.0\"\n[package.metadata.rayx]\nplaywright_specs = [\"demo/\", \"shared/a.spec.ts\"]\nplaywright_projects = [\"chromium\"]\n[dependencies]\nui_runtime = { path = \"../../crates/ui_runtime\" }\n",
        )?;
        let app = workspace.app();
        assert_eq!(
            app.playwright_specs()?,
            Some(vec!["demo/".to_string(), "shared/a.spec.ts".to_string()])
        );
        assert_eq!(
            app.playwright_projects()?,
            Some(vec!["chromium".to_string()])
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn generated_manifest_paths_strip_windows_verbatim_prefix() {
        assert_eq!(
            cargo_toml_path(Path::new(r"\\?\C:\repo\apps\example")),
            "C:/repo/apps/example"
        );
    }
}
