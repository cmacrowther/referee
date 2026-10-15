//! Encoder rate-control profiles and quality bounds.

use serde::{Deserialize, Serialize};

pub(crate) const MAX_VSR_QUALITY: u8 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodingProfile {
    /// Target bitrate in kb/s.
    pub bitrate: u32,
    /// Peak bitrate in kb/s.
    pub max_bitrate: u32,
    /// NVENC-style preset (`p1`–`p7`). VCEEncC translates it; AMF, VAAPI, and
    /// CPU encodes ignore it.
    pub preset: String,
    pub lookahead: u32,
    pub bframes: u32,
    /// HLS segment duration in seconds. GOPs are aligned to it.
    pub hls_time: u32,
}

/// The default profile for an output resolution. A user override in
/// `settings.encoding_profiles` replaces it wholesale.
pub fn get_encoding_profile(resolution: &str) -> EncodingProfile {
    let (bitrate, lookahead) = match resolution {
        "2560x1440" => (35_000, 12),
        "3840x2160" => (50_000, 8),
        _ => (25_000, 8),
    };
    EncodingProfile {
        bitrate,
        max_bitrate: bitrate * 3 / 2,
        preset: "p4".to_string(),
        lookahead,
        bframes: 3,
        hls_time: 1,
    }
}

pub fn clamp_quality(_resolution: &str, quality: u8) -> u8 {
    quality.clamp(1, MAX_VSR_QUALITY)
}

#[cfg(test)]
mod tests {
    use super::get_encoding_profile;

    #[test]
    fn default_profiles_scale_bitrate_with_resolution() {
        let hd = get_encoding_profile("1920x1080");
        let qhd = get_encoding_profile("2560x1440");
        let uhd = get_encoding_profile("3840x2160");

        assert_eq!(
            (hd.bitrate, hd.max_bitrate, hd.lookahead),
            (25_000, 37_500, 8)
        );
        assert_eq!(
            (qhd.bitrate, qhd.max_bitrate, qhd.lookahead),
            (35_000, 52_500, 12)
        );
        assert_eq!(
            (uhd.bitrate, uhd.max_bitrate, uhd.lookahead),
            (50_000, 75_000, 8)
        );
        assert_eq!(get_encoding_profile("unknown").bitrate, 25_000);
        assert!([hd, qhd, uhd].iter().all(|profile| profile.preset == "p4"
            && profile.bframes == 3
            && profile.hls_time == 1));
    }
}
