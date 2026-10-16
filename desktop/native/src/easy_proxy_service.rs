//! Runs the EasyProxy install from `deps::easy_proxy` on demand: started for
//! the first stream that needs it and stopped after [`IDLE_SHUTDOWN`] without
//! one. It listens on loopback, on a port and with an API password chosen for
//! this REFEREE run ([`ServerAccess`]), so the routes that sessions hold
//! survive a restart.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info, warn};

use crate::deps::easy_proxy::{self as installer, InstallState};
use crate::deps::SetupProgress;
use crate::source::easy_proxy::{EasyProxyRoute, DEFAULT_EASY_PROXY_PORT};

/// How long the server keeps running with no stream routed through it.
pub const IDLE_SHUTDOWN: Duration = Duration::from_secs(300);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const STARTUP_POLL: Duration = Duration::from_millis(250);
const STDERR_TAIL_LINES: usize = 12;

/// How the managed server, which only ever listens on loopback, is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerAccess {
    /// A free port and an API password chosen for this REFEREE run.
    Private,
    /// EasyProxy's default port while it is free, and no API password, so its
    /// extractor and admin pages can be opened by hand during development.
    Open,
}

impl ServerAccess {
    /// `Open` in development builds, `Private` in release builds.
    pub fn for_build() -> Self {
        if cfg!(debug_assertions) {
            Self::Open
        } else {
            Self::Private
        }
    }
}

pub struct EasyProxyService {
    lib_dir: PathBuf,
    /// EasyProxy's `CONFIG_DIR`: its admin settings and recordings. Kept
    /// outside the install so updates and removal leave it alone.
    data_dir: PathBuf,
    client: reqwest::Client,
    access: ServerAccess,
    /// `None` with [`ServerAccess::Open`].
    password: Option<String>,
    install: Mutex<InstallProgress>,
    process: tokio::sync::Mutex<Option<ManagedProcess>>,
    port: Mutex<Option<u16>>,
    last_used: Mutex<Instant>,
}

#[derive(Default)]
struct InstallProgress {
    running: bool,
    progress: Option<SetupProgress>,
    error: Option<String>,
}

struct ManagedProcess {
    child: tokio::process::Child,
    route: EasyProxyRoute,
}

impl ManagedProcess {
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEasyProxyStatus {
    pub install_state: InstallState,
    pub installing: bool,
    pub progress: Option<SetupProgress>,
    pub error: Option<String>,
    pub running: bool,
    pub starting: bool,
    pub version: &'static str,
    pub commit: &'static str,
}

impl EasyProxyService {
    pub fn new(
        lib_dir: PathBuf,
        data_dir: PathBuf,
        client: reqwest::Client,
        access: ServerAccess,
    ) -> Arc<Self> {
        Arc::new(Self {
            lib_dir,
            data_dir,
            client,
            access,
            password: (access == ServerAccess::Private)
                .then(|| uuid::Uuid::new_v4().simple().to_string()),
            install: Mutex::new(InstallProgress::default()),
            process: tokio::sync::Mutex::new(None),
            port: Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
        })
    }

    pub fn install_state(&self) -> InstallState {
        installer::install_state(&self.lib_dir)
    }

    pub fn status(&self) -> ManagedEasyProxyStatus {
        let install_state = self.install_state();
        let install = self.install.lock().unwrap();
        // The process lock is held while the server starts.
        let (running, starting) = match self.process.try_lock() {
            Ok(mut process) => (
                process.as_mut().is_some_and(ManagedProcess::is_alive),
                false,
            ),
            Err(_) => (false, !install.running),
        };
        ManagedEasyProxyStatus {
            install_state,
            installing: install.running,
            progress: install.progress.clone(),
            error: install.error.clone(),
            running,
            starting,
            version: installer::EASY_PROXY_VERSION,
            commit: installer::pinned_commit(),
        }
    }

    /// Installs or updates EasyProxy in the background, unless an install is
    /// already running. Stops the server first, because Windows cannot move
    /// files a running Python holds open.
    pub fn start_install(self: &Arc<Self>) {
        {
            let mut install = self.install.lock().unwrap();
            if install.running {
                return;
            }
            *install = InstallProgress {
                running: true,
                ..InstallProgress::default()
            };
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            service.stop().await;
            let result = installer::install(&service.client, &service.lib_dir, &mut |update| {
                service.install.lock().unwrap().progress = Some(update);
            })
            .await;
            let mut install = service.install.lock().unwrap();
            install.running = false;
            if let Err(error) = result {
                warn!("[EasyProxy]: Install failed: {}", error);
                install.error = Some(error);
            }
        });
    }

    /// The route to the running server, starting it when needed. Fails while
    /// EasyProxy is missing or outdated, and starts that install.
    pub async fn ensure_running(self: &Arc<Self>) -> Result<EasyProxyRoute, String> {
        self.touch();
        let mut process = self.process.lock().await;
        {
            let install = self.install.lock().unwrap();
            if install.running {
                let percent = install.progress.as_ref().map_or(0, |update| update.percent);
                return Err(format!(
                    "EasyProxy is still being installed ({percent}%). Try again when it finishes."
                ));
            }
        }

        match self.install_state() {
            InstallState::Current => {}
            InstallState::Unsupported => {
                return Err(
                    "REFEREE can only run EasyProxy on Windows and Linux x86_64. \
                     Use an external EasyProxy server instead."
                        .to_string(),
                )
            }
            state => {
                if let Some(error) = self.install.lock().unwrap().error.clone() {
                    return Err(format!("EasyProxy is not installed: {error}"));
                }
                drop(process);
                self.start_install();
                return Err(if state == InstallState::Missing {
                    "EasyProxy is being installed for first use. Try again when it finishes."
                } else {
                    "EasyProxy is being updated. Try again when it finishes."
                }
                .to_string());
            }
        }

        if let Some(running) = process.as_mut() {
            if running.is_alive() && running.route.check().await.is_ok() {
                return Ok(running.route.clone());
            }
            warn!("[EasyProxy]: The managed server stopped responding; restarting it.");
            running.kill().await;
            *process = None;
        }

        let started = self.spawn().await?;
        let route = started.route.clone();
        *process = Some(started);
        Ok(route)
    }

    /// Whether `route` points at this service's server.
    pub fn serves(&self, route: &EasyProxyRoute) -> bool {
        self.port
            .lock()
            .unwrap()
            .is_some_and(|port| route.origin() == format!("http://127.0.0.1:{port}"))
    }

    pub async fn stop(&self) {
        if let Some(mut running) = self.process.lock().await.take() {
            running.kill().await;
            info!("[EasyProxy]: Stopped the managed server.");
        }
    }

    /// Stops the server once no stream has used it for `idle_after`.
    pub async fn stop_if_idle(&self, in_use: bool, idle_after: Duration) {
        if in_use {
            self.touch();
            return;
        }
        if self.last_used.lock().unwrap().elapsed() < idle_after {
            return;
        }
        // Held while the server starts, which counts as use.
        let Ok(mut process) = self.process.try_lock() else {
            return;
        };
        if let Some(mut running) = process.take() {
            running.kill().await;
            info!("[EasyProxy]: Stopped the managed server; no stream is using it.");
        }
    }

    /// Stops the server and deletes the install, keeping EasyProxy's data.
    pub async fn remove(&self) -> Result<(), String> {
        if self.install.lock().unwrap().running {
            return Err("EasyProxy is being installed. Try again when it finishes.".to_string());
        }
        let mut process = self.process.lock().await;
        if let Some(mut running) = process.take() {
            running.kill().await;
        }
        let lib_dir = self.lib_dir.clone();
        tokio::task::spawn_blocking(move || installer::remove(&lib_dir))
            .await
            .map_err(|error| format!("EasyProxy removal task failed: {error}"))??;
        self.install.lock().unwrap().error = None;
        Ok(())
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// The port from earlier in this run when it is still free, so routes held
    /// by sessions stay valid across a restart. Then, for open access,
    /// EasyProxy's default port.
    fn pick_port(&self) -> Result<u16, String> {
        let is_free = |port: u16| std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        let mut port = self.port.lock().unwrap();
        if let Some(previous) = *port {
            if is_free(previous) {
                return Ok(previous);
            }
        }
        if self.access == ServerAccess::Open {
            if is_free(DEFAULT_EASY_PROXY_PORT) {
                *port = Some(DEFAULT_EASY_PROXY_PORT);
                return Ok(DEFAULT_EASY_PROXY_PORT);
            }
            warn!(
                "[EasyProxy]: Port {} is in use, so the managed server takes another port.",
                DEFAULT_EASY_PROXY_PORT
            );
        }
        let chosen = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .map_err(|error| format!("Cannot find a free port for EasyProxy: {error}"))?
            .port();
        *port = Some(chosen);
        Ok(chosen)
    }

    async fn spawn(&self) -> Result<ManagedProcess, String> {
        let install_dir = installer::install_dir(&self.lib_dir);
        let launcher = installer::write_launcher(&install_dir)?;
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|error| format!("Cannot create EasyProxy's data folder: {error}"))?;
        let port = self.pick_port()?;
        let route = EasyProxyRoute::new(
            Some(&format!("http://127.0.0.1:{port}")),
            self.password.as_deref(),
        )?;

        let mut command = tokio::process::Command::new(installer::python_path(&install_dir));
        command
            .args(["-I", "-u", "-X", "utf8"])
            .arg(&launcher)
            .current_dir(&install_dir)
            .env("PORT", port.to_string());
        // EasyProxy checks no password when `API_PASSWORD` is unset, so one
        // inherited from REFEREE's environment must not reach it.
        match &self.password {
            Some(password) => command.env("API_PASSWORD", password),
            None => command.env_remove("API_PASSWORD"),
        };
        command
            .env("CONFIG_DIR", &self.data_dir)
            .env("REFEREE_PARENT_PID", std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::process::hide_tokio_command_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("Cannot start EasyProxy: {error}"))?;

        // EasyProxy logs every request URL, so its output stays at debug level.
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&tail);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("[EasyProxy:server]: {}", line);
                    let mut tail = tail.lock().unwrap();
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        let output = |tail: &Mutex<VecDeque<String>>| {
            let tail = tail.lock().unwrap();
            if tail.is_empty() {
                "no output".to_string()
            } else {
                tail.iter().cloned().collect::<Vec<_>>().join(" | ")
            }
        };

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "EasyProxy exited during startup ({status}): {}",
                    output(&tail)
                ));
            }
            if let Ok(status) = route.check().await {
                info!(
                    "[EasyProxy]: Started the managed server {} on 127.0.0.1:{}{}",
                    status
                        .version
                        .as_deref()
                        .unwrap_or(installer::EASY_PROXY_VERSION),
                    port,
                    if self.password.is_none() {
                        " with no API password"
                    } else {
                        ""
                    }
                );
                return Ok(ManagedProcess { child, route });
            }
            if Instant::now() >= deadline {
                let _ = child.kill().await;
                return Err(format!(
                    "EasyProxy did not start within {} s: {}",
                    STARTUP_TIMEOUT.as_secs(),
                    output(&tail)
                ));
            }
            tokio::time::sleep(STARTUP_POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EasyProxyService, ServerAccess, IDLE_SHUTDOWN};
    use crate::deps::easy_proxy::InstallState;
    use crate::source::easy_proxy::{EasyProxyRoute, DEFAULT_EASY_PROXY_PORT};
    use std::time::Duration;

    fn service_with(access: ServerAccess) -> std::sync::Arc<EasyProxyService> {
        let root = std::env::temp_dir().join(format!("referee-easyproxy-{}", uuid::Uuid::new_v4()));
        EasyProxyService::new(
            root.join("lib"),
            root.join("data"),
            reqwest::Client::new(),
            access,
        )
    }

    fn service() -> std::sync::Arc<EasyProxyService> {
        service_with(ServerAccess::Private)
    }

    #[test]
    fn reports_a_missing_install_and_a_stopped_server() {
        let service = service();
        let status = service.status();
        assert_eq!(status.install_state, InstallState::Missing);
        assert!(!status.installing && !status.running && !status.starting);
        assert_eq!(status.version, crate::deps::easy_proxy::EASY_PROXY_VERSION);
        assert_eq!(service.password.as_deref().map(str::len), Some(32));
    }

    #[test]
    fn open_access_uses_the_default_port_while_free_and_no_password() {
        assert_eq!(
            ServerAccess::for_build(),
            if cfg!(debug_assertions) {
                ServerAccess::Open
            } else {
                ServerAccess::Private
            }
        );
        let service = service_with(ServerAccess::Open);
        assert_eq!(service.password, None);

        // Another EasyProxy may already hold the default port on this machine.
        let default_free =
            std::net::TcpListener::bind(("127.0.0.1", DEFAULT_EASY_PROXY_PORT)).is_ok();
        let port = service.pick_port().unwrap();
        assert_eq!(
            port == DEFAULT_EASY_PROXY_PORT,
            default_free,
            "picked {port}"
        );
        assert_eq!(service.pick_port().unwrap(), port);
    }

    #[test]
    fn keeps_its_port_while_free_and_recognizes_its_own_routes() {
        let service = service();
        let port = service.pick_port().unwrap();
        assert_eq!(service.pick_port().unwrap(), port);

        let own = EasyProxyRoute::new(Some(&format!("http://127.0.0.1:{port}")), None).unwrap();
        let other = EasyProxyRoute::new(Some("http://127.0.0.1:7860"), None).unwrap();
        assert!(service.serves(&own));
        assert!(!service.serves(&other));

        // Taken by something else: a new port is chosen.
        let _holder = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        assert_ne!(service.pick_port().unwrap(), port);
    }

    #[tokio::test]
    async fn idle_stop_waits_for_the_idle_period() {
        let service = service();
        service.stop_if_idle(false, IDLE_SHUTDOWN).await;
        service.stop_if_idle(true, Duration::ZERO).await;
        service.stop_if_idle(false, Duration::ZERO).await;
        assert!(!service.status().running);
    }

    /// Downloads about 110 MB and runs the real server. Run with
    /// `cargo test --features headless --no-default-features live_managed_easy_proxy -- --ignored --nocapture`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_managed_easy_proxy_installs_starts_and_routes_a_stream() {
        let service = service();
        service.start_install();
        let mut last_detail = String::new();
        while service.status().installing {
            if let Some(progress) = service.status().progress {
                if progress.detail != last_detail {
                    println!("{:>3}% {}", progress.percent, progress.detail);
                    last_detail = progress.detail;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let status = service.status();
        assert_eq!(status.error, None);
        assert_eq!(status.install_state, InstallState::Current);

        let started = std::time::Instant::now();
        let route = service.ensure_running().await.unwrap();
        println!("started in {:?} at {}", started.elapsed(), route.origin());
        assert!(service.status().running);
        assert_eq!(
            route.check().await.unwrap().version.as_deref(),
            Some(crate::deps::easy_proxy::EASY_PROXY_VERSION)
        );

        // The manifest comes back rewritten onto EasyProxy, and every entry is
        // something the input proxy will fetch from it.
        let routed = route.route(
            "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8",
            &std::collections::HashMap::new(),
        );
        let mut request = reqwest::Client::new().get(&routed.url);
        for (name, value) in &routed.headers {
            request = request.header(name, value);
        }
        let manifest = request.send().await.unwrap().text().await.unwrap();
        println!("{manifest}");
        let entries: Vec<_> = manifest
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .collect();
        assert!(!entries.is_empty(), "{manifest}");
        for entry in entries {
            let url = reqwest::Url::parse(entry).unwrap();
            assert!(route.serves(&url), "{entry}");
            assert!(route.allows_relay_target(&url), "{entry}");
        }

        // Through REFEREE's input proxy, as the encoder pulls it: master
        // playlist (pinned to one variant), variant playlist, then a segment.
        let mut target = routed.url.clone();
        for step in ["master", "variant"] {
            let response = crate::source::hls_relay::relay_request(
                "127.0.0.1",
                14002,
                "live-test",
                &std::collections::HashMap::new(),
                Some(&route),
                &target,
            )
            .await
            .unwrap();
            assert!(
                response.status().is_success(),
                "{step}: {}",
                response.status()
            );
            let body = axum::body::to_bytes(response.into_body(), 4 << 20)
                .await
                .unwrap();
            let playlist = String::from_utf8(body.to_vec()).unwrap();
            let next = playlist
                .lines()
                .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .unwrap_or_else(|| panic!("{step} playlist has no entries: {playlist}"));
            let relayed = reqwest::Url::parse(next).unwrap();
            assert_eq!(relayed.path(), "/v1/input/live-test", "{step}: {next}");
            target = relayed
                .query_pairs()
                .find(|(name, _)| name == "url")
                .unwrap()
                .1
                .into_owned();
        }
        let segment = crate::source::hls_relay::relay_request(
            "127.0.0.1",
            14002,
            "live-test",
            &std::collections::HashMap::new(),
            Some(&route),
            &target,
        )
        .await
        .unwrap();
        assert!(
            segment.status().is_success(),
            "segment: {}",
            segment.status()
        );
        let bytes = axum::body::to_bytes(segment.into_body(), 64 << 20)
            .await
            .unwrap();
        println!("segment: {} bytes from {}", bytes.len(), target);
        assert_eq!(bytes.first(), Some(&0x47), "not an MPEG-TS segment");

        // A restart keeps the port and password, so routes held by sessions
        // stay valid.
        service.stop().await;
        assert_eq!(service.ensure_running().await.unwrap(), route);

        service.remove().await.unwrap();
        assert_eq!(service.install_state(), InstallState::Missing);
    }
}
