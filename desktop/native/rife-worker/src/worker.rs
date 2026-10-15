//! The streaming worker: NUT on stdin → RIFE frame-rate conversion → NUT on
//! stdout, with audio passed through.
//!
//! ```text
//! stdin ─feeder─► ffmpeg decoder ─rgb24─► reader ─► scheduler ─┬─► writer ─► ffmpeg encoder ─► stdout
//!                       │                                       └─► RIFE pool ─┘      ▲
//!                       └──────── audio (NUT over loopback TCP, relayed) ─────────────┘
//! ```

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, ChildStdout, Command as Process, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::args::{Command, RunArgs};
use crate::engine::{Gpu, PlanarFrame, Rife};
use crate::nut::{sniff_streams, NutStreams};
use crate::timeline::{LagMonitor, OutputFrame, Schedule};
use crate::{args, ffmpeg};

/// Give up looking for the NUT stream headers after this much input.
const HEADER_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const STDIN_CHUNK_BYTES: usize = 1024 * 1024;
/// Stop blending once output is this far behind real time...
const LAG_ENTER_SECONDS: f64 = 1.0;
/// ...and resume once back within this.
const LAG_EXIT_SECONDS: f64 = 0.25;
const STATS_INTERVAL_SECONDS: f64 = 30.0;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn main() -> i32 {
    let command = match args::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("[rife_worker] error: {error}");
            return 2;
        }
    };
    let result = match command {
        Command::Probe { model_path, gpu } => probe(&model_path, gpu),
        Command::Run(args) => run(&args),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("[rife_worker] fatal: {error}");
            1
        }
    }
}

/// Loads the model and runs one small inference, so the app can tell whether
/// RIFE works on this machine.
fn probe(model_path: &str, gpu_index: Option<i32>) -> Result<(), String> {
    let gpu = Gpu::init()?;
    let index = gpu_index.unwrap_or_else(|| gpu.default_index());
    let rife = Rife::load(&gpu, index, model_path)?;
    let frame = PlanarFrame::new(256, 256);
    let mut out = PlanarFrame::new(256, 256);
    let started = Instant::now();
    rife.interpolate(&frame, &frame, 0.5, &mut out)?;
    eprintln!(
        "[rife_worker] probe ok: gpu={} inference_ms={}",
        gpu.name(index).unwrap_or_default(),
        started.elapsed().as_millis()
    );
    Ok(())
}

struct InputFrame {
    rgb: Vec<u8>,
    planar: PlanarFrame,
}

/// One output frame, in output order.
enum OutputItem {
    Input(Arc<InputFrame>),
    Blended(Receiver<Result<Vec<u8>, String>>),
}

struct BlendJob {
    from: Arc<InputFrame>,
    to: Arc<InputFrame>,
    timestep: f32,
    reply: SyncSender<Result<Vec<u8>, String>>,
}

#[derive(Default)]
struct Stats {
    copied: u64,
    blended: u64,
    repeated: u64,
}

fn run(args: &RunArgs) -> Result<(), String> {
    let gpu = Gpu::init()?;
    let gpu_index = args.gpu.unwrap_or_else(|| gpu.default_index());
    let rife = Rife::load(&gpu, gpu_index, &args.model_path)?;
    eprintln!(
        "[rife_worker] gpu {gpu_index}: {}, {} -> {} fps",
        gpu.name(gpu_index).unwrap_or_default(),
        args.source_rate.ffmpeg_arg(),
        args.target_rate.ffmpeg_arg()
    );
    eprintln!("[rife_worker] ready");

    let Some((header, streams)) = read_stream_header()? else {
        return Ok(()); // Empty input.
    };
    let (width, height) = streams.video.ok_or("the input has no video stream")?;

    let audio = if streams.has_audio {
        Some(AudioRelay::start().map_err(|error| format!("audio relay: {error}"))?)
    } else {
        None
    };
    let mut decoder = spawn(
        &args.ffmpeg_path,
        ffmpeg::decoder_args(
            args.source_rate,
            audio.as_ref().map(|relay| relay.decoder_port),
        ),
        Stdio::piped(),
        Stdio::piped(),
    )?;
    let mut encoder = spawn(
        &args.ffmpeg_path,
        ffmpeg::encoder_args(
            width,
            height,
            args.target_rate,
            &args.pixel_format,
            audio.as_ref().map(|relay| relay.encoder_port),
        ),
        Stdio::piped(),
        Stdio::inherit(),
    )?;

    let decoder_stdin = decoder.stdin.take().ok_or("decoder stdin unavailable")?;
    let decoder_stdout = decoder.stdout.take().ok_or("decoder stdout unavailable")?;
    let encoder_stdin = encoder.stdin.take().ok_or("encoder stdin unavailable")?;

    // Detached: it may block reading stdin until the upstream stage exits.
    thread::spawn(move || feed_decoder(header, decoder_stdin));

    let schedule = Schedule::new(args.source_rate, args.target_rate);
    let (frame_tx, frame_rx) = mpsc::sync_channel::<Arc<InputFrame>>(3);
    let (job_tx, job_rx) = mpsc::sync_channel::<BlendJob>(args.threads * 2);
    let job_rx = Mutex::new(job_rx);
    let (output_tx, output_rx) = mpsc::sync_channel::<OutputItem>(args.threads * 2 + 2);

    let outcome = thread::scope(|scope| {
        let reader = scope.spawn(move || read_frames(decoder_stdout, width, height, frame_tx));
        let writer = scope.spawn(move || write_frames(output_rx, encoder_stdin));
        for _ in 0..args.threads {
            scope.spawn(|| blend_frames(&rife, &job_rx, width, height));
        }

        let scheduled = schedule_frames(schedule, frame_rx, job_tx, output_tx);
        if scheduled.is_err() {
            // Unblock the reader and writer so the scope can join them.
            let _ = decoder.kill();
            let _ = encoder.kill();
        }
        let read = reader
            .join()
            .unwrap_or_else(|_| Err("reader panicked".into()));
        if read.is_err() {
            // A failed decoder can leave the encoder waiting on its audio
            // input, and the writer blocked on it.
            let _ = encoder.kill();
        }
        let written = writer
            .join()
            .unwrap_or_else(|_| Err("writer panicked".into()));
        // The writer and reader see the root cause (encoder, RIFE, or decoder
        // failure); the scheduler only sees a closed channel.
        written.and(read).and(scheduled)
    });

    match outcome {
        Ok(stats) => {
            let _ = encoder.wait();
            let _ = decoder.wait();
            eprintln!(
                "[rife_worker] done: {} interpolated, {} copied, {} repeated to keep up",
                stats.blended, stats.copied, stats.repeated
            );
            Ok(())
        }
        Err(error) => {
            let _ = decoder.kill();
            let _ = encoder.kill();
            Err(error)
        }
    }
}

/// Reads stdin until the NUT stream headers are complete. Returns the bytes
/// read (to replay into the decoder) and the streams, or `None` on empty input.
fn read_stream_header() -> Result<Option<(Vec<u8>, NutStreams)>, String> {
    let mut stdin = io::stdin().lock();
    let mut header = Vec::new();
    let mut chunk = vec![0u8; STDIN_CHUNK_BYTES];
    loop {
        let read = stdin
            .read(&mut chunk)
            .map_err(|error| format!("reading stdin: {error}"))?;
        if read == 0 {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err("input ended before its NUT stream headers".into())
            };
        }
        header.extend_from_slice(&chunk[..read]);
        if let Some(streams) = sniff_streams(&header) {
            return Ok(Some((header, streams)));
        }
        if header.len() > HEADER_LIMIT_BYTES {
            return Err("no NUT stream headers in the input".into());
        }
    }
}

fn feed_decoder(header: Vec<u8>, mut decoder_stdin: ChildStdin) {
    if decoder_stdin.write_all(&header).is_ok() {
        // Ends at EOF or when the decoder goes away (the worker is stopping).
        let _ = io::copy(&mut io::stdin().lock(), &mut decoder_stdin);
    }
}

fn read_frames(
    mut decoder_stdout: ChildStdout,
    width: u32,
    height: u32,
    frames: SyncSender<Arc<InputFrame>>,
) -> Result<(), String> {
    let (width, height) = (width as usize, height as usize);
    loop {
        let mut rgb = vec![0u8; width * height * 3];
        if !read_full(&mut decoder_stdout, &mut rgb)? {
            return Ok(());
        }
        let mut planar = PlanarFrame::new(width, height);
        planar.load_rgb24(&rgb);
        if frames.send(Arc::new(InputFrame { rgb, planar })).is_err() {
            return Ok(());
        }
    }
}

/// Fills `buf`; `false` on a clean EOF before the first byte.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err("decoder output ended mid-frame".into()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("reading decoder output: {error}")),
        }
    }
    Ok(true)
}

/// Decides how each output frame is made, in order, and hands blends to the
/// RIFE pool. Falls back to repeating the nearest frame while output is behind
/// real time.
fn schedule_frames(
    schedule: Schedule,
    frames: Receiver<Arc<InputFrame>>,
    jobs: SyncSender<BlendJob>,
    output: SyncSender<OutputItem>,
) -> Result<Stats, String> {
    let mut window: VecDeque<Arc<InputFrame>> = VecDeque::new();
    let mut window_start = 0u64;
    let mut received = 0u64;
    let mut started: Option<Instant> = None;
    let mut lag = LagMonitor::new(LAG_ENTER_SECONDS, LAG_EXIT_SECONDS);
    let mut stats = Stats::default();
    let mut last_report = 0.0;

    for index in 0u64.. {
        let frame = schedule.frame(index);
        while received <= frame.last_input() {
            let Ok(input) = frames.recv() else {
                return Ok(stats); // End of input.
            };
            started.get_or_insert_with(Instant::now);
            window.push_back(input);
            received += 1;
        }

        let first_needed = match frame {
            OutputFrame::Copy(input) => input,
            OutputFrame::Blend { from, .. } => from,
        };
        while window_start < first_needed {
            window.pop_front();
            window_start += 1;
        }
        let input = |index: u64| Arc::clone(&window[(index - window_start) as usize]);

        let wall_seconds = started.map_or(0.0, |started| started.elapsed().as_secs_f64());
        let behind = lag.update(schedule.output_time(index), wall_seconds);
        let item = match frame {
            OutputFrame::Copy(index) => {
                stats.copied += 1;
                OutputItem::Input(input(index))
            }
            OutputFrame::Blend { .. } if behind => {
                stats.repeated += 1;
                OutputItem::Input(input(frame.nearest()))
            }
            OutputFrame::Blend { from, timestep } => {
                stats.blended += 1;
                let (reply, result) = mpsc::sync_channel(1);
                jobs.send(BlendJob {
                    from: input(from),
                    to: input(from + 1),
                    timestep,
                    reply,
                })
                .map_err(|_| "the RIFE pool stopped")?;
                OutputItem::Blended(result)
            }
        };
        output
            .send(item)
            .map_err(|_| "the encoder stopped accepting frames")?;

        if wall_seconds - last_report >= STATS_INTERVAL_SECONDS {
            last_report = wall_seconds;
            eprintln!(
                "[rife_worker] {:.0}s: {} interpolated, {} copied, {} repeated to keep up",
                wall_seconds, stats.blended, stats.copied, stats.repeated
            );
        }
    }
    unreachable!("output frame index overflowed");
}

fn blend_frames(rife: &Rife<'_>, jobs: &Mutex<Receiver<BlendJob>>, width: u32, height: u32) {
    let mut out = PlanarFrame::new(width as usize, height as usize);
    loop {
        let job = jobs.lock().map(|receiver| receiver.recv());
        let Ok(Ok(job)) = job else {
            return;
        };
        let result = rife
            .interpolate(&job.from.planar, &job.to.planar, job.timestep, &mut out)
            .map(|()| {
                let mut rgb = Vec::new();
                out.store_rgb24(&mut rgb);
                rgb
            });
        let _ = job.reply.send(result);
    }
}

fn write_frames(output: Receiver<OutputItem>, mut encoder_stdin: ChildStdin) -> Result<(), String> {
    for item in output {
        let written = match item {
            OutputItem::Input(frame) => encoder_stdin.write_all(&frame.rgb),
            OutputItem::Blended(result) => {
                let rgb = result.recv().map_err(|_| "a RIFE thread stopped")??;
                encoder_stdin.write_all(&rgb)
            }
        };
        written.map_err(|error| format!("writing to the encoder: {error}"))?;
    }
    Ok(())
}

/// Relays the decoder's audio (NUT) to the encoder over loopback TCP,
/// buffering whatever arrives before the encoder connects.
struct AudioRelay {
    decoder_port: u16,
    encoder_port: u16,
}

impl AudioRelay {
    fn start() -> io::Result<Self> {
        let from_decoder = TcpListener::bind("127.0.0.1:0")?;
        let to_encoder = TcpListener::bind("127.0.0.1:0")?;
        let relay = Self {
            decoder_port: from_decoder.local_addr()?.port(),
            encoder_port: to_encoder.local_addr()?.port(),
        };
        let (chunks_tx, chunks_rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = from_decoder.accept()?;
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let read = stream.read(&mut chunk)?;
                if read == 0 || chunks_tx.send(chunk[..read].to_vec()).is_err() {
                    return Ok(());
                }
            }
        });
        thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = to_encoder.accept()?;
            stream.set_nodelay(true)?;
            for chunk in chunks_rx {
                stream.write_all(&chunk)?;
            }
            Ok(())
        });
        Ok(relay)
    }
}

fn spawn(program: &str, args: Vec<String>, stdin: Stdio, stdout: Stdio) -> Result<Child, String> {
    let mut command = Process::new(program);
    command
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|error| format!("failed to start '{program}': {error}"))
}
