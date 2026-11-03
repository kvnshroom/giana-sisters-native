//! What the audio device plays: one chip (the original sound) or two chips (music and effects with their own volume).

use crate::player::AudioEngine;

/// `gain` and the part gains: 256 is full volume, 0 is silence.
pub struct Mixer {
    /// the chip of the original sound; in the split mode the chip of the music
    pub main: AudioEngine,
    /// the chip of the effects, used in the split mode only
    pub effects: AudioEngine,
    /// music and effects on a chip of their own, each fed its own registers (`live_regs` of both engines)
    pub split: bool,
    pub music_gain: i32,
    pub effects_gain: i32,
    /// the master volume of the settings (and the mute): applies to everything
    pub gain: i32,
    scratch: Vec<i16>,
}

impl Mixer {
    pub fn new() -> Self {
        let mut effects = AudioEngine::new_live();
        effects.live_regs = Some([0; 25]);
        Self { main: AudioEngine::new(), effects, split: false, music_gain: 256, effects_gain: 256, gain: 256, scratch: Vec::new() }
    }

    /// Fill a buffer with 44100 Hz mono 16-bit samples.
    ///
    /// Without the split this is the one chip and the master volume, exactly as before the split existed.
    pub fn fill(&mut self, out: &mut [i16]) {
        self.main.fill_audio_buffer(out);
        if self.split {
            self.scratch.resize(out.len(), 0);
            self.effects.fill_audio_buffer(&mut self.scratch);
            for (m, &e) in out.iter_mut().zip(&self.scratch) {
                let mixed = (*m as i32 * self.music_gain >> 8) + (e as i32 * self.effects_gain >> 8);
                *m = mixed.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        }
        if self.gain != 256 {
            for sample in out.iter_mut() {
                *sample = (*sample as i32 * self.gain >> 8) as i16;
            }
        }
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split::SplitDrivers;
    use giana_core::sound_driver::{Driver, DriverCall};

    /// How far the samples swing (a constant level, which the chip makes with its volume on, is not sound)
    fn peak(s: &[i16]) -> i32 {
        s.iter().map(|&v| v as i32).max().unwrap_or(0) - s.iter().map(|&v| v as i32).min().unwrap_or(0)
    }

    /// A game's driver that plays a track and requests effects on and off; its calls as a list per step.
    fn session(steps: usize) -> (Vec<[u8; 25]>, Vec<Vec<DriverCall>>) {
        let mut game = Driver::new();
        game.call(0x4346, 0, 0, 0xF0, false);
        game.record = Some(Vec::new());
        let mut regs = Vec::new();
        let mut calls = Vec::new();
        for step in 0..steps {
            if step == 0 {
                game.call(0x49E6, 1, 0, 0, false);
            }
            if step % 120 == 60 {
                game.call(0x4987, 0x03, 0, 0, false);
            }
            game.call(0x439E, 0, 0, 0, false);
            let mut r = [0u8; 25];
            r.copy_from_slice(&game.sid[..25]);
            regs.push(r);
            calls.push(game.take_calls());
        }
        (regs, calls)
    }

    #[test]
    fn without_the_split_the_sound_is_the_one_chip_with_the_master_volume() {
        giana_core::skip_without_data!();
        let (regs, _) = session(150);
        for gain in [256, 100] {
            let mut mixer = Mixer::new();
            mixer.gain = gain;
            let mut plain = AudioEngine::new();
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for r in &regs {
                mixer.main.live_regs = Some(*r);
                plain.live_regs = Some(*r);
                // one step of 50 Hz is 882 samples at 44.1 kHz
                let mut x = vec![0i16; 882];
                let mut y = vec![0i16; 882];
                mixer.fill(&mut x);
                plain.fill_audio_buffer(&mut y);
                if gain != 256 {
                    y.iter_mut().for_each(|s| *s = (*s as i32 * gain >> 8) as i16);
                }
                a.extend(x);
                b.extend(y);
            }
            assert!(a == b, "gain {gain}: the output is not identical to the single chip");
            assert!(peak(&a) > 100, "the original mix is not silent");
        }
    }

    #[test]
    fn music_and_effects_alone_both_sound_and_the_volumes_apply() {
        giana_core::skip_without_data!();
        let (_, calls) = session(240);
        let mut render = |music_gain: i32, effects_gain: i32| {
            let mut mixer = Mixer::new();
            mixer.split = true;
            mixer.music_gain = music_gain;
            mixer.effects_gain = effects_gain;
            let mut split = SplitDrivers::new();
            let mut out = Vec::new();
            for c in &calls {
                split.feed(c);
                let (m, e) = split.regs();
                mixer.main.live_regs = Some(m);
                mixer.effects.live_regs = Some(e);
                let mut buf = vec![0i16; 882];
                mixer.fill(&mut buf);
                out.extend(buf);
            }
            out
        };
        let both = render(256, 256);
        let music_only = render(256, 0);
        let effects_only = render(0, 256);
        let nothing = render(0, 0);
        let half = render(128, 128);
        assert!(peak(&music_only) > 100, "the music stream is not silent");
        assert!(peak(&effects_only) > 100, "the effects stream is not silent");
        assert_eq!(peak(&nothing), 0);
        assert!(peak(&half) < peak(&both));
        // the first effect request is at step 60: before it the effects chip is silent (after its start-up click)
        assert_eq!(peak(&effects_only[20 * 882..60 * 882]), 0);
        // the sum of both parts is the mix (up to the rounding of the two shifts)
        let worst = both.iter().zip(music_only.iter().zip(&effects_only)).map(|(&b, (&m, &e))| (b as i32 - m as i32 - e as i32).abs()).max().unwrap();
        assert!(worst <= 1, "both = music + effects, off by {worst}");
    }
}
