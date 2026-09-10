//! Plugin installation/uninstallation for the UI plugin panel (ADR 0009 §4
//! addendum): install from a local directory or a zip archive into a user
//! plugin directory, uninstall user-managed plugins by id.
//!
//! Hard rules: the manifest is validated before anything is copied; zip
//! extraction is Zip-Slip-safe (no `..`/absolute entries, size and count
//! caps); the panel can only uninstall from the directory it was given —
//! env/workspace sources must be managed at their origin.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::manifest::PluginManifest;

/// Hard cap on total uncompressed zip payload (prevents zip bombs).
const ZIP_TOTAL_CAP: u64 = 128 * 1024 * 1024;
/// Hard cap on zip entry count.
const ZIP_ENTRY_CAP: usize = 2_000;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("path does not exist: {0}")]
    NotFound(String),
    #[error("unsupported install source (expected a plugin directory or .zip): {0}")]
    UnsupportedSource(String),
    #[error(transparent)]
    Manifest(#[from] super::manifest::ManifestError),
    #[error("plugin '{0}' is already installed — uninstall it first")]
    AlreadyInstalled(String),
    #[error("plugin '{0}' is not installed")]
    PluginNotInstalled(String),
    #[error("zip archive is unsafe or malformed: {0}")]
    ZipUnsafe(String),
    #[error("zip archive contains no plugin.toml (at archive root or in a single top-level folder)")]
    ZipNoManifest,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// The user plugin directory: `<config>/nuomi/plugins` (created on demand),
/// overridable via `NUOMI_PLUGINS_DIR`. Thin convenience wrapper — core
/// install/uninstall functions take the directory explicitly for testability.
pub fn user_plugins_dir() -> Result<PathBuf, InstallError> {
    let dir = std::env::var_os("NUOMI_PLUGINS_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("nuomi").join("plugins")))
        .ok_or_else(|| InstallError::Io(std::io::Error::other("no user config dir")))?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Installs a plugin from a directory or a `.zip` archive into
/// `plugins_dir/<manifest.id>`. Returns the manifest of the installed plugin
/// (read back from the installed location).
pub fn install_into(plugins_dir: &Path, source_path: &Path) -> Result<PluginManifest, InstallError> {
    if source_path.is_dir() {
        install_directory(plugins_dir, source_path)
    } else if source_path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        install_zip(plugins_dir, source_path)
    } else {
        Err(InstallError::UnsupportedSource(
            source_path.display().to_string(),
        ))
    }
}

fn install_directory(plugins_dir: &Path, source: &Path) -> Result<PluginManifest, InstallError> {
    if !source.exists() {
        return Err(InstallError::NotFound(source.display().to_string()));
    }
    let manifest = PluginManifest::load(source)?;
    install_manifest_dir(plugins_dir, &manifest, source)
}

/// Copies `from` into `<plugins_dir>/<manifest.id>`, failing if the id is
/// already installed (uninstall first — keeps the panel predictable).
fn install_manifest_dir(
    plugins_dir: &Path,
    manifest: &PluginManifest,
    from: &Path,
) -> Result<PluginManifest, InstallError> {
    let target = plugins_dir.join(&manifest.id);
    if target.exists() {
        return Err(InstallError::AlreadyInstalled(manifest.id.clone()));
    }
    copy_dir_recursive(from, &target)?;
    // Read back from the installed location: what the user gets is what
    // gets validated (guards against source mutations mid-copy).
    match PluginManifest::load(&target) {
        Ok(manifest) => Ok(manifest),
        Err(e) => {
            let _ = fs::remove_dir_all(&target);
            Err(e.into())
        }
    }
}

/// Installs from a zip archive: safe-extract to a temp dir, locate the
/// manifest (archive root or single top-level folder), validate, then copy
/// into the user plugin directory.
fn install_zip(plugins_dir: &Path, zip_path: &Path) -> Result<PluginManifest, InstallError> {
    if !zip_path.exists() {
        return Err(InstallError::NotFound(zip_path.display().to_string()));
    }
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| InstallError::ZipUnsafe(format!("cannot open archive: {e}")))?;
    if archive.len() > ZIP_ENTRY_CAP {
        return Err(InstallError::ZipUnsafe("too many entries".into()));
    }

    // Staging under the OS temp dir; a uuid keeps concurrent installs apart.
    // Always cleaned up, even on failure.
    let staging_root = std::env::temp_dir()
        .join(format!("nuomi-plugin-install-{}", crate::domain::new_id()));
    fs::create_dir_all(&staging_root)?;
    let result = extract_and_install(plugins_dir, &mut archive, &staging_root);
    let _ = fs::remove_dir_all(&staging_root);
    result
}

fn extract_and_install(
    plugins_dir: &Path,
    archive: &mut zip::ZipArchive<fs::File>,
    staging: &Path,
) -> Result<PluginManifest, InstallError> {
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| InstallError::ZipUnsafe(format!("bad entry: {e}")))?;
        let name = entry.name().to_string();
        let dest = sanitize_zip_path(staging, &name)?;
        if entry.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        total += entry.size();
        if total > ZIP_TOTAL_CAP {
            return Err(InstallError::ZipUnsafe("uncompressed payload too large".into()));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    let manifest_dir = locate_manifest_dir(staging)?;
    let manifest = PluginManifest::load(&manifest_dir)?;
    install_manifest_dir(plugins_dir, &manifest, &manifest_dir)
}

/// Rejects Zip-Slip entries: absolute paths, `..` components, and empty
/// names. Returns the destination path under `staging`.
fn sanitize_zip_path(staging: &Path, entry_name: &str) -> Result<PathBuf, InstallError> {
    let rel = Path::new(entry_name);
    if entry_name.starts_with('/') || entry_name.starts_with('\\') {
        return Err(InstallError::ZipUnsafe(format!("absolute entry path: {entry_name}")));
    }
    let mut safe = staging.to_path_buf();
    for component in rel.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            _ => return Err(InstallError::ZipUnsafe(format!("unsafe entry path: {entry_name}"))),
        }
    }
    Ok(safe)
}

/// The manifest lives either at the extraction root or in a single
/// top-level folder (the GitHub-style `<repo>-main/` wrapper).
fn locate_manifest_dir(staging: &Path) -> Result<PathBuf, InstallError> {
    if staging.join("plugin.toml").is_file() {
        return Ok(staging.to_path_buf());
    }
    let mut top_dirs: Vec<PathBuf> = fs::read_dir(staging)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("plugin.toml").is_file())
        .collect();
    top_dirs.sort();
    match top_dirs.len() {
        1 => Ok(top_dirs.remove(0)),
        _ => Err(InstallError::ZipNoManifest),
    }
}

/// Uninstalls a plugin by id from `plugins_dir`. The panel only passes the
/// user plugin dir here; env/workspace sources are managed at their origin.
pub fn uninstall_from(plugins_dir: &Path, plugin_id: &str) -> Result<PathBuf, InstallError> {
    let dir = plugins_dir.join(plugin_id);
    if !dir.join("plugin.toml").is_file() {
        return Err(InstallError::PluginNotInstalled(plugin_id.to_string()));
    }
    let manifest = PluginManifest::load(&dir)?;
    if manifest.id != plugin_id {
        return Err(InstallError::PluginNotInstalled(plugin_id.to_string()));
    }
    fs::remove_dir_all(&dir)?;
    Ok(dir)
}

/// Recursive directory copy (no external dep; plugin dirs are small).
fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if entry_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
id = "installer-test"
name = "Installer Test"
version = "0.1.0"
api_version = 1
entry = ["node", "plugin.cjs"]
"#;

    fn write_plugin_dir(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.toml"), MANIFEST).unwrap();
        fs::write(dir.join("plugin.cjs"), "console.log('hi')").unwrap();
        dir
    }

    #[test]
    fn installs_from_directory_into_target() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let src = write_plugin_dir(work.path(), "src-plugin");

        let manifest = install_into(plugins_dir.path(), &src).unwrap();
        assert_eq!(manifest.id, "installer-test");

        let installed = plugins_dir.path().join("installer-test");
        assert!(installed.join("plugin.toml").is_file());
        assert!(installed.join("plugin.cjs").is_file());
    }

    #[test]
    fn duplicate_install_is_rejected() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let src = write_plugin_dir(work.path(), "src-plugin");

        install_into(plugins_dir.path(), &src).unwrap();
        let err = install_into(plugins_dir.path(), &src).unwrap_err();
        assert!(matches!(err, InstallError::AlreadyInstalled(_)), "{err}");
    }

    #[test]
    fn invalid_manifest_source_is_rejected_without_copy() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let src = work.path().join("bad");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("plugin.toml"), "id = ").unwrap();

        assert!(install_into(plugins_dir.path(), &src).is_err());
        assert!(!plugins_dir.path().join("bad").exists());
    }

    #[test]
    fn non_directory_non_zip_is_unsupported() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let file = work.path().join("readme.txt");
        fs::write(&file, "hi").unwrap();
        let err = install_into(plugins_dir.path(), &file).unwrap_err();
        assert!(matches!(err, InstallError::UnsupportedSource(_)), "{err}");
    }

    fn build_zip(path: &Path, entries: &[(&str, &str)]) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, body) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, SimpleFileOptions::default()).unwrap();
            } else {
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    #[test]
    fn installs_from_zip_with_wrapper_folder() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let zip_path = work.path().join("upper-main.zip");
        build_zip(
            &zip_path,
            &[
                ("upper-main/", ""),
                ("upper-main/plugin.toml", MANIFEST),
                ("upper-main/plugin.cjs", "console.log('hi')"),
            ],
        );

        let manifest = install_into(plugins_dir.path(), &zip_path).unwrap();
        assert_eq!(manifest.id, "installer-test");
        assert!(plugins_dir
            .path()
            .join("installer-test")
            .join("plugin.cjs")
            .is_file());
    }

    #[test]
    fn installs_from_flat_zip() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let zip_path = work.path().join("flat.zip");
        build_zip(
            &zip_path,
            &[("plugin.toml", MANIFEST), ("plugin.cjs", "console.log('hi')")],
        );
        let manifest = install_into(plugins_dir.path(), &zip_path).unwrap();
        assert_eq!(manifest.id, "installer-test");
        let _ = manifest;
    }

    #[test]
    fn zip_slip_entries_are_rejected() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let zip_path = work.path().join("evil.zip");
        build_zip(
            &zip_path,
            &[("plugin.toml", MANIFEST), ("../evil.txt", "pwned")],
        );
        let err = install_into(plugins_dir.path(), &zip_path).unwrap_err();
        assert!(matches!(err, InstallError::ZipUnsafe(_)), "{err}");
        assert!(!plugins_dir.path().join("installer-test").exists());
    }

    #[test]
    fn zip_without_manifest_is_rejected() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let zip_path = work.path().join("empty.zip");
        build_zip(&zip_path, &[("readme.txt", "nothing here")]);
        let err = install_into(plugins_dir.path(), &zip_path).unwrap_err();
        assert!(matches!(err, InstallError::ZipNoManifest), "{err}");
    }

    #[test]
    fn uninstall_round_trip() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let src = write_plugin_dir(work.path(), "src-plugin");

        // Not installed yet.
        assert!(matches!(
            uninstall_from(plugins_dir.path(), "installer-test"),
            Err(InstallError::PluginNotInstalled(_))
        ));

        install_into(plugins_dir.path(), &src).unwrap();
        let removed = uninstall_from(plugins_dir.path(), "installer-test").unwrap();
        assert!(!removed.exists());
        assert!(matches!(
            uninstall_from(plugins_dir.path(), "installer-test"),
            Err(InstallError::PluginNotInstalled(_))
        ));
    }

    #[test]
    fn uninstall_refuses_wrong_id() {
        let work = tempfile::tempdir().unwrap();
        let plugins_dir = tempfile::tempdir().unwrap();
        let src = write_plugin_dir(work.path(), "src-plugin");
        install_into(plugins_dir.path(), &src).unwrap();
        // Directory named differently from manifest id is not uninstallable
        // by that id (defends against hand-copied dirs).
        assert!(matches!(
            uninstall_from(plugins_dir.path(), "other-id"),
            Err(InstallError::PluginNotInstalled(_))
        ));
    }
}
