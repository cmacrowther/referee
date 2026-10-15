//! Frame generation settings: the target (30 fps, 60 fps, or the viewer's
//! display refresh rate), the engine, and their translation into the
//! planner's interpolation request.

use serde::{Deserialize, Serialize};

use crate::graph::{parse_resolution, InterpolationRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FramegenTarget {
    #[serde(rename = "30fps")]
    Fps30,
    #[default]
    #[serde(rename = "60fps")]
    Fps60,
    #[serde(rename = "matchDisplay")]
    MatchDisplay,
}

/// Which interpolator generates frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FramegenEngine {
    /// The best interpolator that reaches any target frame rate: NVEncC
    /// optical flow (FRUC) where available, otherwise RIFE, otherwise FFmpeg
    /// `minterpolate` on the staged AMD path.
    #[default]
    OpticalFlow,
    /// RIFE in its own preprocess stage (`rife-worker`), on any executor.
    /// Falls back to optical flow where the worker or a model is missing.
    Rife,
    /// The GPU vendor's native frame generation: NVIDIA Framegen (Maxine) or
    /// AMD Fluid Motion (AMF FRC). Doubles the source frame rate and ignores
    /// the target.
    #[serde(alias = "nvidiaFramegen")]
    NativeDouble,
}

/// Refresh rate Match Display assumes when neither the client nor the local
/// display reports one.
const DEFAULT_DISPLAY_REFRESH_HZ: u32 = 60;
const MIN_DISPLAY_REFRESH_HZ: f64 = 20.0;
const MAX_DISPLAY_REFRESH_HZ: f64 = 1000.0;
/// Integer rates whose NTSC variant (x 1000/1001) Windows reports truncated,
/// for example 59.94 Hz as 59.
const NTSC_DISPLAY_RATES: [u32; 7] = [24, 30, 48, 60, 120, 144, 240];

impl FramegenTarget {
    /// Output frame rate for this target. `display_refresh_hz` is the viewer's
    /// display refresh rate when known; Match Display assumes 60 Hz without it.
    pub fn resolve_fps(self, display_refresh_hz: Option<f64>, output_resolution: &str) -> u32 {
        match self {
            Self::Fps30 => 30,
            Self::Fps60 => 60,
            Self::MatchDisplay => {
                let refresh_hz = display_refresh_hz
                    .and_then(normalize_display_refresh_hz)
                    .unwrap_or(DEFAULT_DISPLAY_REFRESH_HZ);
                display_matched_fps(refresh_hz, output_resolution)
            }
        }
    }
}

/// Frame generation settings that shape a stream's interpolation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramegenSettings {
    pub enabled: bool,
    pub target: FramegenTarget,
    pub engine: FramegenEngine,
}

impl FramegenSettings {
    /// The interpolation request for a stream at `output_resolution` viewed on
    /// a display refreshing at `display_refresh_hz`. `rife_available` says
    /// whether RIFE can run for this stream (worker, model, and a known source
    /// frame rate).
    pub fn interpolation_request(
        self,
        display_refresh_hz: Option<f64>,
        output_resolution: &str,
        rife_available: bool,
    ) -> InterpolationRequest {
        let target = || {
            self.target
                .resolve_fps(display_refresh_hz, output_resolution)
        };
        match (self.enabled, self.engine) {
            (false, _) => InterpolationRequest::Off,
            (true, FramegenEngine::NativeDouble) => InterpolationRequest::NativeDouble,
            (true, FramegenEngine::Rife) if rife_available => {
                InterpolationRequest::RifeTargetFps(target())
            }
            (true, FramegenEngine::OpticalFlow | FramegenEngine::Rife) => {
                InterpolationRequest::TargetFps(target())
            }
        }
    }
}

/// Rounds a reported refresh rate to whole hertz, restoring rates Windows
/// truncates (59 -> 60). `None` for implausible values.
pub fn normalize_display_refresh_hz(hz: f64) -> Option<u32> {
    if !hz.is_finite() || !(MIN_DISPLAY_REFRESH_HZ..=MAX_DISPLAY_REFRESH_HZ).contains(&hz) {
        return None;
    }
    let rounded = hz.round() as u32;
    if NTSC_DISPLAY_RATES.contains(&(rounded + 1)) {
        Some(rounded + 1)
    } else {
        Some(rounded)
    }
}

/// Highest Match Display output rate. At 4K this keeps HEVC within level 5.2
/// (at most 128 fps at 4K), which viewer decoders commonly support. Below 4K
/// it bounds NVENC and FRUC load.
fn max_display_matched_fps(output_resolution: &str) -> u32 {
    match parse_resolution(output_resolution) {
        Some((_, height)) if height >= 2160 => 120,
        _ => 144,
    }
}

/// The refresh rate, divided by the smallest whole number that brings it
/// within the cap, so every frame stays on screen for the same number of
/// refreshes (240 Hz -> 120 fps, 144 Hz at 4K -> 72 fps).
fn display_matched_fps(refresh_hz: u32, output_resolution: &str) -> u32 {
    let divisor = refresh_hz.div_ceil(max_display_matched_fps(output_resolution));
    (refresh_hz as f64 / divisor as f64).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_targets_ignore_the_display() {
        assert_eq!(
            FramegenTarget::Fps30.resolve_fps(Some(144.0), "1920x1080"),
            30
        );
        assert_eq!(
            FramegenTarget::Fps60.resolve_fps(Some(144.0), "1920x1080"),
            60
        );
    }

    #[test]
    fn match_display_uses_the_refresh_rate_within_the_cap() {
        for (refresh_hz, resolution, expected) in [
            (Some(60.0), "1920x1080", 60),
            (Some(59.0), "1920x1080", 60),
            (Some(59.94), "1920x1080", 60),
            (Some(75.0), "1920x1080", 75),
            (Some(120.0), "3840x2160", 120),
            (Some(143.0), "2560x1440", 144),
            (Some(144.0), "2560x1440", 144),
            (Some(144.0), "3840x2160", 72),
            (Some(165.0), "1920x1080", 83),
            (Some(240.0), "1920x1080", 120),
            (Some(360.0), "1920x1080", 120),
            (None, "1920x1080", 60),
            (Some(f64::NAN), "1920x1080", 60),
            (Some(5000.0), "1920x1080", 60),
        ] {
            assert_eq!(
                FramegenTarget::MatchDisplay.resolve_fps(refresh_hz, resolution),
                expected,
                "{refresh_hz:?} Hz at {resolution}"
            );
        }
    }

    fn framegen(enabled: bool, target: FramegenTarget, engine: FramegenEngine) -> FramegenSettings {
        FramegenSettings {
            enabled,
            target,
            engine,
        }
    }

    #[test]
    fn interpolation_request_follows_the_engine() {
        let request =
            |settings: FramegenSettings| settings.interpolation_request(None, "1920x1080", false);
        assert_eq!(
            request(framegen(
                false,
                FramegenTarget::Fps60,
                FramegenEngine::OpticalFlow
            )),
            InterpolationRequest::Off
        );
        assert_eq!(
            request(framegen(
                true,
                FramegenTarget::Fps30,
                FramegenEngine::OpticalFlow
            )),
            InterpolationRequest::TargetFps(30)
        );
        assert_eq!(
            request(framegen(true, FramegenTarget::Fps60, FramegenEngine::Rife)),
            InterpolationRequest::TargetFps(60)
        );
        // With a worker, a model, and a known source rate, RIFE is requested.
        assert_eq!(
            framegen(true, FramegenTarget::Fps30, FramegenEngine::Rife).interpolation_request(
                None,
                "1920x1080",
                true
            ),
            InterpolationRequest::RifeTargetFps(30)
        );
        assert_eq!(
            request(framegen(
                true,
                FramegenTarget::Fps30,
                FramegenEngine::NativeDouble
            )),
            InterpolationRequest::NativeDouble
        );
    }

    #[test]
    fn serializes_with_api_names() {
        assert_eq!(
            serde_json::to_value(FramegenTarget::MatchDisplay).unwrap(),
            serde_json::json!("matchDisplay")
        );
        assert_eq!(
            serde_json::from_value::<FramegenTarget>(serde_json::json!("30fps")).unwrap(),
            FramegenTarget::Fps30
        );
        assert_eq!(
            serde_json::to_value(FramegenEngine::NativeDouble).unwrap(),
            serde_json::json!("nativeDouble")
        );
    }
}
