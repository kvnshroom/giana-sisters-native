//! Follows the sound driver of the game that is heard for the split sound (MUSIC and EFFECTS on two chips).

use giana_audio::split::SplitDrivers;
use giana_core::state::GameState;

/// The SID registers of the music chip and of the effects chip for the game that is heard (`which`: 0 yours, 1 the other
/// player's). The sound driver of that game records its calls (`Driver::record`), and they are played on two drivers of our
/// own, one without the effect requests and one without the music requests. Both are started again from a copy of the
/// game's driver when the game that is heard changes or is a new one (its driver does not record yet).
pub fn follow_split(split_game: &mut Option<(SplitDrivers, u8)>, game: &mut GameState, other: Option<&mut GameState>, which: u8) -> ([u8; 25], [u8; 25]) {
    let (shown, hidden) = match (which, other) {
        (1, Some(o)) => (o, Some(game)),
        (_, o) => (game, o),
    };
    if let Some(h) = hidden {
        h.sound.record = None;
    }
    let fresh = shown.sound.record.is_none() || split_game.as_ref().is_none_or(|(_, w)| *w != which);
    if fresh {
        // a game that was suspended while the split sound played brings both drivers along, and the calls that its driver
        // got since then (a restored game records from the start)
        let pending = shown.sound.take_calls();
        shown.sound.record = Some(Vec::new());
        let drivers = match SplitDrivers::take_from(shown) {
            Some(mut d) => {
                d.feed(&pending);
                d
            }
            None => SplitDrivers::from_game(&shown.sound),
        };
        *split_game = Some((drivers, which));
    } else if let Some((drivers, _)) = split_game.as_mut() {
        drivers.feed(&shown.sound.take_calls());
    }
    split_game.as_ref().map_or(([0; 25], [0; 25]), |(d, _)| d.regs())
}

/// The games stop recording the calls of their sound driver (nothing listens, and the list would only grow).
pub fn stop_recording(split_game: &mut Option<(SplitDrivers, u8)>, game: &mut GameState, other: Option<&mut GameState>) {
    // a restored game keeps recording (while it is paused, for instance) until its split drivers have been taken
    if game.split_sound.is_some() {
        return;
    }
    if split_game.take().is_some() || game.sound.record.is_some() || other.as_ref().is_some_and(|o| o.sound.record.is_some()) {
        game.sound.record = None;
        if let Some(o) = other {
            o.sound.record = None;
        }
    }
}
