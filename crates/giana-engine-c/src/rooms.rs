//! Treasure rooms (work package O3 of `docs/rom-hack-family-c.md`, 7.4): native ports of the room code of the main loop
//! (`$3FA1`, called while `$C8 != 0`), verified call by call, frame by frame and on random states against the original
//! running in the oracle machine (`crates/giana-c30-oracle/tests/pickups.rs`).
//!
//! # The room cycle
//!
//! A door (tile `$8F` at Giana's feet, [`crate::blocks::room_entry`]) sets `$C8 = $FF` and saves what the way back
//! needs. The main loop calls `$3FA1` several times a frame (after the status bar) while `$C8 != 0`:
//!
//! 1. **Entering** (`$C8 = $FF`, [`room_logic`] returns [`RoomPath::Entering`]): the logic freezes (`$20 = 1`); then, with
//!    the waits and builders of the stage start in between (package F: `$3F8C` waits for the block animations, `$23A6`
//!    turns the screen off, `$8C0E` builds the level, `$2852`, `$4152` the screen, `$35A1` Giana's start, `$23AE` the
//!    screen on): [`room_mark`] (`$C8 = $80`), [`room_select`] (the next room), [`room_hide`], [`room_place`] (Giana's
//!    sprites, the lift of three sprites 5..7 at the room's end marker, the stage number back).
//! 2. **Inside** (`$C8 = $80`): [`room_exit_check`]: Giana touching the lift from above starts the way back (`$02E0`).
//! 3. **Leaving** (`$02E0` set): per frame (after the wait `$4194` for the raster) [`room_rise`] lifts the lift and Giana 2
//!    pixels; at the top (or at once when `$98` is set, the stage ends) [`room_leave`] (`$C8 = 1`), then the builders
//!    again with [`room_scroll_back`] (`$67` back) between them, [`room_door`] (the door is closed: tiles `$52/$53`),
//!    [`room_back`] (sprites and VIC registers back, the input off, sound 8; a delay `$371E` follows) and [`room_done`]
//!    (the input on, `$C8 = 0`).
//!
//! The room counter `$CFFF` runs `$65..$69` (rooms 101..105) and wraps; the records of 102 (`$FD80`) and 105 (`$FDFC`)
//! are read from the room table like the others (see 2.7 for what they hold).

use crate::layout::C30aLayout;
use crate::player;

/// VIC registers the room code writes (hardware).
const VIC_PRIORITY: usize = 0x1B;
const VIC_MULTICOLOR: usize = 0x1C;
const VIC_EXTRA_COLOURS: usize = 0x2B;

/// Where [`room_logic`] goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomPath {
    /// `$C8 = $FF`: the room is being entered (`$20 = 1`, `$33 = 0` done); next the wait `$3F8C` (`$3FB9`)
    Entering,
    /// `$C8 = $80`: inside the room; next [`room_exit_check`] (`$4057`)
    Inside,
    /// `$02E0` set: the lift rises; next the wait `$4194` (`$409E`), then [`room_rise`]
    Rising,
    /// `$02E0` and `$98` set: [`room_leave`] done; next the wait `$3F8C` (`$40C3`)
    Leaving,
}

/// `$3FA1..$3FB8` (and `$409A..$409D`): which part of the room cycle runs (see the module documentation). The exit
/// check of [`RoomPath::Inside`] is the code at `$4057` ([`room_exit_check`]), whose first instruction reads `$D01E`.
pub fn room_logic(ram: &mut [u8], l: &C30aLayout) -> RoomPath {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    if r(ram, l.state_02e0) != 0 {
        if r(ram, l.stage_end) == 0 {
            return RoomPath::Rising;
        }
        room_leave(ram, l);
        return RoomPath::Leaving;
    }
    if r(ram, l.in_room) != 0xFF {
        return RoomPath::Inside;
    }
    ram[l.game_state as usize] = 1;
    ram[l.speed as usize] = 0;
    RoomPath::Entering
}

/// `$4057`: in the room, `coll` = the byte read from `$D01E`. When Giana touches one of the lift's sprites (5..7) and is
/// over it (`lift - Giana` within `-60..13` pixels horizontally, `-13..18` vertically), her vertical movement stops; from
/// 9 pixels above it on she stands on it and the way back starts: her y on the lift, the standing frame of her direction
/// (`$2BCB`), `$33 = 0`, `$02E0 = $FF`, `$20 = 1`.
pub fn room_exit_check(ram: &mut [u8], l: &C30aLayout, coll: u8) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    if coll & 0xE0 == 0 {
        return;
    }
    let minus = |a: u8, m: u8| a.wrapping_sub(m) & 0x80 != 0;
    let lift_x = r(ram, l.player_x + 5);
    let lift_y = r(ram, l.player_y + 5);
    let dx = lift_x.wrapping_sub(r(ram, l.player_x));
    if !minus(dx, 0x0E) || minus(dx, 0xC4) {
        return;
    }
    let dy = lift_y.wrapping_sub(r(ram, l.player_y));
    if !minus(dy, 0x13) || minus(dy, 0xF3) {
        return;
    }
    if !minus(dy, 9) {
        ram[l.player_y as usize] = lift_y.wrapping_sub(0x12);
        // `$2BCB`: the standing frame of her direction
        ram[l.anim_index as usize] = 0xFF;
        let frame = if r(ram, l.facing) == 0 { 0xC3 } else { 0xE4 };
        player::set_frames(ram, l, frame);
        ram[l.speed as usize] = 0;
        ram[l.state_02e0 as usize] = 0xFF;
        ram[l.game_state as usize] = 1;
    }
    ram[l.vert_dir as usize] = 0;
    ram[l.bounce as usize] = 0;
}

/// `$40A1..$40B3`: one frame of the way back: the lift (sprites 5..7) and Giana on it rise 2 pixels; at the top (Giana's
/// y below `$40`) [`room_leave`] and `true`.
pub fn room_rise(ram: &mut [u8], l: &C30aLayout) -> bool {
    let y = ram[(l.player_y + 5) as usize];
    let (v, c) = (y.wrapping_sub(2), y >= 2);
    for k in 5..8 {
        ram[(l.player_y + k) as usize] = v;
    }
    let g = v.wrapping_sub(0x12).wrapping_sub(u8::from(!c));
    ram[l.player_y as usize] = g;
    if g >= 0x40 {
        return false;
    }
    room_leave(ram, l);
    true
}

/// `$40B4..$40C2`: the room is left: `$C8 = 1`, all sprites off, `$02E0`, `$33`, `$14` cleared.
pub fn room_leave(ram: &mut [u8], l: &C30aLayout) {
    ram[l.in_room as usize] = 1;
    for a in [l.sprite_enable, l.state_02e0, l.speed, l.scroll_step] {
        ram[a as usize] = 0;
    }
}

/// `$3FBC`: the room is being built (`$C8 = $80`).
pub fn room_mark(ram: &mut [u8], l: &C30aLayout) {
    ram[l.in_room as usize] = 0x80;
}

/// `$3FC3..$3FD5`: the next room: the counter `$CFFF` counts up through `$65..$69` (rooms 101..105, wrapping, and from
/// any other value to `$65`), and the stage number `$FC` is the room's while the room is built.
pub fn room_select(ram: &mut [u8], l: &C30aLayout) {
    let x = ram[l.room_counter as usize].wrapping_add(1);
    let x = if (0x65..0x6A).contains(&x) { x } else { 0x65 };
    ram[l.room_counter as usize] = x;
    ram[l.stage as usize] = x;
}

/// `$3FDC..$3FE3`: the room's fine scroll is 7 and all sprites are off while it is drawn.
pub fn room_hide(ram: &mut [u8], l: &C30aLayout) {
    ram[l.fine_scroll as usize] = 7;
    ram[l.sprite_enable as usize] = 0;
}

/// `$3FEC..$404F`: after Giana's start position (`$35A1`): the lift: three sprites (5..7, frames `$91..$93`, 24 pixels
/// apart, hi-res, in front, colour `$E`) at the room's end marker (header bytes 14 and 15: x = column * 8 + 14, y from
/// the row table `$4C19` minus 9), sprites 0..2 and 5..7 on; the stage number comes back from `$0129`.
pub fn room_place(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    for k in 0..3u16 {
        ram[(l.player_frames + 5 + k) as usize] = 0x91 + k as u8;
    }
    vic[VIC_PRIORITY] &= 7;
    vic[VIC_MULTICOLOR] &= 7;
    for k in 1..4 {
        vic[VIC_EXTRA_COLOURS + k] = 0x0E;
    }
    // `ADC #1`, three `ASL`: the carry out of the last one is the first sprite's x high bit (`PHP`/`PLP` around `ADC #6`)
    let t = ram[(l.stage_header + 14) as usize].wrapping_add(1);
    let mut c = t & 0x20 != 0;
    let mut a = (t << 3).wrapping_add(6);
    for k in 0..3u16 {
        if c {
            let bits = ram[(l.room_lift_msb + 2 - k) as usize];
            ram[l.sprite_x_msb as usize] |= bits;
        }
        ram[(l.player_x + 5 + k) as usize] = a;
        let s = u16::from(a) + 0x18;
        (a, c) = (s as u8, s > 0xFF);
    }
    let row = ram[(l.stage_header + 15) as usize];
    let y = ram[l.spawn_y_table.wrapping_add(u16::from(row)) as usize].wrapping_sub(9);
    for k in 5..8 {
        ram[(l.player_y + k) as usize] = y;
    }
    ram[l.sprite_enable as usize] = 0xE7;
    ram[l.stage as usize] = ram[l.room_return as usize];
}

/// `$40C9..$40CD`: the stage's fine scroll comes back (`$012A`).
pub fn room_scroll_back(ram: &mut [u8], l: &C30aLayout) {
    ram[l.fine_scroll as usize] = ram[(l.room_return + 1) as usize];
}

/// `$40DD..$412B`: the door Giana came through is closed: in the screen row saved on entering (`$027F/$0280`, the
/// probe's row-4 pointer) the run of door tiles `$8F` at or next to the probe column `$0200` (searched leftwards from two
/// columns right of it, then rightwards to column 39; else the probe column itself) becomes the closed door, tiles
/// `$53`, `$52` alternating, with colour 8 in the colour RAM row (`color`, the 1 KB at `$D800`: the pointer + `$1000`).
pub fn room_door(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    let lo = ram[l.room_pointer as usize];
    let hi = ram[(l.room_pointer + 1) as usize];
    ram[l.print_ptr as usize] = lo;
    ram[l.text_ptr as usize] = lo;
    ram[(l.print_ptr + 1) as usize] = hi;
    ram[(l.text_ptr + 1) as usize] = hi | 0x10;
    let row = u16::from_le_bytes([lo, hi]);
    let colours = u16::from_le_bytes([lo, hi | 0x10]);
    let at = |ram: &[u8], y: u8| ram[row.wrapping_add(u16::from(y)) as usize];
    let col = ram[l.obj_behind as usize];
    // `$40ED`: leftwards from the column + 2 (while `Y` stays positive), then rightwards from the column to 39
    let mut found = None;
    let mut y = col.wrapping_add(2);
    loop {
        if at(ram, y) == 0x8F {
            found = Some(y);
            break;
        }
        y = y.wrapping_sub(1);
        if y & 0x80 != 0 {
            break;
        }
    }
    if found.is_none() {
        y = col;
        loop {
            if at(ram, y) == 0x8F {
                found = Some(y);
                break;
            }
            y = y.wrapping_add(1);
            if y >= 0x28 {
                break;
            }
        }
    }
    let mut y = match found {
        // `$410F`: back to the first door tile of the run
        Some(mut y) => loop {
            if at(ram, y) != 0x8F {
                break y;
            }
            y = y.wrapping_sub(1);
            if y & 0x80 != 0 {
                break y;
            }
        },
        None => col,
    };
    y = y.wrapping_add(1);
    let mut x = 0x52u8;
    loop {
        x ^= 1;
        ram[row.wrapping_add(u16::from(y)) as usize] = x;
        // the screen row + `$1000`: the colour RAM row (the I/O window is on); elsewhere RAM
        match colours.wrapping_add(u16::from(y)) {
            a @ 0xD800..=0xDBFF => color[usize::from(a - 0xD800)] = 8,
            a => ram[a as usize] = 8,
        }
        y = y.wrapping_add(1);
        if at(ram, y) != 0x8F {
            return;
        }
    }
}

/// `$412C..$4148` (with `$41F3`, `$41C7`, `$2BE8`): back in the stage: Giana's and the objects' sprite variables `$70..$88`,
/// the sprite priority, multicolour and colour registers as saved on entering, `$14..$1F` cleared, the input off (an `RTS`
/// over `$8F95`), `$3D`, `$98`, `$20`, `$32` cleared, sound 8, `$31 = 1`. Returns `X`, `Y` for the delay `$371E` that
/// follows (`$FF`, `$C0`).
pub fn room_back(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) -> (u8, u8) {
    room_back_restore(ram, vic, l);
    room_back_flags(ram, l)
}

/// `$412C..$4134`, the first part of [`room_back`]: the sprite variables and VIC registers back, `$14..$1F` cleared,
/// the input off.
pub fn room_back_restore(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    let (src, dst) = (l.room_sprites as usize, l.player_x as usize);
    ram.copy_within(src..src + 0x19, dst);
    let v = l.room_vic as usize;
    vic[VIC_PRIORITY] = ram[v];
    vic[VIC_MULTICOLOR] = ram[v + 1];
    for k in 0..4 {
        vic[VIC_EXTRA_COLOURS + k] = ram[v + 2 + k];
    }
    clear_motion(ram, l);
    ram[l.read_input as usize] = 0x60;
}

/// `$4135..$4148`, the second part of [`room_back`]: `$3D`, `$98`, `$20`, `$32` cleared, sound 8, `$31 = 1`; returns
/// `X`, `Y` for the delay.
pub fn room_back_flags(ram: &mut [u8], l: &C30aLayout) -> (u8, u8) {
    for a in [l.probe_tiles + 4, l.stage_end, l.game_state, l.fire] {
        ram[a as usize] = 0;
    }
    ram[l.effect as usize] = 8;
    ram[l.joy as usize] = 1;
    (0xFF, 0xC0)
}

/// `$41C7`: Giana's movement state `$14..$1F` cleared (also from `$35A1`, the start position).
pub fn clear_motion(ram: &mut [u8], l: &C30aLayout) {
    let a = l.scroll_step as usize;
    ram[a..a + 12].fill(0);
}

/// `$414C..$4150`: after the delay: the input on again (`LDA #$A9` back at `$8F95`), `$C8 = y` (0: the delay loop counted
/// `Y` down).
pub fn room_done(ram: &mut [u8], l: &C30aLayout, y: u8) {
    ram[l.read_input as usize] = 0xA9;
    ram[l.in_room as usize] = y;
}
