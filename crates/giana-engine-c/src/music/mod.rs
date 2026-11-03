//! Music and sound effects of the 30A preview: Chris Huelsbeck's SID driver at `$5609..$5CD4`, the same player as the
//! original game's with small changes, translated instruction by instruction by `tools/recompile6502.py --mem`
//! (listing from `tools/c30/listing.py`, see `tools/c30/README.md`). The generated [`driver`] contains code only; the
//! song data, tables and instruments are read from the 64 KB memory image the player's disk unpacks to.
//!
//! How the game uses it (from the disassembly, all `JSR` sites of the entry points):
//!
//! | entry | call | meaning |
//! |---|---|---|
//! | `$5609` | once at boot (`$1D2F`) | initialise the driver (also called by the song start) |
//! | `$5650` | every frame, from the `$FB` raster IRQ and from wait loops | one tick of 3 voices, writes `$D400..$D418` |
//! | `$5C93` | Y = song: stage start (`Y` = header byte 3: 1 or 2), `6` at stage start, `7` stage clear / game over jingle | start a song, skipped when it already plays (`$02D4`) |
//! | `$5C9B` | Y = song: title (`3`), `6`, and a computed one at `$2181` | start a song unconditionally |
//! | `$5C35` | Y = effect id, from `$3423` when `$D5` is not `$FF` | start a sound effect on voice 3 |
//!
//! Effects are requested by storing an id in `$D5` (`$FF` = none); see `docs/rom-hack-family-c.md` for the sites.

mod driver;

pub use driver::{Driver, DriverCall};

pub const INIT: u16 = 0x5609;
pub const TICK: u16 = 0x5650;
pub const START_SONG: u16 = 0x5C93;
pub const RESTART_SONG: u16 = 0x5C9B;
pub const START_EFFECT: u16 = 0x5C35;

/// The driver on its own memory, with the calls the game makes.
#[derive(Clone)]
pub struct Music {
    pub driver: Driver,
}

impl Music {
    /// `ram` is the 64 KB image (from [`crate::Image`], or a RAM dump).
    pub fn new(ram: Vec<u8>) -> Music {
        Music { driver: Driver::new(ram) }
    }

    /// `JSR $5609` (what the game does once at boot).
    pub fn init(&mut self) {
        self.driver.call(INIT, 0, 0, 0, false);
    }

    /// `JSR $5C93` with Y = `song`.
    pub fn start_song(&mut self, song: u8) {
        self.driver.call(START_SONG, 0, 0, song, false);
    }

    /// `JSR $5C9B` with Y = `song`.
    pub fn restart_song(&mut self, song: u8) {
        self.driver.call(RESTART_SONG, 0, 0, song, false);
    }

    /// `JSR $5C35` with Y = `effect`.
    pub fn start_effect(&mut self, effect: u8) {
        self.driver.call(START_EFFECT, 0, 0, effect, false);
    }

    /// One frame (`JSR $5650`).
    pub fn tick(&mut self) {
        self.driver.call(TICK, 0, 0, 0, false);
    }

    /// The SID registers `$D400..$D418` as the driver wrote them.
    pub fn regs(&self) -> [u8; 25] {
        let mut r = [0u8; 25];
        r.copy_from_slice(&self.driver.sid[..25]);
        r
    }
}
