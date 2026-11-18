//! WIDESCREEN ON: the objects of the game live in the wide picture, not in the C64 window.
//!
//! The C64 shows 38 columns; objects (enemies, shots, items) are created just beyond the right edge of that window and
//! disappear beyond its edges. With the wide picture those edges would be visible, so with `wide_k = k` (extra width
//! `E = 8 * k` pixels per side) the object life cycle moves outwards by `E`: the spawn trigger looks `k` map columns ahead and
//! the spawn X, the right/left clip and the walls of edge-bound objects shift by `E`. Giana keeps the scroll trigger; her walls
//! move into the extra areas (below). Everything that does not depend on the screen edge is untouched. With `k = 0` every hook
//! falls back to the original code, so OFF stays bit exact.
//!
//! Objects in the extra areas need ground to walk on: the tile probes of the enemies read through [`GameState::wide_tile`]
//! outside of the 40 columns of screen RAM. To the right the cells come from the map (and `wide_right`), to the left from
//! `wide_left`, the columns that the scroll pushed out of the screen (so a destroyed block stays destroyed there).
//!
//! One fixed `k` per game, not one that follows the picture size: `WIDE_K` = 6 (48 px) is the widest view (with the border);
//! a narrower picture (no border: 24 px) only sees the inner part, and objects that appear 24 px outside of it are not seen
//! either. A fixed value keeps the rules independent of the window (network lockstep, suspend, tests).
//!
//! Rooms (statue and bonus rooms, treasure rooms) are not scrolling levels: there the rules are the C64's (`wide_on` is
//! false) and the picture shows blank margins.
//!
//! Giana walks `GIANA_EXTRA` pixels into each extra area (her walls in `sub_player_x_sub_9ef1` / `add_player_x_sub_9ec3`), the
//! part that every picture size shows. Her X stays above 0 (the C64's wall is 25 pixels right of X = 0), so everything that
//! compares it unsigned keeps working. Hits between sprites (the chip's collision and the game's distance tests) count X as
//! signed, because enemies live left of X = 0 in ON.
//!
//! The cells of the extra areas are cells of the level like any other: Giana stands on them, hits the blocks there and takes
//! the diamonds, debris falls and items rise through them, enemies walk on them. The tile routines of the original (`sub_9DF4`,
//! `sub_1A17` and the animations of `tileanim.rs`) address cells by pointers into the 40 x 25 screen RAM (`$C800 + 40 row +
//! column`), where the cell left of column 0 is column 39 of the row above. With ON they get *wide pointers* instead
//! ([`wide_ptr`]): the same cell as row and column in a wider address space (`WP_STRIDE` cells per row), so a column left of 0
//! or right of 39 is a cell of an extra area. The routines keep their code; only their pointer arithmetic (`$28` is one row
//! down, `$52` two rows and two columns...) goes through [`ptr_add`], and their reads and writes of cells outside of the 40
//! columns through [`GameState::wide_cell`] / [`GameState::wide_put`]. The scroll moves wide pointers one column left like the
//! C64 ones (the 16-bit decrement of `$1326`). With OFF no wide pointer is ever made, and every routine runs the C64's
//! arithmetic.
//!
//! The left extra area is `wide_left` (the columns the scroll pushed out, with their colours), the right one the map with
//! `wide_right`, the cells of it the game changed (the scroll brings those into column 39 instead of the map's cells). The
//! picture draws the extra areas from the same cells (`giana_render::wide::WideCols`).

use crate::image::img;
use crate::state::GameState;

/// The `k` of the game for WIDESCREEN ON (`E = 48` pixels per side, the widest view that the renderer shows).
pub const WIDE_K: u8 = 6;
/// Pixels that Giana may walk into each extra area with ON: the part of it that every picture size shows (24 without the border,
/// 48 with it), so that she never leaves the picture.
pub const GIANA_EXTRA: u8 = 24;
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

/// Wide pointers: screen cells at `WP_SCREEN + WP_STRIDE (row + WP_ROW0) + column + WP_COL0`, colour cells `$1000` above
/// (colour RAM is `$1000` above screen RAM in the original's pointer arithmetic, `sub_1D46`). Rows -4..28 and columns -44..84:
/// a pointer that the scroll moves left stays unambiguous for 44 columns.
const WP_SCREEN: u16 = 0x4000;
const WP_COLOUR: u16 = 0x5000;
const WP_END: u16 = 0x6000;
const WP_STRIDE: i32 = 128;
const WP_ROW0: i32 = 4;
const WP_COL0: i32 = 44;

/// The wide pointer of the screen cell at `row`, `col` (see the module text).
pub fn wide_ptr(row: i32, col: i32) -> u16 {
    (WP_SCREEN as i32 + WP_STRIDE * (row + WP_ROW0) + col + WP_COL0) as u16
}

/// `p` is a wide pointer (of a screen or a colour cell).
#[inline]
pub fn is_wide_ptr(p: u16) -> bool {
    (WP_SCREEN..WP_END).contains(&p)
}

/// A wide pointer as (colour cell, row, column).
pub fn wide_ptr_cell(p: u16) -> (bool, i32, i32) {
    let colour = p >= WP_COLOUR;
    let o = (p - if colour { WP_COLOUR } else { WP_SCREEN }) as i32;
    (colour, o.div_euclid(WP_STRIDE) - WP_ROW0, o.rem_euclid(WP_STRIDE) - WP_COL0)
}

/// `p + d` where `d` is a step of the original's pointer arithmetic (40 cells per row): a C64 pointer adds it as it is, a wide
/// pointer moves by the rows and columns that `d` means (`$28` one row down, `$52` two rows and two columns, `-1` one column
/// left: the nearest row, columns within -20..19).
#[inline]
pub fn ptr_add(p: u16, d: i32) -> u16 {
    if !is_wide_ptr(p) {
        return p.wrapping_add(d as u16);
    }
    let rows = (d + 20).div_euclid(40);
    let cols = d - 40 * rows;
    (p as i32 + WP_STRIDE * rows + cols) as u16
}

/// The C64 address of the cell of `p` (`$C800`/`$D800 + 40 row + column`); a C64 pointer as it is.
pub fn ptr_linear(p: u16) -> u16 {
    if !is_wide_ptr(p) {
        return p;
    }
    let (colour, r, c) = wide_ptr_cell(p);
    (if colour { 0xD800 } else { 0xC800 } + 40 * r + c) as u16
}

/// A sprite X of 9 bit as a signed number: from `NEG_FROM` on it is left of X = 0.
pub fn signed_x9(x: i32) -> i32 {
    if x >= NEG_FROM {
        x - 512
    } else {
        x
    }
}

/// What the rules of WIDESCREEN ON did to the objects (counted in every game, read by the tests and the soak report).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WideStats {
    /// writes of the tile routines to a cell that the extra areas do not keep (too far out, or the status bar rows)
    pub lost_writes: u32,
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

    /// How far (pixels) Giana's walls are moved out into the extra areas: `GIANA_EXTRA` while ON is in effect, else 0 (the C64's
    /// walls).
    #[inline]
    pub fn wide_giana_e(&self) -> u8 {
        (8 * self.wide_k_now()).min(GIANA_EXTRA)
    }

    /// ON: the first of the three columns that Giana's tile probe (`sub_9DF4`) reads (any column: she may stand in an extra
    /// area); `None` with the C64's rules, and the original probe does it.
    pub(crate) fn wide_giana_col(&self) -> Option<i32> {
        if self.wide_giana_e() == 0 {
            return None;
        }
        let x = self.player_x_low as i32 | ((self.sprite_x_msb & 1) as i32) << 8;
        Some((x - 0x15 - self.scroll_fine as i32).div_euclid(8))
    }

    /// ON: the screen row and column of the first cell of Giana's tile probe (`sub_9DF4`: the cells `t[0..3]` at her head, her
    /// head cell is the row above, her feet three rows below); `None` with the C64's rules or when the probe reads nothing.
    pub fn wide_giana_cell(&self) -> Option<(i32, i32)> {
        let col = self.wide_giana_col()?;
        let py = self.player_y_c64;
        if !(0x42..0xE1).contains(&py) {
            return None;
        }
        let row = (py.wrapping_sub(0x33) >> 3) as usize;
        if row >= 25 {
            return None;
        }
        let off = (crate::state::ROW_OFFSETS_HIGH_9BB9[row] as i32) << 8 | crate::state::ROW_OFFSETS_LOW_9BA0[row] as i32;
        Some((off / 40, off % 40 + col))
    }

    /// `a - b` of two sprite X (low byte, MSB) as the original's 9-bit subtraction gives it: low byte, high byte and borrow (`a`
    /// left of `b`). With ON the X are signed (an object left of X = 0 in the extra area is left of everything), with the C64's
    /// rules unsigned; both give the same bytes as long as no X is negative.
    pub fn x9_sub(&self, a: (u8, u8), b: (u8, u8)) -> (u8, u8, bool) {
        let x = |v: (u8, u8)| (v.1 as i32 & 1) << 8 | v.0 as i32;
        let d = if self.wide_on() { signed_x9(x(a)) - signed_x9(x(b)) } else { x(a) - x(b) };
        (d as u8, (d >> 8) as u8, d < 0)
    }

    /// The stage number is a room (no scrolling level): the rooms are `$22..$26` (original), `$42..$46` (family T); engine B
    /// knows its rooms. The number before them (`$21` / `$41`, the dragon after the last stage) is entered like a room but is a
    /// scrolling stage.
    pub fn wide_in_room(&self) -> bool {
        if let Some(b) = &self.engine_b {
            b.is_room(b.current.number)
        } else {
            self.stage >= self.tnw_room_numbers().3 as u32
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
        self.wide_left_colour.clear();
        self.wide_right.clear();
        if self.wide_k == 0 || self.current_map.len() < 256 * ROWS {
            return;
        }
        let col0 = self.wide_col39() - 39;
        for i in 0..self.wide_k as usize + LEFT_EXTRA {
            let m = (col0 - 1 - i as i32).max(0) as usize;
            let c: [u8; ROWS] = std::array::from_fn(|r| self.current_map[r * 256 + m]);
            self.wide_left_colour.push(c.map(|ch| self.char_colour(ch)));
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
            // the colours of a row are copied after its characters (`SCROLL_LOOPS`): colour RAM still has the cell that leaves
            self.wide_stage[r - 5] = self.screen_ram[r * 40];
            self.wide_stage_colour[r - 5] = self.color_ram[r * 40];
        }
        if to == 25 {
            let keep = self.wide_k as usize + LEFT_EXTRA;
            self.wide_left.insert(0, self.wide_stage);
            self.wide_left.truncate(keep);
            self.wide_left_colour.insert(0, self.wide_stage_colour);
            self.wide_left_colour.truncate(keep);
        }
    }

    /// The map column of screen column `col` (any column; beyond the end of the map the wrapped ones).
    fn wide_map_col(&self, col: i32) -> u8 {
        (self.wide_col39() + (col - 39)).rem_euclid(256) as u8
    }

    /// The cell of map column `m`, playfield row `row` (0..20) as the right extra area has it: changed by the game, or the map's.
    fn wide_right_cell(&self, m: u8, row: usize) -> (u8, u8) {
        match self.wide_right.iter().find(|e| e[0] == m && e[1] as usize == row) {
            Some(e) => (e[2], e[3]),
            None => {
                let ch = self.current_map.get(row * 256 + m as usize).copied().unwrap_or(0);
                (ch, self.char_colour(ch))
            }
        }
    }

    /// ON: the scroll copies map column `m` into column 39: the character of playfield row `row` (`colour` false) or its colour
    /// (`colour` true, after the character), when the game changed that cell in the right extra area. After the colour the cell
    /// is screen RAM, and the change is dropped from `wide_right`.
    pub(crate) fn wide_scroll_in(&mut self, m: u8, row: usize, colour: bool) -> Option<u8> {
        if self.wide_right.is_empty() {
            return None;
        }
        let i = self.wide_right.iter().position(|e| e[0] == m && e[1] as usize == row)?;
        let e = self.wide_right[i];
        if colour {
            self.wide_right.remove(i);
            return Some(e[3]);
        }
        Some(e[2])
    }

    /// Character and colour of the cell at screen row `row` (0..25) and column `col` (any) as the wide game has it: screen and
    /// colour RAM for columns 0..39, else the extra areas.
    pub fn wide_cell(&self, row: usize, col: i32) -> (u8, u8) {
        if (0..40).contains(&col) || row < 5 {
            // status bar and cloud band: the sky ring goes on (39 cells: the shift at $102C drops cell 0 and copies the
            // new cell 0 to cell 39)
            let c = if (0..40).contains(&col) { col as usize } else { col.rem_euclid(39) as usize };
            let i = row * 40 + c;
            return (self.screen_ram.get(i).copied().unwrap_or(0), self.color_ram.get(i).copied().unwrap_or(0));
        }
        if row >= 25 {
            return (0, 0);
        }
        if col < 0 {
            let i = ((-col - 1) as usize).min(self.wide_left.len().saturating_sub(1));
            return match (self.wide_left.get(i), self.wide_left_colour.get(i)) {
                (Some(c), Some(k)) => (c[row - 5], k[row - 5]),
                (Some(c), None) => (c[row - 5], self.char_colour(c[row - 5])),
                _ => (self.screen_ram[row * 40], self.color_ram[row * 40]),
            };
        }
        self.wide_right_cell(self.wide_map_col(col), row - 5)
    }

    /// Write the character (`ch`) and/or the colour of the cell at screen row `row`, column `col` (any) of the wide game. A cell
    /// of the extra areas that is not kept (their status bar rows, columns too far out) is not written, and counted.
    pub fn wide_put(&mut self, row: usize, col: i32, ch: Option<u8>, colour: Option<u8>) {
        if (0..40).contains(&col) {
            let i = row * 40 + col as usize;
            if i < 1000 {
                if let Some(v) = ch {
                    self.screen_ram[i] = v;
                }
                if let Some(v) = colour {
                    self.color_ram[i] = v;
                }
            }
            return;
        }
        if !(5..25).contains(&row) {
            self.wide_stats.lost_writes += 1;
            return;
        }
        if col < 0 {
            let i = (-col - 1) as usize;
            if i >= self.wide_left.len() {
                self.wide_stats.lost_writes += 1;
                return;
            }
            if let Some(v) = ch {
                self.wide_left[i][row - 5] = v;
            }
            if let (Some(v), Some(k)) = (colour, self.wide_left_colour.get_mut(i)) {
                k[row - 5] = v;
            }
            return;
        }
        if col >= 40 + (MAX_K as usize + LEFT_EXTRA) as i32 {
            self.wide_stats.lost_writes += 1;
            return;
        }
        let (m, r) = (self.wide_map_col(col), (row - 5) as u8);
        let (c0, k0) = self.wide_right_cell(m, row - 5);
        let e = [m, r, ch.unwrap_or(c0), colour.unwrap_or(k0)];
        match self.wide_right.iter_mut().find(|x| x[0] == m && x[1] == r) {
            Some(x) => *x = e,
            None => self.wide_right.push(e),
        }
    }

    /// The character at screen row `row` (0..25) and column `col` (any: -k-8 ..= 40+k+..) as the wide game has it.
    pub fn wide_tile(&self, row: usize, col: i32) -> u8 {
        self.wide_cell(row, col).0
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
