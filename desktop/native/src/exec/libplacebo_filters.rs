//! Shared helpers for constructing libplacebo filter arguments.
//!
//! These functions are used by both the universal FFmpeg executor and the
//! FFmpeg preprocessor to build libplacebo filter strings. They were previously
//! duplicated across `exec/universal.rs` and `preprocess/ffmpeg_preprocessor.rs`.

use crate::graph::{ColorPrimariesIntent, MatrixCoefficientsIntent, TransferCharacteristicIntent};

/// Parses a resolution string of the form `"WxH"` into a `(width, height)` tuple.
///
/// Returns `None` if the string is not in the expected format or either component
/// cannot be parsed as a `u32`.
///
/// # Examples
///
pub(crate) fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

/// Appends libplacebo resize arguments that fit content inside the requested output box
/// without distorting the source aspect ratio.
///
/// The target width/height remain the requested output canvas, while
/// `force_original_aspect_ratio=decrease` and `normalize_sar=true` keep the source
/// picture geometry intact and pad as needed instead of stretching.
pub(crate) fn push_aspect_preserving_resize_args(
    args: &mut Vec<String>,
    width: u32,
    height: u32,
    upscaler: &str,
) {
    args.push(format!("w={}", width));
    args.push(format!("h={}", height));
    args.push("force_original_aspect_ratio=decrease".to_string());
    args.push("normalize_sar=true".to_string());
    args.push("pad_crop_ratio=0.0".to_string());
    args.push(format!("upscaler={}", upscaler));
}

/// Selects the libplacebo scaler name corresponding to an optional quality hint.
///
/// # Returns
/// The scaler name: `bilinear` for `1`, `spline36` for `2` (and when `quality` is `None`),
/// and `ewa_lanczos` for any other value.
///
/// # Examples
///
pub(crate) fn map_quality_to_libplacebo_scaler(quality: Option<u8>) -> &'static str {
    match quality.unwrap_or(2) {
        1 => "bilinear",
        2 => "spline36",
        _ => "ewa_lanczos",
    }
}

/// Maps a color primaries intent to the corresponding libplacebo/FFmpeg name.
///
/// Returns `Some("bt2020")` when the intent requests BT.2020 primaries, or `None` when the
/// source primaries should be preserved.
///
/// # Examples
///
pub(crate) fn map_color_primaries(intent: ColorPrimariesIntent) -> Option<&'static str> {
    match intent {
        ColorPrimariesIntent::PreserveSource => None,
        ColorPrimariesIntent::Bt2020 => Some("bt2020"),
    }
}

/// Maps a `TransferCharacteristicIntent` to the corresponding FFmpeg/libplacebo transfer identifier.
///
/// # Returns
/// `Some("smpte2084")` when the intent requests SMPTE ST 2084 (PQ) transfer, `None` when the
/// source transfer should be preserved.
///
/// # Examples
///
pub(crate) fn map_transfer(intent: TransferCharacteristicIntent) -> Option<&'static str> {
    match intent {
        TransferCharacteristicIntent::PreserveSource => None,
        TransferCharacteristicIntent::Smpte2084 => Some("smpte2084"),
    }
}

/// Maps a `MatrixCoefficientsIntent` to the FFmpeg/libplacebo matrix identifier.
///
/// Returns the FFmpeg matrix name when the intent specifies an explicit matrix, or `None` to
/// indicate the source matrix should be preserved.
///
/// # Examples
///
pub(crate) fn map_matrix(intent: MatrixCoefficientsIntent) -> Option<&'static str> {
    match intent {
        MatrixCoefficientsIntent::PreserveSource => None,
        MatrixCoefficientsIntent::Bt2020Nc => Some("bt2020nc"),
    }
}

#[cfg(test)]
mod tests {
    use super::push_aspect_preserving_resize_args;

    #[test]
    fn aspect_preserving_resize_args_keep_target_box_but_disable_stretching() {
        let mut args = Vec::new();
        push_aspect_preserving_resize_args(&mut args, 3840, 2160, "ewa_lanczos");

        assert_eq!(
            args,
            vec![
                "w=3840",
                "h=2160",
                "force_original_aspect_ratio=decrease",
                "normalize_sar=true",
                "pad_crop_ratio=0.0",
                "upscaler=ewa_lanczos",
            ]
        );
    }
}
