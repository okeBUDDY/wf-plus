//! Ogg Vorbis decoding into a [`SoundBuffer`](super::SoundBuffer).

use super::SoundBuffer;
use std::sync::Arc;

pub fn decode(bytes: &[u8], group: usize) -> Result<SoundBuffer, String> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let channels = reader.ident_hdr.audio_channels as usize;
    let rate = reader.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
        for frame in packet.chunks(channels.max(1)) {
            let l = frame[0] as f32 / 32768.0;
            let r = if channels > 1 { frame[1] as f32 / 32768.0 } else { l };
            samples.push(l);
            samples.push(r);
        }
    }
    Ok(SoundBuffer { samples: Arc::new(samples), rate, group })
}
