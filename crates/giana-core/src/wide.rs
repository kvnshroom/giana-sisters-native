//! WIDESCREEN ON: the objects of the game live in the wide picture, not in the C64 window.
//!
//! The C64 shows 38 columns; objects (enemies, shots, items) are created just beyond the right edge of that window and
//! disappear beyond its edges. With the wide picture those edges would be visible, so with `wide_k = k` (extra width
//! `E = 8 * k` pixels per side) the object life cycle moves outwards by `E`: the spawn trigger looks `k` map columns ahead and
//! the spawn X, the right/left clip and the walls of edge-bound objects shift by `E`. Giana herself is untouched (same walls,
//! same scroll trigger, same tile collision), and so is everything that does not depend on the screen edge. With `k = 0`
//! every hook falls back to the original code, so OFF and VIEW ONLY stay bit exact.
//!
//! Objects in the extra areas need ground to walk on: the tile probes of the enemies read through [`GameState::wide_tile`]
//! outside of the 40 columns of screen RAM. To the right the cells come straight from the map (it is never modified), to the
//! left from `wide_left`, the columns that the scroll pushed out of the screen (so a destroyed block stays destroyed there).
//!
//! One fixed `k` per game, not one that follows the picture size: `WIDE_K` = 6 (48 px) is the widest view (with the border);
//! a narrower picture (no border: 24 px) only sees the inner part, and objects that appear 24 px outside of it are not seen
//! either. A fixed value keeps the rules independent of the window (network lockstep, suspend, tests).
//!
//! Rooms (statue and bonus rooms, treasure rooms) are not scrolling levels: there the rules are the C64's (`wide_on` is
//! false) and the picture shows blank margins.

use crate::image::img;
use crate::state::GameState;

/// The `k` of the game for WIDESCREEN ON (`E = 48` pixels per side, the widest view that the renderer shows).
pub const WIDE_K: u8 = 6;
/// The largest `k` accepted (`E = 48`): the hole of the right/left clip must stay wide enough for a fireball (8 px per frame)
/// and the left clamp `$20 - E` of edge-bound objects must stay a valid sprite X.
pub const MAX_K: u8 = 6;
/// Columns kept on the left: the extra width, plus the columns an object can still be probed in before the left clip takes it
/// (its X is down to `-17 - E`, the probe looks 21 px to the left of it and 3 columns to the right).
const LEFT_EXTRA: usize = 8;
/// Rows of the playfield (screen rows 5..25)
const ROWS: usize = 20;
/// X (9 bit) from which a sprite counts as left of X = 0 (negative). Objects live from `-17 - E` to `335 + E` (and bosses on to
/// `0x1AF`); the clip hole starts at `0x15A + E`.
const NEG_FROM: i32 = 0x1B8;

/// What the rules of WIDESCREEN ON did to the objects (counted in every game, read by the tests and the soak report).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WideStats {
    /// objects created from the list of the stage (not counting the ones of the first screen of engine B)
    pub spawned: u32,
    /// objects that the list wanted and no sprite was free for (original: lost for good)
    pub lost_spawns: u32,
    /// engine B: a live enemy overwritten in slot 7 because no sprite was free
    pub overwrites: u32,
    /// engine B: a chain of four wiped all enemies
    pub wipes: u32,
    /// the list entries created in one go at the start of a stage (the ones the C64 would still create beyond its edge)
    pub prefilled: u32,
    /// the entries created (stage, trigger column, row, kind), recorded when `log` is set (not saved)
    #[serde(skip)]
    pub log: Option<Vec<(u8, u8, u8, u8)>>,
    /// the sprites taken by the right/left clip (slot, unsigned X of 9 bit, chain head, the extra width `E` in effect: 0 in a room), recorded when set (not saved)
    #[serde(skip)]
    pub clip_log: Option<Vec<(usize, u16, bool, u16)>>,
}

impl GameState {
    /// Set the rules of WIDESCREEN ON: `k` columns of extra width per side (0 = the C64's rules). Call it before the first frame
    /// of the game (or at any time: the left margin is rebuilt from the map).
    pub fn set_wide(&mut self, k: u8) {
        self.wide_k = k.min(MAX_K);
        self.wide_left.clear();
        self.wide_rebuild_left();
    }

    /// `k` while the rules apply (not in a room), else 0.
    #[inline]
    pub fn wide_k_now(&self) -> u8 {
        if self.wide_k == 0 || self.wide_in_room() {
            0
        } else {
            self.wide_k
        }
    }

    /// WIDESCREEN ON is in effect.
    #[inline]
    pub fn wide_on(&self) -> bool {
        self.wide_k_now() != 0
    }

    /// Extra width in pixels per side (0 when not in effect).
    #[inline]
    pub fn wide_e(&self) -> u16 {
        8 * self.wide_k_now() as u16
    }

    /// The stage number is a room (no scrolling level): the last number of the stages is `$20` (original), `$40` (family T);
    /// engine B knows its rooms.
    pub fn wide_in_room(&self) -> bool {
        if let Some(b) = &self.engine_b {
            b.is_room(b.current.number)
        } else if self.tnw.is_some() {
            self.stage >= 0x41
        } else {
            self.stage >= 0x21
        }
    }

    /// Map column on screen column 39 (the scroll counts the column when the copy starts, the copy is done in the next frame).
    fn wide_col39(&self) -> i32 {
        self.map_column as i32 - self.column_scroll_pending as i32
    }

    /// Fill the left margin from the map: the columns left of screen column 0 (the map column on screen column 0 is `col39 - 39`);
    /// before the start of the map the first column repeats (the ground goes on).
    pub fn wide_rebuild_left(&mut self) {
        self.wide_left.clear();
        if self.wide_k == 0 || self.current_map.len() < 256 * ROWS {
            return;
        }
        let col0 = self.wide_col39() - 39;
        for i in 0..self.wide_k as usize + LEFT_EXTRA {
            let m = (col0 - 1 - i as i32).max(0) as usize;
            let mut c = [0u8; ROWS];
            for (r, v) in c.iter_mut().enumerate() {
                *v = self.current_map[r * 256 + m];
            }
            self.wide_left.push(c);
        }
    }

    /// The column that leaves the screen on the left (screen column 0 of rows 5..25, taken before the shift) becomes the newest
    /// column of the left margin; `wide_stage` collects it while the rows are shifted in two parts.
    pub(crate) fn wide_note_shift(&mut self, from: usize, to: usize) {
        if self.wide_k == 0 {
            return;
        }
        for r in from..to {
            self.wide_stage[r - 5] = self.screen_ram[r * 40];
        }
        if to == 25 {
            let keep = self.wide_k as usize + LEFT_EXTRA;
            self.wide_left.insert(0, self.wide_stage);
            self.wide_left.truncate(keep);
        }
    }

    /// The character at screen row `row` (0..25) and column `col` (any: -k-8 ..= 40+k+..) as the wide picture has it.
    pub fn wide_tile(&self, row: usize, col: i32) -> u8 {
        if (0..40).contains(&col) {
            return self.screen_ram.get(row * 40 + col as usize).copied().unwrap_or(0);
        }
        if row < 5 {
            // status bar and cloud band: the sky ring goes on (39 cells: the shift at $102C drops cell 0 and copies the
            // new cell 0 to cell 39)
            return self.screen_ram.get(row * 40 + col.rem_euclid(39) as usize).copied().unwrap_or(0);
        }
        if row >= 25 {
            return 0;
        }
        if col < 0 {
            let i = (-col - 1) as usize;
            return match self.wide_left.get(i.min(self.wide_left.len().saturating_sub(1))) {
                Some(c) => c[row - 5],
                None => self.screen_ram[row * 40],
            };
        }
        let m = (self.wide_col39() + (col - 39)).rem_euclid(256) as usize;
        self.current_map.get((row - 5) * 256 + m).copied().unwrap_or(0)
    }

    /// Test hook: note that the clip takes sprite `slot` away.
    pub(crate) fn wide_note_clip(&mut self, slot: usize) {
        if self.wide_stats.clip_log.is_some() {
            let x = self.spr(2 * slot) as u16 | (((self.sprite_x_msb >> slot) & 1) as u16) << 8;
            let chain = self.engine_b.is_some() && self.bm_chain(slot);
            let e = self.wide_e();
            if let Some(l) = self.wide_stats.clip_log.as_mut() {
                l.push((slot, x, chain, e));
            }
        }
    }

    /// X (9 bit) of sprite `slot` as a signed number: sprites that left X = 0 to the left are negative.
    pub fn wide_x9(&self, slot: usize) -> i32 {
        let x = self.spr(2 * slot) as i32 | (((self.sprite_x_msb >> slot) & 1) as i32) << 8;
        if x >= NEG_FROM {
            x - 512
        } else {
            x
        }
    }

    /// Sprite `slot` is a live object (not a boss part) that is in the wide picture: its 24 pixels overlap `32 - E ..= 336 + E`.
    pub fn wide_enemy_in_view(&self, slot: usize) -> bool {
        if self.en.vic_me >> slot & 1 == 0 {
            return false;
        }
        let x = self.wide_x9(slot);
        let e = self.wide_e() as i32;
        x + 24 > 32 - e && x < 336 + e
    }

    /// Put sprite `slot` at the signed X `x9` (low byte and the MSB bit).
    pub fn wide_set_x9(&mut self, slot: usize, x9: i32) {
        let x = x9.rem_euclid(512);
        self.set_spr(2 * slot, (x & 0xFF) as u8);
        if x & 0x100 != 0 {
            self.sprite_x_msb |= 1 << slot;
        } else {
            self.sprite_x_msb &= !(1 << slot);
        }
    }

    /// The five tiles an enemy probe looks at for sprite `slot` when it is outside of the 38 columns that the original probe
    /// reads (`[0x47C, 0x481, 0x47D, 0x47E, 0x47F]`: the cell, two to the right of it, the three below). `None`: inside, the
    /// original lookup does it. `row_index` is `(Y - $23) >> 3`; `hi_table` the address of the high bytes of the row addresses.
    pub(crate) fn wide_probe(&self, slot: usize, row_index: usize, hi_table: usize) -> Option<[u8; 5]> {
        if self.wide_k_now() == 0 {
            return None;
        }
        let x = self.wide_x9(slot) - self.scroll_fine as i32 - 0x15;
        let col = x.div_euclid(8);
        if (0..=37).contains(&col) {
            return None;
        }
        // `$9BA0,X` and the high table: the address of the row's first cell, relative to $C800
        let base = (img(0x9BA0 + row_index) as usize | (img(hi_table + row_index) as usize) << 8) as i32;
        let (row, cell) = ((base / 40) as usize, base % 40);
        let c = cell + col;
        Some([self.wide_tile(row, c), self.wide_tile(row, c + 2), self.wide_tile(row + 1, c), self.wide_tile(row + 1, c + 1), self.wide_tile(row + 1, c + 2)])
    }
}
