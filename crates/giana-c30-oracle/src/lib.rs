//! **Test/dev oracle for the 30A preview (engine family C). Not part of the shipped game.**
//!
//! The shipped game is native Rust; nothing in `giana-app` may depend on this crate. Its job is to be the *reference
//! machine* for the native work packages (P physics, S scroll/draw, B blocks/HUD, O objects, W sprite hardware): it runs
//! the hack's **original** code in-process, and a work package
//!
//! 1. replaces one routine of the original by its native Rust port with [`Machine::hook`], and
//! 2. diffs RAM / sprite state / SID registers per frame against the *unhooked* machine ([`Snapshot::diff`],
//!    [`lockstep`]).
//!
//! What is modelled (recipes: `docs/rom-hack-family-c.md`, section 7.5):
//!
//! * 64 KB RAM with the C64's I/O window (`$01` decides whether `$D000..$DFFF` is I/O or RAM; there is no ROM, the game
//!   runs with `$01 = $35`), `giana_module::cpu::Cpu` (all opcodes the game uses, incl. undocumented ones),
//! * a raster-line VIC: `$D011/$D012/$D019/$D01A` raster IRQs (the game's chain at lines `$41/$42/$5A/$FB`), the sprite DMA
//!   state machine of the VIC-II documentation (Y match, expansion flip-flop, MCBASE), sprite-sprite collision `$D01E`
//!   computed per raster line from the sprite shapes in RAM (latched, cleared on read), colour RAM, a log of every VIC
//!   register write with its raster line ([`VicWrite`]) and of every read of `$D01E`,
//! * CIA 1: joystick port 2 on `$DC00` and a keyboard matrix on `$DC01`; SID register image with a write log.
//!
//! Not modelled: sprite-background collision (`$D01F` stays 0; the game never reads it), CIA timers, NMI, KERNAL/BASIC/
//! character ROM (sprite shapes are always read from RAM), cycle-exact IRQ latency; bad-line cycle stealing is optional
//! ([`Machine::steal_cycles`]). Speed: `examples/speed.rs`.

pub mod kernal;
mod hw;
mod machine;
mod script;
mod snapshot;

pub use hw::{Hw, Timing, VicWrite, WatchHit};
pub use machine::{Hook, Machine, FRAME_PC, PICTURE_PC};
pub use script::{lockstep, Script, Trace};
pub use snapshot::{Diff, Picture, Snapshot};

/// Zero-page cells the game uses as scratch between routines (`$02..$05`, `$0E/$0F`, `$14`, `$22`, `$24/$25`): their value at a
/// given instant depends on which routine ran last, so a port that is not meant to reproduce the original's temporaries
/// drops them from a diff with [`Diff::ignore_ram`].
pub const SCRATCH: &[u16] = &[0x02, 0x03, 0x04, 0x05, 0x0E, 0x0F, 0x14, 0x22, 0x24, 0x25];

/// Joystick port 2 bits on `$DC00` (active low on the wire; [`Machine::set_joy`] takes these as *pressed* bits).
pub mod joy {
    pub const UP: u8 = 0x01;
    pub const DOWN: u8 = 0x02;
    pub const LEFT: u8 = 0x04;
    pub const RIGHT: u8 = 0x08;
    pub const FIRE: u8 = 0x10;
}
