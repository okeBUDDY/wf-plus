//! RIFF WAVE decoding (CSndFileWAV): 8-bit unsigned or 16-bit signed PCM,
//! mono or stereo, into a [`SoundBuffer`](super::SoundBuffer).

use super::SoundBuffer;
use std::sync::Arc;

pub fn decode(bytes: &[u8], group: usize) -> Result<SoundBuffer, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF WAVE file".into());
    }
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = (body + size).min(bytes.len());
        match id {
            b"fmt " if size >= 16 => format = Some((u16_at(body), u16_at(body + 2), u32_at(body + 4), u16_at(body + 14))),
            b"data" => {
                let (tag, channels, rate, bits) = format.ok_or("data before fmt")?;
                if tag != 1 {
                    return Err(format!("unsupported WAVE format {tag}"));
                }
                let channels = channels.max(1) as usize;
                let data = &bytes[body..end];
                let mut samples = Vec::new();
                let sample = |i: usize| -> f32 {
                    match bits {
                        8 => (data[i] as f32 - 128.0) / 128.0,
                        _ => i16::from_le_bytes([data[2 * i], data[2 * i + 1]]) as f32 / 32768.0,
                    }
                };
                let width = if bits == 8 { 1 } else { 2 };
                if bits != 8 && bits != 16 {
                    return Err(format!("unsupported {bits}-bit WAVE"));
                }
                let frames = data.len() / width / channels;
                samples.reserve(frames * 2);
                for f in 0..frames {
                    let l = sample(f * channels);
                    let r = if channels > 1 { sample(f * channels + 1) } else { l };
                    samples.push(l);
                    samples.push(r);
                }
                return Ok(SoundBuffer { samples: Arc::new(samples), rate, group });
            }
            _ => {}
        }
        at = body + size + (size & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_16_bit_mono() {
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&[1, 0, 1, 0]);
        w.extend_from_slice(&22050u32.to_le_bytes());
        w.extend_from_slice(&44100u32.to_le_bytes());
        w.extend_from_slice(&[2, 0, 16, 0]);
        w.extend_from_slice(b"data");
        w.extend_from_slice(&4u32.to_le_bytes());
        w.extend_from_slice(&16384i16.to_le_bytes());
        w.extend_from_slice(&(-32768i16).to_le_bytes());
        let s = super::decode(&w, 1).unwrap();
        assert_eq!(s.rate, 22050);
        assert_eq!(*s.samples, vec![0.5, 0.5, -1.0, -1.0]);
    }
}
