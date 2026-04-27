use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

use crate::deps;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: String,
    pub name: Option<String>,
    pub backend: Option<String>,
    pub encoder_path: Option<PathBuf>,
    /// Path to the compatible FFmpeg binary used by the Universal executor.
    /// This may be the bundled fallback or a system/custom override.
    pub ffmpeg_path: Option<PathBuf>,
    /// Path to the compiled `rife-worker` binary used for frame interpolation.
    pub rife_worker_path: Option<PathBuf>,
    /// Resolved model directory passed to the `rife-worker` binary.
    pub rife_model_path: Option<PathBuf>,
    pub utilization: Option<u8>,
}

impl Default for GpuInfo {
    /// Creates a default `GpuInfo` with `vendor` set to `"unknown"` and all other optional fields set to `None`.
    ///
    /// # Examples
    ///
    fn default() -> Self {
        Self {
            vendor: "unknown".to_string(),
            name: None,
            backend: None,
            encoder_path: None,
            ffmpeg_path: None,
            rife_worker_path: None,
            rife_model_path: None,
            utilization: None,
        }
    }
}

/// Candidate filenames for the NVEncC encoder on Windows.
///
/// The slice contains executable names that may be present for NVEncC-based encoding.
///
/// # Examples
///
fn nvencc_binaries() -> &'static [&'static str] {
    &["NVEncC64.exe"] // Windows-only
}

/// Returns the list of candidate executable names for the AMD VCE encoder on the current platform.
///
/// # Examples
///
fn vceenc_binaries() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["VCEEncC64.exe"]
    } else {
        &["vceencc"]
    }
}

/// Searches `lib_dir` for the first file whose name matches one of `candidates` and returns its full path.
///
/// If no candidate is found and `log_missing` is `true`, an informational log entry is emitted listing the tried candidates.
///
/// # Examples
///
///
/// # Returns
///
/// `Some(PathBuf)` with the first existing candidate's full path, or `None` if none of the candidates exist.
fn find_encoder_binary(lib_dir: &Path, candidates: &[&str], log_missing: bool) -> Option<PathBuf> {
    for candidate in candidates {
        let path = lib_dir.join(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    if log_missing {
        info!(
            "[GPU]: Encoder not found in {:?} (tried: {:?})",
            lib_dir, candidates
        );
    }
    None
}

fn normalize_gpu_name(model: &str) -> Option<String> {
    let cleaned = model
        .replace("NVIDIA", "")
        .replace("nvidia", "")
        .replace("GeForce", "")
        .replace("geforce", "")
        .replace("Advanced Micro Devices", "")
        .replace("AMD", "")
        .replace("amd", "")
        .replace("Radeon", "")
        .replace("radeon", "")
        .replace("ATI", "")
        .replace("ati", "");
    let trimmed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Detects the system GPU and resolves compatible encoder, FFmpeg, and rife-worker paths.
///
/// Inspects the host GPU to determine vendor, normalized name, and when available utilization,
/// then resolves a compatible encoder backend, FFmpeg path, rife-worker binary, and model path.
///
/// The function returns a `GpuInfo` populated with detected `vendor`, `name`, optional
/// `backend`, `encoder_path`, `ffmpeg_path`, `rife_worker_path`, `rife_model_path`, and
/// `utilization`.
pub fn detect_compatible_gpu(lib_dir: &Path) -> GpuInfo {
    let (vendor, name, utilization) = detect_gpu_info();

    let ffmpeg_path = match deps::resolve_ffmpeg_path(lib_dir) {
        Ok(path) => path,
        Err(error) => {
            warn!("[GPU]: Failed to resolve compatible FFmpeg: {}", error);
            None
        }
    };

    let rife_worker_path = deps::resolve_rife_worker_path(lib_dir);
    let rife_model_path = deps::resolve_rife_model_path(lib_dir);

    if let Some(ref p) = rife_worker_path {
        info!("[GPU]: rife-worker available at {}.", p.display());
    }
    if rife_worker_path.is_some() && rife_model_path.is_none() {
        warn!("[GPU]: rife-worker found but no VS-RIFE model directory resolved — run setup to install models.");
    }

    // Prefer native vendor encoders when they are present; otherwise fall back
    // to the portable FFmpeg/libplacebo path.
    let (backend, encoder_path) = if cfg!(target_os = "windows") && vendor == "nvidia" {
        if let Some(path) = find_encoder_binary(lib_dir, nvencc_binaries(), false) {
            (Some("nvenc".to_string()), Some(path))
        } else if ffmpeg_path.is_some() {
            (Some("universal".to_string()), ffmpeg_path.clone())
        } else {
            (None, None)
        }
    } else if vendor == "amd" {
        if let Some(path) = find_encoder_binary(lib_dir, vceenc_binaries(), false) {
            (Some("vceenc".to_string()), Some(path))
        } else if ffmpeg_path.is_some() {
            (Some("universal".to_string()), ffmpeg_path.clone())
        } else {
            (None, None)
        }
    } else {
        if ffmpeg_path.is_some() {
            (Some("universal".to_string()), ffmpeg_path.clone())
        } else {
            (None, None)
        }
    };

    GpuInfo {
        vendor,
        name,
        backend,
        encoder_path,
        ffmpeg_path,
        rife_worker_path,
        rife_model_path,
        utilization,
    }
}

/// Detects the primary GPU vendor, a cleaned GPU name, and (for NVIDIA) current GPU utilization on Windows.
///
/// Returns a tuple (vendor, normalized_name, utilization). `vendor` will be `"nvidia"`, `"amd"`, or `"unknown"`. `normalized_name` is the cleaned GPU model name or `None` if unavailable. `utilization` is the GPU utilization percentage for NVIDIA cards when obtainable, otherwise `None`.
///
/// # Examples
///
#[cfg(target_os = "windows")]
fn detect_gpu_info() -> (String, Option<String>, Option<u8>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // Use PowerShell Get-CimInstance (wmic is deprecated/removed on modern Windows)
    match Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object Name, AdapterCompatibility | Format-List",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_name = String::new();
            let mut current_compat = String::new();
            let mut compat_was_set = false;

            for line in stdout.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("Name") {
                    current_name = val.trim_start_matches(|c: char| c == ' ' || c == ':').trim().to_string();
                } else if let Some(val) = line.strip_prefix("AdapterCompatibility") {
                    current_compat = val.trim_start_matches(|c: char| c == ' ' || c == ':').trim().to_lowercase();
                    compat_was_set = true;
                }

                // Evaluate when we've seen AdapterCompatibility (even if empty) and have a name.
                // This handles adapters where AdapterCompatibility is blank by falling back to the name alone.
                if !current_name.is_empty() && compat_was_set {
                    let name_lower = current_name.to_lowercase();
                    let vendor = if current_compat.contains("nvidia")
                        || name_lower.contains("nvidia")
                        || name_lower.contains("geforce")
                        || name_lower.contains("rtx")
                    {
                        Some("nvidia")
                    } else if current_compat.contains("amd")
                        || current_compat.contains("advanced micro")
                        || name_lower.contains("radeon")
                    {
                        Some("amd")
                    } else {
                        None
                    };

                    if let Some(v) = vendor {
                        let utilization = read_nvidia_utilization().filter(|_| v == "nvidia");
                        return (
                            v.to_string(),
                            normalize_gpu_name(&current_name),
                            utilization,
                        );
                    }

                    // Reset for next adapter
                    current_name.clear();
                    current_compat.clear();
                    compat_was_set = false;
                }
            }
            ("unknown".to_string(), None, None)
        }
        Err(e) => {
            warn!("[GPU]: Failed to run PowerShell for GPU detection: {}", e);
            ("unknown".to_string(), None, None)
        }
    }
}

/// Detects the system GPU vendor, a normalized GPU name if available, and an optional utilization value on Linux.
///
/// This returns the detected vendor identifier (`"nvidia"` or `"amd"` when detected, otherwise `"unknown"`),
/// the normalized GPU name when one can be extracted, and the GPU utilization percentage when available (only for NVIDIA).
///
/// # Examples
///
#[cfg(target_os = "linux")]
fn detect_gpu_info() -> (String, Option<String>, Option<u8>) {
    if let Some(info) = detect_nvidia_gpu_info() {
        return info;
    }

    // Use lspci to detect GPU on Linux
    match Command::new("lspci").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let lower = line.to_lowercase();
                if lower.contains("vga")
                    || lower.contains("3d controller")
                    || lower.contains("display")
                {
                    if lower.contains("nvidia") {
                        let name = extract_gpu_name_from_lspci(line);
                        return ("nvidia".to_string(), name, read_nvidia_utilization());
                    } else if lower.contains("amd")
                        || lower.contains("radeon")
                        || lower.contains("advanced micro")
                    {
                        let name = extract_gpu_name_from_lspci(line);
                        return ("amd".to_string(), name, None);
                    }
                }
            }
            ("unknown".to_string(), None, None)
        }
        Err(e) => {
            warn!("[GPU]: Failed to run lspci for GPU detection: {}", e);
            ("unknown".to_string(), None, None)
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_nvidia_gpu_info() -> Option<(String, Option<String>, Option<u8>)> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().find(|line| !line.trim().is_empty())?;
    let mut parts = first_line.splitn(2, ',');
    let raw_name = parts.next()?.trim();
    let utilization = parts
        .next()
        .and_then(|value| value.trim().parse::<u8>().ok());

    Some((
        "nvidia".to_string(),
        normalize_gpu_name(raw_name),
        utilization,
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn detect_gpu_info() -> (String, Option<String>, Option<u8>) {
    ("unknown".to_string(), None, None)
}

#[cfg(target_os = "linux")]
fn extract_gpu_name_from_lspci(line: &str) -> Option<String> {
    // Format: "XX:XX.X VGA compatible controller: NVIDIA Corporation GeForce RTX 4090 (rev XX)"
    if let Some(idx) = line.find(": ") {
        let raw = &line[idx + 2..];
        let cleaned = if let Some(rev_idx) = raw.rfind(" (rev") {
            &raw[..rev_idx]
        } else {
            raw
        };
        normalize_gpu_name(cleaned)
    } else {
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn read_nvidia_utilization() -> Option<u8> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=utilization.gpu",
        "--format=csv,noheader,nounits",
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.trim().parse::<u8>().ok()
        }
        Err(_) => None,
    }
}

/// Reads current GPU utilization without doing a full GPU detection pass.
/// Intended for frequent polling (e.g. once per second) in the status push loop.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn read_utilization(vendor: &str) -> Option<u8> {
    match vendor {
        "nvidia" => read_nvidia_utilization(),
        _ => None,
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn read_utilization(_vendor: &str) -> Option<u8> {
    None
}
