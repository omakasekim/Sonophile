//! Minimal WAV writer (32-bit IEEE float, 24-bit and 16-bit PCM) for exporting
//! synthesized RIRs to DAWs / game engines.

/// Sample format for WAV export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavFormat {
    Float32,
    Pcm24,
    Pcm16,
}

impl WavFormat {
    pub fn from_str(s: &str) -> WavFormat {
        match s {
            "pcm24" => WavFormat::Pcm24,
            "pcm16" => WavFormat::Pcm16,
            _ => WavFormat::Float32,
        }
    }
    fn bits(self) -> u16 {
        match self {
            WavFormat::Float32 => 32,
            WavFormat::Pcm24 => 24,
            WavFormat::Pcm16 => 16,
        }
    }
    fn format_code(self) -> u16 {
        match self {
            WavFormat::Float32 => 3, // IEEE float
            _ => 1,                  // PCM
        }
    }
}

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn push_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Write an interleaved multi-channel WAV. `samples.len()` must be a multiple
/// of `channels`.
pub fn write_wav(
    interleaved: &[f32],
    sample_rate: u32,
    channels: u16,
    format: WavFormat,
) -> Vec<u8> {
    let ch = channels.max(1) as usize;
    let frames = interleaved.len() / ch;
    let bytes_per_sample = (format.bits() / 8) as usize;
    let block_align = (ch * bytes_per_sample) as u16;
    let data_bytes = frames * ch * bytes_per_sample;

    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    push_u32(&mut out, 36 + data_bytes as u32);
    out.extend_from_slice(b"WAVE");
    // fmt chunk
    out.extend_from_slice(b"fmt ");
    push_u32(&mut out, 16);
    push_u16(&mut out, format.format_code());
    push_u16(&mut out, channels);
    push_u32(&mut out, sample_rate);
    push_u32(&mut out, sample_rate * block_align as u32);
    push_u16(&mut out, block_align);
    push_u16(&mut out, format.bits());
    // data chunk
    out.extend_from_slice(b"data");
    push_u32(&mut out, data_bytes as u32);

    match format {
        WavFormat::Float32 => {
            for &s in interleaved {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        WavFormat::Pcm16 => {
            for &s in interleaved {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        WavFormat::Pcm24 => {
            for &s in interleaved {
                let v = (s.clamp(-1.0, 1.0) * 8388607.0).round() as i32;
                // 24-bit little-endian, sign-extended from 24-bit value.
                let bytes = v.to_le_bytes();
                out.extend_from_slice(&bytes[0..3]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_valid() {
        let data = vec![0.0_f32, 0.0, 0.0, 0.0]; // 2 frames mono
        let w = write_wav(&data, 48000, 2, WavFormat::Float32);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(&w[12..16], b"fmt ");
        assert_eq!(&w[36..40], b"data");
        // channels = 2 at offset 22.
        assert_eq!(u16::from_le_bytes([w[22], w[23]]), 2);
    }

    #[test]
    fn pcm24_roundtrip_size() {
        let data = vec![1.0_f32, -1.0, 0.5, 0.25]; // 2 frames stereo
        let w = write_wav(&data, 44100, 2, WavFormat::Pcm24);
        // 44-byte header + 4 samples × 3 bytes.
        assert_eq!(w.len(), 44 + 12);
    }
}
