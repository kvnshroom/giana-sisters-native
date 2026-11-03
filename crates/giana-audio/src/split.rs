//! Music and effects on chips of their own.
//!
//! The original has one chip and one sound driver: a sound effect takes over the third voice of the music for a moment. To
//! give music and effects their own volume, the app asks the ported sound driver of the game to record its calls
//! (`Driver::record`) and plays them on two drivers of its own: `music` gets everything except the effect requests, `effects`
//! everything except the music requests. Both run in step (same ticks), each feeds its own chip, and the two outputs are
//! mixed with the two volumes (`Mixer`). Because the music driver never hears of an effect, the music keeps playing under the
//! effects: that is how this mode sounds, in contrast to the original mix.
//!
//! The game itself is not touched: its own driver keeps running as it always did, and the oracle tests compare that one.

use giana_core::sound_driver::{Driver, DriverCall};

/// `$41C3`: init, `$41C6`/`$4987`: start an effect, `$49E6`: start a music track, `$439E`: one tick
const ENTRY_INIT: u16 = 0x4346;
const ENTRY_EFFECT: u16 = 0x4987;
const ENTRY_TRACK: u16 = 0x49E6;

/// Which of the two drivers the call is for: (music, effects).
fn goes_to(call: &DriverCall) -> (bool, bool) {
    match call {
        DriverCall::Call { entry, .. } | DriverCall::Limited { entry, .. } => (*entry != ENTRY_EFFECT, *entry != ENTRY_TRACK),
        // a paused call is finished on the drivers that started it (a resume does nothing on one that has none)
        DriverCall::Resume => (true, true),
    }
}

pub struct SplitDrivers {
    music: Driver,
    effects: Driver,
}

impl SplitDrivers {
    /// Starts from the driver of the game as it is now: the music driver is a copy of it (so the tune plays on from where
    /// it is), the effects driver is new and silent.
    pub fn from_game(game: &Driver) -> Self {
        let mut music = game.clone();
        music.record = None;
        Self { music, effects: Self::silent_effects() }
    }

    /// Nothing plays yet: both drivers are just initialised (the menu starts with this one before its own calls).
    pub fn new() -> Self {
        let mut music = Driver::new();
        music.call(ENTRY_INIT, 0, 0, 0xF0, false);
        Self { music, effects: Self::silent_effects() }
    }

    fn silent_effects() -> Driver {
        let mut d = Driver::new();
        d.call(ENTRY_INIT, 0, 0, 0xF0, false);
        d
    }

    /// Plays the calls that the game's driver made since the last time on the two drivers.
    pub fn feed(&mut self, calls: &[DriverCall]) {
        for &call in calls {
            let (music, effects) = goes_to(&call);
            if music {
                self.music.replay(call);
            }
            if effects {
                self.effects.replay(call);
            }
        }
    }

    /// The SID registers of the music and of the effects (`$D400..$D418`). A driver that has no track still lets voices 1
    /// and 2 sound after its init (the original starts a track right away); the effects only use voice 3, so the rest is
    /// silenced.
    pub fn regs(&self) -> ([u8; 25], [u8; 25]) {
        let mut music = [0u8; 25];
        music.copy_from_slice(&self.music.sid[..25]);
        let mut effects = [0u8; 25];
        effects.copy_from_slice(&self.effects.sid[..25]);
        effects[..14].fill(0);
        (music, effects)
    }
}

impl Default for SplitDrivers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: DriverCall = DriverCall::Call { entry: 0x439E, a: 0, x: 0, y: 0, carry: false };

    fn calls(entry: u16, a: u8) -> DriverCall {
        DriverCall::Call { entry, a, x: 0, y: 0, carry: false }
    }

    /// Voice gates of a register set (`$D404`, `$D40B`, `$D412`)
    fn gates(r: &[u8; 25]) -> [bool; 3] {
        [r[4] & 1 == 1, r[11] & 1 == 1, r[18] & 1 == 1]
    }

    #[test]
    fn effects_stay_out_of_the_music_and_the_music_out_of_the_effects() {
        giana_core::skip_without_data!();
        let mut s = SplitDrivers::new();
        s.feed(&[calls(0x49E6, 1)]);
        let (mut music_gates, mut fx_gates) = ([false; 3], [false; 3]);
        for step in 0..200 {
            if step == 50 {
                s.feed(&[calls(0x4987, 0x03)]);
            }
            s.feed(&[TICK]);
            let (m, e) = s.regs();
            if step < 50 {
                assert_eq!(gates(&e), [false; 3], "no effect yet: the effects chip is silent (step {step})");
            }
            for v in 0..3 {
                music_gates[v] |= gates(&m)[v];
                fx_gates[v] |= gates(&e)[v];
            }
        }
        assert!(music_gates.iter().any(|&g| g), "the music plays");
        assert_eq!(fx_gates, [false, false, true], "the effect plays on voice 3 only");
    }

    #[test]
    fn the_music_on_voice_3_is_not_interrupted_by_an_effect() {
        giana_core::skip_without_data!();
        // the game's own driver: the effect takes over voice 3, so its registers differ from the plain music driver's
        let mut game = Driver::new();
        game.call(ENTRY_INIT, 0, 0, 0xF0, false);
        game.call(ENTRY_TRACK, 1, 0, 0, false);
        let mut plain = game.clone();
        let mut s = SplitDrivers::from_game(&game);
        game.record = Some(Vec::new());
        let mut differs = false;
        for step in 0..300 {
            if step == 100 {
                game.call(ENTRY_EFFECT, 0x03, 0, 0, false);
            }
            game.call(0x439E, 0, 0, 0, false);
            plain.call(0x439E, 0, 0, 0, false);
            let log = game.take_calls();
            s.feed(&log);
            let (music, _) = s.regs();
            assert_eq!(music[..], plain.sid[..25], "the music chip gets what the music driver alone would write (step {step})");
            differs |= game.sid[14..21] != plain.sid[14..21];
        }
        assert!(differs, "the game's own driver does let the effect take over voice 3");
    }

    #[test]
    fn a_new_game_starts_both_drivers_over() {
        giana_core::skip_without_data!();
        let mut s = SplitDrivers::new();
        s.feed(&[calls(0x49E6, 1), TICK, TICK, calls(0x4987, 3), TICK]);
        s.feed(&[DriverCall::Call { entry: ENTRY_INIT, a: 0, x: 0, y: 0xF0, carry: false }, TICK, TICK, TICK]);
        let (m, e) = s.regs();
        assert_eq!(gates(&e), [false; 3]);
        assert_eq!(gates(&m)[2], false);
    }
}
