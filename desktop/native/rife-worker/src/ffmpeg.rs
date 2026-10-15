//! FFmpeg command lines for the worker's two helper processes.
//!
//! The decoder turns the incoming NUT stream into rgb24 frames on an exact
//! source-rate grid that starts at 0, and sends the audio, also starting at 0,
//! to a loopback TCP port. Padding the later-starting stream to 0 (repeating
//! the first frame, or prepending silence) keeps the input's A/V offset. The
//! encoder lays the interpolated frames on the target-rate grid from 0, keeps
//! the audio's timestamps with `-copyts`, and writes NUT for the next stage.
//! With both streams starting at exactly 0, later stages cannot skew them by
//! rebasing each stream separately.

use crate::timeline::FrameRate;

const QUIET: [&str; 4] = ["-hide_banner", "-loglevel", "error", "-nostats"];

pub fn decoder_args(source_rate: FrameRate, audio_port: Option<u16>) -> Vec<String> {
    let mut args: Vec<String> = QUIET.iter().map(|arg| arg.to_string()).collect();
    args.extend(
        [
            "-f",
            "nut",
            "-i",
            "pipe:0",
            "-map",
            "0:v:0",
            "-vf",
            &format!("fps=fps={}:start_time=0", source_rate.ffmpeg_arg()),
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ]
        .map(String::from),
    );
    if let Some(port) = audio_port {
        args.extend(
            [
                "-map",
                "0:a:0",
                "-af",
                "aresample=async=0:first_pts=0",
                "-c:a",
                "pcm_s16le",
                "-f",
                "nut",
                &format!("tcp://127.0.0.1:{port}"),
            ]
            .map(String::from),
        );
    }
    args
}

pub fn encoder_args(
    width: u32,
    height: u32,
    target_rate: FrameRate,
    pixel_format: &str,
    audio_port: Option<u16>,
) -> Vec<String> {
    let mut args: Vec<String> = QUIET.iter().map(|arg| arg.to_string()).collect();
    args.extend(
        [
            "-copyts",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-video_size",
            &format!("{width}x{height}"),
            "-framerate",
            &target_rate.ffmpeg_arg(),
            "-i",
            "pipe:0",
        ]
        .map(String::from),
    );
    if let Some(port) = audio_port {
        args.extend(
            [
                // The audio's codec parameters are in the NUT header; read no
                // further before starting so it never waits on video.
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
                "-thread_queue_size",
                "1024",
                "-f",
                "nut",
                "-i",
                &format!("tcp://127.0.0.1:{port}"),
            ]
            .map(String::from),
        );
    }
    args.extend(["-map", "0:v:0"].map(String::from));
    if audio_port.is_some() {
        args.extend(["-map", "1:a:0", "-c:a", "copy"].map(String::from));
    }
    args.extend(
        [
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            pixel_format,
            "-fps_mode",
            "passthrough",
            "-f",
            "nut",
            "pipe:1",
        ]
        .map(String::from),
    );
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(text: &str) -> FrameRate {
        FrameRate::parse(text).unwrap()
    }

    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len())
            .any(|window| window == expected)
    }

    #[test]
    fn decoder_normalizes_to_the_source_grid_and_splits_out_audio() {
        let args = decoder_args(rate("29.97"), Some(5000));
        assert!(has_subsequence(
            &args,
            &["-vf", "fps=fps=30000/1001:start_time=0"]
        ));
        assert!(has_subsequence(&args, &["-pix_fmt", "rgb24", "pipe:1"]));
        assert!(has_subsequence(
            &args,
            &[
                "-map",
                "0:a:0",
                "-af",
                "aresample=async=0:first_pts=0",
                "-c:a",
                "pcm_s16le",
                "-f",
                "nut",
                "tcp://127.0.0.1:5000"
            ]
        ));

        let silent = decoder_args(rate("24"), None);
        assert!(!silent.iter().any(|arg| arg.contains("0:a")));
    }

    #[test]
    fn encoder_muxes_target_rate_video_with_original_audio_timing() {
        let args = encoder_args(1280, 720, rate("60"), "nv12", Some(5001));
        assert!(args.contains(&"-copyts".to_string()));
        assert!(has_subsequence(
            &args,
            &["-video_size", "1280x720", "-framerate", "60"]
        ));
        assert!(has_subsequence(&args, &["-i", "tcp://127.0.0.1:5001"]));
        assert!(has_subsequence(
            &args,
            &["-map", "0:v:0", "-map", "1:a:0", "-c:a", "copy"]
        ));
        assert!(has_subsequence(&args, &["-pix_fmt", "nv12"]));
        assert_eq!(args.last().map(String::as_str), Some("pipe:1"));

        let silent = encoder_args(848, 480, rate("48"), "yuv420p", None);
        assert!(!silent
            .iter()
            .any(|arg| arg.starts_with("tcp://") || arg == "1:a:0"));
    }
}
