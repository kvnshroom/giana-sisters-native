//! The suspend point: a whole running game as bytes, and back.
//!
//! Everything that makes the game what it is lives in [`GameState`] (the C64 variables, the maps, the entities, the sound
//! driver with its memory and the state of engine B or family T), so a snapshot is that state, serialised. What comes from
//! the installed game data (the image, the stage tables, the module's disk files) is not part of it: [`restore`] takes it
//! from the data that is installed now. [`data_fingerprint`] says which data that was, so that a snapshot is only used
//! with the data it was made with.

use crate::data;
use crate::features::{self, ConstructionKit};
use crate::state::GameState;

/// Bump this whenever the saved layout of the state changes (a field added, removed, renamed or retyped): slots of another
/// format are discarded. The test `the_saved_layout_is_guarded` in `giana-module/tests/suspend.rs` fails until it is bumped.
pub const SNAPSHOT_FORMAT: u32 = 3;

/// The state of `game` as bytes (JSON; the app compresses it).
pub fn save(game: &GameState) -> Vec<u8> {
    serde_json::to_vec(game).expect("a game state can always be written")
}

/// A game from the bytes of [`save`], tied to the installed game data. Nothing in `bytes` can make this panic: damaged or
/// foreign bytes are an error.
pub fn restore(bytes: &[u8]) -> Result<GameState, String> {
    let mut game: GameState = serde_json::from_slice(bytes).map_err(|e| format!("the saved game cannot be read: {e}"))?;
    // the buffers that the game indexes without looking: a file that was made by hand must not crash it later
    if game.current_map.len() != 5120 || game.sprite_mem.len() != 0x4000 || game.sprite_swap.len() != 0xA00 || game.anim_9d.len() != 0x4E || game.sound.mem.len() != 0x1_0000 {
        return Err("the saved game has parts of the wrong size".to_string());
    }
    match (game.engine_b.as_mut(), features::engine_b_files()) {
        (Some(b), Some(files)) => b.attach(files),
        (None, None) => {}
        _ => return Err("the saved game belongs to another ROM module".to_string()),
    }
    match (game.tnw.as_mut(), features::tnw_files()) {
        (Some(t), Some(files)) => t.attach(files),
        (None, None) => {}
        _ => return Err("the saved game belongs to another ROM module".to_string()),
    }
    // the calls of the sound driver are only recorded while a split sound plays: a game that was saved with the drivers of
    // the split sound goes on recording, so that the first step after the restore reaches them too
    game.sound.record = game.split_sound.is_some().then(Vec::new);
    Ok(game)
}

/// FNV-1a, 64 bit: a hash that is the same on every machine and in every version of the program.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf29ce484222325)
    }

    fn put(&mut self, bytes: &[u8]) {
        // the length first, so that two parts cannot run into each other
        for b in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
}

/// A hash of the installed game data: the image and tables, the feature set and the disk files of the module. A snapshot
/// made with other data would play differently, so it is not used.
pub fn data_fingerprint() -> String {
    let d = data::get();
    let mut h = Fnv::new();
    for part in [&d.ingame, &d.collision_table, &d.colortable, &d.stage1_map, &d.stage_palettes, &d.title_koala] {
        h.put(part);
    }
    h.put(&[match features::construction_kit() {
        ConstructionKit::Off => 0,
        ConstructionKit::Basic => 1,
        ConstructionKit::Full => 2,
    }]);
    for (tag, files) in [(b'b', features::engine_b_files()), (b't', features::tnw_files())] {
        if let Some(files) = files {
            h.put(&[tag]);
            for f in files.iter() {
                h.put(&f.name);
                h.put(&f.data);
            }
        }
    }
    format!("{:016x}", h.0)
}
