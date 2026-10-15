use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Captured output of a one-off capability probe such as `--help` or `-filters`.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ProbeOutput {
    pub(crate) success: bool,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Probe outputs keyed by binary path, arguments, and the binary's size and
/// mtime, so a binary replaced by setup or an update is probed again.
type ProbeCacheKey = (PathBuf, Vec<String>, u64, SystemTime);

static PROBE_CACHE: LazyLock<Mutex<HashMap<ProbeCacheKey, Arc<ProbeOutput>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Runs `program args` and caches the output for the lifetime of the process.
///
/// Capability probes depend only on the binary, but spawning a large encoder
/// or a static FFmpeg build costs hundreds of milliseconds, and stream start
/// used to pay that several times per session. Binaries whose metadata cannot
/// be read (for example a bare name resolved through `PATH`) are probed
/// uncached.
#[allow(dead_code)]
pub(crate) fn cached_probe_output(
    program: &Path,
    args: &[&str],
) -> std::io::Result<Arc<ProbeOutput>> {
    let key = std::fs::metadata(program)
        .and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
        .ok()
        .map(|(len, modified)| {
            (
                program.to_path_buf(),
                args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
                len,
                modified,
            )
        });

    if let Some(key) = &key {
        if let Some(cached) = PROBE_CACHE.lock().unwrap().get(key) {
            return Ok(Arc::clone(cached));
        }
    }

    let mut command = std::process::Command::new(program);
    command.args(args);
    hide_std_command_window(&mut command);
    let output = command.output()?;
    let probe = Arc::new(ProbeOutput {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: output.stderr,
    });

    if let Some(key) = key {
        PROBE_CACHE.lock().unwrap().insert(key, Arc::clone(&probe));
    }
    Ok(probe)
}

#[allow(dead_code)]
pub(crate) fn hide_std_command_window(command: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

pub(crate) fn hide_tokio_command_window(command: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}
