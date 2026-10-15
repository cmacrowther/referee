//! The `libplacebo` filter shared by the Universal FFmpeg executor and the
//! FFmpeg preprocessor.

use crate::graph::{
    parse_resolution, ColorPrimariesIntent, HdrPlan, MatrixCoefficientsIntent, OutputBitDepth,
    ResizePlan, TransferCharacteristicIntent,
};

/// Resize and color work rendered by one `libplacebo` filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibplaceboParams {
    pub output_resolution: Option<String>,
    pub resize_quality: Option<u8>,
    pub output_bit_depth: OutputBitDepth,
    pub color_primaries: ColorPrimariesIntent,
    pub transfer: TransferCharacteristicIntent,
    pub matrix: MatrixCoefficientsIntent,
    pub tone_map_to_hdr10: bool,
}

impl Default for LibplaceboParams {
    /// No resize, 8-bit output, and source color tags preserved.
    fn default() -> Self {
        Self {
            output_resolution: None,
            resize_quality: None,
            output_bit_depth: OutputBitDepth::Bit8,
            color_primaries: ColorPrimariesIntent::PreserveSource,
            transfer: TransferCharacteristicIntent::PreserveSource,
            matrix: MatrixCoefficientsIntent::PreserveSource,
            tone_map_to_hdr10: false,
        }
    }
}

impl LibplaceboParams {
    pub fn apply_resize(&mut self, resize: &ResizePlan) {
        self.output_resolution = Some(resize.target_resolution.clone());
        self.resize_quality = resize.quality;
    }

    /// Takes the output bit depth and color tags from an HDR plan. Callers
    /// decide `tone_map_to_hdr10` themselves.
    pub fn apply_hdr_color(&mut self, hdr: &HdrPlan) {
        self.output_bit_depth = hdr.output_bit_depth;
        self.color_primaries = hdr.color_primaries;
        self.transfer = hdr.transfer;
        self.matrix = hdr.matrix;
    }

    /// Clears color work that an earlier stage already rendered, keeping the
    /// output bit depth.
    pub fn preserve_source_color(&mut self) {
        self.color_primaries = ColorPrimariesIntent::PreserveSource;
        self.transfer = TransferCharacteristicIntent::PreserveSource;
        self.matrix = MatrixCoefficientsIntent::PreserveSource;
        self.tone_map_to_hdr10 = false;
    }

    /// Whether a resize, tonemap, or explicit color conversion is requested.
    /// Bit-depth-only conversions are left to the caller.
    pub fn has_resize_or_color_work(&self) -> bool {
        self.output_resolution.is_some()
            || self.tone_map_to_hdr10
            || !matches!(self.color_primaries, ColorPrimariesIntent::PreserveSource)
            || !matches!(self.transfer, TransferCharacteristicIntent::PreserveSource)
            || !matches!(self.matrix, MatrixCoefficientsIntent::PreserveSource)
    }

    /// Renders the `libplacebo=...` filter.
    ///
    /// `shader_upscale` hands spatial reconstruction to a custom upscale
    /// shader, so libplacebo's own scaler is set to `none`. `custom_shader` is
    /// the shader file name, resolved against the process working directory.
    pub fn render(&self, shader_upscale: bool, custom_shader: Option<&str>) -> String {
        let mut args = Vec::new();

        if let Some((width, height)) = self.output_resolution.as_deref().and_then(parse_resolution)
        {
            let upscaler = if shader_upscale {
                "none"
            } else {
                map_quality_to_libplacebo_scaler(self.resize_quality)
            };
            push_aspect_preserving_resize_args(&mut args, width, height, upscaler);
        }

        if let Some(shader) = custom_shader {
            args.push(format!("custom_shader_path={}", shader));
        }

        if self.tone_map_to_hdr10 {
            args.push("tonemapping=bt.2390".to_string());
        }
        if let Some(primaries) = map_color_primaries(self.color_primaries) {
            args.push(format!("color_primaries={}", primaries));
        }
        if let Some(trc) = map_transfer(self.transfer) {
            args.push(format!("color_trc={}", trc));
        }
        if let Some(colorspace) = map_matrix(self.matrix) {
            args.push(format!("colorspace={}", colorspace));
        }
        args.push(format!(
            "format={}",
            self.output_bit_depth.planar_yuv_format()
        ));

        format!("libplacebo={}", args.join(":"))
    }
}

/// Appends libplacebo resize arguments that fit content inside the requested output box
/// without distorting the source aspect ratio.
///
/// The target width/height remain the requested output canvas, while
/// `force_original_aspect_ratio=decrease` and `normalize_sar=true` keep the source
/// picture geometry intact and pad as needed instead of stretching.
fn push_aspect_preserving_resize_args(
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

/// Selects the libplacebo scaler for a quality hint: `bilinear` for 1,
/// `spline36` for 2 (and when unset), `ewa_lanczos` otherwise.
fn map_quality_to_libplacebo_scaler(quality: Option<u8>) -> &'static str {
    match quality.unwrap_or(2) {
        1 => "bilinear",
        2 => "spline36",
        _ => "ewa_lanczos",
    }
}

fn map_color_primaries(intent: ColorPrimariesIntent) -> Option<&'static str> {
    match intent {
        ColorPrimariesIntent::PreserveSource => None,
        ColorPrimariesIntent::Bt2020 => Some("bt2020"),
    }
}

fn map_transfer(intent: TransferCharacteristicIntent) -> Option<&'static str> {
    match intent {
        TransferCharacteristicIntent::PreserveSource => None,
        TransferCharacteristicIntent::Smpte2084 => Some("smpte2084"),
    }
}

fn map_matrix(intent: MatrixCoefficientsIntent) -> Option<&'static str> {
    match intent {
        MatrixCoefficientsIntent::PreserveSource => None,
        MatrixCoefficientsIntent::Bt2020Nc => Some("bt2020nc"),
    }
}

#[cfg(test)]
mod tests {
    use super::{push_aspect_preserving_resize_args, LibplaceboParams};
    use crate::graph::{
        ColorPrimariesIntent, MatrixCoefficientsIntent, OutputBitDepth,
        TransferCharacteristicIntent,
    };

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

    #[test]
    fn render_orders_resize_shader_tonemap_color_and_format() {
        let params = LibplaceboParams {
            output_resolution: Some("1920x1080".to_string()),
            resize_quality: Some(3),
            output_bit_depth: OutputBitDepth::Bit10,
            color_primaries: ColorPrimariesIntent::Bt2020,
            transfer: TransferCharacteristicIntent::Smpte2084,
            matrix: MatrixCoefficientsIntent::Bt2020Nc,
            tone_map_to_hdr10: true,
        };

        assert_eq!(
            params.render(false, None),
            "libplacebo=w=1920:h=1080:force_original_aspect_ratio=decrease:normalize_sar=true:\
             pad_crop_ratio=0.0:upscaler=ewa_lanczos:tonemapping=bt.2390:color_primaries=bt2020:\
             color_trc=smpte2084:colorspace=bt2020nc:format=yuv420p10le"
        );
        assert!(params
            .render(true, Some("shader.glsl"))
            .contains("upscaler=none:custom_shader_path=shader.glsl:tonemapping"));
    }

    #[test]
    fn bit_depth_alone_is_not_resize_or_color_work() {
        let params = LibplaceboParams {
            output_bit_depth: OutputBitDepth::Bit10,
            ..LibplaceboParams::default()
        };
        assert!(!params.has_resize_or_color_work());
    }
}
