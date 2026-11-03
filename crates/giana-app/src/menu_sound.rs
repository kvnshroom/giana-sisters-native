//! Sounds for the menus, made by the sound driver of the original.
//!
//! The menus have no sounds of their own in the original (it has no menus). The effects that the game requests through the
//! queue at `$154C` are played here by a driver of its own, entry `$4987` (the same call as `consume_sfx_sub_1519`):
//!
//! | cue | effect | what it is in the game |
//! | :-- | :-- | :-- |
//! | move | `$02` | the short high blip of a block that is hit |
//! | confirm | `$01` | a short rising sweep |
//! | back | `$0C` | a short falling blip |
//!
//! Outside a game this driver also plays the title tune (track 3, `$49E6`), so that the effects mix with the music like in the
//! game: they take over the third voice for a moment. In the pause menu the game is silent and only the effects play.
//! The sound goes through the volume and mute of the settings like everything else.
//!
//! `MENU SOUNDS` (`MenuSoundMode`) picks what the cues sound like: `Classic` is the above, `Soft` makes short quiet ticks
//! on voice 3 of the same chip model instead (the registers are written here, the game's driver is not asked), `Off` is
//! silence. The title tune is not touched by the setting.

use crate::menu::Nav;
use crate::settings::MenuSoundMode;
use giana_core::sound_driver::Driver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Move,
    Confirm,
    Back,
}

impl Cue {
    /// The number of the effect (`$154C`)
    fn effect(self) -> u8 {
        match self {
            Cue::Move => 0x02,
            Cue::Confirm => 0x01,
            Cue::Back => 0x0C,
        }
    }
}

/// The cue that a menu key makes, if any.
pub fn cue_for(nav: Nav) -> Option<Cue> {
    match nav {
        Nav::Up | Nav::Down | Nav::Left | Nav::Right => Some(Cue::Move),
        Nav::Confirm => Some(Cue::Confirm),
        Nav::Back => Some(Cue::Back),
        Nav::Clear => None,
        Nav::Favorite | Nav::Sort => Some(Cue::Confirm),
    }
}

const TITLE_TRACK: u8 = 3;
/// The title tune repeats exactly every 3648 steps once its first 100 steps are played (like `giana-audio` plays it)
const LOOP_START: u32 = 100;
const LOOP_LEN: u32 = 3648;
/// `Soft`: a repeated move cue within this many steps (80 ms) is dropped, so a held key does not rattle
const SOFT_MOVE_GAP: u32 = 4;
/// `Soft`: steps after the last note in which the release of the envelope is heard
const SOFT_RELEASE_STEPS: u8 = 5;
const SID_PAL_CLOCK: f64 = 985_248.0;

/// One note of a soft cue: pitch, attack (0 to 15, a slow attack keeps a short note quiet: the gate closes before the
/// envelope has risen far) and the number of steps with the gate on (20 ms each)
struct Note {
    hz: f64,
    attack: u8,
    steps: u8,
}

/// The notes of a soft cue, all a thin pulse wave: move is one short tick, confirm rises and back falls.
fn soft_notes(cue: Cue) -> &'static [Note] {
    match cue {
        Cue::Move => &[Note { hz: 1319.0, attack: 5, steps: 1 }],
        Cue::Confirm => &[Note { hz: 784.0, attack: 9, steps: 1 }, Note { hz: 1046.0, attack: 9, steps: 1 }],
        Cue::Back => &[Note { hz: 1046.0, attack: 9, steps: 1 }, Note { hz: 698.0, attack: 9, steps: 1 }],
    }
}

/// Voice 3 registers (`$D40E..$D414`) of a soft cue at `step` (0 is the first step), None when it is over.
fn soft_voice(cue: Cue, step: u8) -> Option<[u8; 7]> {
    let notes = soft_notes(cue);
    // The chip is written in register order, the control register (gate) before the envelope: so the first step only
    // sets up the voice with the gate off (20 ms), and the gate opens in the next one with the right attack.
    if step == 0 {
        return Some(soft_regs(&notes[0], false));
    }
    let step = step - 1;
    let mut at = 0u8;
    for note in notes {
        if step < at + note.steps {
            return Some(soft_regs(note, true));
        }
        at += note.steps;
    }
    let last = notes.last()?;
    (step < at + SOFT_RELEASE_STEPS).then(|| soft_regs(last, false))
}

fn soft_regs(note: &Note, gate: bool) -> [u8; 7] {
    let f = (note.hz * 16_777_216.0 / SID_PAL_CLOCK).round() as u16;
    // pulse width 50 %, no sustain, release about 70 ms: a soft pluck instead of a held tone
    [f as u8, (f >> 8) as u8, 0x00, 0x08, 0x40 | u8::from(gate), note.attack << 4, 0x03]
}

/// While an effect plays its voice is kept quiet: the sustain level is limited to this (0..15)
const QUIET_SUSTAIN: u8 = 6;
/// Steps after a cue in which the effect is made quieter
const EFFECT_STEPS: u8 = 30;

/// Which part of the menu sound this driver makes. `Both` is the original mix; with the MUSIC and EFFECTS sliders (see
/// `Settings::split_sound`) the title tune and the cues come from two drivers, one for each, so the tune plays on under a cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Both,
    Music,
    Effects,
}

pub struct MenuSound {
    part: Part,
    driver: Driver,
    music: bool,
    steps: u32,
    /// the driver as it was at `LOOP_START`
    loop_state: Option<Driver>,
    queued: Vec<Cue>,
    effect_left: u8,
    mode: MenuSoundMode,
    /// `Soft`: the cue that sounds and the step it is at
    soft: Option<(Cue, u8)>,
    /// `Soft`: steps since the last move tick
    since_move: u32,
}

impl MenuSound {
    pub fn new() -> Self {
        Self::with_part(Part::Both)
    }

    pub fn with_part(part: Part) -> Self {
        let mut s = Self { part, driver: Driver::new(), music: false, steps: 0, loop_state: None, queued: Vec::new(), effect_left: 0, mode: MenuSoundMode::Classic, soft: None, since_move: u32::MAX };
        s.silence();
        s
    }

    fn init(&mut self) {
        self.driver = Driver::new();
        self.driver.call(0x4346, 0, 0, 0xF0, false); // `$41C3`: init
        self.steps = 0;
        self.loop_state = None;
        self.effect_left = 0;
        self.soft = None;
    }

    /// What the cues sound like. Cues that wait are played in the new mode.
    pub fn set_mode(&mut self, mode: MenuSoundMode) {
        self.mode = mode;
        self.soft = None;
        self.effect_left = 0;
    }

    /// The title tune plays, from the start.
    pub fn start_title(&mut self) {
        self.init();
        if self.part != Part::Effects {
            self.driver.call(0x49E6, TITLE_TRACK, 0, 0, false);
            self.music = true;
        }
    }

    /// No music: only the effects (the pause menu).
    pub fn silence(&mut self) {
        self.init();
        self.music = false;
    }

    /// Forget the cues that did not get played (the menu was left).
    pub fn clear(&mut self) {
        self.queued.clear();
    }

    /// Ask for an effect; it starts at the next step.
    pub fn cue(&mut self, cue: Cue) {
        // A few at most: a held key repeats faster than the effect lasts
        if self.queued.len() < 2 && self.part != Part::Music && self.mode != MenuSoundMode::Off {
            self.queued.push(cue);
        }
    }

    /// One step of the driver (50 per second): the SID registers `$D400..$D418`.
    pub fn step(&mut self) -> [u8; 25] {
        for cue in std::mem::take(&mut self.queued) {
            match self.mode {
                MenuSoundMode::Off => {}
                MenuSoundMode::Classic => {
                    self.driver.call(0x4987, cue.effect(), 0, 0, false); // `$41C6`: start the effect
                    self.effect_left = EFFECT_STEPS;
                }
                MenuSoundMode::Soft => {
                    // a held direction key ticks at most every 80 ms
                    if cue != Cue::Move || self.since_move >= SOFT_MOVE_GAP {
                        self.soft = Some((cue, 0));
                        if cue == Cue::Move {
                            self.since_move = 0;
                        }
                    }
                }
            }
        }
        self.since_move = self.since_move.saturating_add(1);
        self.driver.call(0x439E, 0, 0, 0, false); // `$41C0`: one tick
        self.steps += 1;
        if self.music {
            if self.steps == LOOP_START {
                self.loop_state = Some(self.driver.clone());
            } else if self.steps == LOOP_START + LOOP_LEN {
                if let Some(state) = &self.loop_state {
                    self.driver = state.clone();
                    self.steps = LOOP_START;
                }
            }
        }
        let mut regs = [0u8; 25];
        regs.copy_from_slice(&self.driver.sid[..25]);
        if !self.music {
            // Without a track the driver still lets voices 1 and 2 play noise: only voice 3 (the effects) may sound
            regs[..14].fill(0);
        }
        if let Some((cue, at)) = self.soft {
            match soft_voice(cue, at) {
                Some(voice) => {
                    regs[14..21].copy_from_slice(&voice);
                    regs[23] &= !4; // voice 3 does not go through the filter
                    self.soft = Some((cue, at + 1));
                }
                None => self.soft = None,
            }
        }
        if self.effect_left > 0 {
            self.effect_left -= 1;
            // voice 3, `$D414`: attack/decay, sustain/release
            let (sustain, release) = (regs[20] >> 4, regs[20] & 15);
            regs[20] = sustain.min(QUIET_SUSTAIN) << 4 | release;
        }
        regs
    }
}

/// The menu sound for one chip (the original mix) or two (music and effects apart, see `Settings::split_sound`).
pub struct MenuSounds {
    /// the only driver; in the split mode the music
    main: MenuSound,
    /// the effects, kept in step with `main` at all times and used in the split mode only
    fx: MenuSound,
    split: bool,
    mode: MenuSoundMode,
}

impl MenuSounds {
    pub fn new() -> Self {
        Self { main: MenuSound::new(), fx: MenuSound::with_part(Part::Effects), split: false, mode: MenuSoundMode::Classic }
    }

    /// `MENU SOUNDS`: what the cues sound like
    pub fn set_mode(&mut self, mode: MenuSoundMode) {
        self.mode = mode;
        self.main.set_mode(mode);
        self.fx.set_mode(mode);
    }

    pub fn is_split(&self) -> bool {
        self.split
    }

    /// Change between one chip and two. `title`: the title tune is wanted (otherwise silence), because a driver that
    /// changes its part starts over.
    pub fn set_split(&mut self, split: bool, title: bool) {
        if split != self.split {
            self.split = split;
            self.main = MenuSound::with_part(if split { Part::Music } else { Part::Both });
            self.main.set_mode(self.mode);
            if title {
                self.start_title();
            } else {
                self.silence();
            }
        }
    }

    pub fn start_title(&mut self) {
        self.main.start_title();
        self.fx.start_title();
    }

    pub fn silence(&mut self) {
        self.main.silence();
        self.fx.silence();
    }

    pub fn clear(&mut self) {
        self.main.clear();
        self.fx.clear();
    }

    pub fn cue(&mut self, cue: Cue) {
        self.main.cue(cue);
        self.fx.cue(cue);
    }

    /// One step: the registers of the chip (the music chip in the split mode) and, in the split mode, those of the effects.
    pub fn step(&mut self) -> ([u8; 25], Option<[u8; 25]>) {
        let fx = self.split.then(|| self.fx.step());
        if !self.split {
            // the effects driver is not used: it need not run
            return (self.main.step(), None);
        }
        (self.main.step(), fx)
    }
}

impl Default for MenuSounds {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MenuSound {
    fn default() -> Self {
        Self::new()
    }
}

/// The samples (44.1 kHz mono, as the audio device plays them) of one cue on a chip of its own, without the title tune:
/// `lead` quiet steps, the cue, and `steps` in all. For tests and for listening (`examples/menu_sound_wav.rs`).
pub fn render_cue(mode: MenuSoundMode, cue: Cue, lead: usize, steps: usize) -> Vec<i16> {
    use giana_audio::player::AudioEngine;
    let mut sound = MenuSound::new();
    sound.set_mode(mode);
    let mut engine = AudioEngine::new_live();
    let mut out = Vec::with_capacity(steps * 882);
    for n in 0..steps {
        if n == lead {
            sound.cue(cue);
        }
        engine.live_regs = Some(sound.step());
        let mut buf = [0i16; 882]; // one step of 50 Hz
        engine.fill_audio_buffer(&mut buf);
        out.extend_from_slice(&buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate of voice 3 (`$D412`, bit 0) over the steps after a cue
    fn gate_steps(cue: Cue) -> usize {
        let mut s = MenuSound::new();
        s.step();
        s.cue(cue);
        (0..60).filter(|_| s.step()[18] & 1 == 1).count()
    }

    #[test]
    fn every_cue_makes_a_short_sound_and_ends() {
        giana_core::skip_without_data!();
        for cue in [Cue::Move, Cue::Confirm, Cue::Back] {
            let n = gate_steps(cue);
            assert!((1..=30).contains(&n), "{cue:?}: voice on for {n} steps");
        }
    }

    #[test]
    fn the_pause_menu_is_silent_without_a_cue_and_has_volume_with_one() {
        giana_core::skip_without_data!();
        let mut s = MenuSound::new();
        for _ in 0..50 {
            let r = s.step();
            assert!([4, 11, 18].iter().all(|&c| r[c] & 1 == 0), "a voice is on without a cue: {r:x?}");
        }
        s.cue(Cue::Move);
        let steps: Vec<[u8; 25]> = (0..10).map(|_| s.step()).collect();
        assert!(steps.iter().all(|r| r[24] & 15 > 0), "master volume is off: nothing would be heard");
        assert!(steps.iter().any(|r| r[18] & 1 == 1), "the effect never starts");
    }

    #[test]
    fn the_title_tune_loops_without_a_jump() {
        giana_core::skip_without_data!();
        let mut s = MenuSound::new();
        s.start_title();
        let mut at_loop_start = None;
        let mut regs_after_loop = None;
        for step in 1..=(LOOP_START + LOOP_LEN) {
            let r = s.step();
            if step == LOOP_START {
                at_loop_start = Some(r);
            }
            if step == LOOP_START + LOOP_LEN {
                regs_after_loop = Some(r);
            }
        }
        assert_eq!(regs_after_loop, at_loop_start, "after the loop the tune is where it was at step {LOOP_START}");
        assert_eq!(s.steps, LOOP_START);
    }

    #[test]
    fn an_effect_over_the_tune_plays_and_the_tune_goes_on() {
        giana_core::skip_without_data!();
        let mut s = MenuSound::new();
        s.start_title();
        for _ in 0..200 {
            s.step();
        }
        s.cue(Cue::Confirm);
        let during: Vec<[u8; 25]> = (0..40).map(|_| s.step()).collect();
        assert!(during.iter().any(|r| r[18] & 1 == 1));
        assert!(during.iter().any(|r| r[4] & 1 == 1 || r[11] & 1 == 1), "the other voices keep playing");
    }

    #[test]
    fn the_two_parts_make_the_tune_and_the_cues_apart() {
        giana_core::skip_without_data!();
        let (mut music, mut effects, mut tune_alone) = (MenuSound::with_part(Part::Music), MenuSound::with_part(Part::Effects), MenuSound::new());
        for s in [&mut music, &mut effects, &mut tune_alone] {
            s.start_title();
        }
        for step in 0..300 {
            if step == 150 {
                music.cue(Cue::Confirm);
                effects.cue(Cue::Confirm);
            }
            let (m, e, plain) = (music.step(), effects.step(), tune_alone.step());
            // the tune chip plays the tune alone, the cue never reaches it
            assert_eq!(m, plain, "step {step}");
            if step < 150 {
                assert!([4, 11, 18].iter().all(|&c| e[c] & 1 == 0), "no tune on the effects chip: {e:x?}");
            }
            if step == 155 {
                assert!(e[18] & 1 == 1, "the cue sounds on the effects chip");
            }
        }
    }

    /// RMS of what a cue adds to the level the chip settles at, up to its last audible sample
    fn rms(mode: MenuSoundMode, cue: Cue) -> f64 {
        let lead = 8;
        let samples = render_cue(mode, cue, lead, 60);
        let quiet = &samples[samples.len() - 5 * 882..]; // where the chip has settled again
        let dc = quiet.iter().map(|&v| v as f64).sum::<f64>() / quiet.len() as f64;
        let after: Vec<f64> = samples[lead * 882..].iter().map(|&v| v as f64 - dc).collect();
        let end = after.iter().rposition(|v| v.abs() > 30.0).map_or(0, |i| i + 1);
        if end == 0 {
            return 0.0;
        }
        (after[..end].iter().map(|v| v * v).sum::<f64>() / end as f64).sqrt()
    }

    #[test]
    fn off_is_silent_and_soft_and_classic_sound() {
        giana_core::skip_without_data!();
        for cue in [Cue::Move, Cue::Confirm, Cue::Back] {
            assert_eq!(rms(MenuSoundMode::Off, cue), 0.0, "{cue:?} OFF");
            assert!(rms(MenuSoundMode::Soft, cue) > 50.0, "{cue:?} SOFT is inaudible");
            assert!(rms(MenuSoundMode::Classic, cue) > 50.0, "{cue:?} CLASSIC is inaudible");
        }
    }

    #[test]
    fn soft_is_at_most_a_third_as_loud_as_classic() {
        giana_core::skip_without_data!();
        for cue in [Cue::Move, Cue::Confirm, Cue::Back] {
            let (soft, classic) = (rms(MenuSoundMode::Soft, cue), rms(MenuSoundMode::Classic, cue));
            println!("{cue:?}: soft rms {soft:.0}, classic rms {classic:.0}");
            assert!(soft * 3.0 <= classic, "{cue:?}: soft {soft:.0} is more than a third of classic {classic:.0}");
        }
    }

    #[test]
    fn soft_is_short() {
        giana_core::skip_without_data!();
        for cue in [Cue::Move, Cue::Confirm, Cue::Back] {
            let mut s = MenuSound::new();
            s.set_mode(MenuSoundMode::Soft);
            s.step();
            s.cue(cue);
            // the gate is open for 20 to 40 ms; with the release the whole cue is over after 160 ms
            let gate: Vec<bool> = (0..12).map(|_| s.step()[18] & 1 == 1).collect();
            let on = gate.iter().filter(|&&g| g).count();
            assert!((1..=2).contains(&on), "{cue:?}: gate on for {on} steps");
            assert!(s.soft.is_none() && s.step()[18] == 0, "{cue:?} is over");
        }
    }

    #[test]
    fn a_held_key_ticks_at_most_every_80_ms() {
        giana_core::skip_without_data!();
        let mut s = MenuSound::new();
        s.set_mode(MenuSoundMode::Soft);
        let mut onsets = 0;
        let mut was_on = false;
        for _ in 0..100 {
            s.cue(Cue::Move); // the extreme: a cue at every step
            let on = s.step()[18] & 1 == 1;
            onsets += usize::from(on && !was_on);
            was_on = on;
        }
        assert!((20..=100 / 4 + 1).contains(&onsets), "{onsets} ticks in 2 s");
        // confirm is not rate limited
        s.cue(Cue::Move);
        s.step();
        s.cue(Cue::Confirm);
        assert!((0..4).any(|_| s.step()[18] & 1 == 1));
    }

    #[test]
    fn soft_keeps_the_title_tune() {
        giana_core::skip_without_data!();
        let (mut soft, mut tune) = (MenuSound::new(), MenuSound::new());
        soft.set_mode(MenuSoundMode::Soft);
        soft.start_title();
        tune.start_title();
        for step in 0..200 {
            if step == 100 {
                soft.cue(Cue::Confirm);
            }
            let (a, b) = (soft.step(), tune.step());
            assert_eq!(a[..14], b[..14], "voices 1 and 2 play on (step {step})");
        }
    }

    #[test]
    fn cues_for_keys() {
        assert_eq!(cue_for(Nav::Down), Some(Cue::Move));
        assert_eq!(cue_for(Nav::Confirm), Some(Cue::Confirm));
        assert_eq!(cue_for(Nav::Back), Some(Cue::Back));
        assert_eq!(cue_for(Nav::Clear), None);
    }
}
