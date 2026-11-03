//! ROM modules (`.grom`): fan hacks as BPS patches against the player's own extracted `ingame.bin` / `f_2.prg`.
//!
//! The crate is pure (no SDL, no game data needed for its tests). A module never contains original data: the maker
//! refuses patches whose literal bytes copy the base ([`leak`]).

pub mod apply;
pub mod bps;
pub mod code_ranges;
pub mod container;
pub mod crc32;
pub mod leak;
pub mod make;

pub use apply::{apply, Applied};
pub use container::{Meta, Module};
pub use make::{detect, make, Input};

/// Tiny deterministic xorshift generator for tests (no `rand` dependency).
#[cfg(test)]
pub(crate) fn test_rng(seed: u64) -> impl FnMut() -> u8 {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 24) as u8
    }
}
