//! Features of fan-hack families, off for the original game.
//!
//! A ROM module (a patched `ingame.bin`) can change 6502 *code* as well as data. The port runs its own Rust version of
//! the code, so code changes only work where this module re-implements them. `giana-module` detects a family from
//! the code signature; the app installs the result with [`set_construction_kit`] when it installs the module's data.
//! With the default ([`ConstructionKit::Off`]) every hook below does nothing, so the original stays bit-exact.
//!
//! # Family "Construction Kit 1989" (see `docs/rom-hack-families.md`)
//!
//! The "Gianna Sisters Construction Kit" patches the game with two jumps and adds routines in the free
//! space of the level data area (`$8F21..$8F6C`). Per stage (33 entries, indexed by `stage - 1`) three tables in the image:
//!
//! | Address | Meaning |
//! |---|---|
//! | `$8F9D` | start value of the time, low byte (`$0449`, BCD): `$01` is the original 100 seconds, `$59` is 159 |
//! | `$8FBE` | opcode of the warp instruction at `$0B9A`: `$69` = `ADC #n`, `$E9` = `SBC #n` (with carry clear = minus n+1) |
//! | `$8FDF` | operand `n` of that instruction |
//!
//! The pipe warp of the original is `$20 += 2` and then the stage exit adds 1; here it is `$20 = f(table)` and then +1,
//! so a stage can warp forward by any amount, continue normally (`ADC #0`), repeat itself or go back.
//! The full variant adds 1000 points when the boss is hit after its death and a final bonus before the ending.

use crate::image::img;
use crate::state::GameState;
use std::sync::atomic::{AtomicU8, Ordering};

/// Which parts of the "Construction Kit 1989" family are active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionKit {
    /// The original game (and every module without these hooks).
    Off,
    /// Per-stage time and warp tables only (Gitty's Dreams 1).
    Basic,
    /// Tables plus the boss and final-bonus routines at `$F900` / `$F90B`.
    Full,
}

static CK: AtomicU8 = AtomicU8::new(0);

/// Sets the active feature set for the whole process. Call it once, when the game data of the module is installed.
pub fn set_construction_kit(level: ConstructionKit) {
    CK.store(level as u8, Ordering::Relaxed);
}

/// The active feature set.
pub fn construction_kit() -> ConstructionKit {
    match CK.load(Ordering::Relaxed) {
        1 => ConstructionKit::Basic,
        2 => ConstructionKit::Full,
        _ => ConstructionKit::Off,
    }
}

pub mod tnw;
pub use tnw::{set_tnw, tnw_files};

// ---- Family B: the 7A3 construction-set engine (see `engine_b/` and `docs/rom-hack-family-b.md`) ----------------------------

static ENGINE_B: std::sync::RwLock<Option<std::sync::Arc<Vec<giana_engine_b::files::ModFile>>>> = std::sync::RwLock::new(None);

/// Switches engine B on for the whole process (or off with `None`): a module whose engine tag is "b" passes its disk
/// files (`Applied::files`: stage, monster and sprite files). Call it together with `data::install` of the module's
/// image, before the first `GameState` is made. With `None` (the default) every engine B hook does nothing.
pub fn set_engine_b(files: Option<Vec<giana_engine_b::files::ModFile>>) {
    *ENGINE_B.write().unwrap() = files.map(std::sync::Arc::new);
}

/// The disk files of the installed family B module, `None` for the original engine.
pub fn engine_b_files() -> Option<std::sync::Arc<Vec<giana_engine_b::files::ModFile>>> {
    ENGINE_B.read().unwrap().clone()
}

const TIME_TABLE: usize = 0x8F9D;
const WARP_OP_TABLE: usize = 0x8FBE;
const WARP_ARG_TABLE: usize = 0x8FDF;

/// Value of `$20` after the warp instruction of the Construction Kit for the 1-based `stage` (before the stage exit adds 1).
pub fn ck_warp_value(stage: u32) -> u32 {
    let i = (stage.wrapping_sub(1) & 0xFF) as usize;
    let (op, n) = (img(WARP_OP_TABLE + i), img(WARP_ARG_TABLE + i));
    let s = stage as u8;
    // CLC; ADC #n or CLC; SBC #n (borrow set by the clear carry)
    (if op == 0xE9 { s.wrapping_sub(n).wrapping_sub(1) } else { s.wrapping_add(n) }) as u32
}

impl GameState {
    /// `$0B97..$0B9C`: the stage after the pipe warp (before the exit adds 1). Original: `$20 + 2`.
    pub fn warp_target_stage(&self) -> u32 {
        if let Some(b) = &self.engine_b {
            // family B (`$0B85`): `$20 += $9002`, the warp step of the stage header
            return self.stage + b.current.header.warp_step as u32;
        }
        match construction_kit() {
            ConstructionKit::Off => self.stage + 2,
            _ => ck_warp_value(self.stage),
        }
    }

    /// Hook at `$0A5E` (`JSR $8F50`): the time of this stage and the hundreds digit of its display.
    pub(crate) fn ck_stage_setup(&mut self) {
        if construction_kit() == ConstructionKit::Off {
            return;
        }
        self.timer_seconds_low = img(TIME_TABLE + (self.stage.wrapping_sub(1) & 0xFF) as usize);
        self.ck_hundreds = b'1';
    }

    /// Hook at `$1300` (`JSR $8F21`, runs after `sub_1D0A`): draws the hundreds digit of the time at `$C84A`.
    /// It is '1' until the two shown digits are "99", then '0'.
    pub(crate) fn ck_timer_digits(&mut self) {
        if construction_kit() == ConstructionKit::Off {
            return;
        }
        if self.screen_ram[0x4B] == b'9' && self.screen_ram[0x4C] == b'9' {
            self.screen_ram[0x4A] = b'0';
            self.ck_hundreds = b'0';
        } else {
            // `CMP #$2F; BCC` never leaves here for digits ('0' is $30 and up)
            self.screen_ram[0x4A] = self.ck_hundreds;
        }
        self.color_ram[0x4A] = 0x07;
    }

    /// Hook at `$291B` (`JSR $F900`, full variant): +1000 points whenever the boss is hit after it has died.
    pub(crate) fn ck_boss_bonus(&mut self) {
        if construction_kit() == ConstructionKit::Full {
            self.add_score_sub_991d(0x00, 0x10);
        }
    }

    /// Hook at `$302E` (`JSR $F90B`, full variant): at the last exit +10000 points (the `$A0` in a BCD byte carries into
    /// the ten-thousands), +2000 per remaining life, then the bonus screen of the stage exit; the ending follows it.
    /// Returns true when the hook took over (the caller then must not start the ending itself).
    pub(crate) fn ck_final_bonus(&mut self) -> bool {
        if construction_kit() != ConstructionKit::Full {
            return false;
        }
        if self.ck_ending_pending {
            return true; // the bonus screen is already running
        }
        self.add_score_sub_991d(0x00, 0xA0);
        while self.lives_c64 != 0 {
            self.add_score_sub_991d(0x00, 0x20);
            self.lives_c64 -= 1;
        }
        self.ck_ending_pending = true;
        self.start_stage_exit_l0bdf();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off() {
        assert_eq!(construction_kit(), ConstructionKit::Off);
    }
}
