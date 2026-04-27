use std::fs;
use std::path::Path;
use tracing::info;

use super::install::{
    extract_7z, extract_deb, extract_tar_gz, extract_tar_xz, extract_zip,
    install_flat_binary_files, install_vs_rife_models,
};
use super::resolution::{
    current_ffmpeg_asset_name, resolve_ffmpeg_binary, resolve_rife_model_path,
    BinaryResolutionScope, BinarySource,
};
use super::{
    vs_rife_model_install_dir, SetupProgress, FFMPEG_PATH_ENV, FFMPEG_REPO, NVENCC_REPO,
    VCEENCC_REPO, VS_RIFE_REPO, VS_RIFE_TAG,
};

// ---------------------------------------------------------------------------
// Per-binary download wrappers
// ---------------------------------------------------------------------------

/// Ensures a compatible FFmpeg binary is available in `lib_dir`, downloading and installing it if necessary.
///
/// Attempts to resolve an existing compatible FFmpeg (environment override, bundled, or system path) and returns immediately if found. If no compatible binary is available, selects the platform-appropriate GitHub release asset and downloads and installs it into `lib_dir`.
///
/// # Returns
///
/// `Ok(())` on success, or `Err(String)` with a human-readable error message on failure.
///
/// # Examples
///
pub(super) async fn download_ffmpeg<F>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress_cb: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
{
    if let Some(resolved) = resolve_ffmpeg_binary(lib_dir, BinaryResolutionScope::Setup)? {
        match resolved.source {
            BinarySource::EnvOverride => info!(
                "[Deps/Download]: Using compatible FFmpeg from {} at {}",
                FFMPEG_PATH_ENV,
                resolved.path.display()
            ),
            BinarySource::Bundled => info!(
                "[Deps/Download]: Compatible bundled FFmpeg already present at {}",
                resolved.path.display()
            ),
            BinarySource::Path => info!(
                "[Deps/Download]: Using compatible system FFmpeg at {}",
                resolved.path.display()
            ),
        }
        return Ok(());
    }

    let asset_name = current_ffmpeg_asset_name().ok_or_else(|| {
        "Bundled FFmpeg downloads are only configured for Windows and Linux".to_string()
    })?;
    download_release_binary(
        client,
        lib_dir,
        FFMPEG_REPO,
        "FFmpeg",
        "ffmpeg",
        move |name: &str| {
            let lower = name.to_lowercase();
            lower.contains(asset_name) && !lower.contains("shared")
        },
        install_flat_binary_files,
        progress_cb,
    )
    .await?;

    Ok(())
}

/// Downloads and installs the NVEncC encoder from its GitHub releases.
///
/// This selects the first release asset whose filename contains `"_x64.7z"`,
/// downloads and extracts it, and installs its binaries into `lib_dir`,
/// reporting progress via `progress_cb`.
///
/// # Parameters
///
/// - `progress_cb`: Callback invoked with `SetupProgress` updates during download and extraction.
///
/// # Returns
///
/// `Ok(())` on successful download and installation, or `Err(String)` with a human-readable
/// error message on failure.
///
/// # Examples
///
pub(super) async fn download_nvencc<F>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress_cb: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
{
    let asset_matcher = |name: &str| -> bool { name.contains("_x64.7z") };
    download_release_binary(
        client,
        lib_dir,
        NVENCC_REPO,
        "NVEncC",
        "encoder",
        asset_matcher,
        install_flat_binary_files,
        progress_cb,
    )
    .await
}

/// Downloads and installs the VCEEncC encoder distribution for the current platform.
///
/// This attempts to fetch a compatible release asset from the VCEEncC GitHub releases,
/// downloads and extracts the chosen archive, and installs executable files into `lib_dir`.
///
/// # Examples
///
///
/// # Returns
///
/// `Ok(())` on success, `Err(String)` with a human-readable error message on failure.
pub(super) async fn download_vceenc<F>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress_cb: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
{
    download_release_binary(
        client,
        lib_dir,
        VCEENCC_REPO,
        "VCEEncC",
        "encoder",
        |name: &str| super::resolution::vceenc_asset_matches_target(name),
        install_flat_binary_files,
        progress_cb,
    )
    .await
}

/// Downloads VS-RIFE model directories into `lib_dir` if they are not already present.
///
/// Checks whether a model is already resolved (env override or installed); if so, logs and
/// returns immediately. Otherwise downloads the VS-RIFE source archive for `VS_RIFE_TAG`
/// from GitHub and extracts the `models/` subdirectory into `lib_dir/vs-rife/models/`.
///
/// # Returns
///
/// `Ok(())` on success, `Err(String)` on network or I/O failure.
pub(super) async fn download_rife_models<F>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress_cb: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
{
    if let Some(model_path) = resolve_rife_model_path(lib_dir) {
        info!(
            "[Deps/Download]: VS-RIFE model directory already present at {}",
            model_path.display()
        );
        return Ok(());
    }

    let models_dir = vs_rife_model_install_dir(lib_dir);
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create VS-RIFE models directory: {}", e))?;

    let download_url = format!(
        "https://github.com/{}/archive/refs/tags/{}.tar.gz",
        VS_RIFE_REPO, VS_RIFE_TAG
    );

    info!(
        "[Deps/Download]: Downloading VS-RIFE models from {}...",
        download_url
    );
    progress_cb(SetupProgress {
        phase: "rife".to_string(),
        percent: 0,
        detail: "Downloading VS-RIFE models...".to_string(),
    });

    let mut response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download VS-RIFE archive: {}", e))?;

    let total_size = response.content_length().unwrap_or(0);
    let mut buffer = Vec::with_capacity(total_size as usize);
    let mut downloaded: u64 = 0;
    let mut last_reported: u32 = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download failed: {}", e))?
    {
        downloaded += chunk.len() as u64;
        buffer.extend_from_slice(&chunk);

        if total_size > 0 {
            let pct = ((downloaded * 90) / total_size) as u32;
            if pct > last_reported {
                last_reported = pct;
                progress_cb(SetupProgress {
                    phase: "rife".to_string(),
                    percent: pct,
                    detail: "Downloading VS-RIFE models...".to_string(),
                });
            }
        }
    }

    progress_cb(SetupProgress {
        phase: "rife".to_string(),
        percent: 90,
        detail: "Extracting VS-RIFE models...".to_string(),
    });

    let temp_archive = models_dir.join("vs-rife.tar.gz");
    let temp_dir = models_dir.join("vs-rife_temp");

    fs::write(&temp_archive, &buffer)
        .map_err(|e| format!("Failed to write VS-RIFE archive: {}", e))?;
    fs::create_dir_all(&temp_dir).ok();

    extract_tar_gz(&temp_archive, &temp_dir)?;
    install_vs_rife_models(&temp_dir, &models_dir, "VS-RIFE models")?;

    let _ = fs::remove_file(&temp_archive);
    let _ = fs::remove_dir_all(&temp_dir);

    info!("[Deps/Download]: VS-RIFE models installed successfully.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Generic GitHub release download engine
// ---------------------------------------------------------------------------

/// Downloads the latest compatible release asset from a GitHub repository, extracts it, and installs its contents into `lib_dir`.
///
/// The function fetches the repository's latest release metadata, selects the first asset for which `asset_matcher` returns `true`,
/// streams and writes the asset to a temporary archive, extracts the archive (supports `.zip`, `.7z`, `.deb`, and `.tar.xz`), calls
/// `installer` to place relevant files into `lib_dir`, and performs cleanup. Progress updates are emitted via `progress_cb` using `phase`.
///
/// # Returns
///
/// `Ok(())` on successful download, extraction, and installation; `Err(String)` with a human-readable message on failure.
///
/// # Examples
///
async fn download_release_binary<F, M, I>(
    client: &reqwest::Client,
    lib_dir: &Path,
    repo: &str,
    label: &str,
    phase: &str,
    asset_matcher: M,
    installer: I,
    progress_cb: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
    M: Fn(&str) -> bool + Send + Sync,
    I: Fn(&Path, &Path, &str) -> Result<(), String> + Send + Sync,
{
    fs::create_dir_all(lib_dir)
        .map_err(|e| format!("Failed to create install directory for {}: {}", label, e))?;
    info!("[Deps/Download]: Downloading {}...", label);
    progress_cb(SetupProgress {
        phase: phase.to_string(),
        percent: 0,
        detail: format!("Fetching {} release info...", label),
    });

    let release_url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let release: serde_json::Value = client
        .get(&release_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    let assets = release["assets"].as_array().ok_or("No assets in release")?;

    let asset = assets.iter().find(|a| {
        let name = a["name"].as_str().unwrap_or("").to_lowercase();
        asset_matcher(&name)
    });

    let asset = asset.ok_or(format!(
        "No compatible {} asset found for this platform",
        label
    ))?;

    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or("Missing download URL")?;

    progress_cb(SetupProgress {
        phase: phase.to_string(),
        percent: 0,
        detail: format!("Downloading {}...", label),
    });

    let temp_archive = lib_dir.join(format!("{}.archive", label.to_lowercase()));
    let temp_dir = lib_dir.join(format!("{}_temp", label.to_lowercase()));

    let mut response = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    let total_size = response.content_length().unwrap_or(0);
    info!(
        "[Deps/Download]: Downloading {} ({} bytes)...",
        download_url, total_size
    );

    let mut buffer = Vec::with_capacity(total_size as usize);
    let mut downloaded: u64 = 0;
    let mut last_reported: u32 = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download failed: {}", e))?
    {
        downloaded += chunk.len() as u64;
        buffer.extend_from_slice(&chunk);

        if total_size > 0 {
            // Scale download to 0–90%; reserve 90–100% for extraction
            let pct = ((downloaded * 90) / total_size) as u32;
            if pct > last_reported {
                last_reported = pct;
                progress_cb(SetupProgress {
                    phase: phase.to_string(),
                    percent: pct,
                    detail: format!("Downloading {}...", label),
                });
            }
        }
    }

    fs::write(&temp_archive, &buffer).map_err(|e| format!("Failed to write archive: {}", e))?;

    progress_cb(SetupProgress {
        phase: phase.to_string(),
        percent: 90,
        detail: format!("Extracting {}...", label),
    });

    // Extract based on file type
    fs::create_dir_all(&temp_dir).ok();
    let archive_name = asset["name"].as_str().unwrap_or("").to_lowercase();

    info!(
        "[Deps/Download]: Extracting archive: {} ({} bytes)",
        archive_name,
        buffer.len()
    );
    if archive_name.ends_with(".zip") {
        extract_zip(&temp_archive, &temp_dir)?;
    } else if archive_name.ends_with(".7z") {
        extract_7z(&temp_archive, &temp_dir)?;
    } else if archive_name.ends_with(".deb") {
        extract_deb(&temp_archive, &temp_dir)?;
    } else if archive_name.ends_with(".tar.xz") {
        extract_tar_xz(&temp_archive, &temp_dir)?;
    } else {
        return Err(format!("Unsupported archive format: {}", archive_name));
    }

    // Find and copy relevant files
    installer(&temp_dir, lib_dir, label)?;

    // Cleanup
    let _ = fs::remove_file(&temp_archive);
    let _ = fs::remove_dir_all(&temp_dir);

    info!("[Deps/Download]: {} installed successfully.", label);
    Ok(())
}
