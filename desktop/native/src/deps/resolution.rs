use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    ffmpeg_binary, rife_worker_binary, vs_rife_model_install_dir, FFMPEG_PATH_ENV,
    RIFE_DEFAULT_MODEL_DIRS, RIFE_MODEL_PATH_ENV, RIFE_WORKER_PATH_ENV,
};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FfmpegInstallState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinarySource {
    EnvOverride,
    Bundled,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinaryResolutionScope {
    Setup,
    Runtime,
}

impl BinaryResolutionScope {
    /// Determines whether the resolution scope permits a candidate from the given binary source.
    ///
    /// In `Setup` scope, `BinarySource::Path` is disallowed; in `Runtime` scope all sources are permitted.
    ///
    /// # Examples
    ///
    pub(super) fn allows_source(self, source: BinarySource) -> bool {
        match self {
            Self::Setup => !matches!(source, BinarySource::Path),
            Self::Runtime => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedFfmpegBinary {
    pub(super) path: PathBuf,
    pub(super) source: BinarySource,
    pub(super) install_state: FfmpegInstallState,
}

// ---------------------------------------------------------------------------
// Asset name helpers
// ---------------------------------------------------------------------------

/// Returns the OS-specific FFmpeg asset filename for Windows or Linux.
///
/// # Returns
/// `Some(&'static str)` with the filename for the current target OS (`windows` or `linux`), or `None` if the current OS is not supported.
///
/// # Examples
///
pub(super) fn current_ffmpeg_asset_name() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        ffmpeg_asset_name_for_target("windows")
    } else if cfg!(target_os = "linux") {
        ffmpeg_asset_name_for_target("linux")
    } else {
        None
    }
}

/// Get the pinned FFmpeg asset filename for a given target OS.
///
/// Supported targets are "windows" and "linux".
///
/// # Returns
///
/// `Some(&str)` with the OS-specific asset filename for "windows" or "linux", `None` for other targets.
///
/// # Examples
///
pub(super) fn ffmpeg_asset_name_for_target(target_os: &str) -> Option<&'static str> {
    match target_os {
        // Matches e.g. "ffmpeg-n8.1-10-gabcdef-win64-gpl-8.1.zip" from rolling autobuild
        // releases, while excluding lgpl ("win64-lgpl-" != "win64-gpl-") and shared builds.
        "windows" => Some("win64-gpl-"),
        "linux" => Some("linux64-gpl-"),
        _ => None,
    }
}

/// Checks whether a vceenc asset filename uses the archive suffix expected for the current OS.
///
/// # Returns
/// `true` if the filename ends with the platform-specific vceenc archive suffix (`_x64.7z` on Windows, `_amd64.deb` on Linux), `false` otherwise.
///
/// # Examples
///
pub(super) fn vceenc_asset_matches_target(asset_name: &str) -> bool {
    if cfg!(target_os = "windows") {
        asset_name.ends_with("_x64.7z")
    } else if cfg!(target_os = "linux") {
        asset_name.ends_with("_amd64.deb")
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Public resolution entry points
// ---------------------------------------------------------------------------

/// Resolves a usable FFmpeg executable path for runtime usage.
///
/// Searches for a validated FFmpeg binary allowed in the runtime scope (environment override,
/// bundled install, or system PATH) and returns its filesystem path when found; returns `Ok(None)`
/// when no suitable FFmpeg is available.
///
/// # Examples
///
pub fn resolve_ffmpeg_path(lib_dir: &Path) -> Result<Option<PathBuf>, String> {
    Ok(
        resolve_ffmpeg_binary(lib_dir, BinaryResolutionScope::Runtime)?
            .map(|resolved| resolved.path),
    )
}

/// Resolve the path to the compiled `rife-worker` binary.
///
/// Search order:
/// 1. `RIFE_WORKER_PATH_ENV` environment variable override.
/// 2. The directory containing the currently-running executable (the Tauri app).
/// 3. `lib_dir` itself (manual placement fallback).
/// 4. Directories on `PATH` (runtime only).
///
/// Returns `None` when no usable `rife-worker` binary is found.
pub fn resolve_rife_worker_path(lib_dir: &Path) -> Option<PathBuf> {
    // 1. Env override.
    if let Ok(val) = std::env::var(RIFE_WORKER_PATH_ENV) {
        let p = PathBuf::from(&val);
        if !p.as_os_str().is_empty() {
            let p = fs::canonicalize(&p).unwrap_or(p);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // 2. Adjacent to the current executable (Tauri app bundle directory).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(rife_worker_binary());
            if let Some(p) = normalized_existing_path(candidate) {
                return Some(p);
            }
        }
    }

    // 3. lib_dir fallback.
    if let Some(p) = normalized_existing_path(lib_dir.join(rife_worker_binary())) {
        return Some(p);
    }

    // 4. System PATH.
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if let Some(p) = normalized_existing_path(dir.join(rife_worker_binary())) {
                return Some(p);
            }
        }
    }

    None
}

/// Resolve the filesystem path to a VS-RIFE model directory.
///
/// Checks `RIFE_MODEL_PATH_ENV` first; otherwise searches `lib_dir/vs-rife/models/`
/// for each model name listed in `RIFE_DEFAULT_MODEL_DIRS` (in preference order).
///
/// Returns `None` when no model directory is found.
pub fn resolve_rife_model_path(lib_dir: &Path) -> Option<PathBuf> {
    // Env override.
    if let Ok(val) = std::env::var(RIFE_MODEL_PATH_ENV) {
        let p = PathBuf::from(&val);
        if !p.as_os_str().is_empty() {
            let p = fs::canonicalize(&p).unwrap_or(p);
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    let models_base = vs_rife_model_install_dir(lib_dir);
    for model_name in RIFE_DEFAULT_MODEL_DIRS {
        let candidate = models_base.join(model_name);
        if candidate.is_dir() {
            return Some(fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// FFmpeg resolution
// ---------------------------------------------------------------------------

/// Checks whether a usable FFmpeg installation can be found for setup.
///
/// This verifies availability according to the `Setup` resolution scope (environment
/// override and bundled locations; system `PATH` candidates are not considered).
///
/// # Returns
///
/// `true` if a usable FFmpeg installation was found for setup, `false` otherwise.
///
/// # Examples
///
pub(super) fn ffmpeg_install_ready(lib_dir: &Path) -> bool {
    resolve_ffmpeg_binary(lib_dir, BinaryResolutionScope::Setup)
        .ok()
        .flatten()
        .is_some()
}

/// Validate that an FFmpeg executable at the given path provides required filters and build flags.
///
/// Ensures the path points to a file, that the binary exposes the `libplacebo` filter, and that it
/// was built with `--enable-vulkan`, `--enable-libshaderc`, and `--enable-libplacebo`. On success
/// returns an `FfmpegInstallState`; on failure returns an error message describing the missing
/// requirement.
///
/// # Returns
///
/// `Ok(FfmpegInstallState)` if all checks pass, `Err(String)` describing the missing requirement otherwise.
///
/// # Examples
///
fn inspect_ffmpeg_binary(ffmpeg_path: &Path) -> Result<FfmpegInstallState, String> {
    if !ffmpeg_path.is_file() {
        return Err(format!(
            "FFmpeg binary {} is missing",
            ffmpeg_path.display()
        ));
    }

    let filters = run_ffmpeg_probe(&ffmpeg_path, &["-hide_banner", "-filters"], "filters")?;
    if !parse_ffmpeg_filter_support(&filters, "libplacebo") {
        return Err(format!(
            "FFmpeg binary {} is missing the libplacebo filter",
            ffmpeg_path.display()
        ));
    }

    let buildconf = run_ffmpeg_probe(&ffmpeg_path, &["-hide_banner", "-buildconf"], "buildconf")?;
    for flag in [
        "--enable-vulkan",
        "--enable-libshaderc",
        "--enable-libplacebo",
    ] {
        if !parse_ffmpeg_buildconf_flag(&buildconf, flag) {
            return Err(format!(
                "FFmpeg binary {} is missing required build flag {}",
                ffmpeg_path.display(),
                flag
            ));
        }
    }

    Ok(FfmpegInstallState)
}

/// Resolves a usable FFmpeg executable and its validated install state according to the given scope.
///
/// Tries an environment override (`FFMPEG_PATH_ENV`) first; if present, that path is validated and
/// an error is returned if it is unusable. Otherwise it checks a bundled binary adjacent to
/// `lib_dir`, and when `resolution_scope` is `Runtime` also scans candidates from the system `PATH`.
/// Only binaries that pass inspection are considered; when multiple validated candidates exist the
/// function selects the preferred one according to `BinaryResolutionScope` and `BinarySource`.
///
/// # Returns
/// `Ok(Some(ResolvedFfmpegBinary))` when a validated binary is selected, `Ok(None)` when no usable
/// binary is found, or `Err(String)` when an environment override was provided but proved unusable.
///
/// # Examples
///
pub(super) fn resolve_ffmpeg_binary(
    lib_dir: &Path,
    resolution_scope: BinaryResolutionScope,
) -> Result<Option<ResolvedFfmpegBinary>, String> {
    if let Some(path) = ffmpeg_env_override_path() {
        let install_state = inspect_ffmpeg_binary(&path).map_err(|error| {
            format!(
                "{} points to an unusable FFmpeg binary at {}: {}",
                FFMPEG_PATH_ENV,
                path.display(),
                error
            )
        })?;
        return Ok(Some(ResolvedFfmpegBinary {
            path,
            source: BinarySource::EnvOverride,
            install_state,
        }));
    }

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    if let Some(path) = normalized_existing_path(lib_dir.join(ffmpeg_binary())) {
        seen.insert(path.clone());
        if let Ok(install_state) = inspect_ffmpeg_binary(&path) {
            candidates.push(ResolvedFfmpegBinary {
                path,
                source: BinarySource::Bundled,
                install_state,
            });
        }
    }

    if matches!(resolution_scope, BinaryResolutionScope::Runtime) {
        for path in ffmpeg_path_candidates() {
            if !seen.insert(path.clone()) {
                continue;
            }
            if let Ok(install_state) = inspect_ffmpeg_binary(&path) {
                candidates.push(ResolvedFfmpegBinary {
                    path,
                    source: BinarySource::Path,
                    install_state,
                });
            }
        }
    }

    Ok(select_best_ffmpeg_candidate(candidates, resolution_scope))
}

/// Reads the `FFMPEG_PATH_ENV` environment variable and returns its path when set and not empty.
///
/// If the environment variable is present and not an empty string, returns a `PathBuf` for that
/// value. When possible the returned path is canonicalized; if canonicalization fails, the raw
/// provided path is returned unchanged.
///
/// # Examples
///
fn ffmpeg_env_override_path() -> Option<PathBuf> {
    let configured = std::env::var_os(FFMPEG_PATH_ENV)?;
    let path = PathBuf::from(configured);
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(fs::canonicalize(&path).unwrap_or(path))
    }
}

/// Scans the process PATH for existing ffmpeg executables and returns their unique paths.
///
/// This function examines each directory in the PATH environment variable, looks for the platform-specific
/// ffmpeg executable name in that directory, and includes only entries that refer to existing files.
/// Returned paths are deduplicated (preserving discovery order) and canonicalized when possible.
///
/// # Examples
///
fn ffmpeg_path_candidates() -> Vec<PathBuf> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(ffmpeg_binary());
        let Some(candidate) = normalized_existing_path(candidate) else {
            continue;
        };
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// Returns a canonicalized `PathBuf` when `path` exists and is a file, or `None` otherwise.
///
/// If canonicalization fails, the original `path` is returned.
///
/// # Examples
///
pub(super) fn normalized_existing_path(path: PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        Some(fs::canonicalize(&path).unwrap_or(path))
    } else {
        None
    }
}

/// Selects the most suitable FFmpeg candidate from a list, honoring the given resolution scope.
///
/// The function filters out candidates whose source is disallowed by `resolution_scope` and then
/// prefers a bundled candidate over others when choosing the best match.
///
/// # Returns
///
/// `Some(ResolvedFfmpegBinary)` with the chosen candidate if any remain after filtering, `None` otherwise.
///
/// # Examples
///
pub(super) fn select_best_ffmpeg_candidate(
    candidates: Vec<ResolvedFfmpegBinary>,
    resolution_scope: BinaryResolutionScope,
) -> Option<ResolvedFfmpegBinary> {
    candidates
        .into_iter()
        .filter(|candidate| resolution_scope.allows_source(candidate.source))
        .max_by_key(|candidate| matches!(candidate.source, BinarySource::Bundled))
}

/// Checks whether a directory name corresponds to a known VS-RIFE model directory.
pub(super) fn is_known_rife_model_dir_name(name: &str) -> bool {
    RIFE_DEFAULT_MODEL_DIRS
        .iter()
        .copied()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

// ---------------------------------------------------------------------------
// FFmpeg probing helpers
// ---------------------------------------------------------------------------

/// Run FFmpeg with the given probe arguments and capture its standard output.
///
/// Invokes the executable at `ffmpeg_path` with `args` and returns its stdout when the process exits successfully.
/// Errors are returned when the process cannot be started, when it exits with a non-success status, or when its stdout is not valid UTF-8.
///
/// # Examples
///
///
/// # Returns
/// `Ok(String)` containing FFmpeg's stdout on success; `Err(String)` with a descriptive error message otherwise.
fn run_ffmpeg_probe(ffmpeg_path: &Path, args: &[&str], probe_name: &str) -> Result<String, String> {
    let mut command = Command::new(ffmpeg_path);
    command.args(args);
    crate::process::hide_std_command_window(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("Failed to inspect FFmpeg {}: {}", probe_name, e))?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg {} probe failed with status {}",
            probe_name, output.status
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| format!("FFmpeg {} output was not valid UTF-8: {}", probe_name, e))
}

/// Returns `true` when the FFmpeg binary at `ffmpeg_path` exposes `filter_name` in its `-filters` output.
///
/// Invokes `ffmpeg -filters`, parses each line, and checks whether any line has `filter_name` as its
/// second whitespace-separated token. Returns `false` on any probe failure.
pub fn detect_ffmpeg_filter(ffmpeg_path: &Path, filter_name: &str) -> bool {
    match run_ffmpeg_probe(ffmpeg_path, &["-hide_banner", "-filters"], "filters") {
        Ok(output) => parse_ffmpeg_filter_support(&output, filter_name),
        Err(_) => false,
    }
}

/// Checks whether the FFmpeg filter listing contains a filter with the given name.
///
/// The function treats each line as whitespace-separated columns and matches `filter_name`
/// against the second column (index 1) exactly.
///
/// # Examples
///
///
/// # Returns
///
/// `true` if any line contains `filter_name` as the second whitespace-separated token, `false` otherwise.
fn parse_ffmpeg_filter_support(filters_output: &str, filter_name: &str) -> bool {
    filters_output
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(filter_name))
}

/// Determines whether an exact build-configuration flag is present in FFmpeg's `-buildconf` output.
///
/// # Examples
///
fn parse_ffmpeg_buildconf_flag(buildconf_output: &str, flag: &str) -> bool {
    buildconf_output
        .lines()
        .map(str::trim)
        .any(|line| line == flag)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        ffmpeg_asset_name_for_target, is_known_rife_model_dir_name, parse_ffmpeg_buildconf_flag,
        parse_ffmpeg_filter_support, select_best_ffmpeg_candidate, vceenc_asset_matches_target,
        BinaryResolutionScope, BinarySource, FfmpegInstallState, ResolvedFfmpegBinary,
    };
    use std::path::PathBuf;

    #[test]
    fn ffmpeg_asset_name_is_pinned_for_windows() {
        assert_eq!(ffmpeg_asset_name_for_target("windows"), Some("win64-gpl-"));
    }

    #[test]
    fn ffmpeg_asset_name_is_pinned_for_linux() {
        assert_eq!(ffmpeg_asset_name_for_target("linux"), Some("linux64-gpl-"));
    }

    /// Verifies that FFmpeg asset selection returns `None` for unsupported target OS values.
    ///
    /// # Examples
    ///
    #[test]
    fn ffmpeg_asset_name_is_none_for_unsupported_platforms() {
        assert_eq!(ffmpeg_asset_name_for_target("macos"), None);
    }

    #[test]
    fn is_known_rife_model_dir_name_recognizes_default_models() {
        assert!(is_known_rife_model_dir_name(
            "rife-v4.25-lite_ensembleFalse"
        ));
        assert!(is_known_rife_model_dir_name(
            "RIFE-V4.25-LITE_ENSEMBLEFALSE"
        ));
        assert!(!is_known_rife_model_dir_name("rife-v4.6"));
        assert!(!is_known_rife_model_dir_name("rife-HD"));
        assert!(!is_known_rife_model_dir_name("unknown-model"));
    }

    #[test]
    fn vceenc_asset_matcher_accepts_current_linux_package_name() {
        if cfg!(target_os = "linux") {
            assert!(vceenc_asset_matches_target("vceencc_9.05_amd64.deb"));
            assert!(!vceenc_asset_matches_target("vceencc-9.05-1.x86_64.rpm"));
        }
    }

    #[test]
    fn vceenc_asset_matcher_accepts_current_windows_archive_name() {
        if cfg!(target_os = "windows") {
            assert!(vceenc_asset_matches_target("vceencc_9.05_x64.7z"));
            assert!(!vceenc_asset_matches_target("vceencc_9.05_win32.7z"));
        }
    }

    #[test]
    fn parse_ffmpeg_filter_support_matches_exact_filter_name() {
        let filters_output = "\
 T.. scale             V->V       Scale the input video size and/or convert the image format.
 .. libplacebo        N->V       Apply various GPU filters from libplacebo
 T.. minterpolate      V->V       Fill missing frames using motion interpolation.
";
        assert!(parse_ffmpeg_filter_support(filters_output, "libplacebo"));
        assert!(!parse_ffmpeg_filter_support(filters_output, "rife"));
    }

    #[test]
    fn parse_ffmpeg_filter_support_does_not_match_partial_names() {
        let filters_output = "\
 T.. norife            V->V       Hypothetical filter containing rife substring.
 T.. sunrise           V->V       Another filter with rife in description.
";
        assert!(!parse_ffmpeg_filter_support(filters_output, "rife"));
    }

    #[test]
    fn parse_ffmpeg_buildconf_flag_matches_exact_lines() {
        let buildconf_output = "\
    --enable-vulkan
    --enable-libshaderc
    --enable-libplacebo
";
        assert!(parse_ffmpeg_buildconf_flag(
            buildconf_output,
            "--enable-vulkan"
        ));
        assert!(parse_ffmpeg_buildconf_flag(
            buildconf_output,
            "--enable-libshaderc"
        ));
        assert!(!parse_ffmpeg_buildconf_flag(
            buildconf_output,
            "--enable-opencl"
        ));
    }

    #[test]
    fn select_best_ffmpeg_candidate_prefers_bundled_binary_at_runtime() {
        let bundled = ResolvedFfmpegBinary {
            path: PathBuf::from("/tmp/bundled-ffmpeg"),
            source: BinarySource::Bundled,
            install_state: FfmpegInstallState,
        };
        let system = ResolvedFfmpegBinary {
            path: PathBuf::from("/usr/local/bin/ffmpeg"),
            source: BinarySource::Path,
            install_state: FfmpegInstallState,
        };

        let selected =
            select_best_ffmpeg_candidate(vec![bundled, system], BinaryResolutionScope::Runtime)
                .unwrap();
        assert_eq!(selected.path, PathBuf::from("/tmp/bundled-ffmpeg"));
    }

    #[test]
    fn select_best_ffmpeg_candidate_prefers_bundled_when_capabilities_match() {
        let bundled = ResolvedFfmpegBinary {
            path: PathBuf::from("/tmp/bundled-ffmpeg"),
            source: BinarySource::Bundled,
            install_state: FfmpegInstallState,
        };
        let system = ResolvedFfmpegBinary {
            path: PathBuf::from("/usr/bin/ffmpeg"),
            source: BinarySource::Path,
            install_state: FfmpegInstallState,
        };

        let selected =
            select_best_ffmpeg_candidate(vec![system, bundled], BinaryResolutionScope::Runtime)
                .unwrap();
        assert_eq!(selected.path, PathBuf::from("/tmp/bundled-ffmpeg"));
        assert_eq!(selected.source, BinarySource::Bundled);
    }

    #[test]
    fn setup_resolution_ignores_system_path_candidates() {
        let bundled = ResolvedFfmpegBinary {
            path: PathBuf::from("/tmp/bundled-ffmpeg"),
            source: BinarySource::Bundled,
            install_state: FfmpegInstallState,
        };
        let system = ResolvedFfmpegBinary {
            path: PathBuf::from("/usr/local/bin/ffmpeg"),
            source: BinarySource::Path,
            install_state: FfmpegInstallState,
        };

        let selected =
            select_best_ffmpeg_candidate(vec![system, bundled], BinaryResolutionScope::Setup)
                .unwrap();
        assert_eq!(selected.path, PathBuf::from("/tmp/bundled-ffmpeg"));
        assert_eq!(selected.source, BinarySource::Bundled);
    }
}
