//! The pieces of the game flow that run without waiting (package F, `docs/rom-hack-family-c.md` 7.9): the stage start
//! (state reset, stage setup with the per-stage patches and character swaps, the level buffer, the first screen,
//! Giana's start), Giana's power-up sprites, and the screen helpers of the menus. [`crate::flow`] runs them in the
//! original's order and does the waits between them. Each function is verified call by call against the original
//! (`crates/giana-c30-oracle/tests/flow.rs`).

use crate::frame::Core;
use crate::layout::C30aLayout;
use crate::text;

fn r(ram: &[u8], a: u16) -> u8 {
    ram[a as usize]
}

fn ptr(ram: &[u8], zp: u16) -> u16 {
    u16::from_le_bytes([ram[zp as usize], ram[zp as usize + 1]])
}

fn set_ptr(ram: &mut [u8], zp: u16, v: u16) {
    ram[zp as usize..zp as usize + 2].copy_from_slice(&v.to_le_bytes());
}

/// `$2BE2`: the joystick read on again (its first byte back to `LDA #`).
pub fn input_on(ram: &mut [u8], l: &C30aLayout) {
    ram[l.read_input as usize] = 0xA9;
}

/// `$419C`: the state of a stage start: no block hit pending (`$0201`, `$0202`, `$D5` = `$FF`), `$9F` = `$C3`, the raster
/// IRQ off, black border and background, `$02D3..$02E0` and `$29..$99` and `$14..$1F` cleared, the SID volume 0.
pub fn reset_state(core: &mut Core) {
    let l = core.l;
    let ram = core.ram();
    for a in [l.head_bump, l.repeat_hits, l.effect] {
        ram[a as usize] = 0xFF;
    }
    ram[l.shimmer_ptr as usize + 1] = 0xC3;
    for a in l.end_bump..=l.end_bump + 0x0D {
        ram[a as usize] = 0;
    }
    for a in 0x29..=0x99usize {
        ram[a] = 0;
    }
    for a in l.scroll_step..=l.scroll_step + 0x0B {
        ram[a as usize] = 0;
    }
    core.vic[0x1A] = 0;
    core.vic[0x20] = 0;
    core.vic[0x21] = 0;
    core.music.driver.sid[0x18] = 0;
}

/// `$2805`: the object tables `$0204..$027F` and `$02A8..$02D2` cleared.
pub fn clear_objects(ram: &mut [u8], l: &C30aLayout) {
    for a in l.obj_flags..=l.obj_flags + 0x7B {
        ram[a as usize] = 0;
    }
    for a in l.group_members + 4..=l.group_members + 4 + 0x2A {
        ram[a as usize] = 0;
    }
}

/// `$2818..$2842`: the play IRQ chain: `$01 = $35`, the `$FB` handler's next IRQ (line `$41`, `$3488`), the NMI vector
/// (`$1D10`: RESTORE restarts the game), the `$FB` handler at line `$FC`. (The screen-off wait and the enable follow.)
pub fn play_irq(core: &mut Core) {
    let l = core.l;
    let ram = core.ram();
    ram[1] = 0x35;
    ram[l.irq_fb_line_op as usize] = 0x41;
    ram[l.irq_fb_vector_lo_op as usize] = l.irq_41 as u8;
    ram[l.irq_fb_vector_hi_op as usize] = (l.irq_41 >> 8) as u8;
    set_ptr(ram, l.nmi_vector, l.game_start);
    set_ptr(ram, l.irq_vector, l.irq_fb);
    core.vic[0x12] = 0xFC;
}

/// `$8C0E`: the level buffer `$0400..$17FF` cleared, the stage setup ([`stage_setup`]), then the map commands of the
/// stage record ([`build_map`]). Returns the banner printer's end (`true`: it printed, the screen-on wait follows).
pub fn level(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> bool {
    level_clear(ram, l);
    level_build(ram, color, l)
}

/// `$8C0E..$8C22`, the first part of [`level`]: the level buffer cleared (20 pages, [`LEVEL_CLEAR_CYCLES`]).
pub fn level_clear(ram: &mut [u8], l: &C30aLayout) {
    for a in l.level..l.level + 0x1400 {
        ram[a as usize] = 0;
    }
    ram[l.print_ptr as usize] = 0;
    ram[l.print_ptr as usize + 1] = (l.level >> 8) as u8 + 0x14;
}

/// `$8C23..`, the rest of [`level`]: the stage setup and the map commands; returns whether the banner was printed.
pub fn level_build(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> bool {
    let printed = stage_setup(ram, color, l);
    build_map(ram, l);
    printed
}

/// CPU cycles of [`level_clear`] (`$8C19`: 11 cycles a byte, 20 pages).
pub const LEVEL_CLEAR_CYCLES: i64 = 20 * 256 * 11 + 20 * 10;

/// `$49B8..$4BF1`: the setup of stage `$FC`: the highest stage reached (`$520F`, stages below `$0E`), the code and
/// table patches of stages 5, 10, 12, 13 and 14, the stage header (16 bytes to `$0100`), the banner (`$0100` first byte
/// = column, row 16; not in a treasure room), the tile colours of the header's tile set, and the character swaps chosen
/// by the header's flag bits (`$0109`: bit 6, 4, 7, 5; `$010A`: bits 7, 6, 5). Leaves the record pointer `$02/$03` at the
/// map commands. Returns whether the banner was printed.
pub fn stage_setup(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> bool {
    let stage = r(ram, l.stage);
    let mut a = stage;
    if a >= 0x65 {
        if r(ram, l.select_cursor) != 0 {
            ram[l.stage as usize] = 0x65;
        }
    } else if a < 0x0E && a >= r(ram, l.highest_stage) {
        ram[l.highest_stage as usize] = a;
    }
    let mut x: u8;
    if a == 0x0C {
        x = 7;
        ram[l.tile_anim_cursor as usize] = 6;
        ram[l.tile_anim_last as usize] = 6;
        ram[l.tile_anim_wrap as usize] = 0xD0;
    } else {
        let v;
        if a == 5 || a == 0x0A {
            x = 0x0F;
            v = 0xAA;
        } else {
            x = 3;
            v = 0xFF;
        }
        ram[l.tile_anim_last as usize] = 5;
        for y in 0..8u16 {
            ram[(l.water_chars + y) as usize] = v;
        }
        ram[l.tile_anim_wrap as usize] = 0x10;
    }
    for y in (0..4u16).rev() {
        ram[(l.tile_colours + 0xD0 + y) as usize] = r(ram, l.stage_tiles + x as u16);
        ram[(l.tile_props + 0xD0 + y) as usize] = r(ram, l.stage_tiles + 0x10 + x as u16);
        x = x.wrapping_sub(1);
    }
    a = r(ram, l.stage);
    let mut y = 6;
    for x in (0..2u16).rev() {
        if a == r(ram, l.stage_tiles - 0x36 + x) {
            y = r(ram, l.stage_tiles - 0x34 + x);
            break;
        }
    }
    ram[(l.tile_colours + 0x8F) as usize] = y;
    ram[l.boss_flag as usize] = if a == 0x0D { 0xFF } else { 0 };
    let v = if a == 0x0E { 1 } else { 0 };
    for x in [8u16, 6, 4, 2, 0] {
        ram[(l.tile_props + 0xDB + x) as usize] = v;
    }
    let mut printed = false;
    let p = l.pointer2;
    if a >= 0x64 {
        let y = (a.wrapping_sub(0x64) as u16) << 1;
        let rec = u16::from_le_bytes([r(ram, l.room_table + y), r(ram, l.room_table + y + 1)]);
        set_ptr(ram, p, rec);
        header(ram, l);
    } else {
        let y = (a as u16) << 1 & 0xFF;
        let rec = u16::from_le_bytes([r(ram, l.stage_table + y), r(ram, l.stage_table + y + 1)]);
        set_ptr(ram, p, rec);
        header(ram, l);
        let rec = ptr(ram, p);
        let mut y: u8;
        if r(ram, l.in_room) != 0 {
            y = 0;
            while r(ram, rec.wrapping_add(y as u16)) != 0xFF {
                y = y.wrapping_add(1);
            }
        } else {
            let col = r(ram, rec);
            ram[l.print_col as usize] = col;
            ram[l.print_col as usize + 1] = 0x10;
            let rec = rec.wrapping_add(1);
            set_ptr(ram, p, rec);
            set_ptr(ram, l.print_ptr, rec);
            y = text::print_from_ptr(ram, color, l);
            printed = true;
        }
        let rec = ptr(ram, p).wrapping_add(y as u16 + 1);
        set_ptr(ram, p, rec);
    }
    // `$4AC3`: the tile set's colours
    let ts = r(ram, l.stage_header + 1) as u16;
    ram[(l.tile_colours + 0xA6) as usize] = r(ram, l.tileset_colours + ts);
    let c = r(ram, l.tileset_colours + 9 + ts);
    ram[(l.tile_colours + 0xA7) as usize] = c;
    ram[(l.tile_colours + 0xA8) as usize] = c;
    ram[(l.tile_colours + 0xA9) as usize] = r(ram, l.tileset_colours + 18 + ts);
    let flags = r(ram, l.stage_header + 9);
    ram[l.palette_patch as usize] = if flags & 0x0F == 0 { 6 } else { 0 };
    // `$4AEA`: flag bit 6
    let marker = r(ram, l.tile_props + 0x7F);
    let set = if flags & 0x40 != 0 { (marker != 0).then_some(0) } else { (marker == 0).then_some(1) };
    if let Some(v) = set {
        for x in 0..4u16 {
            ram[(l.tile_props + 0x7F + x) as usize] = v;
        }
        swap(ram, l.swap_a_live, l.swap_a_store, 0x20);
    }
    // `$4B19`: flag bit 4
    let t = r(ram, l.tile_colours + 0xAA);
    if (flags & 0x10 != 0 && t != 5) || (flags & 0x10 == 0 && t != 6) {
        swap(ram, l.tile_colours + 0xAA, 0xFFBF, 0x0E);
        swap1(ram, l.swap_b_live, l.swap_b_store, 0x70);
    }
    // `$4B54`: flag bit 7
    let t = r(ram, l.tile_colours + 0xB8);
    if (flags & 0x80 != 0 && t != 0x0A) || (flags & 0x80 == 0 && t != 1) {
        swap(ram, l.tile_colours + 0xB8, 0xFFCD, 0x16);
        swap1(ram, l.swap_c_live, l.swap_c_store, 0xB0);
    }
    let mc = r(ram, l.band_colours);
    ram[(l.tile_colours + 0xCF) as usize] = if mc & 0x0F == 8 { 0x0B } else { 0x0F };
    // `$4B9D`: flag bit 5
    let t = r(ram, l.swap_d_live + 2);
    if (flags & 0x20 != 0 && t & 0x80 != 0) || (flags & 0x20 == 0 && t & 0x80 == 0) {
        swap1(ram, l.swap_d_live, l.swap_d_store, 0x50);
    }
    // `$4BC2`: `$010A` bits 7, 6, 5
    ram[(l.tile_colours + 0xD9) as usize] = if mc & 0x80 != 0 { 0 } else { 6 };
    let v = if mc & 0x40 != 0 { 0xFF } else { 0 };
    for x in 0..8u16 {
        ram[(l.water_chars + 0x30 + x) as usize] = v;
    }
    let v = if mc & 0x20 != 0 { 0 } else { 0xFF };
    for x in 0..8u16 {
        ram[(l.water_chars + 0x128 + x) as usize] = v;
    }
    printed
}

/// `n` bytes from `a` swapped with `n` bytes from `b` (`Y = n - 1 .. 0`).
fn swap(ram: &mut [u8], a: u16, b: u16, n: u16) {
    for y in (0..n).rev() {
        ram.swap((a + y) as usize, (b + y) as usize);
    }
}

/// The swaps that run `Y = n .. 1` (`BNE`): the bytes `a + 1 ..= a + n` with `b + 1 ..= b + n`.
fn swap1(ram: &mut [u8], a: u16, b: u16, n: u16) {
    for y in (1..=n).rev() {
        ram.swap((a + y) as usize, (b + y) as usize);
    }
}

/// `$4BF2`: the 16 bytes at `$02/$03` to the stage header `$0100`, the pointer moves past them.
fn header(ram: &mut [u8], l: &C30aLayout) {
    let p = ptr(ram, l.pointer2);
    for y in 0..16u16 {
        ram[(l.stage_header + y) as usize] = r(ram, p.wrapping_add(y));
    }
    set_ptr(ram, l.pointer2, p.wrapping_add(16));
}

/// `$8C26..$8D19`: the map commands at `$02/$03` into the level buffer (the shapes of the table at `$8757`, repeated in
/// x and y; cell value `$1C` is transparent), up to the `$FF` that ends them; `$9A/$9B` then point at the spawn list
/// after it. The zero-page cells `$04..$11` are left as the original leaves them.
pub fn build_map(ram: &mut [u8], l: &C30aLayout) {
    let p = l.pointer2;
    loop {
        let at = ptr(ram, p);
        let cmd = r(ram, at);
        let bytes: [u8; 5] = std::array::from_fn(|i| r(ram, at.wrapping_add(i as u16)));
        let b = |i: u16| bytes[i as usize];
        let len;
        let x = cmd.wrapping_shl(1);
        if cmd & 0x80 != 0 {
            if cmd == 0xFF {
                set_ptr(ram, l.spawn_list, at.wrapping_add(1));
                return;
            }
            let (b1, b2) = (b(1), b(2));
            ram[0x10] = b1;
            ram[0x06] = (b2 >> 6) + 1;
            ram[0x0F] = 1 + ((b2 >> 5) & 1);
            let page = (b2 & 0x1F) + 4;
            ram[0x05] = page;
            ram[0x0E] = page;
            len = 3;
        } else {
            let (b1, b2) = (b(1), b(2));
            ram[0x10] = b1;
            if b2 >= 0x14 {
                let page = (b2 & 0x1F) + 4;
                ram[0x05] = page;
                ram[0x0E] = page;
                ram[0x0F] = b2 >> 5;
                ram[0x06] = b(3);
                len = 4;
            } else {
                let page = b2 + 4;
                ram[0x05] = page;
                ram[0x0E] = page;
                ram[0x06] = b(3);
                ram[0x0F] = b(4);
                len = 5;
            }
        }
        set_ptr(ram, p, at.wrapping_add(len));
        let shape = u16::from_le_bytes([r(ram, l.shape_table + x as u16), r(ram, l.shape_table + x as u16 + 1)]);
        let w = r(ram, shape);
        let h = r(ram, shape.wrapping_add(1));
        ram[0x0A] = w;
        ram[0x0B] = h;
        ram[0x11] = h;
        set_ptr(ram, 0x08, shape.wrapping_add(2));
        loop {
            // `$8CCC`: one shape row, `$0F` times in y
            ram[0x07] = ram[0x0F];
            loop {
                let page = ram[0x05];
                if !(4..0x18).contains(&page) {
                    break;
                }
                ram[0x04] = ram[0x10];
                let mut xr = ram[0x06];
                loop {
                    let src = ptr(ram, 0x08);
                    let dst = ptr(ram, 0x04);
                    let mut y = w as i16;
                    while y >= 0 {
                        let v = r(ram, src.wrapping_add(y as u16));
                        if v != 0x1C {
                            ram[dst.wrapping_add(y as u16) as usize] = v;
                        }
                        y -= 1;
                    }
                    ram[0x04] = ram[0x04].wrapping_add(w).wrapping_add(1);
                    xr = xr.wrapping_sub(1);
                    if xr == 0 {
                        break;
                    }
                }
                ram[0x05] = ram[0x05].wrapping_add(ram[0x0B]).wrapping_add(1);
                ram[0x07] = ram[0x07].wrapping_sub(1);
                if ram[0x07] == 0 {
                    break;
                }
            }
            ram[0x0E] = ram[0x0E].wrapping_add(1);
            ram[0x05] = ram[0x0E];
            let s = ptr(ram, 0x08).wrapping_add(w as u16 + 1);
            set_ptr(ram, 0x08, s);
            ram[0x11] = ram[0x11].wrapping_sub(1);
            if ram[0x11] & 0x80 != 0 {
                break;
            }
        }
    }
}

/// `$260B`: the playfield (screen rows 5-24) drawn from the level buffer at the restart column `$CBF3`, with the tile
/// colours `$BF00`; `$67 = 7`, `$F8 = 0`. Leaves `$08`, `$E6..$EB` as the original.
pub fn draw_playfield(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    ram[l.fine_scroll as usize] = 7;
    ram[l.scroll_px as usize] = 0;
    let col = r(ram, l.restart_column);
    draw_playfield_at(ram, color, l, col);
}

/// `$2616`: the playfield from level column `col` (the high-score screen enters here with 0).
pub fn draw_playfield_at(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, col: u8) {
    let zp = l.obj_probe_ptr2 as usize; // `$E6..$EB`
    ram[zp] = col;
    ram[zp + 1] = 0x17;
    for x in (0..0x14u16).rev() {
        ram[0x08] = x as u8;
        let lo = r(ram, l.row_lo + 6 + x);
        let hi = r(ram, l.row_hi + 6 + x);
        ram[zp + 2] = lo;
        ram[zp + 4] = lo;
        ram[zp + 3] = hi;
        ram[zp + 5] = hi | 0x10;
        let src = u16::from_le_bytes([ram[zp], ram[zp + 1]]);
        let dst = u16::from_le_bytes([lo, hi]);
        let cdst = u16::from_le_bytes([lo, hi | 0x10]);
        for y in (0..0x28u16).rev() {
            let t = r(ram, src.wrapping_add(y));
            text::put(ram, color, dst.wrapping_add(y), t);
            let c = r(ram, l.tile_colours + t as u16);
            text::put(ram, color, cdst.wrapping_add(y), c);
        }
        ram[zp + 1] = ram[zp + 1].wrapping_sub(1);
    }
}

/// `$2852`: the background colour from the header (`$0109` -> `$D021`), Giana's second colour (`$3608`: 0 on
/// background 6, else 6), the cloud rows 2-4 cleared and, with a cloud colour (`$0108`), coloured and drawn
/// ([`crate::scroll::clouds_left`]).
pub fn cloud_band(core: &mut Core) {
    let l = core.l;
    let bg = core.peek(l.stage_header + 9);
    core.vic[0x21] = bg;
    core.poke(l.colour_table, if bg & 0x0F == 6 { 0 } else { 6 });
    crate::flow::cloud_rows(core);
}

/// `$359A`: Giana at the start (or restart column's) position of the header (`$010C/$010D`, or `$010E/$010F` after
/// the middle), frames `$C3`, the sprite colours. Up to the frame wait `$35BD`.
pub fn giana_start(core: &mut Core) {
    let y = if core.peek(core.l.restart_column) != 0 { 2 } else { 0 };
    giana_start_at(core, y);
}

/// `$35A1` with `Y` = `y` (0: the header's start position, 2: the restart position; a treasure room enters with 0).
pub fn giana_start_at(core: &mut Core, y: u16) {
    let l = core.l;
    for a in l.scroll_step..=l.scroll_step + 0x0B {
        core.poke(a, 0);
    }
    crate::player::set_frames(core.ram(), l, 0xC3);
    let x = core.peek(l.stage_header + 0x0C + y).wrapping_add(5);
    core.poke(l.player_x, x);
    core.poke(l.object_sprite_x, x);
    let yy = core.peek(l.stage_header + 0x0D + y);
    core.poke(l.player_y, yy);
    core.poke(l.object_sprite_y, yy);
    core.vic[0x25] = 0x0A;
    core.vic[0x26] = 2;
    giana_colours(core);
}

/// `$35F3`: Giana's colours of the current player (`$3606`, `$3608`), `$D028 = 1`.
pub fn giana_colours(core: &mut Core) {
    let l = core.l;
    let p = core.peek(l.player_number) as u16;
    let c0 = core.peek(l.colour_table - 2 + p);
    core.poke(l.layer0_colour, c0);
    let c1 = core.peek(l.colour_table + p);
    core.poke(l.sprite_color, c1);
    core.vic[0x28] = 1;
}

/// `$35C0..$35E1`, after the frame wait: sprites 0..2 on, and the objects of the 39 columns from the restart column
/// spawned (not in a treasure room).
///
/// The original spawns them with IRQs on, so the object update of a `$FB` IRQ can see a half-spawned object (about one
/// spawn step in 1,000): the native game spawns them all before the next object update (decision of package F).
pub fn giana_start_spawn(core: &mut Core) {
    let l = core.l;
    core.poke(l.sprite_x_msb, 0);
    core.poke(l.sprite_enable, 7);
    if core.peek(l.in_room) == 0 {
        core.poke(l.spawn_mode, 0xFF);
        let col = core.peek(l.restart_column);
        core.poke(l.scroll_column, col);
        core.poke(0x08, 0x26);
        loop {
            let (ram, _, vic) = core.parts();
            crate::objects::spawn(ram, vic, l);
            let c = core.peek(l.scroll_column).wrapping_add(1);
            core.poke(l.scroll_column, c);
            let n = core.peek(0x08).wrapping_sub(1);
            core.poke(0x08, n);
            if n & 0x80 != 0 {
                break;
            }
        }
    }
}

/// `$35E2..$35E8`: the end of the stage start: `$F6 = 0`, `$20 = 0` (the logic runs).
pub fn giana_start_end(core: &mut Core) {
    let l = core.l;
    core.poke(l.spawn_mode, 0);
    core.poke(l.game_state, 0);
}

/// `$365D..$3675`: the power-up sprite set: 64-byte blocks of Giana's shapes at `$ED00` swapped with the stored set at
/// `$B5C0` (63 bytes each; byte 63 of the stored block ends the list when its bit 7 is set). Leaves `$02..$05` as the
/// original.
pub fn sprite_swap(ram: &mut [u8], l: &C30aLayout) -> u32 {
    let (mut a, mut b) = (l.giana_shapes, l.power_shapes);
    let mut blocks = 0;
    loop {
        let flag = r(ram, b + 0x3F);
        for y in (0..0x3Fu16).rev() {
            ram.swap((a + y) as usize, (b + y) as usize);
        }
        a += 0x40;
        b += 0x40;
        blocks += 1;
        if flag & 0x80 != 0 {
            break;
        }
    }
    set_ptr(ram, l.pointer2, a);
    set_ptr(ram, l.print_ptr, b);
    blocks
}

/// `$3676..$36B4`: the left-facing shapes: each 64-byte block at `$ED00` mirrored into `$F540` (21 rows of 3 bytes in
/// reverse order; byte 63's bit 0 says hi-res: bits reversed, else multicolour pairs reversed; its bit 7 ends the list).
/// Leaves `$02..$05` and `$DE..$E0` as the original.
pub fn sprite_mirror(ram: &mut [u8], l: &C30aLayout) -> u32 {
    let (mut a, mut b) = (l.giana_shapes, l.giana_shapes_left);
    let mut blocks = 0;
    let mut last;
    loop {
        let flag = r(ram, a + 0x3F);
        let hires = flag & 1 != 0;
        let rev = |v: u8| if hires { v.reverse_bits() } else { (0..4).fold(0u8, |acc, k| acc | ((v >> (2 * k)) & 3) << (6 - 2 * k)) };
        let mut y = 0x3Eu16;
        loop {
            let row = [rev(r(ram, a + y)), rev(r(ram, a + y - 1)), rev(r(ram, a + y - 2))];
            ram[(b + y - 2) as usize] = row[0];
            ram[(b + y - 1) as usize] = row[1];
            ram[(b + y) as usize] = row[2];
            last = row;
            if y < 3 {
                break;
            }
            y -= 3;
        }
        a += 0x40;
        b += 0x40;
        blocks += 1;
        if flag & 0x80 != 0 {
            break;
        }
    }
    ram[l.mirror_bytes as usize..l.mirror_bytes as usize + 3].copy_from_slice(&last);
    set_ptr(ram, l.pointer2, a);
    set_ptr(ram, l.print_ptr, b);
    blocks
}

/// `$4152..$4193`: the playfield rows 5..24 of the screen swapped with the first 40 columns of the level buffer's 20
/// rows (one page each from `$0400`); each cell that comes onto the screen gets its colour from the tile colours
/// `$BF00`. The treasure rooms use it both ways: going in, the stage's screen is kept in the buffer; going out, it comes
/// back. Leaves `$04..$09` and the row counter `$EA` (`obj_probe_ptr2 + 4`) as the original.
pub fn screen_swap(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    let screen = l.screen as usize + 200;
    for row in 0..20usize {
        for col in 0..40usize {
            let (b, s) = (l.level as usize + row * 256 + col, screen + row * 40 + col);
            let tile = ram[b];
            ram.swap(b, s);
            color[200 + row * 40 + col] = ram[l.tile_colours as usize + tile as usize] & 15;
        }
    }
    let end = (screen + 800) as u16;
    set_ptr(ram, l.print_ptr, l.level + 20 * 256);
    set_ptr(ram, l.text_ptr, end);
    set_ptr(ram, l.text_ptr + 2, end.wrapping_add(0x1000));
    ram[l.obj_probe_ptr2 as usize + 4] = 0xFF;
}
