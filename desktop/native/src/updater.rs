//! Updates REFEREE from its GitHub releases. A check picks the newest stable
//! release that publishes an installer for this installation's bundle type
//! and CPU. The installer is verified against the SHA-256 digest GitHub
//! records for the asset, then runs elevated: the NSIS installer on Windows,
//! which relaunches REFEREE when it finishes, or `dpkg`/`rpm` through `pkexec`
//! on Linux, followed by a restart.
//!
//! With `autoUpdate` on, [`spawn_auto_updates`] checks every
//! [`CHECK_INTERVAL`], downloads a new release straight away, and installs it
//! once no stream has run for [`IDLE_BEFORE_INSTALL`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

use crate::commands::AppState;
use crate::deps::{self, SetupProgress};

const RELEASES_URL: &str = "https://api.github.com/repos/cmacrowther/referee/releases?per_page=30";
/// Lets launch-time dependency setup settle before the first check.
const STARTUP_DELAY: Duration = Duration::from_secs(120);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// How long no stream must have run before an automatic install starts.
const IDLE_BEFORE_INSTALL: Duration = Duration::from_secs(5 * 60);
const IDLE_POLL: Duration = Duration::from_secs(30);
const METADATA_TIMEOUT: Duration = Duration::from_secs(15);
const UNSUPPORTED_INSTALL: &str = "This installation cannot update itself.";
/// Passive install (progress window, no questions), update mode, then
/// relaunch REFEREE as the signed-in user: the `/P`, `/UPDATE` and `/R`
/// options of Tauri's NSIS installer template.
#[cfg(windows)]
const NSIS_ARGS: &str = "/P /UPDATE /R";

/// How this installation was packaged, which decides the release asset and
/// how it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Installer {
    Nsis,
    Deb,
    Rpm,
}

impl Installer {
    /// The bundle type the Tauri bundler stamped into this binary; `Err` says
    /// why the installation cannot update itself.
    fn detect() -> Result<Self, &'static str> {
        use tauri::utils::config::BundleType;
        match tauri::utils::platform::bundle_type() {
            Some(BundleType::Nsis) => Ok(Self::Nsis),
            Some(BundleType::Deb) => Ok(Self::Deb),
            Some(BundleType::Rpm) => Ok(Self::Rpm),
            Some(_) => Err("This kind of installation cannot update itself."),
            None => Err("Only installed builds of REFEREE can update themselves."),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Nsis => "exe",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
        }
    }

    /// Whether `name` is this installer for `arch` (`std::env::consts::ARCH`),
    /// going by the file names the Tauri bundler produces.
    fn matches(self, name: &str, arch: &str) -> bool {
        let suffix = match (self, arch) {
            (Self::Nsis, "x86_64") => "_x64-setup.exe",
            (Self::Nsis, "aarch64") => "_arm64-setup.exe",
            (Self::Deb, "x86_64") => "_amd64.deb",
            (Self::Deb, "aarch64") => "_arm64.deb",
            (Self::Rpm, "x86_64") => ".x86_64.rpm",
            (Self::Rpm, "aarch64") => ".aarch64.rpm",
            _ => return false,
        };
        name.to_ascii_lowercase().ends_with(suffix)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Asset {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
    /// `sha256:<hex>`, recorded by GitHub when the asset was uploaded.
    #[serde(default)]
    digest: Option<String>,
}

/// A release newer than this one, with this installation's installer from it
/// when the installation can update itself.
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    version: Version,
    release_url: String,
    asset: Option<Asset>,
}

fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

fn is_sha256_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The newest stable release above `current`. With an installer, only
/// releases that publish its asset with a digest count, so a release whose
/// build for this platform is missing is skipped rather than offered.
fn select_update(
    current: &Version,
    releases: &[Release],
    installer: Option<Installer>,
    arch: &str,
) -> Option<Candidate> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = parse_tag(&release.tag_name)
                .filter(|version| version.pre.is_empty() && version > current)?;
            let asset = match installer {
                Some(installer) => Some(
                    release
                        .assets
                        .iter()
                        .find(|asset| {
                            installer.matches(&asset.name, arch)
                                && asset.digest.as_deref().is_some_and(is_sha256_digest)
                        })?
                        .clone(),
                ),
                None => None,
            };
            Some(Candidate {
                version,
                release_url: release.html_url.clone(),
                asset,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Downloading,
    /// Downloaded and verified, waiting to install.
    Ready,
    Installing,
}

/// What the renderer shows, also sent as the `update-status` event.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub phase: UpdatePhase,
    /// The newest release above this one, from the last check.
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    /// 0–100 while downloading.
    pub progress: Option<u32>,
    pub error: Option<String>,
    /// Unix time in milliseconds of the last check that reached GitHub.
    pub last_checked: Option<u64>,
    /// Why updates cannot install here; `None` when they can.
    pub unsupported_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct Downloaded {
    version: Version,
    path: PathBuf,
    digest: String,
}

#[derive(Default)]
struct State {
    phase: UpdatePhase,
    candidate: Option<Candidate>,
    downloaded: Option<Downloaded>,
    progress: Option<u32>,
    error: Option<String>,
    last_checked: Option<u64>,
    /// A version whose install failed or was declined. Automatic updates
    /// leave it alone until the next launch, so the prompt does not return.
    declined: Option<Version>,
}

pub struct Updater {
    current: Version,
    installer: Result<Installer, &'static str>,
    releases_url: String,
    client: reqwest::Client,
    /// Private to the user, since the installer in it runs elevated.
    download_dir: PathBuf,
    state: Mutex<State>,
    /// Held across a check, download or install so they never overlap.
    busy: tokio::sync::Mutex<()>,
    on_change: Box<dyn Fn(UpdateStatus) + Send + Sync>,
}

impl Updater {
    pub fn new(
        current: Version,
        client: reqwest::Client,
        download_dir: PathBuf,
        on_change: impl Fn(UpdateStatus) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Self::build(
            current,
            Installer::detect(),
            RELEASES_URL.to_string(),
            client,
            download_dir,
            Box::new(on_change),
        )
    }

    fn build(
        current: Version,
        installer: Result<Installer, &'static str>,
        releases_url: String,
        client: reqwest::Client,
        download_dir: PathBuf,
        on_change: Box<dyn Fn(UpdateStatus) + Send + Sync>,
    ) -> Arc<Self> {
        // Installers from earlier updates are spent once this version runs.
        let _ = std::fs::remove_dir_all(&download_dir);
        Arc::new(Self {
            current,
            installer,
            releases_url,
            client,
            download_dir,
            state: Mutex::new(State::default()),
            busy: tokio::sync::Mutex::new(()),
            on_change,
        })
    }

    pub fn status(&self) -> UpdateStatus {
        self.snapshot(&self.state.lock().unwrap())
    }

    fn snapshot(&self, state: &State) -> UpdateStatus {
        UpdateStatus {
            current_version: self.current.to_string(),
            phase: state.phase,
            latest_version: state.candidate.as_ref().map(|c| c.version.to_string()),
            release_url: state.candidate.as_ref().map(|c| c.release_url.clone()),
            progress: state.progress,
            error: state.error.clone(),
            last_checked: state.last_checked,
            unsupported_reason: self.installer.err(),
        }
    }

    /// Applies `change`, then reports the new status.
    fn update(&self, change: impl FnOnce(&mut State)) {
        let status = {
            let mut state = self.state.lock().unwrap();
            change(&mut state);
            self.snapshot(&state)
        };
        (self.on_change)(status);
    }

    fn declined(&self, version: &Version) -> bool {
        self.state.lock().unwrap().declined.as_ref() == Some(version)
    }

    /// Checks GitHub for a newer release, unless a check, download or install
    /// is already running. A failed check is reported in the status.
    pub async fn check(&self) -> UpdateStatus {
        if let Ok(_busy) = self.busy.try_lock() {
            self.check_locked().await;
        }
        self.status()
    }

    async fn check_locked(&self) -> Option<Candidate> {
        self.update(|state| {
            state.phase = UpdatePhase::Checking;
            state.error = None;
        });
        let result = self.fetch_releases().await.map(|releases| {
            select_update(
                &self.current,
                &releases,
                self.installer.ok(),
                std::env::consts::ARCH,
            )
        });
        let mut found = None;
        self.update(|state| {
            match result {
                Ok(candidate) => {
                    state.last_checked = Some(unix_millis());
                    // A download of a release that is no longer the newest is dropped.
                    if state.downloaded.as_ref().map(|d| &d.version)
                        != candidate.as_ref().map(|c| &c.version)
                    {
                        state.downloaded = None;
                    }
                    found = candidate.clone();
                    state.candidate = candidate;
                }
                Err(error) => state.error = Some(error),
            }
            state.phase = if state.downloaded.is_some() {
                UpdatePhase::Ready
            } else {
                UpdatePhase::Idle
            };
        });
        found
    }

    async fn fetch_releases(&self) -> Result<Vec<Release>, String> {
        let response = self
            .client
            .get(&self.releases_url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("Could not reach GitHub: {e}"))?;
        let status = response.status();
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err("GitHub is limiting update checks right now. Try again later.".into());
        }
        if !status.is_success() {
            return Err(format!(
                "GitHub returned HTTP {status} for the release list."
            ));
        }
        response
            .json()
            .await
            .map_err(|e| format!("GitHub sent an unreadable release list: {e}"))
    }

    /// Downloads and verifies `candidate`'s installer. An earlier download is
    /// reused while its file still matches the digest.
    async fn download_locked(
        &self,
        candidate: &Candidate,
        installer: Installer,
    ) -> Result<Downloaded, String> {
        let earlier = self
            .state
            .lock()
            .unwrap()
            .downloaded
            .clone()
            .filter(|done| done.version == candidate.version);
        if let Some(done) = earlier {
            if file_matches(&done.path, &done.digest).await {
                return Ok(done);
            }
            warn!("[Updater]: The downloaded update changed on disk; downloading it again");
        }

        let asset = candidate
            .asset
            .as_ref()
            .ok_or("This release has no installer for this platform.")?;
        let digest = asset
            .digest
            .clone()
            .ok_or("This release's installer has no checksum.")?;
        self.update(|state| {
            state.phase = UpdatePhase::Downloading;
            state.progress = Some(0);
            state.error = None;
            state.downloaded = None;
        });
        let path = self.download_dir.join(format!(
            "REFEREE-{}-update.{}",
            candidate.version,
            installer.extension()
        ));
        let label = format!("REFEREE v{}", candidate.version);
        let result = async {
            let _ = std::fs::remove_dir_all(&self.download_dir);
            std::fs::create_dir_all(&self.download_dir)
                .map_err(|e| format!("Cannot create the update folder: {e}"))?;
            let download = deps::VerifiedDownload {
                url: &asset.browser_download_url,
                size: (asset.size > 0).then_some(asset.size),
                digest: Some(&digest),
                label: &label,
                phase: "update",
            };
            deps::download_verified(
                &self.client,
                &download,
                &path,
                &mut |progress: SetupProgress| {
                    // The downloader reports 0–90%; the rest is for unpacking, which updates skip.
                    let percent = (progress.percent * 100 / 90).min(100);
                    self.update(|state| state.progress = Some(percent));
                },
            )
            .await
        }
        .await;

        let downloaded = result.map(|()| Downloaded {
            version: candidate.version.clone(),
            path,
            digest,
        });
        self.update(|state| {
            state.progress = None;
            match &downloaded {
                Ok(done) => {
                    state.phase = UpdatePhase::Ready;
                    state.downloaded = Some(done.clone());
                }
                Err(error) => {
                    state.phase = UpdatePhase::Idle;
                    state.error = Some(error.clone());
                }
            }
        });
        downloaded
    }
}

/// Downloads the release found by the last check if needed, runs its
/// installer and exits; REFEREE comes back on the new version. Returns only
/// when the update could not start.
pub async fn install_update(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let updater = state.updater.clone();
    let Ok(_busy) = updater.busy.try_lock() else {
        return Err("An update is already in progress.".into());
    };
    install_locked(app, &state, &updater).await
}

async fn install_locked(
    app: &AppHandle,
    state: &AppState,
    updater: &Updater,
) -> Result<(), String> {
    let installer = updater.installer?;
    let candidate = updater
        .state
        .lock()
        .unwrap()
        .candidate
        .clone()
        .ok_or("No update is available. Check for updates first.")?;
    let streaming = || !state.server_state.sessions.is_empty();
    if streaming() {
        return Err("Stop active streams before updating.".into());
    }
    let downloaded = updater.download_locked(&candidate, installer).await?;
    // A stream may have started while the installer downloaded.
    if streaming() {
        return Err("Stop active streams before updating.".into());
    }
    updater.update(|state| {
        state.phase = UpdatePhase::Installing;
        state.error = None;
    });
    if let Err(error) = run_installer(installer, downloaded.path).await {
        warn!(
            "[Updater]: Installing REFEREE v{} failed: {}",
            candidate.version, error
        );
        updater.update(|state| {
            state.phase = UpdatePhase::Ready;
            state.error = Some(error.clone());
            state.declined = Some(candidate.version.clone());
        });
        return Err(error);
    }

    info!(
        "[Updater]: Installing REFEREE v{}; exiting for the installer",
        candidate.version
    );
    crate::server::shutdown(&state.server_state.sessions).await;
    state.server_state.easy_proxy.stop().await;
    match installer {
        // The installer relaunches REFEREE when it finishes (`/R`).
        Installer::Nsis => app.exit(0),
        // The package is already replaced; start its binary.
        Installer::Deb | Installer::Rpm => app.request_restart(),
    }
    Ok(())
}

async fn file_matches(path: &Path, digest: &str) -> bool {
    let path = path.to_path_buf();
    let digest = digest.to_string();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok::<_, std::io::Error>(format!("sha256:{:x}", hasher.finalize()))
    })
    .await
    .ok()
    .and_then(Result::ok)
    .is_some_and(|actual| actual.eq_ignore_ascii_case(&digest))
}

#[cfg(windows)]
async fn run_installer(installer: Installer, path: PathBuf) -> Result<(), String> {
    match installer {
        Installer::Nsis => tokio::task::spawn_blocking(move || launch_nsis(&path))
            .await
            .map_err(|e| format!("The installer task failed: {e}"))?,
        Installer::Deb | Installer::Rpm => Err(UNSUPPORTED_INSTALL.into()),
    }
}

/// Starts the installer through the shell, which shows the administrator
/// prompt that the per-machine installer requires; `CreateProcess` cannot
/// start it. Returns once the installer runs or the prompt was declined.
#[cfg(windows)]
fn launch_nsis(path: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(Some(0)).collect()
    }
    let verb = wide(OsStr::new("open"));
    let file = wide(path.as_os_str());
    let parameters = wide(OsStr::new(NSIS_ARGS));
    // SAFETY: all-zero is a valid SHELLEXECUTEINFOW (null pointers and handles).
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: `info` is initialized and the strings it points to outlive the call.
    if unsafe { ShellExecuteExW(&mut info) } != 0 {
        return Ok(());
    }
    // SAFETY: reads the calling thread's last error, set by ShellExecuteExW.
    match unsafe { GetLastError() } {
        ERROR_CANCELLED => Err("The update was cancelled at the administrator prompt.".into()),
        code => Err(format!(
            "Could not start the installer: {}",
            std::io::Error::from_raw_os_error(code as i32)
        )),
    }
}

/// Installs the package as root through polkit, which asks for a password.
#[cfg(target_os = "linux")]
async fn run_installer(installer: Installer, path: PathBuf) -> Result<(), String> {
    let (tool, option) = match installer {
        Installer::Deb => ("dpkg", "-i"),
        Installer::Rpm => ("rpm", "-U"),
        Installer::Nsis => return Err(UNSUPPORTED_INSTALL.into()),
    };
    let output = tokio::process::Command::new("pkexec")
        .arg(tool)
        .arg(option)
        .arg(&path)
        .output()
        .await
        .map_err(|e| format!("Could not run pkexec to install the update: {e}"))?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(126) => Err("The update was cancelled at the password prompt.".into()),
        Some(127) => Err("Not authorized to install the update.".into()),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("no output");
            Err(format!("{tool} could not install the update: {detail}"))
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
async fn run_installer(_installer: Installer, _path: PathBuf) -> Result<(), String> {
    Err(UNSUPPORTED_INSTALL.into())
}

/// Checks for updates in the background and, while `autoUpdate` is on,
/// downloads new releases and installs them once REFEREE is idle.
pub fn spawn_auto_updates(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            auto_update(&app).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn auto_update_enabled(state: &AppState) -> bool {
    state.settings_manager.get().auto_update
}

async fn auto_update(app: &AppHandle) {
    let state = app.state::<AppState>();
    let updater = state.updater.clone();
    let candidate = {
        let _busy = updater.busy.lock().await;
        let Some(candidate) = updater.check_locked().await else {
            return;
        };
        let Ok(installer) = updater.installer else {
            return;
        };
        if !auto_update_enabled(&state) || updater.declined(&candidate.version) {
            return;
        }
        if let Err(error) = updater.download_locked(&candidate, installer).await {
            warn!(
                "[Updater]: Downloading REFEREE v{} failed: {}",
                candidate.version, error
            );
            return;
        }
        candidate
    };

    info!(
        "[Updater]: REFEREE v{} is ready; installing once no stream has run for {} minutes",
        candidate.version,
        IDLE_BEFORE_INSTALL.as_secs() / 60
    );
    let mut idle_since: Option<Instant> = None;
    loop {
        tokio::time::sleep(IDLE_POLL).await;
        if !auto_update_enabled(&state) {
            return;
        }
        let in_use = !state.server_state.sessions.is_empty()
            || state.setup_state.lock().unwrap().in_progress;
        if in_use {
            idle_since = None;
        } else if idle_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_BEFORE_INSTALL {
            break;
        }
    }

    let _busy = updater.busy.lock().await;
    // A manual check or a failed install may have changed things meanwhile.
    let unchanged = updater.state.lock().unwrap().candidate.as_ref() == Some(&candidate);
    if !unchanged || updater.declined(&candidate.version) {
        return;
    }
    notify_installing(app, &candidate.version);
    // Failures are logged and shown in the status; the next check tries again.
    let _ = install_locked(app, &state, &updater).await;
}

/// Shown whatever the stream notification setting, since it explains the
/// administrator or password prompt that follows.
fn notify_installing(app: &AppHandle, version: &Version) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("REFEREE")
        .body(format!(
            "Installing REFEREE v{version}. It restarts when the update finishes."
        ))
        .show();
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const ARCH_NAME: &str = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };

    fn digest_of(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    fn asset(name: &str, digest: Option<&str>) -> Asset {
        Asset {
            name: name.to_string(),
            size: 1,
            browser_download_url: format!("https://example.invalid/{name}"),
            digest: digest.map(str::to_string),
        }
    }

    fn release(tag: &str, assets: Vec<Asset>) -> Release {
        Release {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
            html_url: format!("https://example.invalid/releases/{tag}"),
            assets,
        }
    }

    fn nsis(version: &str) -> Asset {
        asset(
            &format!("REFEREE.Upscaler_{version}_x64-setup.exe"),
            Some(&digest_of(version.as_bytes())),
        )
    }

    fn release_history() -> Vec<Release> {
        vec![
            // Newest, but its Windows build was published without a digest.
            release(
                "v0.9.0",
                vec![asset("REFEREE.Upscaler_0.9.0_x64-setup.exe", None)],
            ),
            Release {
                prerelease: true,
                ..release("v0.8.3", vec![nsis("0.8.3")])
            },
            Release {
                draft: true,
                ..release("v0.8.2", vec![nsis("0.8.2")])
            },
            release("v0.8.4-beta.1", vec![nsis("0.8.4-beta.1")]),
            release(
                "v0.8.5",
                vec![asset(
                    "REFEREE.Upscaler_0.8.5_arm64-setup.exe",
                    Some(&digest_of(b"arm")),
                )],
            ),
            release("v0.8.0", vec![nsis("0.8.0")]),
            release(
                "v0.8.1",
                vec![
                    asset("REFEREE.Upscaler_0.8.1_amd64.deb", Some(&digest_of(b"deb"))),
                    nsis("0.8.1"),
                ],
            ),
            release("nightly", vec![nsis("nightly")]),
            release("v0.6.0", vec![nsis("0.6.0")]),
        ]
    }

    #[test]
    fn selects_the_newest_stable_release_with_a_verifiable_installer_for_this_platform() {
        let candidate = select_update(
            &Version::new(0, 7, 0),
            &release_history(),
            Some(Installer::Nsis),
            "x86_64",
        )
        .unwrap();
        assert_eq!(candidate.version, Version::new(0, 8, 1));
        assert_eq!(
            candidate.asset.unwrap().name,
            "REFEREE.Upscaler_0.8.1_x64-setup.exe"
        );
        assert_eq!(
            candidate.release_url,
            "https://example.invalid/releases/v0.8.1"
        );
    }

    #[test]
    fn offers_nothing_when_this_version_is_current() {
        assert_eq!(
            select_update(
                &Version::new(0, 8, 1),
                &release_history(),
                Some(Installer::Nsis),
                "x86_64",
            ),
            None
        );
    }

    #[test]
    fn installations_that_cannot_update_still_learn_of_newer_versions() {
        let candidate =
            select_update(&Version::new(0, 7, 0), &release_history(), None, "x86_64").unwrap();
        assert_eq!(candidate.version, Version::new(0, 9, 0));
        assert_eq!(candidate.asset, None);
    }

    #[test]
    fn a_stable_release_replaces_its_own_prerelease() {
        let releases = vec![release("v0.8.0", vec![nsis("0.8.0")])];
        let current = Version::parse("0.8.0-beta.2").unwrap();
        let candidate = select_update(&current, &releases, Some(Installer::Nsis), "x86_64");
        assert_eq!(candidate.unwrap().version, Version::new(0, 8, 0));
    }

    #[test]
    fn matches_the_installer_names_tauri_produces() {
        assert!(Installer::Nsis.matches("REFEREE.Upscaler_0.7.0_x64-setup.exe", "x86_64"));
        assert!(!Installer::Nsis.matches("REFEREE.Upscaler_0.7.0_x64-setup.exe", "aarch64"));
        assert!(!Installer::Nsis.matches("REFEREE.Upscaler_0.7.0_x64_en-US.msi", "x86_64"));
        assert!(Installer::Deb.matches("REFEREE.Upscaler_0.7.0_amd64.deb", "x86_64"));
        assert!(Installer::Deb.matches("referee-upscaler_0.7.0_arm64.deb", "aarch64"));
        assert!(Installer::Rpm.matches("REFEREE.Upscaler-0.7.0-1.x86_64.rpm", "x86_64"));
        assert!(!Installer::Rpm.matches("REFEREE.Upscaler_0.7.0_amd64.deb", "x86_64"));
        assert!(!Installer::Deb.matches("REFEREE.Upscaler_0.7.0_amd64.deb", "riscv64"));
    }

    #[test]
    fn only_well_formed_sha256_digests_count() {
        assert!(is_sha256_digest(&digest_of(b"x")));
        assert!(!is_sha256_digest("sha256:abc"));
        assert!(!is_sha256_digest(
            &digest_of(b"x").replace("sha256", "sha1")
        ));
        assert_eq!(parse_tag("v1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(parse_tag("1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(parse_tag("latest"), None);
    }

    struct TestServer {
        url: String,
        downloads: Arc<AtomicUsize>,
        /// Makes `/releases` answer HTTP 500.
        failing: Arc<AtomicBool>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    /// Serves `/releases` listing one release of `version` whose installer,
    /// at `/installer`, is `installer` and claims `digest`.
    async fn serve_release(version: &str, installer: Vec<u8>, digest: String) -> TestServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let releases = serde_json::json!([{
            "tag_name": format!("v{version}"),
            "html_url": format!("{url}/release"),
            "assets": [{
                "name": format!("REFEREE.Upscaler_{version}_{ARCH_NAME}-setup.exe"),
                "size": installer.len(),
                "browser_download_url": format!("{url}/installer"),
                "digest": digest,
            }],
        }])
        .to_string();
        let downloads = Arc::new(AtomicUsize::new(0));
        let count = downloads.clone();
        let failing = Arc::new(AtomicBool::new(false));
        let fail = failing.clone();
        let app = axum::Router::new()
            .route(
                "/releases",
                axum::routing::get(move || {
                    let releases = releases.clone();
                    let status = if fail.load(Ordering::SeqCst) {
                        reqwest::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        reqwest::StatusCode::OK
                    };
                    async move { (status, releases) }
                }),
            )
            .route(
                "/installer",
                axum::routing::get(move || {
                    let installer = installer.clone();
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        installer
                    }
                }),
            );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        TestServer {
            url,
            downloads,
            failing,
            task,
        }
    }

    fn test_updater(releases_url: String) -> (Arc<Updater>, Arc<Mutex<Vec<UpdateStatus>>>) {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let sink = reported.clone();
        let updater = Updater::build(
            Version::new(0, 7, 0),
            Ok(Installer::Nsis),
            releases_url,
            reqwest::Client::builder().no_proxy().build().unwrap(),
            std::env::temp_dir().join(format!("referee-update-test-{}", uuid::Uuid::new_v4())),
            Box::new(move |status| sink.lock().unwrap().push(status)),
        );
        (updater, reported)
    }

    async fn check_and_download(updater: &Updater) -> Result<Downloaded, String> {
        let _busy = updater.busy.lock().await;
        let candidate = updater.check_locked().await.expect("an update");
        updater.download_locked(&candidate, Installer::Nsis).await
    }

    #[tokio::test]
    async fn downloads_a_verified_installer_once_and_replaces_a_changed_file() {
        let installer = b"installer bytes".to_vec();
        let server = serve_release("0.8.0", installer.clone(), digest_of(&installer)).await;
        let (updater, reported) = test_updater(format!("{}/releases", server.url));

        let status = updater.check().await;
        assert_eq!(status.phase, UpdatePhase::Idle);
        assert_eq!(status.latest_version.as_deref(), Some("0.8.0"));
        assert_eq!(status.release_url, Some(format!("{}/release", server.url)));
        assert!(status.last_checked.is_some());
        assert_eq!(status.unsupported_reason, None);

        let downloaded = check_and_download(&updater).await.unwrap();
        assert_eq!(std::fs::read(&downloaded.path).unwrap(), installer);
        assert_eq!(updater.status().phase, UpdatePhase::Ready);
        assert!(reported
            .lock()
            .unwrap()
            .iter()
            .any(|status| status.phase == UpdatePhase::Downloading));

        // A fresh check of the same release keeps the verified download.
        let downloaded = check_and_download(&updater).await.unwrap();
        assert_eq!(server.downloads.load(Ordering::SeqCst), 1);

        std::fs::write(&downloaded.path, b"tampered").unwrap();
        let downloaded = check_and_download(&updater).await.unwrap();
        assert_eq!(server.downloads.load(Ordering::SeqCst), 2);
        assert_eq!(std::fs::read(&downloaded.path).unwrap(), installer);
        let _ = std::fs::remove_dir_all(&updater.download_dir);
    }

    #[tokio::test]
    async fn an_installer_that_fails_its_checksum_is_never_ready() {
        let server = serve_release("0.8.0", b"installer".to_vec(), digest_of(b"other")).await;
        let (updater, _) = test_updater(format!("{}/releases", server.url));

        let error = check_and_download(&updater).await.unwrap_err();
        assert!(error.contains("checksum"), "{error}");
        let status = updater.status();
        assert_eq!(status.phase, UpdatePhase::Idle);
        assert_eq!(status.error, Some(error));
        assert!(updater.state.lock().unwrap().downloaded.is_none());
        let _ = std::fs::remove_dir_all(&updater.download_dir);
    }

    #[tokio::test]
    async fn a_failed_check_is_reported_without_forgetting_the_last_result() {
        let installer = b"installer".to_vec();
        let server = serve_release("0.8.0", installer.clone(), digest_of(&installer)).await;
        let (updater, _) = test_updater(format!("{}/releases", server.url));
        let checked = updater.check().await;

        server.failing.store(true, Ordering::SeqCst);
        let status = updater.check().await;
        assert_eq!(status.phase, UpdatePhase::Idle);
        assert!(status.error.unwrap().contains("HTTP 500"));
        assert_eq!(status.latest_version.as_deref(), Some("0.8.0"));
        assert_eq!(status.last_checked, checked.last_checked);

        server.failing.store(false, Ordering::SeqCst);
        assert_eq!(updater.check().await.error, None);
    }
}
