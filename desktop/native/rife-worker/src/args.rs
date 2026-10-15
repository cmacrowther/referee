//! Command-line parsing.

use crate::timeline::FrameRate;

/// Concurrent RIFE inferences. Two hide most of each call's CPU and transfer
/// time; more rarely help.
const DEFAULT_THREADS: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Initialize the GPU, load the model, run one inference, and exit.
    Probe {
        model_path: String,
        gpu: Option<i32>,
    },
    Run(RunArgs),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunArgs {
    pub ffmpeg_path: String,
    pub model_path: String,
    /// Vulkan device index; `None` lets ncnn pick (a discrete GPU first).
    pub gpu: Option<i32>,
    pub source_rate: FrameRate,
    pub target_rate: FrameRate,
    pub threads: usize,
    /// Output pixel format: `yuv420p`, or `nv12` for NVEncC.
    pub pixel_format: String,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let mut probe = false;
    let mut ffmpeg_path = None;
    let mut model_path = None;
    let mut gpu = None;
    let mut source_rate = None;
    let mut target_rate = None;
    let mut threads = DEFAULT_THREADS;
    let mut pixel_format = "yuv420p".to_string();

    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} requires a value"));
        match arg.as_str() {
            "--probe" => probe = true,
            "--ffmpeg-path" => ffmpeg_path = Some(value("--ffmpeg-path")?),
            "--model-path" => model_path = Some(value("--model-path")?),
            "--gpu-id" => {
                let id = value("--gpu-id")?;
                gpu = match id.as_str() {
                    "auto" => None,
                    id => Some(
                        id.parse()
                            .map_err(|_| format!("--gpu-id: invalid value {id}"))?,
                    ),
                };
            }
            "--source-fps" => source_rate = Some(rate("--source-fps", &value("--source-fps")?)?),
            "--target-fps" => target_rate = Some(rate("--target-fps", &value("--target-fps")?)?),
            "--pix-fmt" => {
                pixel_format = value("--pix-fmt")?;
                if !matches!(pixel_format.as_str(), "yuv420p" | "nv12") {
                    return Err(format!("--pix-fmt: unsupported format {pixel_format}"));
                }
            }
            "--threads" => {
                threads = value("--threads")?
                    .parse()
                    .ok()
                    .filter(|count| *count > 0)
                    .ok_or("--threads must be a positive integer")?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let model_path = model_path.ok_or("missing --model-path")?;
    if probe {
        return Ok(Command::Probe { model_path, gpu });
    }
    Ok(Command::Run(RunArgs {
        ffmpeg_path: ffmpeg_path.ok_or("missing --ffmpeg-path")?,
        model_path,
        gpu,
        source_rate: source_rate.ok_or("missing --source-fps")?,
        target_rate: target_rate.ok_or("missing --target-fps")?,
        threads,
        pixel_format,
    }))
}

fn rate(name: &str, value: &str) -> Result<FrameRate, String> {
    FrameRate::parse(value).ok_or(format!("{name}: invalid frame rate {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn parses_a_run() {
        let command = parse(args(&[
            "--ffmpeg-path",
            "ffmpeg",
            "--gpu-id",
            "0",
            "--model-path",
            "m",
            "--source-fps",
            "23.976",
            "--target-fps",
            "60",
        ]))
        .unwrap();
        assert_eq!(
            command,
            Command::Run(RunArgs {
                ffmpeg_path: "ffmpeg".into(),
                model_path: "m".into(),
                gpu: Some(0),
                source_rate: FrameRate::parse("24000/1001").unwrap(),
                target_rate: FrameRate::parse("60").unwrap(),
                threads: DEFAULT_THREADS,
                pixel_format: "yuv420p".into(),
            })
        );
    }

    #[test]
    fn parses_a_probe_with_automatic_gpu() {
        assert_eq!(
            parse(args(&["--probe", "--model-path", "m", "--gpu-id", "auto"])).unwrap(),
            Command::Probe {
                model_path: "m".into(),
                gpu: None
            }
        );
    }

    #[test]
    fn rejects_missing_and_invalid_values() {
        assert!(parse(args(&["--model-path", "m"])).is_err());
        assert!(parse(args(&["--probe"])).is_err());
        assert!(parse(args(&["--probe", "--model-path", "m", "--threads", "0"])).is_err());
        assert!(parse(args(&["--source-fps"])).is_err());
        assert!(parse(args(&["--bogus"])).is_err());
        assert!(parse(args(&[
            "--probe",
            "--model-path",
            "m",
            "--pix-fmt",
            "rgb24"
        ]))
        .is_err());
    }
}
