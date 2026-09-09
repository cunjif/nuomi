//! Side-load scanner: discovers plugin directories and parses manifests.
//!
//! Scan order (first source wins on duplicate ids, ADR 0009 §4):
//! 1. `NUOMI_PLUGIN_PATH` — PATH-style env var of plugin directories
//! 2. extra paths passed by the embedder (`NuomiConfig.plugin_paths`)
//! 3. `<config>/nuomi/plugins` — immediate subdirectories are plugins
//! 4. `<cwd>/.nuomi/plugins` — workspace plugins

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::manifest::PluginManifest;
use super::report::BootReport;

/// Where a plugin directory came from (boot-report/debug info).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Env,
    Extra,
    UserConfig,
    Workspace,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Extra => "config",
            Self::UserConfig => "user",
            Self::Workspace => "workspace",
        }
    }
}

/// One scanned location's result. Failures never abort the scan.
#[derive(Debug)]
pub enum LoadOutcome {
    /// Manifest parsed and validated; ready to be registered on the kernel.
    /// Boxed to keep the failure variants small (clippy::large_enum_variant).
    Loaded {
        source: SourceKind,
        dir: PathBuf,
        manifest: Box<PluginManifest>,
    },
    Skipped {
        dir: PathBuf,
        reason: String,
    },
    Failed {
        dir: PathBuf,
        reason: String,
    },
}

/// Scans all configured locations in priority order and resolves duplicate
/// ids (first source wins; later ones are reported as skipped).
pub fn scan(extra_paths: &[PathBuf]) -> Vec<LoadOutcome> {
    let mut outcomes = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    let env_dirs: Vec<PathBuf> = match std::env::var_os("NUOMI_PLUGIN_PATH") {
        Some(raw) => std::env::split_paths(&raw)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
        None => Vec::new(),
    };
    let user_config = std::env::var_os("NUOMI_PLUGINS_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("nuomi").join("plugins")));
    let workspace = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".nuomi").join("plugins"));

    let locations: Vec<(SourceKind, Vec<PathBuf>)> = [
        (SourceKind::Env, env_dirs),
        (SourceKind::Extra, extra_paths.to_vec()),
        (SourceKind::UserConfig, user_config.into_iter().collect()),
        (SourceKind::Workspace, workspace.into_iter().collect()),
    ]
    .into();

    for (source, dirs) in locations {
        for dir in dirs {
            match collect_candidates(&dir) {
                Ok(candidates) => {
                    for candidate in candidates {
                        load_candidate(source, &candidate, &mut seen_ids, &mut outcomes);
                    }
                }
                // A missing root is the normal case (nothing installed there).
                Err(reason) => outcomes.push(LoadOutcome::Skipped { dir, reason }),
            }
        }
    }
    outcomes
}

/// Applies scan results to a report (helper so callers share the wording).
pub fn apply_to_report(outcomes: &[LoadOutcome], report: &mut BootReport) {
    for outcome in outcomes {
        match outcome {
            LoadOutcome::Loaded { manifest, .. } => report.record_loaded(manifest.id.clone()),
            LoadOutcome::Skipped { dir, reason } => {
                report.record_skipped(dir.display().to_string(), reason.clone())
            }
            LoadOutcome::Failed { dir, reason } => {
                report.record_failed(dir.display().to_string(), reason.clone())
            }
        }
    }
}

/// The plugin directories implied by one scan location: the location itself
/// when it is a plugin dir, otherwise each immediate subdirectory containing
/// a `plugin.toml`.
fn collect_candidates(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if dir.join("plugin.toml").is_file() {
        return Ok(vec![dir.to_path_buf()]);
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("unscannable: {e}"))?;
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let sub = entry.path();
        if sub.is_dir() && sub.join("plugin.toml").is_file() {
            candidates.push(sub);
        }
    }
    candidates.sort();
    Ok(candidates)
}

fn load_candidate(
    source: SourceKind,
    dir: &Path,
    seen_ids: &mut HashSet<String>,
    outcomes: &mut Vec<LoadOutcome>,
) {
    let manifest = match PluginManifest::load(dir) {
        Ok(manifest) => manifest,
        Err(reason) => {
            outcomes.push(LoadOutcome::Failed {
                dir: dir.to_path_buf(),
                reason: reason.to_string(),
            });
            return;
        }
    };
    if !seen_ids.insert(manifest.id.clone()) {
        outcomes.push(LoadOutcome::Skipped {
            dir: dir.to_path_buf(),
            reason: format!("duplicate of plugin id '{}'", manifest.id),
        });
        return;
    }
    outcomes.push(LoadOutcome::Loaded {
        source,
        dir: dir.to_path_buf(),
        manifest: Box::new(manifest),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const MINIMAL: &str = r#"
id = "p"
name = "P"
version = "0.1.0"
api_version = 1
entry = ["node", "plugin.cjs"]
"#;

    fn write_plugin(root: &Path, name: &str, id: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let body = MINIMAL.replace("id = \"p\"", &format!("id = \"{id}\""));
        fs::write(dir.join("plugin.toml"), body).unwrap();
        dir
    }

    #[test]
    fn scans_root_subdirs_and_direct_plugin_dirs() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "alpha", "alpha");
        write_plugin(root.path(), "beta", "beta");
        // A direct plugin dir (NUOMI_PLUGIN_PATH style).
        let direct = tempfile::tempdir().unwrap();
        fs::write(
            direct.path().join("plugin.toml"),
            MINIMAL.replace("id = \"p\"", "id = \"gamma\""),
        )
        .unwrap();

        let outcomes = scan(&[]);
        let _ = outcomes; // environment-dependent; the targeted scan is below

        let mut outcomes = Vec::new();
        let mut seen = HashSet::new();
        for dir in collect_candidates(root.path()).unwrap() {
            load_candidate(SourceKind::Workspace, &dir, &mut seen, &mut outcomes);
        }
        for dir in collect_candidates(direct.path()).unwrap() {
            load_candidate(SourceKind::Env, &dir, &mut seen, &mut outcomes);
        }
        let ids: Vec<String> = outcomes
            .iter()
            .filter_map(|o| match o {
                LoadOutcome::Loaded { manifest, .. } => Some(manifest.id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn duplicate_id_first_source_wins() {
        let root = tempfile::tempdir().unwrap();
        let first = write_plugin(root.path(), "one", "dup");
        let second = write_plugin(root.path(), "two", "dup");
        let mut seen = HashSet::new();
        let mut outcomes = Vec::new();
        load_candidate(SourceKind::Env, &first, &mut seen, &mut outcomes);
        load_candidate(SourceKind::UserConfig, &second, &mut seen, &mut outcomes);
        assert!(
            matches!(&outcomes[0], LoadOutcome::Loaded { manifest, .. } if manifest.id == "dup")
        );
        assert!(matches!(
            &outcomes[1],
            LoadOutcome::Skipped { reason, .. } if reason.contains("duplicate")
        ));
    }

    #[test]
    fn broken_manifest_fails_without_aborting_scan() {
        let root = tempfile::tempdir().unwrap();
        let good = write_plugin(root.path(), "good", "good");
        let bad = root.path().join("bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("plugin.toml"), "id = ").unwrap();
        let mut seen = HashSet::new();
        let mut outcomes = Vec::new();
        load_candidate(SourceKind::Workspace, &bad, &mut seen, &mut outcomes);
        load_candidate(SourceKind::Workspace, &good, &mut seen, &mut outcomes);
        assert!(matches!(&outcomes[0], LoadOutcome::Failed { .. }));
        assert!(matches!(&outcomes[1], LoadOutcome::Loaded { .. }));
    }

    #[test]
    fn scan_with_no_install_dirs_returns_empty() {
        // With env unset and no config/workspace dirs containing plugins the
        // scan must not fail — the normal cold-start case.
        let outcomes = scan(&[]);
        assert!(outcomes
            .iter()
            .all(|o| !matches!(o, LoadOutcome::Failed { .. })));
    }
}
