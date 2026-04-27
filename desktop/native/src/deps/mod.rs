use std::path::{Path, PathBuf};
use tracing::warn;

mod download;
mod install;
mod resolution;
mod shaders;

pub use resolution::{
    detect_ffmpeg_filter, resolve_ffmpeg_path, resolve_rife_model_path, resolve_rife_worker_path,
};
pub use shaders::ensure_universal_shaders;

/// NVEncC (Rigaya) — specialized Windows + NVIDIA encoder for the NGX-VSR / FRUC / TrueHDR path.
/// Only used on Windows; Linux NVIDIA routes through FFmpeg NVENC via the Universal executor.
const NVENCC_REPO: &str = "rigaya/NVEnc";
/// VCEEncC (Rigaya) — AMD native encoder path used when available.
const VCEENCC_REPO: &str = "rigaya/VCEEnc";
/// BtbN FFmpeg builds — pinned LGPL static assets for the release line we ship.
///
/// The floating `master` assets change frequently, so we select the exact
/// release-line archives we verified for our supported desktop targets.
const FFMPEG_REPO: &str = "BtbN/FFmpeg-Builds";
/// Tag in the styler00dollar/VapourSynth-RIFE-ncnn-Vulkan repo that ships the
/// RIFE model set we download for rife-worker at first-run.
const VS_RIFE_REPO: &str = "styler00dollar/VapourSynth-RIFE-ncnn-Vulkan";
const VS_RIFE_TAG: &str = "r9_mod_v33";
/// Optional absolute path to a custom FFmpeg binary that should be preferred
/// over the bundled fallback. The pointed-to binary must still expose
/// libplacebo/Vulkan support.
const FFMPEG_PATH_ENV: &str = "REFEREE_FFMPEG_PATH";
/// Optional absolute path to a custom `rife-worker` binary that should be
/// preferred over the one adjacent to the Tauri executable.
const RIFE_WORKER_PATH_ENV: &str = "REFEREE_RIFE_WORKER_PATH";
/// Optional absolute path to a RIFE model directory. When unset, model lookup
/// falls back to the vs-rife models downloaded at first-run.
const RIFE_MODEL_PATH_ENV: &str = "REFEREE_RIFE_MODEL_PATH";

/// VS-RIFE model directory names in preference order (most capable lite model first,
/// then additional fallbacks).
pub(super) const RIFE_DEFAULT_MODEL_DIRS: [&str; 8] = [
    "rife-v4.25-lite_ensembleFalse",
    "rife-v4.26_ensembleFalse",
    "rife-v4.25_ensembleFalse",
    "rife-v4.24_ensembleFalse",
    "rife-v4.22_lite_ensembleFalse",
    "rife-v4.22_ensembleFalse",
    "rife-v4.20_ensembleFalse",
    "rife-v4.17_lite_ensembleFalse",
];

/// NVEncC encoder executable filename for Windows.
fn nvencc_binary() -> &'static str {
    "NVEncC64.exe" // Windows-only
}

/// Determines the VCEEncC executable filename for the current platform.
fn vceenc_binary() -> &'static str {
    if cfg!(target_os = "windows") {
        "VCEEncC64.exe"
    } else {
        "vceencc"
    }
}

/// Selects the platform-specific FFmpeg executable filename.
fn ffmpeg_binary() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// Platform-specific `rife-worker` executable filename.
fn rife_worker_binary() -> &'static str {
    if cfg!(target_os = "windows") {
        "rife-worker.exe"
    } else {
        "rife-worker"
    }
}

/// Returns the directory where VS-RIFE models are installed inside `lib_dir`.
fn vs_rife_model_install_dir(lib_dir: &Path) -> PathBuf {
    lib_dir.join("vs-rife").join("models")
}

/// Check whether the required encoder binaries for a given GPU vendor are present.
///
/// FFmpeg with libplacebo must be available. For AMD, the VCEEncC binary must exist
/// in `lib_dir`. On Windows with NVIDIA, the NVEncC64 binary must exist in `lib_dir`.
/// Portable RIFE or other optional components are not considered by this check.
///
/// # Returns
///
/// `true` if all required binaries for the specified `gpu_vendor` are present, `false` otherwise.
pub fn binaries_ready(lib_dir: &Path, gpu_vendor: &str) -> bool {
    if !resolution::ffmpeg_install_ready(lib_dir) {
        return false;
    }
    if gpu_vendor == "amd" {
        return lib_dir.join(vceenc_binary()).exists();
    }
    // NVEncC is only needed on Windows + NVIDIA
    if cfg!(target_os = "windows") && gpu_vendor == "nvidia" {
        return lib_dir.join(nvencc_binary()).exists();
    }
    true
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupProgress {
    pub phase: String,
    pub percent: u32,
    pub detail: String,
}

/// Downloads and prepares required and optional native dependencies into `lib_dir`.
///
/// Always downloads FFmpeg with libplacebo. Conditionally downloads vendor encoder
/// binaries (NVEncC on Windows for NVIDIA, VCEEncC for AMD). VS-RIFE model files are
/// downloaded and treated as optional: failures are logged but do not prevent the
/// overall setup from succeeding.
///
/// # Errors
///
/// Returns `Err(String)` if creating `lib_dir` fails or if a required download
/// (FFmpeg or a vendor encoder) fails.
pub async fn download_binaries<F>(
    client: &reqwest::Client,
    lib_dir: &Path,
    gpu_vendor: &str,
    mut progress_cb: F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
{
    std::fs::create_dir_all(lib_dir).map_err(|e| format!("Failed to create lib dir: {}", e))?;

    // FFmpeg with libplacebo is always required for the Universal executor.
    download::download_ffmpeg(client, lib_dir, &mut progress_cb).await?;

    // NVEncC is only needed on Windows + NVIDIA for the specialized path.
    if cfg!(target_os = "windows") && gpu_vendor == "nvidia" {
        download::download_nvencc(client, lib_dir, &mut progress_cb).await?;
    } else if gpu_vendor == "amd" {
        download::download_vceenc(client, lib_dir, &mut progress_cb).await?;
    }

    // RIFE model files (VS-RIFE) are optional; missing models fall back to
    // FFmpeg minterpolate interpolation.
    if let Err(error) = download::download_rife_models(client, lib_dir, &mut progress_cb).await {
        warn!("[Deps]: VS-RIFE model download skipped: {}", error);
    }

    progress_cb(SetupProgress {
        phase: "done".to_string(),
        percent: 100,
        detail: "Setup complete".to_string(),
    });

    Ok(())
}
