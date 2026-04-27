// rife_worker — standalone NUT-in → NUT-out RIFE frame interpolation process.
//
// Spawned as a long-running subprocess by StreamingRifePreprocessor.
// Reads NUT frames from stdin (piped from the normalizer stage), interpolates
// using the VS-RIFE C++ engine via the rife_c shim, and writes the output
// NUT stream to stdout for the executor stage.
//
// Protocol:
//   • Arguments: see parse_args()
//   • Prints "[rife_worker] ready" to stderr once the model is loaded.
//   • Terminates when stdin reaches EOF or if a fatal error occurs.

// ---------------------------------------------------------------------------
// FFI — raw bindings to rife_c.h
// ---------------------------------------------------------------------------

use std::ffi::{c_char, c_int};

extern "C" {
    fn rife_init_gpu() -> c_int;
    fn rife_cleanup_gpu();
    #[allow(dead_code)]
    fn rife_gpu_count() -> c_int;
    fn rife_create(gpuid: c_int, model_dir: *const c_char) -> *mut ();
    fn rife_process_v4(
        handle: *mut (),
        src0_r: *const f32,
        src0_g: *const f32,
        src0_b: *const f32,
        src1_r: *const f32,
        src1_g: *const f32,
        src1_b: *const f32,
        dst_r: *mut f32,
        dst_g: *mut f32,
        dst_b: *mut f32,
        w: c_int,
        h: c_int,
        stride: isize,
        timestep: f32,
    ) -> c_int;
    fn rife_destroy(handle: *mut ());
}

// ---------------------------------------------------------------------------
// PlanarFrame — planar float32 RGB frame buffer
// ---------------------------------------------------------------------------

struct PlanarFrame {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
    w: usize,
    h: usize,
}

impl PlanarFrame {
    fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            r: vec![0.0f32; n],
            g: vec![0.0f32; n],
            b: vec![0.0f32; n],
            w,
            h,
        }
    }
}

// ---------------------------------------------------------------------------
// RIFE instance
// ---------------------------------------------------------------------------

struct RifeInstance(*mut ());

unsafe impl Send for RifeInstance {}

impl RifeInstance {
    fn new(gpuid: i32, model_dir: &str) -> Option<Self> {
        use std::ffi::CString;
        let cpath = CString::new(model_dir).ok()?;
        let ptr = unsafe { rife_create(gpuid, cpath.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    fn interpolate(
        &self,
        src0: &PlanarFrame,
        src1: &PlanarFrame,
        dst: &mut PlanarFrame,
        timestep: f32,
    ) -> bool {
        let stride = (dst.w as isize) * std::mem::size_of::<f32>() as isize;
        let ret = unsafe {
            rife_process_v4(
                self.0,
                src0.r.as_ptr(),
                src0.g.as_ptr(),
                src0.b.as_ptr(),
                src1.r.as_ptr(),
                src1.g.as_ptr(),
                src1.b.as_ptr(),
                dst.r.as_mut_ptr(),
                dst.g.as_mut_ptr(),
                dst.b.as_mut_ptr(),
                dst.w as c_int,
                dst.h as c_int,
                stride,
                timestep,
            )
        };
        ret == 0
    }
}

impl Drop for RifeInstance {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { rife_destroy(self.0) };
            self.0 = std::ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel format helpers
// ---------------------------------------------------------------------------

/// Convert interleaved rgb24 bytes → planar float32 RGB (0.0–1.0).
fn rgb24_to_planar(src: &[u8], dst: &mut PlanarFrame) {
    let n = dst.w * dst.h;
    debug_assert_eq!(src.len(), n * 3);
    for i in 0..n {
        dst.r[i] = src[i * 3] as f32 / 255.0;
        dst.g[i] = src[i * 3 + 1] as f32 / 255.0;
        dst.b[i] = src[i * 3 + 2] as f32 / 255.0;
    }
}

/// Convert planar float32 RGB (0.0–1.0) → interleaved rgb24 bytes.
fn planar_to_rgb24(src: &PlanarFrame, dst: &mut Vec<u8>) {
    let n = src.w * src.h;
    dst.resize(n * 3, 0);
    for i in 0..n {
        dst[i * 3] = (src.r[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        dst[i * 3 + 1] = (src.g[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        dst[i * 3 + 2] = (src.b[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

struct Args {
    ffmpeg_path: String,
    gpu_id: i32,
    model_path: String,
    source_fps: f64,
    target_fps: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1); // skip binary name
    let mut ffmpeg_path = None::<String>;
    let mut gpu_id = 0i32;
    let mut model_path = None::<String>;
    let mut source_fps = None::<f64>;
    let mut target_fps = None::<f64>;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ffmpeg-path" => {
                ffmpeg_path = args.next().ok_or("--ffmpeg-path requires a value")?.into();
            }
            "--gpu-id" => {
                gpu_id = args
                    .next()
                    .ok_or("--gpu-id requires a value")?
                    .parse::<i32>()
                    .map_err(|e| format!("--gpu-id: {e}"))?;
            }
            "--model-path" => {
                model_path = args.next().ok_or("--model-path requires a value")?.into();
            }
            "--source-fps" => {
                source_fps = Some(
                    args.next()
                        .ok_or("--source-fps requires a value")?
                        .parse::<f64>()
                        .map_err(|e| format!("--source-fps: {e}"))?,
                );
            }
            "--target-fps" => {
                target_fps = Some(
                    args.next()
                        .ok_or("--target-fps requires a value")?
                        .parse::<f64>()
                        .map_err(|e| format!("--target-fps: {e}"))?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        ffmpeg_path: ffmpeg_path.ok_or("missing --ffmpeg-path")?,
        gpu_id,
        model_path: model_path.ok_or("missing --model-path")?,
        source_fps: source_fps.ok_or("missing --source-fps")?,
        target_fps: target_fps.ok_or("missing --target-fps")?,
    })
}

// ---------------------------------------------------------------------------
// Stream dimension detection from ffmpeg stderr
// ---------------------------------------------------------------------------

/// Parse the first `NNNxMMM` (both >= 16) resolution token from an ffmpeg
/// stream description line e.g.
///   "  Stream #0:0: Video: rawvideo, yuv420p, 1920x1080, SAR 1:1, 25 tbr"
fn parse_dims_from_line(line: &str) -> Option<(u32, u32)> {
    for part in line.split(',') {
        let p = part.trim();
        if let Some(idx) = p.find('x') {
            let w_s = p[..idx].trim();
            let rest = &p[idx + 1..];
            let h_s = rest
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("");
            if let (Ok(w), Ok(h)) = (w_s.parse::<u32>(), h_s.parse::<u32>()) {
                if w >= 16 && h >= 16 {
                    return Some((w, h));
                }
            }
        }
    }
    None
}

/// Drain `reader` line by line until we find a stream info line carrying
/// frame dimensions.  Returns `None` on EOF without a match.
fn detect_dims<R: std::io::BufRead>(reader: R) -> Option<(u32, u32)> {
    for line in reader.lines().map_while(Result::ok) {
        if line.contains("Stream #0:") && line.contains("Video:") {
            if let Some(dims) = parse_dims_from_line(&line) {
                // Drain remaining stderr in background to prevent pipe deadlock.
                return Some(dims);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

/// Read exactly `buf.len()` bytes from `r`.
/// Returns `true` if all bytes were read, `false` if clean EOF on first byte.
fn read_frame<R: std::io::Read>(r: &mut R, buf: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0usize;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false), // clean EOF
            Ok(0) => return Err("unexpected EOF mid-frame".to_string()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("read error: {e}")),
        }
    }
    Ok(true)
}

/// Write all bytes from `buf` to `w`, ignoring `BrokenPipe` (downstream gone).
fn write_frame<W: std::io::Write>(w: &mut W, buf: &[u8]) -> Result<bool, String> {
    if let Err(e) = w.write_all(buf) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            return Err(format!("write error: {e}"));
        }
        return Ok(false);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn worker_error(message: impl std::fmt::Display) {
    eprintln!("[rife_worker] error: {message}");
}

fn worker_fatal(message: impl std::fmt::Display) -> ! {
    eprintln!("[rife_worker] fatal: {message}");
    std::process::exit(1);
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            worker_error(e);
            std::process::exit(2);
        }
    };

    // Initialize Vulkan/ncnn.
    if unsafe { rife_init_gpu() } != 0 {
        worker_fatal("failed to initialize GPU (Vulkan unavailable?)");
    }

    // Load RIFE model.
    let rife = match RifeInstance::new(args.gpu_id, &args.model_path) {
        Some(r) => r,
        None => {
            unsafe { rife_cleanup_gpu() };
            worker_fatal(format!(
                "failed to load RIFE model from '{}'",
                args.model_path
            ));
        }
    };

    // Signal readiness to the supervisor.
    eprintln!("[rife_worker] ready");

    // Compute interpolation ratio: how many output frames per input frame interval.
    let ratio = {
        let r = (args.target_fps / args.source_fps).round() as usize;
        if r < 2 {
            2
        } else {
            r
        }
    };

    // -----------------------------------------------------------------------
    // Spawn ffmpeg decoder: stdin=inherited → raw rgb24 frames on stdout
    // -----------------------------------------------------------------------
    #[cfg(target_os = "windows")]
    let decode_creation_flags: u32 = 0x08000000; // CREATE_NO_WINDOW

    let mut decode_proc = {
        let mut cmd = std::process::Command::new(&args.ffmpeg_path);
        cmd.args([
            "-loglevel",
            "verbose",
            "-f",
            "nut",
            "-i",
            "pipe:0",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-vsync",
            "passthrough",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(decode_creation_flags);
        }

        match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                unsafe { rife_cleanup_gpu() };
                worker_fatal(format!(
                    "failed to spawn ffmpeg decoder '{}': {error}",
                    args.ffmpeg_path
                ));
            }
        }
    };

    // Read decoder stderr in a separate thread to detect frame dimensions
    // and prevent the stderr pipe from blocking the decoder.
    let decoder_stderr = match decode_proc.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = decode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal("ffmpeg decoder stderr pipe was unavailable");
        }
    };
    let (dim_tx, dim_rx) = std::sync::mpsc::channel::<(u32, u32)>();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(decoder_stderr);
        if let Some(dims) = detect_dims(reader) {
            let _ = dim_tx.send(dims);
        }
        // Reader drop drains remaining bytes automatically.
    });

    // Wait up to 30 s for the decoder to announce stream dimensions.
    let (w, h) = match dim_rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(dims) => dims,
        Err(_) => {
            let _ = decode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal("timed out waiting for stream dimensions from ffmpeg");
        }
    };

    // -----------------------------------------------------------------------
    // Spawn ffmpeg encoder: raw rgb24 frames on stdin → NUT on inherited stdout
    // -----------------------------------------------------------------------
    let mut encode_proc = {
        let mut cmd = std::process::Command::new(&args.ffmpeg_path);
        cmd.args([
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            &format!("{w}x{h}"),
            "-r",
            &format!("{}", args.target_fps),
            "-i",
            "pipe:0",
            "-vf",
            "format=yuv420p",
            "-c:v",
            "rawvideo",
            "-f",
            "nut",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(decode_creation_flags);
        }

        match cmd.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = decode_proc.kill();
                unsafe { rife_cleanup_gpu() };
                worker_fatal(format!(
                    "failed to spawn ffmpeg encoder '{}': {error}",
                    args.ffmpeg_path
                ));
            }
        }
    };

    let encode_stdin = match encode_proc.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = decode_proc.kill();
            let _ = encode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal("ffmpeg encoder stdin pipe was unavailable");
        }
    };
    let mut enc_writer = std::io::BufWriter::with_capacity(4 * 1024 * 1024, encode_stdin);

    // -----------------------------------------------------------------------
    // Frame interpolation loop
    // -----------------------------------------------------------------------
    let frame_bytes = (w as usize) * (h as usize) * 3;
    let mut raw_buf = vec![0u8; frame_bytes];
    let mut rgb_out = Vec::<u8>::with_capacity(frame_bytes);

    let mut prev = PlanarFrame::new(w as usize, h as usize);
    let mut cur = PlanarFrame::new(w as usize, h as usize);
    let mut interp = PlanarFrame::new(w as usize, h as usize);

    let mut stdout = match decode_proc.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = decode_proc.kill();
            let _ = encode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal("ffmpeg decoder stdout pipe was unavailable");
        }
    };

    // Read first frame.
    match read_frame(&mut stdout, &mut raw_buf) {
        Ok(true) => {}
        Ok(false) => {
            // Empty input — nothing to do.
            drop(enc_writer);
            let _ = encode_proc.wait();
            drop(rife);
            unsafe { rife_cleanup_gpu() };
            return;
        }
        Err(error) => {
            let _ = decode_proc.kill();
            let _ = encode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal(error);
        }
    }
    rgb24_to_planar(&raw_buf, &mut prev);
    planar_to_rgb24(&prev, &mut rgb_out);
    match write_frame(&mut enc_writer, &rgb_out) {
        Ok(true) => {}
        Ok(false) => {
            drop(enc_writer);
            let _ = encode_proc.wait();
            drop(rife);
            unsafe { rife_cleanup_gpu() };
            return;
        }
        Err(error) => {
            let _ = decode_proc.kill();
            let _ = encode_proc.kill();
            unsafe { rife_cleanup_gpu() };
            worker_fatal(error);
        }
    }

    // Process remaining frames.
    'frames: loop {
        match read_frame(&mut stdout, &mut raw_buf) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                let _ = decode_proc.kill();
                let _ = encode_proc.kill();
                unsafe { rife_cleanup_gpu() };
                worker_fatal(error);
            }
        }
        rgb24_to_planar(&raw_buf, &mut cur);

        // Generate (ratio - 1) intermediate frames between prev and cur.
        for step in 1..ratio {
            let timestep = step as f32 / ratio as f32;
            rife.interpolate(&prev, &cur, &mut interp, timestep);
            planar_to_rgb24(&interp, &mut rgb_out);
            match write_frame(&mut enc_writer, &rgb_out) {
                Ok(true) => {}
                Ok(false) => break 'frames,
                Err(error) => {
                    let _ = decode_proc.kill();
                    let _ = encode_proc.kill();
                    unsafe { rife_cleanup_gpu() };
                    worker_fatal(error);
                }
            }
        }

        // Write current frame.
        planar_to_rgb24(&cur, &mut rgb_out);
        match write_frame(&mut enc_writer, &rgb_out) {
            Ok(true) => {}
            Ok(false) => break 'frames,
            Err(error) => {
                let _ = decode_proc.kill();
                let _ = encode_proc.kill();
                unsafe { rife_cleanup_gpu() };
                worker_fatal(error);
            }
        }

        // Advance rolling buffer without re-allocating.
        std::mem::swap(&mut prev, &mut cur);
    }

    // Flush and close encoder.
    drop(enc_writer);
    let _ = encode_proc.wait();

    // Tear down RIFE and GPU before exit.
    drop(rife);
    unsafe { rife_cleanup_gpu() };
}

// ---------------------------------------------------------------------------
// Unit tests (dimension parsing only — no GPU required)
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::parse_dims_from_line;

    #[test]
    fn parses_1080p() {
        let line = "  Stream #0:0: Video: rawvideo, yuv420p, 1920x1080, SAR 1:1, 25 tbr, 25 tbn";
        assert_eq!(parse_dims_from_line(line), Some((1920, 1080)));
    }

    #[test]
    fn parses_4k() {
        let line = "  Stream #0:0: Video: rawvideo, yuv420p, 3840x2160 [SAR 1:1 DAR 16:9], 60 tbr";
        assert_eq!(parse_dims_from_line(line), Some((3840, 2160)));
    }

    #[test]
    fn ignores_sar_ratio() {
        // "SAR 1:1" should not be mistaken for a dimension
        let line = "  Stream #0:0: Video: rawvideo, yuv420p, 1280x720, SAR 1:1, 30 tbr";
        assert_eq!(parse_dims_from_line(line), Some((1280, 720)));
    }
}
