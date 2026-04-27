use std::fs;
use std::path::Path;
use tracing::info;

// ---------------------------------------------------------------------------
// Bundled Universal executor shaders
// ---------------------------------------------------------------------------

/// The Anime4K v3.2 upscale-only CNN x2 (Medium) shader, embedded at compile time.
const ANIME4K_UPSCALE_SHADER: &[u8] =
    include_bytes!("../../shaders/anime4k/Anime4K_Upscale_CNN_x2_M.glsl");
/// The ArtCNN x2 C4F16 shader, embedded at compile time.
const ARTCNN_UPSCALE_SHADER: &[u8] = include_bytes!("../../shaders/artcnn/ArtCNN_C4F16.glsl");

/// Writes bundled Universal executor GLSL shaders to `lib_dir/shaders/` if
/// they are not already present. This stays intentionally idempotent and
/// cheap: files are only written when absent.
pub fn ensure_universal_shaders(lib_dir: &Path) -> Result<(), String> {
    install_shader_asset(
        &lib_dir.join("shaders").join("anime4k"),
        "Anime4K_Upscale_CNN_x2_M.glsl",
        ANIME4K_UPSCALE_SHADER,
    )?;
    install_shader_asset(
        &lib_dir.join("shaders").join("artcnn"),
        "ArtCNN_C4F16.glsl",
        ARTCNN_UPSCALE_SHADER,
    )?;
    Ok(())
}

/// Ensures the target shader directory exists and installs the shader file when absent.
///
/// Creates `shader_dir` if needed and writes `contents` to `shader_dir/filename` only if the destination
/// file does not already exist. Returns an `Err(String)` describing the failure if directory creation
/// or file writing fails.
///
/// # Examples
///
///
/// # Returns
///
/// `Ok(())` on success, `Err(String)` with a diagnostic message on failure.
fn install_shader_asset(shader_dir: &Path, filename: &str, contents: &[u8]) -> Result<(), String> {
    fs::create_dir_all(shader_dir).map_err(|e| {
        format!(
            "Failed to create shader dir {}: {}",
            shader_dir.display(),
            e
        )
    })?;

    let dest = shader_dir.join(filename);
    if !dest.exists() {
        fs::write(&dest, contents)
            .map_err(|e| format!("Failed to write shader {}: {}", filename, e))?;
        info!(
            "[Deps/Shaders]: Installed {} to {}",
            filename,
            dest.display()
        );
    }
    Ok(())
}
