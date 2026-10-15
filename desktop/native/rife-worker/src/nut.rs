//! Reads the stream headers at the start of a NUT stream (the container the
//! pipeline's stages exchange) to learn the video size and whether an audio
//! stream follows, before any decoder is started.
//!
//! Layout per the NUT specification (and FFmpeg's nutenc.c): every header
//! packet is a 64-bit startcode, `forward_ptr` (v), and a 32-bit checksum when
//! `forward_ptr > 4096`. A stream header then carries `stream_id` (v),
//! `stream_class` (v: 0 video, 1 audio), `fourcc` (vb), four v fields,
//! `stream_flags` (v), `codec_specific_data` (vb), and for video `width` (v)
//! and `height` (v). All stream headers precede the first syncpoint.

const STREAM_STARTCODE: u64 = 0x4E53_1140_5BF2_F9DB;
const SYNCPOINT_STARTCODE: u64 = 0x4E4B_E4AD_EECA_4569;
const LARGE_PACKET_THRESHOLD: u64 = 4096;

const STREAM_CLASS_VIDEO: u64 = 0;
const STREAM_CLASS_AUDIO: u64 = 1;

/// What the NUT stream headers declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NutStreams {
    /// Width and height of the first video stream.
    pub video: Option<(u32, u32)>,
    pub has_audio: bool,
}

/// Parses the stream headers from the first bytes of a NUT stream. Returns
/// `None` until `header` reaches the first syncpoint (all headers are known),
/// so callers can keep reading and retry.
pub fn sniff_streams(header: &[u8]) -> Option<NutStreams> {
    let syncpoint = find_startcode(header, SYNCPOINT_STARTCODE, 0)?;
    let mut streams = NutStreams {
        video: None,
        has_audio: false,
    };

    let mut search_from = 0;
    while let Some(start) = find_startcode(&header[..syncpoint], STREAM_STARTCODE, search_from) {
        search_from = start + 8;
        let mut reader = Reader {
            bytes: &header[start + 8..syncpoint],
            position: 0,
        };
        let Some(stream) = reader.stream_header() else {
            continue;
        };
        match stream {
            StreamHeader::Video { width, height } if streams.video.is_none() => {
                streams.video = Some((width, height));
            }
            StreamHeader::Audio => streams.has_audio = true,
            _ => {}
        }
    }

    Some(streams)
}

fn find_startcode(bytes: &[u8], startcode: u64, from: usize) -> Option<usize> {
    let pattern = startcode.to_be_bytes();
    bytes
        .get(from..)?
        .windows(pattern.len())
        .position(|window| window == pattern)
        .map(|offset| offset + from)
}

enum StreamHeader {
    Video { width: u32, height: u32 },
    Audio,
    Other,
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.position)?;
        self.position += 1;
        Some(byte)
    }

    /// A NUT `v`: big-endian base-128 with a continuation bit.
    fn v(&mut self) -> Option<u64> {
        let mut value = 0u64;
        loop {
            let byte = self.byte()?;
            value = value.checked_mul(128)? | u64::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
    }

    fn skip(&mut self, count: u64) -> Option<()> {
        let end = self.position.checked_add(usize::try_from(count).ok()?)?;
        (end <= self.bytes.len()).then(|| self.position = end)
    }

    /// The stream header body following its startcode.
    fn stream_header(&mut self) -> Option<StreamHeader> {
        let forward_ptr = self.v()?;
        if forward_ptr > LARGE_PACKET_THRESHOLD {
            self.skip(4)?; // header_checksum
        }
        let _stream_id = self.v()?;
        let stream_class = self.v()?;
        let fourcc_len = self.v()?;
        self.skip(fourcc_len)?;
        for _ in 0..4 {
            // time_base_id, msb_pts_shift, max_pts_distance, decode_delay
            self.v()?;
        }
        let _stream_flags = self.v()?;
        let codec_data_len = self.v()?;
        self.skip(codec_data_len)?;

        Some(match stream_class {
            STREAM_CLASS_VIDEO => StreamHeader::Video {
                width: u32::try_from(self.v()?).ok()?,
                height: u32::try_from(self.v()?).ok()?,
            },
            STREAM_CLASS_AUDIO => StreamHeader::Audio,
            _ => StreamHeader::Other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_v(out: &mut Vec<u8>, value: u64) {
        let mut groups = vec![(value & 0x7F) as u8];
        let mut rest = value >> 7;
        while rest > 0 {
            groups.push((rest & 0x7F) as u8 | 0x80);
            rest >>= 7;
        }
        out.extend(groups.iter().rev());
    }

    fn stream_header(stream_id: u64, class: u64, tail: &[u64]) -> Vec<u8> {
        let mut body = Vec::new();
        put_v(&mut body, stream_id);
        put_v(&mut body, class);
        put_v(&mut body, 4);
        body.extend(b"RGB3");
        for value in [0, 0, 32768, 0, 0] {
            put_v(&mut body, value);
        }
        put_v(&mut body, 0); // no codec-specific data
        for value in tail {
            put_v(&mut body, *value);
        }
        body.extend([0, 0, 0, 0]); // checksum

        let mut packet = STREAM_STARTCODE.to_be_bytes().to_vec();
        put_v(&mut packet, body.len() as u64);
        packet.extend(body);
        packet
    }

    fn nut(streams: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"nut/multimedia container\0".to_vec();
        bytes.extend(0x4E4D_7A56_1F5F_04ADu64.to_be_bytes()); // main header
        bytes.extend([3, 3, 2, 0, 0]);
        for stream in streams {
            bytes.extend(stream);
        }
        bytes.extend(SYNCPOINT_STARTCODE.to_be_bytes());
        bytes
    }

    #[test]
    fn reads_video_size_and_audio_presence() {
        let header = nut(&[
            stream_header(0, STREAM_CLASS_VIDEO, &[1920, 1080, 1, 1, 0]),
            stream_header(1, STREAM_CLASS_AUDIO, &[48000, 1, 2]),
        ]);
        assert_eq!(
            sniff_streams(&header),
            Some(NutStreams {
                video: Some((1920, 1080)),
                has_audio: true
            })
        );
    }

    #[test]
    fn reports_no_audio_for_video_only_streams() {
        let header = nut(&[stream_header(0, STREAM_CLASS_VIDEO, &[848, 480, 1, 1, 0])]);
        assert_eq!(
            sniff_streams(&header),
            Some(NutStreams {
                video: Some((848, 480)),
                has_audio: false
            })
        );
    }

    #[test]
    fn waits_for_the_first_syncpoint() {
        let header = nut(&[stream_header(0, STREAM_CLASS_VIDEO, &[848, 480, 1, 1, 0])]);
        assert_eq!(sniff_streams(&header[..header.len() - 4]), None);
    }

    #[test]
    fn decodes_multi_byte_values() {
        let mut bytes = Vec::new();
        put_v(&mut bytes, 3840);
        put_v(&mut bytes, 48000);
        let mut reader = Reader {
            bytes: &bytes,
            position: 0,
        };
        assert_eq!(reader.v(), Some(3840));
        assert_eq!(reader.v(), Some(48000));
        assert_eq!(reader.v(), None);
    }
}
