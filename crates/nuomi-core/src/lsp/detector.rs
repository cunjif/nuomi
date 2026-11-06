//! LSP auto-detection: PATH probing + SDK inference (ADR-0028).
//!
//! Implements the third tier of the discovery strategy — when no user config
//! and no persisted auto-detected config exist, probe PATH and well-known SDK
//! installation locations to find an available Language Server automatically.
//!
//! Detection is best-effort and side-effect-free: it returns `None` when no
//! candidate is found, letting the caller fall back to the hardcoded default
//! (which will fail at spawn time if the command truly doesn't exist).

use std::path::PathBuf;

use super::types::{LspLanguageId, LspServerConfig};
use crate::adapters::spawn_candidates;

/// Detect an LSP config for the given language by probing PATH and SDK
/// installation locations. Returns the first match or `None`.
///
/// Order per language (ADR-0028 §1):
/// - java: `jdtls` → `jdt-language-server` → vscode-java extension scan
/// - python: `pyright-langserver` → `pylsp` → `ruff-lsp` → `VIRTUAL_ENV`/`CONDA_PREFIX` bin dir inference
/// - typescript/javascript: `typescript-language-server`
/// - go: `gopls` → `bingo` → `GOBIN`/`GOPATH/bin`/`GOROOT/bin` inference
/// - rust: `rust-analyzer`
/// - c/cpp: `clangd` → `ccls`
pub fn detect_config(language: &LspLanguageId) -> Option<LspServerConfig> {
    match language.as_str() {
        "java" => detect_java(),
        "python" => detect_python(),
        "typescript" => detect_typescript("typescript"),
        "javascript" => detect_typescript("javascript"),
        "go" => detect_go(),
        "rust" => detect_rust(),
        "c" => detect_clangd("c"),
        "cpp" => detect_clangd("cpp"),
        _ => None,
    }
}

/// Returns true if `command` is found on PATH.
///
/// On Windows, tries PATHEXT variants (`.cmd`, `.bat`, `.exe`, `.com`) via
/// [`spawn_candidates`]. Only checks file existence — does not execute the
/// command, so it is cheap and safe to call.
pub fn is_in_path(command: &str) -> bool {
    find_in_path(command).is_some()
}

/// Resolves `command` to its full path on PATH if found, else `None`.
pub fn find_in_path(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for candidate in spawn_candidates(command) {
            let full = dir.join(&candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

// ---------- per-language detectors ----------

fn detect_java() -> Option<LspServerConfig> {
    for cmd in ["jdtls", "jdt-language-server"] {
        if is_in_path(cmd) {
            return Some(LspServerConfig::new("java", cmd));
        }
    }
    if let Some(path) = find_vscode_jdtls() {
        return Some(LspServerConfig::new("java", path.to_string_lossy().into_owned()));
    }
    None
}

fn detect_python() -> Option<LspServerConfig> {
    // Tier 1: PATH.
    for cmd in ["pyright-langserver", "pylsp", "ruff-lsp"] {
        if is_in_path(cmd) {
            return Some(python_config(cmd));
        }
    }
    // Tier 2: venv / conda inference — probe VIRTUAL_ENV then CONDA_PREFIX.
    // On Windows the bin dir is `Scripts/`, on Unix `bin/`.
    for env_var in ["VIRTUAL_ENV", "CONDA_PREFIX"] {
        if let Some(env_root) = std::env::var_os(env_var) {
            let bin_dir = if cfg!(windows) {
                PathBuf::from(&env_root).join("Scripts")
            } else {
                PathBuf::from(&env_root).join("bin")
            };
            if let Some((bare, full)) = find_python_ls_in_dir(&bin_dir) {
                return Some(python_config_with_path(bare, &full));
            }
        }
    }
    None
}

/// Build a Python LspServerConfig for a bare command name found on PATH.
fn python_config(cmd: &str) -> LspServerConfig {
    let args = if cmd == "pyright-langserver" {
        vec!["--stdio".to_string()]
    } else {
        Vec::new()
    };
    LspServerConfig::new("python", cmd).with_args(args)
}

/// Build a Python LspServerConfig for a command resolved to an absolute path
/// inside a venv/conda env. The bare name is kept for arg decisions only.
fn python_config_with_path(bare_name: &str, full_path: &std::path::Path) -> LspServerConfig {
    let args = if bare_name == "pyright-langserver" {
        vec!["--stdio".to_string()]
    } else {
        Vec::new()
    };
    LspServerConfig::new("python", full_path.to_string_lossy().into_owned()).with_args(args)
}

/// Probe a venv/conda bin dir for any Python LS executable. Returns the bare
/// command name (e.g. "pyright-langserver") and the resolved full path of the
/// first match, respecting the same candidate priority as PATH detection.
fn find_python_ls_in_dir(bin_dir: &std::path::Path) -> Option<(&'static str, PathBuf)> {
    for cmd in ["pyright-langserver", "pylsp", "ruff-lsp"] {
        for candidate in spawn_candidates(cmd) {
            let full = bin_dir.join(&candidate);
            if full.is_file() {
                return Some((cmd, full));
            }
        }
    }
    None
}

fn detect_typescript(language: &str) -> Option<LspServerConfig> {
    if is_in_path("typescript-language-server") {
        return Some(
            LspServerConfig::new(language, "typescript-language-server")
                .with_args(vec!["--stdio".into()]),
        );
    }
    None
}

fn detect_go() -> Option<LspServerConfig> {
    // Tier 1: PATH.
    if is_in_path("gopls") {
        return Some(go_config("gopls", "gopls"));
    }
    if is_in_path("bingo") {
        return Some(go_config("bingo", "bingo"));
    }
    // Tier 2: SDK inference — probe GOBIN, GOPATH/bin, GOROOT/bin in that order.
    if let Some(bare) = find_go_ls_in_sdk() {
        let full = resolve_go_sdk_path(bare);
        return Some(go_config(bare, &full.to_string_lossy()));
    }
    None
}

/// Build a Go LspServerConfig. `bare` is the canonical command name used to
/// decide args; `command` is what actually gets spawned (bare name for PATH
/// hits, absolute path for SDK inference hits).
fn go_config(bare: &str, command: &str) -> LspServerConfig {
    let args = if bare == "gopls" {
        vec!["serve".to_string()]
    } else {
        Vec::new()
    };
    LspServerConfig::new("go", command).with_args(args)
}

/// Probe Go SDK installation locations for gopls/bingo. Returns the bare name
/// of the first match. Order: GOBIN → GOPATH/bin (each entry) → GOROOT/bin.
fn find_go_ls_in_sdk() -> Option<&'static str> {
    for dir in go_sdk_candidate_dirs() {
        for cmd in ["gopls", "bingo"] {
            for candidate in spawn_candidates(cmd) {
                if dir.join(&candidate).is_file() {
                    return Some(cmd);
                }
            }
        }
    }
    None
}

/// Resolve the absolute path of a Go LS binary found via SDK inference.
/// Re-scans the candidate dirs in the same order as `find_go_ls_in_sdk`.
fn resolve_go_sdk_path(bare: &str) -> PathBuf {
    for dir in go_sdk_candidate_dirs() {
        for candidate in spawn_candidates(bare) {
            let full = dir.join(&candidate);
            if full.is_file() {
                return full;
            }
        }
    }
    PathBuf::from(bare)
}

/// Collect Go SDK bin candidate directories from environment variables.
/// Order: GOBIN → GOPATH/bin (each entry) → GOROOT/bin.
fn go_sdk_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(gobin) = std::env::var_os("GOBIN") {
        dirs.push(PathBuf::from(gobin));
    }
    if let Some(gopath) = std::env::var_os("GOPATH") {
        for p in std::env::split_paths(&gopath) {
            dirs.push(p.join("bin"));
        }
    }
    if let Some(goroot) = std::env::var_os("GOROOT") {
        dirs.push(PathBuf::from(goroot).join("bin"));
    }
    dirs
}

fn detect_rust() -> Option<LspServerConfig> {
    if is_in_path("rust-analyzer") {
        return Some(LspServerConfig::new("rust", "rust-analyzer"));
    }
    None
}

fn detect_clangd(language: &str) -> Option<LspServerConfig> {
    if is_in_path("clangd") {
        return Some(LspServerConfig::new(language, "clangd"));
    }
    if is_in_path("ccls") {
        return Some(LspServerConfig::new(language, "ccls"));
    }
    None
}

// ---------- vscode-java extension scan ----------

/// Scans `~/.vscode/extensions/redhat.java-*/server/` for the jdtls launch
/// script (`jdtls` on Unix, `jdtls.cmd` on Windows). Returns the first match.
///
/// Cross-platform home resolution: `$HOME` (Unix) → `%USERPROFILE%` (Windows).
fn find_vscode_jdtls() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let ext_dir = PathBuf::from(home).join(".vscode").join("extensions");
    let entries = std::fs::read_dir(&ext_dir).ok()?;
    let script = if cfg!(windows) { "jdtls.cmd" } else { "jdtls" };

    let mut matching: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("redhat.java-") {
            let server_script = entry.path().join("server").join(script);
            if server_script.is_file() {
                matching.push(server_script);
            }
        }
    }
    // Sort for deterministic selection (newest version first by dir name).
    matching.sort_by(|a, b| b.cmp(a));
    matching.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_config_unknown_language_returns_none() {
        assert!(detect_config(&LspLanguageId::new("brainfuck")).is_none());
    }

    #[test]
    fn is_in_path_returns_bool() {
        // Every OS has some command on PATH; we just verify it returns a bool
        // without panicking. The actual result depends on the test environment.
        let _ = is_in_path("definitely_not_a_real_command_xyz");
        let _ = is_in_path("ls");
    }

    #[test]
    fn find_in_path_none_for_nonexistent() {
        assert!(find_in_path("definitely_not_a_real_command_xyz").is_none());
    }

    #[test]
    fn detect_java_falls_back_to_none_when_nothing_found() {
        // When neither jdtls, jdt-language-server, nor vscode-java is present,
        // detect_java returns None. We can't guarantee the test env lacks
        // these, so we only verify it doesn't panic.
        let _ = detect_java();
    }

    #[test]
    fn detect_python_does_not_panic() {
        let result = detect_python();
        if let Some(cfg) = result {
            assert_eq!(cfg.language.as_str(), "python");
            assert!(!cfg.command.is_empty());
        }
    }

    #[test]
    fn detect_typescript_uses_stdio_arg() {
        let result = detect_typescript("typescript");
        if let Some(cfg) = result {
            assert_eq!(cfg.language.as_str(), "typescript");
            assert_eq!(cfg.command, "typescript-language-server");
            assert_eq!(cfg.args, vec!["--stdio"]);
        }
    }

    #[test]
    fn detect_go_uses_serve_arg_for_gopls() {
        // Only check args structure if gopls is actually found.
        if is_in_path("gopls") {
            let cfg = detect_go().expect("gopls detected");
            assert_eq!(cfg.command, "gopls");
            assert_eq!(cfg.args, vec!["serve"]);
        }
    }

    // ---------- Python venv/conda inference ----------

    #[test]
    fn find_python_ls_in_dir_finds_pyright() {
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_py_venv_pyright");
        let bin_dir = if cfg!(windows) {
            tmp.join("Scripts")
        } else {
            tmp.join("bin")
        };
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script_name = if cfg!(windows) {
            "pyright-langserver.exe"
        } else {
            "pyright-langserver"
        };
        std::fs::write(bin_dir.join(script_name), "#!/bin/sh\n").unwrap();

        let found = find_python_ls_in_dir(&bin_dir);
        std::fs::remove_dir_all(&tmp).ok();
        assert_eq!(found.map(|(bare, _)| bare), Some("pyright-langserver"));
    }

    #[test]
    fn find_python_ls_in_dir_returns_none_when_empty() {
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_py_venv_empty");
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(find_python_ls_in_dir(&tmp), None);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn find_python_ls_in_dir_respects_priority() {
        // When both pyright-langserver and pylsp exist, pyright wins.
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_py_venv_prio");
        std::fs::create_dir_all(&tmp).unwrap();
        let ext = if cfg!(windows) { ".exe" } else { "" };
        std::fs::write(tmp.join(format!("pyright-langserver{ext}")), "#!/bin/sh\n").unwrap();
        std::fs::write(tmp.join(format!("pylsp{ext}")), "#!/bin/sh\n").unwrap();

        let found = find_python_ls_in_dir(&tmp);
        std::fs::remove_dir_all(&tmp).ok();
        assert_eq!(found.map(|(bare, _)| bare), Some("pyright-langserver"));
    }

    #[test]
    fn detect_python_venv_inference_when_path_lacks_ls() {
        // End-to-end: when PATH has no Python LS but VIRTUAL_ENV points to a
        // venv containing one, detect_python should return a config whose
        // command is the absolute path inside the venv.
        if is_in_path("pyright-langserver") || is_in_path("pylsp") || is_in_path("ruff-lsp") {
            return; // PATH already has a LS; venv inference not reachable.
        }
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_py_venv_e2e");
        let bin_dir = if cfg!(windows) {
            tmp.join("Scripts")
        } else {
            tmp.join("bin")
        };
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script_name = if cfg!(windows) {
            "pyright-langserver.exe"
        } else {
            "pyright-langserver"
        };
        let script_path = bin_dir.join(script_name);
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();

        let old = std::env::var_os("VIRTUAL_ENV");
        std::env::set_var("VIRTUAL_ENV", &tmp);
        let cfg = detect_python();
        match old {
            Some(v) => std::env::set_var("VIRTUAL_ENV", v),
            None => std::env::remove_var("VIRTUAL_ENV"),
        }
        std::fs::remove_dir_all(&tmp).ok();

        let cfg = cfg.expect("venv inference should find pyright");
        assert_eq!(cfg.language.as_str(), "python");
        assert_eq!(cfg.args, vec!["--stdio"]);
        // command must be the absolute path to the venv script, not the bare name.
        assert!(
            cfg.command.ends_with(script_name),
            "expected command to end with {script_name}, got {}",
            cfg.command
        );
    }

    // ---------- Go SDK inference ----------

    #[test]
    fn go_sdk_candidate_dirs_includes_gobin_gopath_goroot() {
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_go_dirs");
        let gobin = tmp.join("gobin");
        let gopath = tmp.join("gopath");
        let goroot = tmp.join("goroot");
        std::fs::create_dir_all(&gobin).unwrap();
        std::fs::create_dir_all(&gopath).unwrap();
        std::fs::create_dir_all(&goroot).unwrap();

        let old_gobin = std::env::var_os("GOBIN");
        let old_gopath = std::env::var_os("GOPATH");
        let old_goroot = std::env::var_os("GOROOT");
        std::env::set_var("GOBIN", &gobin);
        std::env::set_var("GOPATH", &gopath);
        std::env::set_var("GOROOT", &goroot);

        let dirs = go_sdk_candidate_dirs();

        match old_gobin {
            Some(v) => std::env::set_var("GOBIN", v),
            None => std::env::remove_var("GOBIN"),
        }
        match old_gopath {
            Some(v) => std::env::set_var("GOPATH", v),
            None => std::env::remove_var("GOPATH"),
        }
        match old_goroot {
            Some(v) => std::env::set_var("GOROOT", v),
            None => std::env::remove_var("GOROOT"),
        }
        std::fs::remove_dir_all(&tmp).ok();

        // GOBIN first, then GOPATH/bin, then GOROOT/bin.
        assert_eq!(dirs.len(), 3, "expected 3 candidate dirs, got {dirs:?}");
        assert_eq!(dirs[0], gobin, "GOBIN should be first");
        assert_eq!(dirs[1], gopath.join("bin"), "GOPATH/bin should be second");
        assert_eq!(dirs[2], goroot.join("bin"), "GOROOT/bin should be third");
    }

    #[test]
    fn go_sdk_candidate_dirs_empty_when_no_env() {
        let old_gobin = std::env::var_os("GOBIN");
        let old_gopath = std::env::var_os("GOPATH");
        let old_goroot = std::env::var_os("GOROOT");
        std::env::remove_var("GOBIN");
        std::env::remove_var("GOPATH");
        std::env::remove_var("GOROOT");

        let dirs = go_sdk_candidate_dirs();

        match old_gobin {
            Some(v) => std::env::set_var("GOBIN", v),
            None => std::env::remove_var("GOBIN"),
        }
        match old_gopath {
            Some(v) => std::env::set_var("GOPATH", v),
            None => std::env::remove_var("GOPATH"),
        }
        match old_goroot {
            Some(v) => std::env::set_var("GOROOT", v),
            None => std::env::remove_var("GOROOT"),
        }
        assert!(dirs.is_empty(), "expected no candidate dirs, got {dirs:?}");
    }

    #[test]
    fn detect_go_sdk_inference_when_path_lacks_gopls() {
        // End-to-end: when PATH has no gopls/bingo but GOBIN points to a dir
        // containing gopls, detect_go should return a config whose command is
        // the absolute path inside GOBIN.
        if is_in_path("gopls") || is_in_path("bingo") {
            return; // PATH already has a Go LS; SDK inference not reachable.
        }
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_go_gobin_e2e");
        std::fs::create_dir_all(&tmp).unwrap();
        let script_name = if cfg!(windows) { "gopls.exe" } else { "gopls" };
        let script_path = tmp.join(script_name);
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();

        let old = std::env::var_os("GOBIN");
        std::env::set_var("GOBIN", &tmp);
        let cfg = detect_go();
        match old {
            Some(v) => std::env::set_var("GOBIN", v),
            None => std::env::remove_var("GOBIN"),
        }
        std::fs::remove_dir_all(&tmp).ok();

        let cfg = cfg.expect("GOBIN inference should find gopls");
        assert_eq!(cfg.language.as_str(), "go");
        assert_eq!(cfg.args, vec!["serve"]);
        assert!(
            cfg.command.ends_with(script_name),
            "expected command to end with {script_name}, got {}",
            cfg.command
        );
    }

    #[test]
    fn detect_rust_returns_rust_analyzer_when_in_path() {
        if is_in_path("rust-analyzer") {
            let cfg = detect_rust().expect("rust-analyzer detected");
            assert_eq!(cfg.command, "rust-analyzer");
            assert!(cfg.args.is_empty());
        }
    }

    #[test]
    fn detect_clangd_preserves_language_field() {
        if is_in_path("clangd") {
            let c_cfg = detect_clangd("c").expect("clangd detected for c");
            assert_eq!(c_cfg.language.as_str(), "c");
            let cpp_cfg = detect_clangd("cpp").expect("clangd detected for cpp");
            assert_eq!(cpp_cfg.language.as_str(), "cpp");
        }
    }

    #[test]
    fn find_vscode_jdtls_returns_none_when_no_extensions_dir() {
        // Point HOME to a temp dir with no .vscode/extensions.
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_no_vscode");
        let old_home = std::env::var_os("HOME");
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("HOME", &tmp);
        let result = find_vscode_jdtls();
        // Restore
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert!(result.is_none());
    }

    #[test]
    fn find_vscode_jdtls_finds_script_in_redhat_dir() {
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_vscode_jdtls");
        let ext_dir = tmp.join(".vscode").join("extensions");
        let server_dir = ext_dir.join("redhat.java-1.0.0").join("server");
        std::fs::create_dir_all(&server_dir).unwrap();
        let script_name = if cfg!(windows) { "jdtls.cmd" } else { "jdtls" };
        let script_path = server_dir.join(script_name);
        std::fs::write(&script_path, "#!/bin/sh\n").unwrap();

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tmp);
        let result = find_vscode_jdtls();
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&tmp).ok();

        assert_eq!(result, Some(script_path));
    }

    #[test]
    fn find_vscode_jdtls_picks_newest_version() {
        let tmp = std::env::temp_dir().join("nuomi_lsp_test_vscode_jdtls_multi");
        let ext_dir = tmp.join(".vscode").join("extensions");
        let script_name = if cfg!(windows) { "jdtls.cmd" } else { "jdtls" };

        for ver in ["1.0.0", "1.2.0", "1.1.0"] {
            let server_dir = ext_dir.join(format!("redhat.java-{ver}")).join("server");
            std::fs::create_dir_all(&server_dir).unwrap();
            std::fs::write(server_dir.join(script_name), "#!/bin/sh\n").unwrap();
        }

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &tmp);
        let result = find_vscode_jdtls();
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        std::fs::remove_dir_all(&tmp).ok();

        let found = result.expect("found jdtls");
        assert!(
            found.to_string_lossy().contains("redhat.java-1.2.0"),
            "expected newest version, got {found:?}"
        );
    }
}
