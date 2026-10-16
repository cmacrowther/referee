//! Installs the EasyProxy server REFEREE runs on demand: a standalone CPython
//! build, one pinned EasyProxy commit, and its Python dependencies at pinned
//! versions. The install is built in a staging directory beside the live one
//! and swapped in as a whole, so a failed or interrupted update keeps the
//! previous install.
//!
//! Layout under `{lib_dir}/easyproxy/`: `python/` (runtime and packages),
//! `app/` (EasyProxy source), `referee_launcher.py`, and `install.json`.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tracing::{debug, info, warn};

use super::download::{download_verified, extract_archive, VerifiedDownload};
use super::SetupProgress;

/// EasyProxy publishes no releases, so REFEREE pins a commit. Bumping it means
/// updating the archive digest, `EASY_PROXY_VERSION`, and `constraints.txt`.
const EASY_PROXY_COMMIT: &str = "1d4ad763408da228fa5d67b8dbb6a50e77e5b064";
/// `APP_VERSION` at `EASY_PROXY_COMMIT`.
pub const EASY_PROXY_VERSION: &str = "2.13.35";
const EASY_PROXY_ARCHIVE_DIGEST: &str =
    "sha256:30a661ec1aa25a291574b6db885a3df3d6edbf002c82eaa89f2fdf356d11c1dc";
/// Not needed to run the server, and most of the archive's size.
const EASY_PROXY_UNUSED_FILES: &[&str] = &["easyproxy.apk"];

/// A python-build-standalone `install_only` archive.
struct PythonBuild {
    file: &'static str,
    size: u64,
    digest: &'static str,
}

const PYTHON_RELEASE: &str = "20261001";
#[cfg(target_os = "windows")]
const PYTHON_BUILD: Option<PythonBuild> = Some(PythonBuild {
    file: "cpython-3.12.15+20261001-x86_64-pc-windows-msvc-install_only.tar.gz",
    size: 46_415_849,
    digest: "sha256:62faca756c1b2e9b2454b6ad9396a6890496aebf7479a0c93e1ed615c67f2985",
});
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PYTHON_BUILD: Option<PythonBuild> = Some(PythonBuild {
    file: "cpython-3.12.15+20261001-x86_64-unknown-linux-gnu-install_only.tar.gz",
    size: 66_932_330,
    digest: "sha256:0e56475e23f61a9c7d254fe16bc4931544026d591fc1cb8b770712ee94fa6260",
});
#[cfg(not(any(
    target_os = "windows",
    all(target_os = "linux", target_arch = "x86_64")
)))]
const PYTHON_BUILD: Option<PythonBuild> = None;

const CONSTRAINTS: &str = include_str!("constraints.txt");
const LAUNCHER: &str = include_str!("launcher.py");
/// Requirements published only as source. Everything else must install from
/// a wheel, so a missing wheel fails fast instead of needing a compiler.
const SOURCE_ONLY_PACKAGES: &str = "construct";
/// One module per requirement, imported to validate a staged install.
const SMOKE_TEST: &str = "import aiohttp, aiohttp_socks, dotenv, zstandard, bs4, lxml, \
     python_socks, Crypto, cryptography, nacl, pydash2hls, PIL, curl_cffi, brotli, psutil, \
     pywidevine";

const INSTALL_DIR: &str = "easyproxy";
const STAGE_PREFIX: &str = ".easyproxy-stage-";
const RETIRED_PREFIX: &str = ".easyproxy-old-";
const LOCK_FILE: &str = ".easyproxy.lock";
const MARKER_FILE: &str = "install.json";
const LAUNCHER_FILE: &str = "referee_launcher.py";
const PIP_TIMEOUT: Duration = Duration::from_secs(900);
const SMOKE_TEST_TIMEOUT: Duration = Duration::from_secs(60);
const PHASE: &str = "easyproxy";
const ERROR_TAIL_LINES: usize = 12;

/// What `install.json` records, compared with the pins compiled into REFEREE.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct InstallMarker {
    commit: String,
    python: String,
    constraints_sha256: String,
}

impl InstallMarker {
    fn current() -> Option<Self> {
        PYTHON_BUILD.as_ref().map(|build| Self {
            commit: EASY_PROXY_COMMIT.to_string(),
            python: build.file.to_string(),
            constraints_sha256: format!("{:x}", Sha256::digest(CONSTRAINTS)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallState {
    /// No Python build is pinned for this platform.
    Unsupported,
    Missing,
    /// Installed by a REFEREE version with different pins.
    Outdated,
    Current,
}

pub fn install_dir(lib_dir: &Path) -> PathBuf {
    lib_dir.join(INSTALL_DIR)
}

pub fn python_path(install_dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        install_dir.join("python").join("python.exe")
    } else {
        install_dir.join("python").join("bin").join("python3")
    }
}

/// Short form of the pinned commit, for display.
pub fn pinned_commit() -> &'static str {
    &EASY_PROXY_COMMIT[..7]
}

pub fn install_state(lib_dir: &Path) -> InstallState {
    let Some(current) = InstallMarker::current() else {
        return InstallState::Unsupported;
    };
    let dir = install_dir(lib_dir);
    if !dir.exists() {
        return InstallState::Missing;
    }
    let marker = fs::read(dir.join(MARKER_FILE))
        .ok()
        .and_then(|data| serde_json::from_slice::<InstallMarker>(&data).ok());
    match marker {
        Some(marker) if marker == current && python_path(&dir).is_file() => InstallState::Current,
        _ => InstallState::Outdated,
    }
}

/// Writes the launcher into the install when it differs from this build's,
/// so a REFEREE update can change it without a reinstall.
pub fn write_launcher(install_dir: &Path) -> Result<PathBuf, String> {
    let path = install_dir.join(LAUNCHER_FILE);
    if fs::read_to_string(&path).ok().as_deref() != Some(LAUNCHER) {
        fs::write(&path, LAUNCHER)
            .map_err(|error| format!("Cannot write the EasyProxy launcher: {error}"))?;
    }
    Ok(path)
}

/// Downloads, installs, and validates EasyProxy, then swaps it in for any
/// existing install. The caller must have stopped the managed process: a
/// running Python holds files open, and Windows cannot move them.
pub async fn install<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress: &mut F,
) -> Result<(), String> {
    let build = PYTHON_BUILD
        .as_ref()
        .ok_or("EasyProxy can only be installed on Windows and Linux x86_64.")?;
    let _lock = lock(lib_dir)?;
    recover(lib_dir);
    let stage = Stage::new(lib_dir)?;

    let python_url = format!(
        "https://github.com/astral-sh/python-build-standalone/releases/download/{PYTHON_RELEASE}/{}",
        build.file.replace('+', "%2B")
    );
    let python_archive = stage.0.join("python.tar.gz");
    let python_download = VerifiedDownload {
        url: &python_url,
        size: Some(build.size),
        digest: Some(build.digest),
        label: "Python",
        phase: PHASE,
    };
    download_verified(
        client,
        &python_download,
        &python_archive,
        &mut scaled(progress, 0, 35),
    )
    .await?;
    report(progress, 35, "Extracting Python...");
    extract(&python_archive, &stage.0).await?;

    let source_url =
        format!("https://codeload.github.com/realbestia1/EasyProxy/tar.gz/{EASY_PROXY_COMMIT}");
    let source_archive = stage.0.join("easyproxy.tar.gz");
    let source_download = VerifiedDownload {
        url: &source_url,
        size: None,
        digest: Some(EASY_PROXY_ARCHIVE_DIGEST),
        label: "EasyProxy",
        phase: PHASE,
    };
    download_verified(
        client,
        &source_download,
        &source_archive,
        &mut scaled(progress, 40, 45),
    )
    .await?;
    let source = stage.0.join("source");
    fs::create_dir(&source).map_err(|error| error.to_string())?;
    extract(&source_archive, &source).await?;
    let app_dir = stage.0.join("app");
    fs::rename(
        source.join(format!("EasyProxy-{EASY_PROXY_COMMIT}")),
        &app_dir,
    )
    .map_err(|error| format!("Unexpected EasyProxy archive layout: {error}"))?;
    for unused in EASY_PROXY_UNUSED_FILES {
        let _ = fs::remove_file(app_dir.join(unused));
    }
    let _ = fs::remove_dir_all(&source);

    report(progress, 45, "Installing Python packages...");
    let constraints = stage.0.join("constraints.txt");
    fs::write(&constraints, CONSTRAINTS).map_err(|error| error.to_string())?;
    let python = python_path(&stage.0);
    install_packages(
        &python,
        &app_dir.join("requirements.txt"),
        &constraints,
        progress,
    )
    .await?;

    report(progress, 95, "Validating EasyProxy...");
    smoke_test(&python).await?;
    write_launcher(&stage.0)?;
    let marker = InstallMarker::current().expect("checked with PYTHON_BUILD above");
    fs::write(
        stage.0.join(MARKER_FILE),
        serde_json::to_vec_pretty(&marker).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Cannot write the EasyProxy install record: {error}"))?;

    swap_in(lib_dir, &stage.0)?;
    info!(
        "[Deps]: Installed EasyProxy {} ({}) with {}",
        EASY_PROXY_VERSION,
        pinned_commit(),
        build.file
    );
    report(progress, 100, "EasyProxy is installed");
    Ok(())
}

/// Deletes the install. EasyProxy's own data directory is kept. The caller
/// must have stopped the managed process.
pub fn remove(lib_dir: &Path) -> Result<(), String> {
    let _lock = lock(lib_dir)?;
    let live = install_dir(lib_dir);
    if !live.exists() {
        return Ok(());
    }
    // Moving first keeps a half-deleted tree from looking like an install.
    let retired = retired_dir(lib_dir);
    fs::rename(&live, &retired)
        .map_err(|error| format!("Cannot remove EasyProxy (is it still running?): {error}"))?;
    if let Err(error) = fs::remove_dir_all(&retired) {
        warn!(
            "[Deps]: EasyProxy files will be cleaned up later: {}",
            error
        );
    }
    info!("[Deps]: Removed EasyProxy");
    Ok(())
}

/// Serializes installs across processes (desktop and headless share `lib_dir`).
fn lock(lib_dir: &Path) -> Result<File, String> {
    fs::create_dir_all(lib_dir).map_err(|error| error.to_string())?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lib_dir.join(LOCK_FILE))
        .map_err(|error| format!("Cannot open the EasyProxy install lock: {error}"))?;
    file.try_lock().map_err(|error| {
        format!("EasyProxy is being installed by another REFEREE process: {error}")
    })?;
    Ok(file)
}

/// Restores an install whose swap was interrupted and deletes leftover
/// staging and retired directories. Caller holds the lock.
fn recover(lib_dir: &Path) {
    let live = install_dir(lib_dir);
    let Ok(entries) = fs::read_dir(lib_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if name.starts_with(RETIRED_PREFIX) && !live.exists() {
            match fs::rename(&path, &live) {
                Ok(()) => info!("[Deps]: Restored the previous EasyProxy install"),
                Err(error) => warn!("[Deps]: Cannot restore {}: {}", path.display(), error),
            }
        } else if name.starts_with(RETIRED_PREFIX) || name.starts_with(STAGE_PREFIX) {
            if let Err(error) = fs::remove_dir_all(&path) {
                debug!("[Deps]: Cannot remove {} yet: {}", path.display(), error);
            }
        }
    }
}

fn swap_in(lib_dir: &Path, staged: &Path) -> Result<(), String> {
    let live = install_dir(lib_dir);
    let retired = retired_dir(lib_dir);
    if live.exists() {
        fs::rename(&live, &retired).map_err(|error| {
            format!("Cannot replace the EasyProxy install (is it still running?): {error}")
        })?;
    }
    if let Err(error) = fs::rename(staged, &live) {
        if retired.exists() {
            let _ = fs::rename(&retired, &live);
        }
        return Err(format!("Cannot install EasyProxy: {error}"));
    }
    if retired.exists() {
        if let Err(error) = fs::remove_dir_all(&retired) {
            debug!(
                "[Deps]: Old EasyProxy files will be cleaned up later: {}",
                error
            );
        }
    }
    Ok(())
}

fn retired_dir(lib_dir: &Path) -> PathBuf {
    lib_dir.join(format!("{RETIRED_PREFIX}{}", uuid::Uuid::new_v4()))
}

/// A staging directory on the same filesystem as the install, removed on drop
/// unless it was swapped in.
struct Stage(PathBuf);

impl Stage {
    fn new(lib_dir: &Path) -> Result<Self, String> {
        let path = lib_dir.join(format!("{STAGE_PREFIX}{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)
            .map_err(|error| format!("Cannot create the EasyProxy staging area: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    let archive = archive.to_path_buf();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let result = extract_archive(&archive, &dest, "archive.tar.gz");
        let _ = fs::remove_file(&archive);
        result
    })
    .await
    .map_err(|error| format!("EasyProxy extraction task failed: {error}"))?
}

/// Runs pip against the pinned constraints. Progress counts the packages pip
/// starts collecting, out of the constraint entries.
async fn install_packages<F: FnMut(SetupProgress)>(
    python: &Path,
    requirements: &Path,
    constraints: &Path,
    progress: &mut F,
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(python);
    command
        .args(["-I", "-m", "pip", "install"])
        .args([
            "--disable-pip-version-check",
            "--no-input",
            "--no-cache-dir",
            "--no-warn-script-location",
            "--progress-bar",
            "off",
            "--only-binary",
            ":all:",
            "--no-binary",
            SOURCE_ONLY_PACKAGES,
        ])
        .arg("-r")
        .arg(requirements)
        .arg("-c")
        .arg(constraints)
        // A user's pip.conf could change the index or install options.
        .env("PIP_CONFIG_FILE", null_device())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::process::hide_tokio_command_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot run pip: {error}"))?;

    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });
    let total = constraint_count();
    let mut collected = 0usize;
    let mut stdout_tail = Vec::new();
    let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
    let read = async {
        while let Ok(Some(line)) = lines.next_line().await {
            debug!("[Deps:pip]: {}", line);
            if line.starts_with("Collecting ") {
                collected = (collected + 1).min(total);
                report(
                    progress,
                    45 + (collected * 45 / total.max(1)) as u32,
                    format!("Installing Python packages ({collected}/{total})..."),
                );
            }
            push_tail(&mut stdout_tail, line);
        }
        child.wait().await
    };
    let status = tokio::time::timeout(PIP_TIMEOUT, read)
        .await
        .map_err(|_| "Installing EasyProxy's Python packages timed out".to_string())?
        .map_err(|error| format!("pip failed to run: {error}"))?;
    let stderr = stderr_task.await.unwrap_or_default();

    if status.success() {
        return Ok(());
    }
    let mut tail = stdout_tail;
    for line in stderr.lines() {
        push_tail(&mut tail, line.to_string());
    }
    Err(format!(
        "Installing EasyProxy's Python packages failed ({status}): {}",
        tail.join(" | ")
    ))
}

async fn smoke_test(python: &Path) -> Result<(), String> {
    let mut command = tokio::process::Command::new(python);
    command
        .args(["-I", "-c", SMOKE_TEST])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    crate::process::hide_tokio_command_window(&mut command);
    let output = tokio::time::timeout(SMOKE_TEST_TIMEOUT, command.output())
        .await
        .map_err(|_| "EasyProxy validation timed out".to_string())?
        .map_err(|error| format!("Cannot run the EasyProxy Python: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "EasyProxy's Python packages failed to load: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn constraint_count() -> usize {
    CONSTRAINTS
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .count()
}

fn push_tail(tail: &mut Vec<String>, line: String) {
    if line.trim().is_empty() {
        return;
    }
    if tail.len() == ERROR_TAIL_LINES {
        tail.remove(0);
    }
    tail.push(line);
}

fn null_device() -> &'static str {
    if cfg!(target_os = "windows") {
        "nul"
    } else {
        "/dev/null"
    }
}

fn report<F: FnMut(SetupProgress)>(progress: &mut F, percent: u32, detail: impl Into<String>) {
    progress(SetupProgress {
        phase: PHASE.into(),
        percent,
        detail: detail.into(),
    });
}

/// Maps a download's 0–90% onto `from..to` of the whole install.
fn scaled<F: FnMut(SetupProgress)>(
    progress: &mut F,
    from: u32,
    to: u32,
) -> impl FnMut(SetupProgress) + '_ {
    move |update: SetupProgress| {
        progress(SetupProgress {
            percent: from + update.percent.min(90) * (to - from) / 90,
            ..update
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        constraint_count, install_dir, install_state, python_path, recover, remove, swap_in,
        write_launcher, InstallMarker, InstallState, LAUNCHER, MARKER_FILE, RETIRED_PREFIX,
        STAGE_PREFIX,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    fn temp_lib_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("referee-easyproxy-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A fake install: the Python path exists and `install.json` is written.
    fn fake_install(dir: &Path, marker: &InstallMarker) {
        let python = python_path(dir);
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(&python, b"").unwrap();
        fs::write(dir.join(MARKER_FILE), serde_json::to_vec(marker).unwrap()).unwrap();
    }

    #[test]
    fn install_state_compares_the_marker_with_the_compiled_pins() {
        let lib_dir = temp_lib_dir();
        assert_eq!(install_state(&lib_dir), InstallState::Missing);

        let current = InstallMarker::current().unwrap();
        fake_install(&install_dir(&lib_dir), &current);
        assert_eq!(install_state(&lib_dir), InstallState::Current);

        let older = InstallMarker {
            commit: "0000000".to_string(),
            ..InstallMarker::current().unwrap()
        };
        fake_install(&install_dir(&lib_dir), &older);
        assert_eq!(install_state(&lib_dir), InstallState::Outdated);

        fs::remove_file(install_dir(&lib_dir).join(MARKER_FILE)).unwrap();
        assert_eq!(install_state(&lib_dir), InstallState::Outdated);
        let _ = fs::remove_dir_all(&lib_dir);
    }

    #[test]
    fn swap_replaces_the_install_and_recover_restores_an_interrupted_swap() {
        let lib_dir = temp_lib_dir();
        let live = install_dir(&lib_dir);
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("version"), "old").unwrap();
        let staged = lib_dir.join(format!("{STAGE_PREFIX}test"));
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("version"), "new").unwrap();

        swap_in(&lib_dir, &staged).unwrap();
        assert_eq!(fs::read_to_string(live.join("version")).unwrap(), "new");
        assert!(!staged.exists());

        // A crash between the two renames leaves only the retired copy.
        let retired = lib_dir.join(format!("{RETIRED_PREFIX}test"));
        fs::rename(&live, &retired).unwrap();
        let abandoned = lib_dir.join(format!("{STAGE_PREFIX}abandoned"));
        fs::create_dir_all(&abandoned).unwrap();
        recover(&lib_dir);
        assert_eq!(fs::read_to_string(live.join("version")).unwrap(), "new");
        assert!(!retired.exists());
        assert!(!abandoned.exists());

        remove(&lib_dir).unwrap();
        assert!(!live.exists());
        assert_eq!(install_state(&lib_dir), InstallState::Missing);
        let _ = fs::remove_dir_all(&lib_dir);
    }

    #[test]
    fn launcher_is_rewritten_only_when_it_changed() {
        let dir = temp_lib_dir();
        let path = write_launcher(&dir).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), LAUNCHER);
        assert!(LAUNCHER.contains("host=\"127.0.0.1\""));

        fs::write(&path, "stale").unwrap();
        write_launcher(&dir).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), LAUNCHER);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn constraints_pin_every_package_exactly() {
        assert!(constraint_count() >= 16);
        for line in super::CONSTRAINTS
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        {
            assert!(line.contains("=="), "{line} is not pinned to one version");
        }
    }
}
