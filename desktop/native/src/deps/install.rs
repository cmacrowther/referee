use std::fs;
use std::path::Path;
use std::process::Command;
use tracing::info;

use super::resolution::is_known_rife_model_dir_name;

// ---------------------------------------------------------------------------
// Archive extraction
// ---------------------------------------------------------------------------

/// Extracts the contents of a `.7z` archive into the specified destination directory.
///
/// Attempts to decompress `archive` into `dest` and returns an error message if decompression fails.
///
/// # Returns
///
/// `Ok(())` if extraction succeeds, `Err(String)` with a formatted failure message otherwise.
///
/// # Examples
///
pub(super) fn extract_7z(archive: &Path, dest: &Path) -> Result<(), String> {
    sevenz_rust::decompress_file(archive, dest)
        .map_err(|e| format!("Failed to extract 7z archive: {}", e))?;
    Ok(())
}

/// Extracts a `.tar.xz` archive into the specified destination directory.
///
/// Attempts to open `archive`, decode its XZ-compressed tar stream, and unpack its contents into `dest`.
///
/// # Returns
///
/// `Ok(())` on success, `Err(String)` with a contextual error message on failure.
///
/// # Examples
///
pub(super) fn extract_tar_xz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("Failed to open archive: {}", e))?;
    let xz = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(xz);
    tar.unpack(dest)
        .map_err(|e| format!("Failed to extract tar.xz: {}", e))?;
    Ok(())
}

/// Extracts a `.tar.gz` archive into the specified destination directory.
pub(super) fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("Failed to open archive: {}", e))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dest)
        .map_err(|e| format!("Failed to extract tar.gz: {}", e))?;
    Ok(())
}

/// Extracts the Debian `data.tar*` payload from a `.deb` archive into `dest`.
///
/// This function attempts to locate one of the supported payload members
/// (`data.tar.xz`, `data.tar.gz`, `data.tar.zst`, `data.tar`) by running
/// the system `ar` tool to read each member. When a member is successfully
/// retrieved, it is decompressed (as needed) and unpacked into `dest`.
///
/// # Returns
///
/// `Ok(())` on successful extraction, `Err(String)` with a human-readable error
/// message if extraction fails or no supported payload is found.
///
/// # Examples
///
pub(super) fn extract_deb(archive: &Path, dest: &Path) -> Result<(), String> {
    let package_data = ["data.tar.xz", "data.tar.gz", "data.tar.zst", "data.tar"];
    let mut last_error: Option<String> = None;

    for member in package_data {
        let output = Command::new("ar")
            .args(["p"])
            .arg(archive)
            .arg(member)
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let cursor = std::io::Cursor::new(output.stdout);
                match member {
                    "data.tar.xz" => {
                        let decoder = xz2::read::XzDecoder::new(cursor);
                        let mut tar = tar::Archive::new(decoder);
                        tar.unpack(dest).map_err(|e| {
                            format!("Failed to extract deb payload {}: {}", member, e)
                        })?;
                    }
                    "data.tar.gz" => {
                        let decoder = flate2::read::GzDecoder::new(cursor);
                        let mut tar = tar::Archive::new(decoder);
                        tar.unpack(dest).map_err(|e| {
                            format!("Failed to extract deb payload {}: {}", member, e)
                        })?;
                    }
                    "data.tar.zst" => {
                        let decoder = zstd::stream::read::Decoder::new(cursor).map_err(|e| {
                            format!("Failed to decode deb payload {}: {}", member, e)
                        })?;
                        let mut tar = tar::Archive::new(decoder);
                        tar.unpack(dest).map_err(|e| {
                            format!("Failed to extract deb payload {}: {}", member, e)
                        })?;
                    }
                    "data.tar" => {
                        let mut tar = tar::Archive::new(cursor);
                        tar.unpack(dest).map_err(|e| {
                            format!("Failed to extract deb payload {}: {}", member, e)
                        })?;
                    }
                    _ => unreachable!(),
                }
                return Ok(());
            }
            Ok(output) => {
                last_error = Some(format!(
                    "ar could not read {} from {} (status {})",
                    member,
                    archive.display(),
                    output.status
                ));
            }
            Err(error) => {
                last_error = Some(format!(
                    "Failed to run ar while extracting {}: {}",
                    archive.display(),
                    error
                ));
                break;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format!(
            "Could not locate a supported data.tar payload in {}",
            archive.display()
        )
    }))
}

/// Extracts a ZIP archive into the given destination directory.
///
/// Each entry's path is validated for safety; directories are created and files are written
/// preserving archive structure. Returns `Err` with a descriptive message if opening the
/// archive, reading an entry, creating directories, or writing files fails.
///
/// # Examples
///
pub(super) fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| format!("Failed to open zip archive: {}", e))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip archive: {}", e))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {}", e))?;
        let out_path = dest.join(entry.enclosed_name().ok_or("Zip entry has unsafe path")?);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("Failed to create dir: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
            }
            let mut out_file =
                fs::File::create(&out_path).map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| format!("Failed to write zip entry: {}", e))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Installation helpers
// ---------------------------------------------------------------------------

/// Installs platform-appropriate flat binary files from a source directory into a destination directory.
///
/// This recursively traverses `source_dir`, copying files that match platform-specific criteria:
/// on Windows, files with `exe` or `dll` extensions; on non-Windows, files with no extension that do not start with `.`.
/// Copied files retain their filename and are placed directly in `dest_dir`. On Unix systems, copied files are
/// given mode `0o755`.
///
/// # Parameters
///
/// - `source_dir`: Directory to search for candidate binary files (recursively).
/// - `dest_dir`: Destination directory where matching files will be copied.
///
/// # Returns
///
/// `Ok(())` on success, or `Err(String)` with a contextual message if reading directories, copying files,
/// or setting permissions fails.
///
/// # Examples
///
pub(super) fn install_flat_binary_files(
    source_dir: &Path,
    dest_dir: &Path,
    label: &str,
) -> Result<(), String> {
    let entries =
        fs::read_dir(source_dir).map_err(|e| format!("Failed to read temp dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            install_flat_binary_files(&path, dest_dir, label)?;
        } else {
            let name = entry.file_name().to_string_lossy().to_string();
            let ext = Path::new(&name)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            let should_keep = if cfg!(target_os = "windows") {
                matches!(ext.as_str(), "exe" | "dll")
            } else {
                // On Linux, executables have no extension (e.g. ffmpeg, ffprobe).
                ext.is_empty() && !name.starts_with('.')
            };

            if should_keep {
                let dest = dest_dir.join(&name);
                fs::copy(&path, &dest).map_err(|e| format!("Failed to copy {}: {}", name, e))?;
                info!("[Deps/Install]: Installed {}", name);

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))
                        .map_err(|e| format!("Failed to set permissions on {}: {}", name, e))?;
                }
            }
        }
    }
    Ok(())
}

/// Installs VS-RIFE model directories from the extracted source archive into `dest_dir`.
///
/// Recursively searches `source_dir` for a directory named `"models"` or individual directories
/// whose names match known VS-RIFE model names, then copies each into `dest_dir`.
///
/// # Returns
///
/// `Ok(())` if all model directories were copied successfully, `Err(String)` otherwise.
pub(super) fn install_vs_rife_models(
    source_dir: &Path,
    dest_dir: &Path,
    label: &str,
) -> Result<(), String> {
    fs::create_dir_all(dest_dir).map_err(|e| format!("Failed to create {} dir: {}", label, e))?;
    copy_vs_rife_model_dirs(source_dir, dest_dir)
}

/// Recursively searches `source_dir` for directories named `"models"` or known VS-RIFE model
/// directory names and copies each matched directory into `dest_dir/<name>`.
fn copy_vs_rife_model_dirs(source_dir: &Path, dest_dir: &Path) -> Result<(), String> {
    let entries =
        fs::read_dir(source_dir).map_err(|e| format!("Failed to read temp dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_known_rife_model_dir_name(&name) {
            let dest = dest_dir.join(&name);
            copy_directory_recursive(&path, &dest)?;
            info!(
                "[Deps/Install]: Installed VS-RIFE model directory {}",
                dest.display()
            );
        } else {
            // Recurse into any container directory (including "models/", archive root, etc.)
            copy_vs_rife_model_dirs(&path, dest_dir)?;
        }
    }

    Ok(())
}

/// Recursively copies the contents of `source_dir` into `dest_dir`, creating directories as needed.
///
/// Copies all files and subdirectories from `source_dir` into `dest_dir`, preserving the
/// directory structure. Returns an `Err` with a contextual message if any filesystem operation fails.
///
/// # Examples
///
pub(super) fn copy_directory_recursive(source_dir: &Path, dest_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed to create directory {}: {}", dest_dir.display(), e))?;

    let entries = fs::read_dir(source_dir)
        .map_err(|e| format!("Failed to read {}: {}", source_dir.display(), e))?;

    for entry in entries.flatten() {
        let source = entry.path();
        let dest = dest_dir.join(entry.file_name());
        if source.is_dir() {
            copy_directory_recursive(&source, &dest)?;
        } else {
            fs::copy(&source, &dest).map_err(|e| {
                format!(
                    "Failed to copy {} to {}: {}",
                    source.display(),
                    dest.display(),
                    e
                )
            })?;
        }
    }

    Ok(())
}
