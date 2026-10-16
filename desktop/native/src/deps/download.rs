use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::install::{
    extract_7z, extract_deb, extract_tar_gz, extract_tar_xz, extract_zip,
    install_flat_binary_files, install_vs_rife_models,
};
use super::resolution::{
    current_ffmpeg_asset_name, parse_ffmpeg_buildconf_flag, parse_ffmpeg_filter_support,
    resolve_ffmpeg_binary, rife_model_complete as model_complete, BinaryResolutionScope,
};
use super::transaction::{begin_install, commit_install, recover_install, StagingDir};
use super::{
    ffmpeg_binary, nvencc_binary, vceenc_binary, SetupProgress, FFMPEG_PATH_ENV, FFMPEG_REPO,
    NVENCC_REPO, RIFE_DEFAULT_MODEL_DIRS, RIFE_MODEL_PATH_ENV, VCEENCC_REPO, VS_RIFE_REPO,
    VS_RIFE_TAG,
};

const ATTEMPTS: usize = 3;
const METADATA_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
enum Package {
    Ffmpeg,
    Nvenc,
    Vceenc,
    Rife,
}

impl Package {
    fn label(self) -> &'static str {
        match self {
            Self::Ffmpeg => "FFmpeg",
            Self::Nvenc => "NVEncC",
            Self::Vceenc => "VCEEncC",
            Self::Rife => "VS-RIFE",
        }
    }
    fn phase(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Rife => "rife",
            _ => "encoder",
        }
    }
    fn manifest(self) -> PathBuf {
        format!(".dependency-{}.json", self.label().to_lowercase()).into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct Asset {
    id: u64,
    name: String,
    size: u64,
    updated_at: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize, Serialize)]
struct InstalledFile {
    path: PathBuf,
    size: u64,
}

#[derive(Deserialize, Serialize)]
struct InstalledRelease {
    tag: String,
    asset: Asset,
    files: Vec<InstalledFile>,
}

pub(super) async fn download_ffmpeg<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress: &mut F,
) -> Result<(), String> {
    // Explicit overrides are user managed; never replace them with downloaded builds.
    if std::env::var_os(FFMPEG_PATH_ENV).is_some_and(|value| !value.is_empty()) {
        resolve_ffmpeg_binary(lib_dir, BinaryResolutionScope::Setup)?
            .ok_or_else(|| "Configured FFmpeg could not be resolved".to_string())?;
        return Ok(());
    }
    let target = current_ffmpeg_asset_name().ok_or("No FFmpeg download for this platform")?;
    update_release(
        client,
        lib_dir,
        Package::Ffmpeg,
        FFMPEG_REPO,
        |name| name.contains(target) && !name.contains("shared"),
        progress,
    )
    .await
}

pub(super) async fn download_nvencc<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress: &mut F,
) -> Result<(), String> {
    update_release(
        client,
        lib_dir,
        Package::Nvenc,
        NVENCC_REPO,
        |name| name.ends_with("_x64.7z"),
        progress,
    )
    .await
}

pub(super) async fn download_vceenc<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress: &mut F,
) -> Result<(), String> {
    update_release(
        client,
        lib_dir,
        Package::Vceenc,
        VCEENCC_REPO,
        super::resolution::vceenc_asset_matches_target,
        progress,
    )
    .await
}

/// Models follow the app-supported revision, since upstream layouts may change.
pub(super) async fn download_rife_models<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    progress: &mut F,
) -> Result<(), String> {
    if std::env::var_os(RIFE_MODEL_PATH_ENV).is_some_and(|value| !value.is_empty()) {
        return super::resolve_rife_model_path(lib_dir)
            .filter(|path| model_complete(path))
            .map(|_| ())
            .ok_or_else(|| "Configured RIFE model is incomplete".to_string());
    }
    let asset = Asset {
        id: 0,
        name: "vs-rife.tar.gz".into(),
        size: 0,
        updated_at: VS_RIFE_TAG.into(),
        browser_download_url: format!(
            "https://github.com/{VS_RIFE_REPO}/archive/refs/tags/{VS_RIFE_TAG}.tar.gz"
        ),
        digest: None,
    };
    let result = install_asset(
        client,
        lib_dir,
        Package::Rife,
        VS_RIFE_TAG,
        &asset,
        progress,
    )
    .await;
    keep_working_install(lib_dir, Package::Rife, result, progress).await
}

async fn update_release<F, M>(
    client: &reqwest::Client,
    lib_dir: &Path,
    package: Package,
    repo: &str,
    matcher: M,
    progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(SetupProgress),
    M: Fn(&str) -> bool,
{
    report(
        progress,
        package,
        0,
        format!("Checking {} for updates...", package.label()),
    );
    let result = async {
        let url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let response = get_response(client, &url, METADATA_TIMEOUT).await?;
        let release: Release = response
            .json()
            .await
            .map_err(|e| format!("Invalid {} release metadata: {e}", package.label()))?;
        if release.draft || release.prerelease || release.tag_name.trim().is_empty() {
            return Err(format!(
                "No stable {} release is available",
                package.label()
            ));
        }
        let asset = select_asset(
            &release.assets,
            &matcher,
            matches!(package, Package::Ffmpeg),
        )
        .ok_or_else(|| format!("No compatible {} release asset", package.label()))?;
        install_asset(client, lib_dir, package, &release.tag_name, asset, progress).await
    }
    .await;
    keep_working_install(lib_dir, package, result, progress).await
}

async fn keep_working_install<F: FnMut(SetupProgress)>(
    lib_dir: &Path,
    package: Package,
    result: Result<(), String>,
    progress: &mut F,
) -> Result<(), String> {
    if let Err(error) = result {
        recover_install(lib_dir)?;
        if validate_package(lib_dir, package).await.is_err() {
            return Err(format!("{} is unavailable: {error}", package.label()));
        }
        warn!(
            "[Deps]: {} update deferred; keeping working installation: {}",
            package.label(),
            error
        );
        report(
            progress,
            package,
            100,
            format!(
                "Using installed {}; update will be retried next launch",
                package.label()
            ),
        );
    }
    Ok(())
}

fn select_asset<'a, M: Fn(&str) -> bool>(
    assets: &'a [Asset],
    matcher: &M,
    prefer_stable_ffmpeg: bool,
) -> Option<&'a Asset> {
    assets
        .iter()
        .filter(|asset| matcher(&asset.name.to_lowercase()))
        .max_by_key(|asset| {
            if prefer_stable_ffmpeg {
                ffmpeg_release_rank(&asset.name)
            } else {
                vec![]
            }
        })
}

fn ffmpeg_release_rank(name: &str) -> Vec<u32> {
    name.strip_prefix("ffmpeg-n")
        .unwrap_or("")
        .split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map_while(|part| part.parse().ok())
        .collect()
}

fn installed_is_current(lib_dir: &Path, package: Package, tag: &str, asset: &Asset) -> bool {
    fs::read(lib_dir.join(package.manifest()))
        .ok()
        .and_then(|data| serde_json::from_slice::<InstalledRelease>(&data).ok())
        .is_some_and(|installed| {
            installed.tag == tag
                && installed.asset == *asset
                && !installed.files.is_empty()
                && installed.files.iter().all(|file| {
                    safe_relative(&file.path)
                        && file.size > 0
                        && fs::metadata(lib_dir.join(&file.path))
                            .is_ok_and(|meta| meta.is_file() && meta.len() == file.size)
                })
        })
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

async fn install_asset<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    lib_dir: &Path,
    package: Package,
    tag: &str,
    asset: &Asset,
    progress: &mut F,
) -> Result<(), String> {
    if installed_is_current(lib_dir, package, tag, asset)
        && validate_package(lib_dir, package).await.is_ok()
    {
        info!("[Deps]: {} {} is current", package.label(), tag);
        report(
            progress,
            package,
            100,
            format!("{} is up to date", package.label()),
        );
        return Ok(());
    }
    let staging = StagingDir::new(lib_dir)?;
    let archive = staging.0.join("download.archive");
    download_archive(client, asset, &archive, package, progress).await?;
    report(
        progress,
        package,
        90,
        format!("Extracting {}...", package.label()),
    );
    let extracted = staging.0.join("extracted");
    let payload = staging.0.join("payload");
    fs::create_dir(&extracted).map_err(|e| e.to_string())?;
    fs::create_dir(&payload).map_err(|e| e.to_string())?;
    let archive_name = asset.name.to_lowercase();
    let payload_task = payload.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        extract_archive(&archive, &extracted, &archive_name)?;
        if matches!(package, Package::Rife) {
            install_vs_rife_models(
                &extracted,
                &payload_task.join("vs-rife/models"),
                package.label(),
            )
        } else {
            install_flat_binary_files(&extracted, &payload_task, package.label())
        }
    })
    .await
    .map_err(|e| format!("Dependency extraction task failed: {e}"))??;

    report(
        progress,
        package,
        95,
        format!("Validating {}...", package.label()),
    );
    validate_package(&payload, package).await?;
    let files = inventory(&payload, &payload)?;
    let manifest = InstalledRelease {
        tag: tag.into(),
        asset: asset.clone(),
        files,
    };
    let manifest_path = package.manifest();
    fs::write(
        payload.join(&manifest_path),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Cannot write dependency version record: {e}"))?;
    let mut paths: Vec<_> = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    paths.push(manifest_path);
    begin_install(lib_dir, &payload, &paths)?;
    if let Err(error) = validate_package(lib_dir, package).await {
        recover_install(lib_dir)?;
        return Err(error);
    }
    if let Err(error) = commit_install(lib_dir) {
        recover_install(lib_dir)?;
        return Err(error);
    }
    info!(
        "[Deps]: Installed {} {} ({})",
        package.label(),
        tag,
        asset.name
    );
    report(
        progress,
        package,
        100,
        format!("{} updated", package.label()),
    );
    Ok(())
}

fn inventory(root: &Path, dir: &Path) -> Result<Vec<InstalledFile>, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            files.extend(inventory(root, &path)?);
        } else if meta.is_file() {
            files.push(InstalledFile {
                path: path.strip_prefix(root).map_err(|e| e.to_string())?.into(),
                size: meta.len(),
            });
        } else {
            return Err(format!("Unsupported dependency file: {}", path.display()));
        }
    }
    Ok(files)
}

pub(super) fn extract_archive(archive: &Path, dest: &Path, name: &str) -> Result<(), String> {
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".7z") {
        extract_7z(archive, dest)
    } else if name.ends_with(".deb") {
        extract_deb(archive, dest)
    } else if name.ends_with(".tar.xz") {
        extract_tar_xz(archive, dest)
    } else if name.ends_with(".tar.gz") {
        extract_tar_gz(archive, dest)
    } else {
        Err(format!("Unsupported dependency archive: {name}"))
    }
}

async fn probe(path: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = tokio::process::Command::new(path);
    command.args(args).kill_on_drop(true);
    super::super::process::hide_tokio_command_window(&mut command);
    let output = tokio::time::timeout(PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("Validation of {} timed out", path.display()))?
        .map_err(|e| format!("Cannot run {}: {e}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} validation failed ({})",
            path.display(),
            output.status
        ));
    }
    Ok(format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

async fn validate_package(root: &Path, package: Package) -> Result<(), String> {
    match package {
        Package::Ffmpeg => {
            let path = root.join(ffmpeg_binary());
            let filters = probe(&path, &["-hide_banner", "-filters"]).await?;
            if !parse_ffmpeg_filter_support(&filters, "libplacebo") {
                return Err("FFmpeg is missing libplacebo".into());
            }
            let build = probe(&path, &["-hide_banner", "-buildconf"]).await?;
            for flag in [
                "--enable-vulkan",
                "--enable-libshaderc",
                "--enable-libplacebo",
            ] {
                if !parse_ffmpeg_buildconf_flag(&build, flag) {
                    return Err(format!("FFmpeg is missing {flag}"));
                }
            }
            let ffprobe = if cfg!(windows) {
                "ffprobe.exe"
            } else {
                "ffprobe"
            };
            probe(&root.join(ffprobe), &["-version"]).await?;
        }
        Package::Nvenc | Package::Vceenc => {
            let binary = if matches!(package, Package::Nvenc) {
                nvencc_binary()
            } else {
                vceenc_binary()
            };
            let help = probe(&root.join(binary), &["--help"]).await?;
            if !help.contains("--codec") {
                return Err(format!("{} returned invalid help output", package.label()));
            }
        }
        Package::Rife => {
            if !RIFE_DEFAULT_MODEL_DIRS
                .iter()
                .any(|name| model_complete(&root.join("vs-rife/models").join(name)))
            {
                return Err("No complete supported RIFE model was installed".into());
            }
        }
    }
    Ok(())
}

async fn get_response(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<reqwest::Response, String> {
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        match client
            .get(url)
            .header(reqwest::header::USER_AGENT, "REFEREE-Dependency-Updater")
            .timeout(timeout)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let status = response.status();
                last_error = format!("HTTP {status} fetching {url}");
                if !(status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS) {
                    break;
                }
            }
            Err(error) => {
                last_error = format!("Request failed for {url}: {error}");
            }
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
        }
    }
    Err(last_error)
}

async fn download_archive<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    asset: &Asset,
    path: &Path,
    package: Package,
    progress: &mut F,
) -> Result<(), String> {
    let download = VerifiedDownload {
        url: &asset.browser_download_url,
        size: (asset.size > 0).then_some(asset.size),
        digest: asset.digest.as_deref(),
        label: package.label(),
        phase: package.phase(),
    };
    download_verified(client, &download, path, progress).await
}

/// A file to download, checked against `size` and a `sha256:<hex>` digest
/// when they are known.
pub(crate) struct VerifiedDownload<'a> {
    pub(crate) url: &'a str,
    pub(crate) size: Option<u64>,
    pub(crate) digest: Option<&'a str>,
    pub(crate) label: &'a str,
    pub(crate) phase: &'a str,
}

/// Downloads to `path` with retries and a stall timeout, reporting 0–90%.
pub(crate) async fn download_verified<F: FnMut(SetupProgress)>(
    client: &reqwest::Client,
    download: &VerifiedDownload<'_>,
    path: &Path,
    progress: &mut F,
) -> Result<(), String> {
    let label = download.label;
    let mut report = |percent: u32, detail: String| {
        progress(SetupProgress {
            phase: download.phase.into(),
            percent,
            detail,
        })
    };
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        report(
            0,
            format!(
                "Downloading {label} (attempt {}/{ATTEMPTS})...",
                attempt + 1
            ),
        );
        let result = async {
            let mut response = client
                .get(download.url)
                .header(reqwest::header::USER_AGENT, "REFEREE-Dependency-Updater")
                .timeout(DOWNLOAD_TIMEOUT)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?;
            let expected = download.size.or_else(|| response.content_length());
            let mut file = fs::File::create(path).map_err(|e| e.to_string())?;
            let mut hasher = Sha256::new();
            let mut downloaded = 0u64;
            let mut last_percent = 0;
            while let Some(chunk) = tokio::time::timeout(IDLE_TIMEOUT, response.chunk())
                .await
                .map_err(|_| "Dependency download stalled".to_string())?
                .map_err(|e| e.to_string())?
            {
                file.write_all(&chunk).map_err(|e| e.to_string())?;
                hasher.update(&chunk);
                downloaded += chunk.len() as u64;
                if let Some(total) = expected.filter(|total| *total > 0) {
                    let percent = (downloaded.saturating_mul(90) / total).min(90) as u32;
                    if percent > last_percent {
                        report(percent, format!("Downloading {label}..."));
                        last_percent = percent;
                    }
                }
            }
            if downloaded == 0 || expected.is_some_and(|size| size != downloaded) {
                return Err("Incomplete dependency download".to_string());
            }
            if let Some(digest) = download.digest {
                let actual = format!("sha256:{:x}", hasher.finalize());
                if !actual.eq_ignore_ascii_case(digest) {
                    return Err("Dependency checksum verification failed".into());
                }
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error;
            }
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
        }
    }
    Err(format!("{label} download failed: {last_error}"))
}

fn report<F: FnMut(SetupProgress)>(
    progress: &mut F,
    package: Package,
    percent: u32,
    detail: String,
) {
    progress(SetupProgress {
        phase: package.phase().into(),
        percent,
        detail,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct TestServer {
        url: String,
        requests: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn serve_bytes(status: reqwest::StatusCode, body: Vec<u8>) -> TestServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move || {
                let body = body.clone();
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    (status, body)
                }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        TestServer {
            url,
            requests,
            task,
        }
    }

    fn model_archive() -> Vec<u8> {
        let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(gzip);
        for name in ["flownet.param", "flownet.bin"] {
            let bytes = b"model data";
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(
                &mut header,
                format!("archive/models/{}/{name}", RIFE_DEFAULT_MODEL_DIRS[0]),
                &bytes[..],
            )
            .unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    #[tokio::test]
    async fn installs_validated_release_and_skips_unchanged_downloads() {
        let body = model_archive();
        let digest = format!("sha256:{:x}", Sha256::digest(&body));
        let size = body.len() as u64;
        let server = serve_bytes(reqwest::StatusCode::OK, body).await;
        let asset = Asset {
            id: 1,
            name: "models.tar.gz".into(),
            size,
            updated_at: "today".into(),
            browser_download_url: server.url.clone(),
            digest: Some(digest),
        };
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        install_asset(&client, &root.0, Package::Rife, "v1", &asset, &mut |_| {})
            .await
            .unwrap();
        assert!(installed_is_current(&root.0, Package::Rife, "v1", &asset));
        install_asset(&client, &root.0, Package::Rife, "v1", &asset, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);
        // Same release with a missing model file must be repaired.
        fs::remove_file(
            root.0
                .join("vs-rife/models")
                .join(RIFE_DEFAULT_MODEL_DIRS[0])
                .join("flownet.bin"),
        )
        .unwrap();
        install_asset(&client, &root.0, Package::Rife, "v1", &asset, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn corrupt_download_keeps_previous_install_and_version() {
        let body = model_archive();
        let size = body.len() as u64;
        let server = serve_bytes(reqwest::StatusCode::OK, body).await;
        let mut asset = Asset {
            id: 1,
            name: "models.tar.gz".into(),
            size,
            updated_at: "today".into(),
            browser_download_url: server.url.clone(),
            digest: None,
        };
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        install_asset(&client, &root.0, Package::Rife, "v1", &asset, &mut |_| {})
            .await
            .unwrap();
        let manifest = fs::read(root.0.join(Package::Rife.manifest())).unwrap();
        asset.digest = Some("sha256:invalid".into());
        let result =
            install_asset(&client, &root.0, Package::Rife, "v2", &asset, &mut |_| {}).await;
        assert!(result.as_ref().unwrap_err().contains("checksum"));
        keep_working_install(&root.0, Package::Rife, result, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(
            fs::read(root.0.join(Package::Rife.manifest())).unwrap(),
            manifest
        );
        validate_package(&root.0, Package::Rife).await.unwrap();
        assert_eq!(server.requests.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn retryable_http_errors_retry_but_not_found_fails_immediately() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let server = serve_bytes(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            b"unavailable".to_vec(),
        )
        .await;
        assert!(get_response(&client, &server.url, METADATA_TIMEOUT)
            .await
            .is_err());
        assert_eq!(server.requests.load(Ordering::SeqCst), ATTEMPTS);
        let server = serve_bytes(reqwest::StatusCode::NOT_FOUND, b"missing".to_vec()).await;
        assert!(get_response(&client, &server.url, METADATA_TIMEOUT)
            .await
            .is_err());
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn incomplete_downloads_never_install() {
        let server = serve_bytes(reqwest::StatusCode::OK, b"partial archive".to_vec()).await;
        let asset = Asset {
            id: 1,
            name: "models.tar.gz".into(),
            size: 999,
            updated_at: "today".into(),
            browser_download_url: server.url.clone(),
            digest: None,
        };
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result =
            install_asset(&client, &root.0, Package::Rife, "v1", &asset, &mut |_| {}).await;
        assert!(result
            .unwrap_err()
            .contains("Incomplete dependency download"));
        assert!(!root.0.join(Package::Rife.manifest()).exists());
        assert!(
            keep_working_install(&root.0, Package::Rife, Err("offline".into()), &mut |_| {})
                .await
                .is_err()
        );
    }

    #[test]
    fn selects_latest_stable_ffmpeg_regardless_of_asset_order() {
        let assets: Vec<Asset> = [
            "ffmpeg-n8.1-win64-gpl-8.1.zip",
            "ffmpeg-master-latest-win64-gpl.zip",
            "ffmpeg-n9.0-win64-gpl-9.0.zip",
            "ffmpeg-n9.0-win64-gpl-shared-9.0.zip",
        ]
        .iter()
        .map(|name| Asset {
            id: 1,
            name: name.to_string(),
            size: 1,
            updated_at: "now".into(),
            browser_download_url: "url".into(),
            digest: None,
        })
        .collect();
        let asset = select_asset(
            &assets,
            &|name: &str| name.contains("win64-gpl") && !name.contains("shared"),
            true,
        )
        .unwrap();
        assert_eq!(asset.name, "ffmpeg-n9.0-win64-gpl-9.0.zip");
    }

    #[test]
    fn rolling_asset_changes_and_missing_files_trigger_reinstallation() {
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        fs::write(root.0.join("encoder"), b"test").unwrap();
        let asset = Asset {
            id: 1,
            name: "build.zip".into(),
            size: 10,
            updated_at: "today".into(),
            browser_download_url: "url".into(),
            digest: None,
        };
        let installed = InstalledRelease {
            tag: "latest".into(),
            asset: asset.clone(),
            files: vec![InstalledFile {
                path: "encoder".into(),
                size: 4,
            }],
        };
        fs::write(
            root.0.join(Package::Nvenc.manifest()),
            serde_json::to_vec(&installed).unwrap(),
        )
        .unwrap();
        assert!(installed_is_current(
            &root.0,
            Package::Nvenc,
            "latest",
            &asset
        ));
        let mut changed = asset.clone();
        changed.updated_at = "tomorrow".into();
        assert!(!installed_is_current(
            &root.0,
            Package::Nvenc,
            "latest",
            &changed
        ));
        fs::write(root.0.join("encoder"), b"").unwrap();
        assert!(!installed_is_current(
            &root.0,
            Package::Nvenc,
            "latest",
            &asset
        ));
    }

    #[test]
    fn empty_model_directories_are_not_accepted() {
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        assert!(!model_complete(&root.0));
        fs::write(root.0.join("flownet.param"), b"param").unwrap();
        assert!(!model_complete(&root.0));
        fs::write(root.0.join("flownet.bin"), b"bin").unwrap();
        assert!(model_complete(&root.0));
    }
}
