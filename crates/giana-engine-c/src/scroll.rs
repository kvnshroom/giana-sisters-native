//! Scrolling (work package S of `docs/rom-hack-family-c.md`, 7.4): native ports of the preview's scroll routines, each
//! verified call by call and frame by frame against the original code running in the oracle machine
//! (`crates/giana-c30-oracle/tests/scroll.rs`).
//!
//! State model: like [`crate::player`], the routines work on the game's own 64 KB memory image at the addresses of
//! [`C30aLayout`]; colour RAM (the I/O window `$D800..$DBE7`, 4-bit cells) is passed separately.
//!
//! How a scroll step happens in the original (measured in the oracle):
//!
//! 1. the frame logic (`$30FE`, from the `$5A` IRQ) ends with [`scroll_apply`] (`$2A59`): the player's request `$F8` moves
//!    the fine scrolls `$67` (playfield) and `$68` (cloud band); when the playfield's fine scroll wraps, `$14 = $80`;
//! 2. back in the `$5A` IRQ (`$323E`), `$14 < 0` starts the step: [`scroll_counters`] (`$3249..$32A6`) moves everything
//!    that holds a screen position one column left, then `$18C4` runs: [`scroll_shift`] (`$18C4..$1AC2`) increments the
//!    column `$21`, **enables IRQs** and shifts the playfield one character left, drawing the new right column from the
//!    level buffer; then it runs the frame logic a second time (`$30FE`) and the block animations (`$389D`, `$3991`,
//!    `$38F8`, unless `$FF`), and the IRQ clears `$14`;
//! 3. the shift alone takes about one whole frame (interrupted by the `$FB`, `$41`, `$42` and the next `$5A` IRQ, which sees
//!    `$14 = $7F` and skips the logic): a scroll step is two video frames with the logic run twice, once per frame.
//!
//! Ported so far:
//!
//! | original | function | what |
//! |---|---|---|
//! | `$2A59..$2ADC` | [`scroll_apply`] | fine scrolls of playfield and cloud band, step request, cloud redraw |
//! | `$3249..$32A6` | [`scroll_counters`] | the step's bookkeeping: the screen addresses of the block animations one column left |
//! | `$18C4..$1AC2` | [`scroll_shift`] | column counter, playfield shift, new right column (screen and colour RAM) |
//! | `$389D..$38F7` | [`anim_389d`] | bumped block: colour cycle, used block |
//! | `$38F8..$3990` | [`anim_38f8`] | breaking block, and the item on top of it starts falling ([`start_fall_above`], `$392A`) |
//! | `$3991..$3B3D` | [`anim_3991`] | item coming out of a block and falling (slides, landing, stacked items) |
//! | `$3B3E..$3B7F` | [`anim_3b3e`] | dissolving ground under a landed item |
//! | `$3725..$389C` | [`tile_anim`] | animated tiles: one of six channels per frame copies its next frame into the character set |

use crate::layout::C30aLayout;

/// First and last screen row of the playfield (rows 0-1 are the status bar, 2-4 the cloud band).
pub const PLAYFIELD_ROWS: std::ops::RangeInclusive<usize> = 5..=24;

/// `$2A59`: applies the player's scroll request `$F8` (pixels) and clears it.
///
/// The playfield's fine scroll `$67` goes down by `$F8` (modulo 8); when it wraps, `$14 = $80` asks the `$5A` IRQ for a
/// column step. In stages with a cloud band (stage header byte 8 non-zero) the cloud band's fine scroll `$68` goes down by
/// the same amount modulo 16 (the IRQ at `$30A4` shows `$68 >> 1`: half speed); when it wraps, the clouds of screen rows
/// 2-4 move one character left: the three cloud columns `$E1..$E3` are redrawn one column further left (two 4-character
/// wide cloud shapes made of the codes `$DA..$E1`, one 5-character shape from the tables at `$897A`/`$897F`) and the
/// columns that were left behind are cleared.
pub fn scroll_apply(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let req = r(ram, l.scroll_px);
    let (fine, borrow) = r(ram, l.fine_scroll).overflowing_sub(req);
    if borrow {
        ram[l.scroll_step as usize] = 0x80;
    }
    ram[l.fine_scroll as usize] = fine & 7;
    if r(ram, l.stage_header + 8) != 0 {
        let (sky, borrow) = r(ram, l.sky_scroll).overflowing_sub(req);
        ram[l.sky_scroll as usize] = sky & 0x0F;
        if borrow {
            clouds_left(ram, l);
        }
    }
    ram[l.scroll_px as usize] = 0;
}

/// `$2A78..$2AD7`: the cloud band moves one character left (see [`scroll_apply`]; the menus enter it by `JMP $2A78`
/// to draw the clouds again).
pub fn clouds_left(ram: &mut [u8], l: &C30aLayout) {
    // the three cloud rows (screen rows 2, 3, 4) and a column 0..39 with the wrap of the original's `DEY`/`BPL`/`LDY #$27`
    let cell = |row: u16, col: u8| (l.screen + row * 40 + u16::from(col)) as usize;
    let left = |c: u8| {
        let d = c.wrapping_sub(1);
        if d & 0x80 != 0 { 39 } else { d }
    };
    let (mut y, mut x) = (r8(ram, l.cloud_columns), r8(ram, l.cloud_columns + 1));
    // two 4-wide clouds: the top one in rows 2-3 at column `$E1`, the lower one in rows 3-4 at `$E2` (right ends)
    ram[cell(2, y)] = 0;
    ram[cell(3, y)] = 0;
    ram[cell(3, x)] = 0;
    ram[cell(4, x)] = 0;
    let mut code = 0xE1u8;
    while code >= 0xDA {
        y = left(y);
        x = left(x);
        ram[cell(3, y)] = code;
        ram[cell(4, x)] = code;
        ram[cell(2, y)] = code - 1;
        ram[cell(3, x)] = code - 1;
        code -= 2;
    }
    // the 5-wide cloud in rows 2-3 at column `$E3`, drawn from its right end
    let mut y = r8(ram, l.cloud_columns + 2);
    ram[cell(2, y)] = 0;
    ram[cell(3, y)] = 0;
    for i in (0..5u16).rev() {
        y = left(y);
        ram[cell(2, y)] = r8(ram, l.cloud_shape_top + i);
        ram[cell(3, y)] = r8(ram, l.cloud_shape_bottom + i);
    }
    for c in 0..3 {
        let a = (l.cloud_columns + c) as usize;
        ram[a] = left(ram[a]);
    }
}

fn r8(ram: &[u8], a: u16) -> u8 {
    ram[a as usize]
}

/// `$3249..$32A6` (inside the `$5A` IRQ, IRQs masked): the bookkeeping of a column step. `$14` goes from `$80` to `$7F`
/// (the step is running), and everything that remembers a screen position moves one column left: the 16-bit screen
/// addresses of the four kinds of block animation (8 slots of `$3991`, 16 of `$3B3E`, 4 of `$38F8`, 3 of `$389D`; all
/// slots, in use or not), the low bytes of the last address each kind was started at (the block code compares them so
/// that one block does not start twice), and the game's pointer `$0E/$0F`.
pub fn scroll_counters(ram: &mut [u8], l: &C30aLayout) {
    let dec = |ram: &mut [u8], a: u16| ram[a as usize] = ram[a as usize].wrapping_sub(1);
    let dec16 = |ram: &mut [u8], lo: u16, hi: u16| {
        if ram[lo as usize] == 0 {
            dec(ram, hi);
        }
        dec(ram, lo);
    };
    dec(ram, l.scroll_step);
    for a in [l.anim_389d_last, l.anim_3b3e_last, l.anim_38f8_last, l.anim_3991_last] {
        dec(ram, a);
    }
    // (low bytes, high bytes, slots)
    for (lo, hi, n) in [
        (l.anim_3991_lo, l.anim_3991_hi, 8),
        (l.anim_3b3e_lo, l.anim_3b3e_hi, 16),
        (l.anim_389d_lo, l.anim_389d_hi, 3),
        (l.anim_38f8_lo, l.anim_38f8_hi, 4),
    ] {
        for x in 0..n {
            dec16(ram, lo + x, hi + x);
        }
    }
    dec16(ram, l.scratch_ptr, l.scratch_ptr + 1);
}

/// `$18C4..$1AC2`: one column step of the playfield. The column `$21` goes up by one, screen rows 5-24 (screen RAM and
/// colour RAM) move one character left, and the new column 39 is drawn from the level buffer: row `r` gets the tile
/// `level[(r - 5) * 256 + $21]` and its colour `$BF00[tile]`. The original enables IRQs (`CLI`) before it starts.
///
/// `color` is colour RAM (`$D800..`, at least 1000 cells, low nibbles). Returns the registers the original leaves for the
/// code after it (`A` = colour, `X` = tile of the bottom row, `Y` = `$21`).
pub fn scroll_shift(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> (u8, u8, u8) {
    let column = ram[l.scroll_column as usize].wrapping_add(1);
    ram[l.scroll_column as usize] = column;
    let screen = l.screen as usize;
    let mut regs = (0, 0, column);
    for row in PLAYFIELD_ROWS {
        let s = screen + row * 40;
        ram.copy_within(s + 1..s + 40, s);
        color.copy_within(row * 40 + 1..row * 40 + 40, row * 40);
        let tile = ram[(l.level as usize + (row - 5) * 256 + column as usize) & 0xFFFF];
        let c = ram[l.tile_colours as usize + tile as usize];
        ram[s + 39] = tile;
        color[row * 40 + 39] = c & 15;
        regs = (c, tile, column);
    }
    regs
}

// ---- block animations ------------------------------------------------------------------------------------------------
//
// Four kinds of animation that the draw engine advances: from the `$42` IRQ when no column step is running (`$3991`,
// `$38F8`, `$389D`, `$3991`, then `$FF = 0` and `$3B3E` twice), and at the end of a column step after the second logic
// run (`$389D`, `$3991`, `$38F8`, `$3991`, skipped while `$FF` is set). Each call advances one slot, round robin (the slot
// cursors at `$CBEC..$CBEF`). The block code of package B starts them (`$3CF6`, `$3D2F`, `$3D8D`, `$3BFF`). An animated
// block is 2 x 3 characters, addressed by the screen address of its top left cell, which the column step moves left with
// the screen ([`scroll_counters`]).

/// A 16-bit pointer in the zero page (`$0E/$0F`, `$10/$11`).
fn ptr(ram: &[u8], zp: u16) -> u16 {
    u16::from_le_bytes([ram[zp as usize], ram[zp as usize + 1]])
}

fn set_ptr(ram: &mut [u8], zp: u16, v: u16) {
    ram[zp as usize..zp as usize + 2].copy_from_slice(&v.to_le_bytes());
}

/// A write through `(zp),Y`: colour RAM for `$D800..$DBFF` (the I/O window, `$01 = $35`), RAM elsewhere. The other
/// I/O addresses are dropped (the blocks are always on the screen, so their colour cells are always colour RAM).
fn put(ram: &mut [u8], color: &mut [u8], addr: u16, v: u8) {
    match addr {
        0xD800..=0xDBFF => color[(addr - 0xD800) as usize] = v & 15,
        0xD000..=0xDFFF => {}
        _ => ram[addr as usize] = v,
    }
}

/// `$3E72`: the colour pointer `$10/$11` for the screen pointer `$0E/$0F` (`$C8xx` -> `$D8xx`).
fn colour_ptr(ram: &mut [u8], l: &C30aLayout) -> u16 {
    let p = ptr(ram, l.scratch_ptr) | 0x1000;
    set_ptr(ram, l.colour_ptr, p);
    p
}

/// The next slot of a round-robin cursor: one down, wrapping from 0 (or any value with bit 7) to `last`.
fn next_slot(ram: &mut [u8], cursor: u16, last: u8) -> u16 {
    let d = ram[cursor as usize].wrapping_sub(1);
    let x = if d & 0x80 != 0 { last } else { d };
    ram[cursor as usize] = x;
    u16::from(x)
}

/// The 2 x 3 cells of a block: row 0 at `+0..+2`, row 1 at `+40..+42`.
const CELLS: [u16; 6] = [0, 1, 2, 40, 41, 42];

/// `$3E7D`: colour `c` into the six cells at the colour pointer `cp`.
fn colour_block(ram: &mut [u8], color: &mut [u8], cp: u16, c: u8) {
    for o in CELLS {
        put(ram, color, cp.wrapping_add(o), c);
    }
}

/// `$3E36`: the used block (characters `$4C..$51`, colour 8) at `$0E/$0F`.
fn used_block(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    let cp = colour_ptr(ram, l);
    let sp = ptr(ram, l.scratch_ptr);
    for (i, o) in CELLS.into_iter().enumerate() {
        put(ram, color, sp.wrapping_add(o), 0x4C + i as u8);
        put(ram, color, cp.wrapping_add(o), 8);
    }
}

/// `$3B2B` (both rows) and `$3B32` (row 0 only): clears the block at `$0E/$0F`.
fn clear_block(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, both_rows: bool) {
    let sp = ptr(ram, l.scratch_ptr);
    let cells: &[u16] = if both_rows { &[40, 41, 42, 0, 1, 2] } else { &[0, 1, 2] };
    for &o in cells {
        put(ram, color, sp.wrapping_add(o), 0);
    }
}

/// `$396C`: frame `f` of the animation tables (`$8E87[f]` and `$8E8D[f]`, each the first of three consecutive
/// characters) into the block at `$0E/$0F`. The additions carry from one to the next as the original's `ADC #1` chain does.
fn draw_frame(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, f: u8) {
    let sp = ptr(ram, l.scratch_ptr);
    let mut carry = false;
    for (row, table) in [(0u16, l.anim_frames_top), (40, l.anim_frames_bottom)] {
        let mut a = ram[table.wrapping_add(u16::from(f)) as usize];
        for c in 0..3 {
            put(ram, color, sp.wrapping_add(row + c), a);
            if c < 2 {
                let s = u16::from(a) + 1 + u16::from(carry);
                (a, carry) = (s as u8, s > 0xFF);
            }
        }
    }
}

/// `$389D`: the bumped block (3 slots, state `$8DD4`, kind `$8DDE`). States 1-5 colour the block from the table at
/// `$8E1C`, state 7 shows the used block (kind 0) or colour `$8E1C[7]`, state 9 ends: a kind other than 0 with `$0202 = 0`
/// shows the used block and sets `$0202 = $FF`; the head-bump flag `$0201 = $FF`, and the slot is freed (`$1E` counts
/// the running ones). The other states only count.
pub fn anim_389d(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    let x = next_slot(ram, l.anim_389d_cursor, 2);
    let state_at = (l.anim_389d_state + x) as usize;
    let state = ram[state_at];
    if state == 0 {
        return;
    }
    ram[state_at] = state.wrapping_add(1);
    let (lo, hi) = (ram[(l.anim_389d_lo + x) as usize], ram[(l.anim_389d_hi + x) as usize]);
    set_ptr(ram, l.scratch_ptr, u16::from_le_bytes([lo, hi]));
    let cp = u16::from_le_bytes([lo, hi | 0x10]);
    set_ptr(ram, l.colour_ptr, cp);
    let kind = ram[(l.anim_389d_kind + x) as usize];
    let colour = |ram: &[u8], s: u8| ram[(l.anim_389d_colours + u16::from(s)) as usize];
    match state {
        0..=5 => colour_block(ram, color, cp, colour(ram, state)),
        7 if kind != 0 => colour_block(ram, color, cp, colour(ram, 7)),
        7 => used_block(ram, color, l),
        9 => {
            if kind != 0 && ram[l.repeat_hits as usize] == 0 {
                ram[l.repeat_hits as usize] = 0xFF;
                used_block(ram, color, l);
            }
            ram[l.head_bump as usize] = 0xFF;
            ram[state_at] = 0;
            ram[l.anim_389d_count as usize] = ram[l.anim_389d_count as usize].wrapping_sub(1);
        }
        _ => {}
    }
}

/// `$38F8`: the breaking block (4 slots, state `$8DD0`). States 1-6 draw frames 1-6 of the animation tables; state 7
/// clears the block, frees the slot (`$2E` counts the running ones) and lets an item that stood on it fall
/// ([`start_fall_above`] over 3 cells with fall state 6).
pub fn anim_38f8(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    let x = next_slot(ram, l.anim_38f8_cursor, 3);
    let state_at = (l.anim_38f8_state + x) as usize;
    let state = ram[state_at];
    if state == 0 {
        return;
    }
    let (lo, hi) = (ram[(l.anim_38f8_lo + x) as usize], ram[(l.anim_38f8_hi + x) as usize]);
    set_ptr(ram, l.scratch_ptr, u16::from_le_bytes([lo, hi]));
    if state == 7 {
        ram[state_at] = 0;
        ram[l.anim_38f8_count as usize] = ram[l.anim_38f8_count as usize].wrapping_sub(1);
        clear_block(ram, color, l, true);
        ram[l.fall_start_state as usize] = 6;
        start_fall_above(ram, l, 3);
    } else {
        ram[state_at] = state.wrapping_add(1);
        draw_frame(ram, color, l, state);
    }
}

/// `$392A`: looks for an item (characters `$28 $29` at its top left) on top of the block at `$0E/$0F`, in `cells` cells
/// starting two rows up and one column left (`$0E - $51`); the first `$28` decides. If it is an item and a fall slot is
/// free, the item starts falling with state `$D9`. `$0E/$0F` is left at the cell where the search stopped. The block code
/// calls it too (`$3CA5`: 2 cells, state 3).
pub fn start_fall_above(ram: &mut [u8], l: &C30aLayout, cells: u8) {
    let mut p = ptr(ram, l.scratch_ptr).wrapping_sub(0x51);
    set_ptr(ram, l.scratch_ptr, p);
    for _ in 0..cells {
        if ram[p as usize] == 0x28 {
            if ram[p.wrapping_add(1) as usize] == 0x29 {
                new_fall(ram, l);
            }
            return;
        }
        p = p.wrapping_add(1);
        set_ptr(ram, l.scratch_ptr, p);
    }
}

/// `$394E`: the highest free fall slot gets state `$D9` and the address `$0E/$0F` (nothing if all 8 run).
fn new_fall(ram: &mut [u8], l: &C30aLayout) {
    if let Some(x) = (0..8u16).rev().find(|&x| ram[(l.anim_3991_state + x) as usize] == 0) {
        ram[(l.anim_3991_state + x) as usize] = ram[l.fall_start_state as usize];
        ram[(l.anim_3991_lo + x) as usize] = ram[l.scratch_ptr as usize];
        ram[(l.anim_3991_hi + x) as usize] = ram[l.scratch_ptr as usize + 1];
    }
}

/// Whether the character at `a` is ground for a falling item (`$BE00[tile] & $83`).
fn solid(ram: &[u8], l: &C30aLayout, a: u16) -> bool {
    ram[(l.tile_props + u16::from(ram[a as usize])) as usize] & 0x83 != 0
}

/// `$3991`: the item that comes out of a block and falls (8 slots, state `$8D80`). States 0-3 draw frames 0-3 of the
/// animation tables (state 2 also counts `$69` down), 4 and 5 wait, state 6 falls (below), anything else frees the slot,
/// as does reaching the bottom of the screen (address `$CB98` or beyond).
///
/// Falling (state 6, only while the block's top left cell is still `$28`): the three cells two rows down decide: all
/// free, it moves one row down; the left one solid, it slides down-right if the other two are free; the left one free and
/// the right one (or the middle one) solid, it slides down-left; else it has landed. A landed item turns the cells under
/// it from `$52/$53` to `$2E`, `$2E` to `$2F` and `$2F` to empty (one step per call, waiting while that happens); when
/// nothing under it changes it is done (state 7, sound `$0C`). After a move the item is drawn at its new place
/// (characters `$28..$2D`, colour 8), and an item stacked on top of it (three rows up, in the columns it came from)
/// follows: directly if it moved sideways and the cells under the stacked item are free (the original enables IRQs there
/// and loops), else through a new fall slot.
///
/// Returns `true` if the original would have enabled IRQs (`CLI` at `$3B20`): its caller continues with IRQs on.
///
/// Not reproduced: the original's slide to the right falls into the wrong branch when the 16-bit address wraps to 0
/// (`$3A0A`), which no screen address does.
pub fn anim_3991(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> bool {
    let x = next_slot(ram, l.anim_3991_cursor, 7);
    let state_at = (l.anim_3991_state + x) as usize;
    let state = ram[state_at];
    if state == 0 {
        return false;
    }
    let (lo, hi) = (ram[(l.anim_3991_lo + x) as usize], ram[(l.anim_3991_hi + x) as usize]);
    let mut p = u16::from_le_bytes([lo, hi]);
    set_ptr(ram, l.scratch_ptr, p);
    if hi >= 0xCB && lo >= 0x98 {
        ram[state_at] = 0;
        return false;
    }
    ram[l.fall_column as usize] = 1;
    if state > 6 {
        ram[state_at] = 0;
        return false;
    }
    if state < 6 {
        ram[state_at] = state + 1;
        if state < 4 {
            if state == 2 {
                ram[l.anim_3991_count as usize] = ram[l.anim_3991_count as usize].wrapping_sub(1);
            }
            draw_frame(ram, color, l, state);
        }
        return false;
    }
    // $39D8: falling
    if ram[p as usize] != 0x28 {
        ram[state_at] = 0;
        return false;
    }
    let below = |ram: &[u8], i: u16| solid(ram, l, p.wrapping_add(0x50 + i));
    let (left, middle, right) = (below(ram, 0), below(ram, 1), below(ram, 2));
    // -1 left, 0 down, 1 right, None landed
    let mv = match (left, middle, right) {
        (false, false, false) => Some(0),
        (false, false, true) | (false, true, false) => Some(-1),
        (true, false, false) => Some(1),
        _ => None,
    };
    let Some(mv) = mv else {
        // $3A3C: landed
        let mut unchanged = true;
        for y in 0x50..0x53u16 {
            let a = p.wrapping_add(y) as usize;
            let new = match ram[a] {
                0x2F => Some(0),
                0x2E => Some(0x2F),
                v if v | 1 == 0x53 => Some(0x2E),
                _ => None,
            };
            if let Some(n) = new {
                ram[a] = n;
                unchanged = false;
            }
        }
        ram[l.fall_landed as usize] = if unchanged { 0x50 } else { 0 };
        if unchanged {
            ram[state_at] = ram[state_at].wrapping_add(1);
            ram[l.effect as usize] = 0x0C;
        }
        return false;
    };
    if mv == 0 {
        clear_block(ram, color, l, false);
    } else {
        // $39FF / $3A2C: the slide clears both rows and moves one column; `$12` remembers where it came from
        clear_block(ram, color, l, true);
        let col = ram[l.fall_column as usize];
        ram[l.fall_column as usize] = if mv > 0 { col.wrapping_sub(1) } else { col.wrapping_add(1) };
        p = if mv > 0 { p.wrapping_add(1) } else { p.wrapping_sub(1) };
    }
    // $3A78: one row down, remember the place
    p = p.wrapping_add(40);
    set_ptr(ram, l.scratch_ptr, p);
    ram[(l.anim_3991_lo + x) as usize] = p as u8;
    ram[(l.anim_3991_hi + x) as usize] = (p >> 8) as u8;
    draw_item_and_follow(ram, color, l)
}

/// `$3A8E..$3B28`: draws the item at `$0E/$0F` (characters `$28..$2D`, colour 8) and handles an item stacked on it.
/// Returns whether IRQs were enabled.
fn draw_item_and_follow(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> bool {
    let mut irqs_on = false;
    loop {
        let cp = colour_ptr(ram, l);
        let sp = ptr(ram, l.scratch_ptr);
        for (o, v) in [(42u16, 0x2D), (41, 0x2C), (40, 0x2B), (2, 0x2A), (1, 0x29), (0, 0x28)] {
            put(ram, color, sp.wrapping_add(o), v);
        }
        for o in CELLS {
            put(ram, color, cp.wrapping_add(o), 0x28);
        }
        // $3AC9: three rows up and two columns left, the three cells from the column offset `$12`
        let mut p = sp.wrapping_sub(0x7A);
        set_ptr(ram, l.scratch_ptr, p);
        let off = ram[l.fall_column as usize];
        let Some(y) = (0..3u8).map(|k| off.wrapping_add(k)).find(|&y| ram[p.wrapping_add(u16::from(y)) as usize] == 0x28) else {
            return irqs_on;
        };
        p = p.wrapping_add(u16::from(y));
        set_ptr(ram, l.scratch_ptr, p);
        if y & 3 == 0 || (0x50..0x53u16).any(|i| ram[p.wrapping_add(i) as usize] != 0) {
            // $3B24
            ram[l.fall_start_state as usize] = 6;
            new_fall(ram, l);
            return irqs_on;
        }
        // $3B06: the stacked item moves down one row at once
        for o in [0u16, 1, 2] {
            put(ram, color, p.wrapping_add(o), 0);
        }
        set_ptr(ram, l.scratch_ptr, p.wrapping_add(40));
        ram[l.fall_column as usize] = 1;
        ram[l.blocks_skip as usize] = 1;
        irqs_on = true;
    }
}

/// `$3B3E`: the dissolving ground under a landed item (16 slots, state `$8D98`): state 1 turns a `$52`/`$53` cell into
/// `$2E` (anything else ends at once), 2 into `$2F`, 3 clears it; then the slot is freed (`$6B` counts the running ones).
pub fn anim_3b3e(ram: &mut [u8], l: &C30aLayout) {
    let x = next_slot(ram, l.anim_3b3e_cursor, 15);
    let state_at = (l.anim_3b3e_state + x) as usize;
    let state = ram[state_at];
    if state == 0 {
        return;
    }
    let p = u16::from_le_bytes([ram[(l.anim_3b3e_lo + x) as usize], ram[(l.anim_3b3e_hi + x) as usize]]);
    set_ptr(ram, l.scratch_ptr, p);
    let new = match state {
        1 if ram[p as usize] | 1 == 0x53 => Some(0x2E),
        2 => Some(0x2F),
        3 => Some(0),
        _ => None,
    };
    match new {
        Some(v) => {
            ram[p as usize] = v;
            ram[state_at] = state + 1;
        }
        None => {
            ram[l.anim_3b3e_count as usize] = ram[l.anim_3b3e_count as usize].wrapping_sub(1);
            ram[state_at] = 0;
        }
    }
}

// ---- character animations ------------------------------------------------------------------------------------------

/// `$3725` (from the `$FB` IRQ, skipped while a column step is requested or running): the animated tiles. Six channels,
/// one per call, round robin (cursor `$8E24`, 5 down to 0; stage 12 patches the code to run channels 6 down to 1); each channel except 3 counts its frame down (`$02F9 + n`,
/// restarting at `$8E25[n]`) and copies that frame's graphics, found through the channel's word table (`$8E2C`/`$8E33`),
/// into the character set:
///
/// | channel | what |
/// |---|---|
/// | 0 | 24 bytes to `$C680` |
/// | 1 | two characters built from the bit patterns at `$1AD7`/`$1AD8` (mirrored pairs at `$C530..$C54F`) |
/// | 2 | 13 bytes spread over `$C234..$C25B` and `$C793..$C797` |
/// | 3 | `$02FC` counts up or down (see the code; not while `$95` is set); its low 2 bits pick a rotation of the patterns at `$18B8`/`$18BC`/`$18C0` for `$C738..$C74F` |
/// | 4 | 48 bytes to `$C760` |
/// | 5 | 8 bytes to `$C0D8` and every third byte from `$EAC0`, then the sparkle at `$C4D3..$C4DC` and the flag `$CF7F` |
///
/// Channels 0-2 stand still while `$CF7F` is not 0 (channel 5 counts it down to -1 and sets it to 1 again: every other
/// round). Channel 6 (two 16-byte frames to `$C680`/`$C690` through `$8E70`) runs only in stage 12, in place of channel 0.
pub fn tile_anim(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    // the stage setup rewrites the cursor's wrap (stage 12: `BNE` instead of `BPL` at `$3729` and 6 at `$372C`: channels
    // 6 down to 1 instead of 5 down to 0), so both are read from the code
    let last = r(ram, l.tile_anim_last);
    let x = if r(ram, l.tile_anim_wrap) == 0xD0 {
        let d = r(ram, l.tile_anim_cursor).wrapping_sub(1);
        let x = if d == 0 { last } else { d };
        ram[l.tile_anim_cursor as usize] = x;
        u16::from(x)
    } else {
        next_slot(ram, l.tile_anim_cursor, last)
    };
    let table = u16::from_le_bytes([r(ram, l.tile_anim_tables_lo + x), r(ram, l.tile_anim_tables_hi + x)]);
    set_ptr(ram, l.tile_anim_ptr, table);
    if x == 3 {
        // $376D
        if r(ram, l.state_95) != 0 {
            return;
        }
        // up while `$02E0` is set or while the first object slot (3 down to 0) of type `$7F` with a linked slot has bit 7
        // of that slot's state set
        let up = r(ram, l.state_02e0) != 0
            || (0..4u16)
                .rev()
                .find_map(|y| {
                    let link = r(ram, l.group_leader + 4 + y);
                    (r(ram, l.object_type + y) == 0x7F && link != 0).then(|| r(ram, l.object_state + u16::from(link)) & 0x80 != 0)
                })
                .unwrap_or(false);
        let c = r(ram, l.tile_anim_phase);
        ram[l.tile_anim_phase as usize] = if up { c.wrapping_add(1) } else { c.wrapping_sub(1) };
        let mut k = u16::from(r(ram, l.tile_anim_phase) & 3);
        for y in (0..8u16).rev() {
            for (dst, src) in [(0u16, 0u16), (8, 4), (16, 8)] {
                ram[(l.tile_anim_rot_chars + dst + y) as usize] = r(ram, l.tile_anim_rot + src + k);
            }
            k = if k == 0 { 3 } else { k - 1 };
        }
        return;
    }
    if x < 3 && r(ram, l.tile_anim_flicker) != 0 {
        return;
    }
    // $3745: the channel's frame counter
    let counter = (l.tile_anim_frames + x) as usize;
    ram[counter] = ram[counter].wrapping_sub(1);
    if ram[counter] & 0x80 != 0 {
        ram[counter] = r(ram, l.tile_anim_lengths + x);
    }
    let mut y = ram[counter] << 1;
    let frame = u16::from_le_bytes([r(ram, table.wrapping_add(u16::from(y))), r(ram, table.wrapping_add(u16::from(y) + 1))]);
    y = y.wrapping_add(1);
    set_ptr(ram, l.tile_anim_ptr + 2, frame);
    let src = |ram: &[u8], i: u16| ram[frame.wrapping_add(i) as usize];
    match x {
        4 => {
            for i in (0..0x30).rev() {
                ram[(l.tile_anim_chars_4 + i) as usize] = src(ram, i);
            }
        }
        6 => {
            // $37BC (unreachable through the cursor)
            let hi = r(ram, l.tile_anim_alt + u16::from(y));
            let lo = r(ram, l.tile_anim_alt + u16::from(y.wrapping_sub(1)));
            let alt = u16::from_le_bytes([lo, hi]);
            set_ptr(ram, l.tile_anim_ptr, alt);
            for i in (0..0x10u16).rev() {
                ram[(l.tile_anim_chars_0 + i) as usize] = src(ram, i);
                ram[(l.tile_anim_chars_0 + 0x10 + i) as usize] = ram[alt.wrapping_add(i) as usize];
            }
        }
        5 => {
            // $3845
            for i in (0..8u16).rev() {
                let v = src(ram, i);
                ram[(l.tile_anim_chars_5 + i) as usize] = v;
                ram[(l.tile_anim_sprite_5 + 3 * i) as usize] = v;
            }
            let s = l.tile_anim_sparkle;
            for (i, mask) in [(5u16, 0xFF), (6, 0xDE), (7, 0x7A), (8, 0x65)] {
                let v = r(ram, s + i) ^ mask;
                ram[(s + i) as usize] = v;
                ram[(s + 9 - i) as usize] = v;
            }
            let v = r(ram, s + 9).rotate_left(2);
            ram[(s + 9) as usize] = v;
            ram[s as usize] = v;
            let f = r(ram, l.tile_anim_flicker).wrapping_sub(1);
            ram[l.tile_anim_flicker as usize] = if f & 0x80 != 0 { 1 } else { f };
        }
        0 => {
            for i in (0..0x18).rev() {
                ram[(l.tile_anim_chars_0 + i) as usize] = src(ram, i);
            }
        }
        1 => {
            // $3813: two characters from the bit patterns, each mirrored into its neighbour
            let mut a = r(ram, l.tile_anim_bits + u16::from(y) - 1);
            let mut b = r(ram, l.tile_anim_bits + u16::from(y));
            let c = l.tile_anim_chars_1;
            for i in 0..8u16 {
                ram[(c + 7 - i) as usize] = a;
                ram[(c + 8 + i) as usize] = a;
                a >>= 1;
            }
            for i in 0..8u16 {
                ram[(c + 0x17 - i) as usize] = b;
                ram[(c + 0x18 + i) as usize] = b;
                b <<= 1;
            }
        }
        _ => {
            // $37E3: 4 + 5 + 4 bytes, each also mirrored
            let c = l.tile_anim_chars_2;
            let mut i = 0u16;
            for (dst, mirror, n) in [(0x14u16, 0u16, 4u16), (0x1C, 0x07, 5), (0x24, 0x10, 4)] {
                for k in 0..n {
                    let v = src(ram, i);
                    ram[(c + dst + k) as usize] = v;
                    ram[(c + mirror + n - 1 - k) as usize] = v;
                    if n == 5 {
                        ram[(l.tile_anim_chars_2b + 4 - k) as usize] = v;
                    }
                    i += 1;
                }
            }
        }
    }
}
