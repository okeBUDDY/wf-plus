//! The game's sound system (SoundSystem / Channel / CSndBuf), on a software
//! mixer. Game code plays decoded sounds on numbered channels with volume
//! groups; the platform layer only pulls mixed stereo samples
//! ([`Mixer::mix`]) from its audio callback.

pub mod ogg;
pub mod seq;
pub mod wav;

use std::sync::{Arc, Mutex};

/// The mixer's output rate.
pub const OUTPUT_RATE: u32 = 44100;
pub const GROUPS: usize = 4;

/// A decoded sound (CSndBuf): interleaved stereo f32 at `rate`, and its
/// volume group (0 effects, 1 music).
pub struct SoundBuffer {
    pub samples: Arc<Vec<f32>>,
    pub rate: u32,
    pub group: usize,
}

impl SoundBuffer {
    pub fn seconds(&self) -> f32 {
        (self.samples.len() / 2) as f32 / self.rate as f32
    }
}

/// The music: a sound, or a .seq of pieces (g_bgm, a CSndStrm).
#[derive(Clone)]
pub enum Music {
    Sound(Arc<SoundBuffer>),
    Seq(Arc<seq::SeqData>),
}

impl Music {
    /// g_bgm.Load: bgm.seq, else bgm.ogg, else bgm.wav, in `dir`.
    pub fn load(dir: &std::path::Path) -> Option<Music> {
        let seq = dir.join("bgm.seq");
        if seq.exists() {
            match seq::SeqData::load(&seq) {
                Ok(d) => return Some(Music::Seq(Arc::new(d))),
                Err(e) => crate::log::warn(&format!("{}: {e}", seq.display())),
            }
        }
        let ogg = dir.join("bgm.ogg");
        if let Ok(b) = std::fs::read(&ogg) {
            match ogg::decode(&b, 1) {
                Ok(s) => return Some(Music::Sound(Arc::new(s))),
                Err(e) => crate::log::warn(&format!("{}: {e}", ogg.display())),
            }
        }
        let wav = dir.join("bgm.wav");
        if let Ok(b) = std::fs::read(&wav) {
            match wav::decode(&b, 1) {
                Ok(s) => return Some(Music::Sound(Arc::new(s))),
                Err(e) => crate::log::warn(&format!("{}: {e}", wav.display())),
            }
        }
        None
    }
}

/// DirectSound volume curve the game was tuned for: level 1 is 0 dB and
/// level 0+ is -50 dB (Channel::SetVolume 0x408f10).
fn gain(level: f32) -> f32 {
    if level <= 0.0 {
        0.0
    } else {
        let hundredths_db = -5000.0 + level.min(1.0) * 5000.0;
        10f32.powf(hundredths_db / 2000.0)
    }
}

/// Pan from -1 to 1 attenuates the other side by up to 50 dB
/// (Channel::SetPan 0x40f4c0).
fn pan_gains(pan: f32) -> (f32, f32) {
    let att = 10f32.powf(-(pan.abs().min(1.0) * 5000.0) / 2000.0);
    if pan < 0.0 { (1.0, att) } else { (att, 1.0) }
}

#[derive(Default, Clone)]
struct Voice {
    samples: Option<Arc<Vec<f32>>>,
    rate: u32,
    group: usize,
    /// Position in source frames (fractional, for resampling).
    pos: f64,
    looping: bool,
    volume: f32,
    pan: f32,
    fade: Option<Fade>,
    /// A .seq: where the next piece comes from.
    seq: Option<Box<seq::SeqPlayer>>,
}

#[derive(Clone, Copy)]
struct Fade {
    from: f32,
    delta: f32,
    time: f32,
    elapsed: f32,
    stop: bool,
}

struct MixerState {
    voices: Vec<Voice>,
    group_volume: [f32; GROUPS],
    muted: u32,
    paused: bool,
}

/// The mixer shared by the game and the audio callback.
#[derive(Clone)]
pub struct Mixer(Arc<Mutex<MixerState>>);

impl Mixer {
    /// Mixes interleaved stereo samples at [`OUTPUT_RATE`] into `out`
    /// (overwriting it).
    #[allow(clippy::mut_range_bound)]
    pub fn mix(&self, out: &mut [f32]) {
        out.fill(0.0);
        let mut s = self.0.lock().unwrap();
        if s.paused {
            return;
        }
        let dt = 1.0 / OUTPUT_RATE as f32;
        let frames = out.len() / 2;
        let MixerState { voices, group_volume, muted, .. } = &mut *s;
        for v in voices.iter_mut() {
            if *muted & (1 << v.group) != 0 {
                continue;
            }
            let step = v.rate as f64 / OUTPUT_RATE as f64;
            let (pl, pr) = pan_gains(v.pan);
            let mut start = 0;
            'piece: while let Some(samples) = v.samples.clone() {
                let src_frames = samples.len() / 2;
                for f in start..frames {
                    if let Some(fade) = &mut v.fade {
                        fade.elapsed += dt;
                        if fade.elapsed >= fade.time {
                            v.volume = fade.from + fade.delta;
                            let stop = fade.stop;
                            v.fade = None;
                            if stop {
                                v.samples = None;
                                break 'piece;
                            }
                        } else {
                            v.volume = fade.from + fade.delta * fade.elapsed / fade.time;
                        }
                    }
                    let mut i = v.pos as usize;
                    if i + 1 >= src_frames {
                        if let Some(player) = &mut v.seq {
                            // CSndFileSEQ::Read: on to the next piece; at
                            // the end, a looping channel starts the music
                            // again (Seek(0)).
                            let mut next = player.next_piece();
                            if next.is_none() && v.looping {
                                player.rewind();
                                next = player.next_piece();
                            }
                            match next {
                                Some(n) if n.len() >= 4 => {
                                    v.pos = (v.pos - src_frames as f64).max(0.0);
                                    v.samples = Some(n);
                                    // The next piece starts at this frame: the
                                    // outer loop builds a new range from it.
                                    start = f;
                                    continue 'piece;
                                }
                                _ => {
                                    v.samples = None;
                                    break 'piece;
                                }
                            }
                        }
                        if v.looping && src_frames > 0 {
                            v.pos -= src_frames as f64;
                            i = v.pos.max(0.0) as usize;
                        } else {
                            v.samples = None;
                            break 'piece;
                        }
                    }
                    let frac = (v.pos - i as f64) as f32;
                    let j = (i + 1).min(src_frames - 1);
                    let l = samples[2 * i] + (samples[2 * j] - samples[2 * i]) * frac;
                    let r = samples[2 * i + 1] + (samples[2 * j + 1] - samples[2 * i + 1]) * frac;
                    let g = gain(v.volume * group_volume[v.group]);
                    out[2 * f] += l * g * pl;
                    out[2 * f + 1] += r * g * pr;
                    v.pos += step;
                }
                break;
            }
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

/// The game-facing sound system.
pub struct SoundSystem {
    mixer: Mixer,
    seq_flags: seq::SeqFlags,
}

impl SoundSystem {
    pub fn new(channels: usize) -> Self {
        SoundSystem {
            mixer: Mixer(Arc::new(Mutex::new(MixerState {
                voices: vec![Voice::default(); channels],
                group_volume: [1.0; GROUPS],
                muted: 0,
                paused: false,
            }))),
            seq_flags: Arc::new(Default::default()),
        }
    }

    /// The handle the platform audio callback mixes from.
    pub fn mixer(&self) -> Mixer {
        self.mixer.clone()
    }

    fn with<R>(&self, f: impl FnOnce(&mut MixerState) -> R) -> R {
        f(&mut self.mixer.0.lock().unwrap())
    }

    /// Plays `sound` on `channel` (Channel::Assign), replacing what it played.
    pub fn play(&self, channel: usize, sound: &SoundBuffer, looping: bool, volume: f32) {
        self.with(|s| {
            if let Some(v) = s.voices.get_mut(channel) {
                *v = Voice {
                    samples: Some(sound.samples.clone()),
                    rate: sound.rate,
                    group: sound.group.min(GROUPS - 1),
                    pos: 0.0,
                    looping,
                    volume,
                    pan: 0.0,
                    fade: None,
                    seq: None,
                };
            }
        });
    }

    /// Plays the music on `channel` (Channel::Assign of g_bgm), from its
    /// start.
    pub fn play_music(&self, channel: usize, music: &Music, looping: bool, volume: f32) {
        match music {
            Music::Sound(s) => self.play(channel, s, looping, volume),
            Music::Seq(d) => {
                let mut player = seq::SeqPlayer::new(d.clone(), self.seq_flags.clone());
                let Some(first) = player.next_piece() else { return };
                self.with(|s| {
                    if let Some(v) = s.voices.get_mut(channel) {
                        *v = Voice {
                            samples: Some(first),
                            rate: d.rate,
                            group: 1,
                            pos: 0.0,
                            looping,
                            volume,
                            pan: 0.0,
                            fade: None,
                            seq: Some(Box::new(player)),
                        };
                    }
                });
            }
        }
    }

    /// Publishes g_seq_flags to the music.
    pub fn set_seq_flags(&self, flags: &[u8; 8]) {
        for (f, &v) in self.seq_flags.iter().zip(flags) {
            f.store(v, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn is_playing(&self, channel: usize) -> bool {
        self.with(|s| s.voices.get(channel).is_some_and(|v| v.samples.is_some()))
    }

    pub fn stop_channel(&self, channel: usize) {
        self.with(|s| {
            if let Some(v) = s.voices.get_mut(channel) {
                v.samples = None;
            }
        });
    }

    /// Stops every channel of `group` (`None`: all) (SoundSystem::Stop).
    pub fn stop_group(&self, group: Option<usize>) {
        self.with(|s| {
            for v in &mut s.voices {
                if group.is_none_or(|g| v.group == g) {
                    v.samples = None;
                }
            }
        });
    }

    pub fn set_volume(&self, channel: usize, volume: f32) {
        self.with(|s| {
            if let Some(v) = s.voices.get_mut(channel) {
                v.volume = volume;
            }
        });
    }

    pub fn set_pan(&self, channel: usize, pan: f32) {
        self.with(|s| {
            if let Some(v) = s.voices.get_mut(channel) {
                v.pan = pan;
            }
        });
    }

    /// Channel::Fade: from `from` to `to` (negative: the current volume) in
    /// `seconds`, then stops the channel when `stop`.
    pub fn fade(&self, channel: usize, from: f32, to: f32, seconds: f32, stop: bool) {
        self.with(|s| {
            if let Some(v) = s.voices.get_mut(channel) {
                let from = if from < 0.0 { v.volume } else { from };
                let to = if to < 0.0 { v.volume } else { to };
                v.volume = from;
                v.fade = Some(Fade { from, delta: to - from, time: seconds.max(1e-3), elapsed: 0.0, stop });
            }
        });
    }

    /// SoundSystem::SetGroupVolume (`None`: every group).
    pub fn set_group_volume(&self, volume: f32, group: Option<usize>) {
        self.with(|s| match group {
            Some(g) if g < GROUPS => s.group_volume[g] = volume,
            _ => s.group_volume = [volume; GROUPS],
        });
    }

    /// SoundSystem::MuteGroup (`None`: every group).
    pub fn mute_group(&self, mute: bool, group: Option<usize>) {
        self.with(|s| {
            let bits = group.map_or(u32::MAX, |g| 1 << g);
            if mute { s.muted |= bits } else { s.muted &= !bits }
        });
    }

    /// Pauses and resumes all output (the application's pause).
    pub fn set_paused(&self, paused: bool) {
        self.with(|s| s.paused = paused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_and_ends() {
        let system = SoundSystem::new(4);
        let sound = SoundBuffer { samples: Arc::new(vec![0.5; 2 * 100]), rate: OUTPUT_RATE, group: 0 };
        system.play(1, &sound, false, 1.0);
        let mut out = vec![0.0; 2 * 64];
        system.mixer().mix(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-4, "full volume is 0 dB");
        assert!(system.is_playing(1));
        system.mixer().mix(&mut out);
        assert!(!system.is_playing(1), "a one-shot sound stops at its end");
        system.set_group_volume(0.5, Some(0));
        system.play(1, &sound, true, 1.0);
        system.mixer().mix(&mut out);
        assert!((out[0] - 0.5 * 10f32.powf(-1.25)).abs() < 1e-4, "half level is -25 dB");
    }
}
