//! Blocks, diamonds, score, time and lives (work package B of `docs/rom-hack-family-c.md`, 7.4): native ports of the
//! preview's block code, each verified call by call and frame by frame against the original code running in the oracle
//! machine (`crates/giana-c30-oracle/tests/blocks.rs`).
//!
//! Same state model as [`crate::player`]: the routines work on the game's own 64 KB memory image (`ram`) at the addresses
//! of [`C30aLayout`] and read their tables from it. The block code also draws into colour RAM (`color`, the 1 KB at
//! `$D800`, low nibbles) and, when Giana enters a treasure room, saves some VIC registers (`vic`, the 64 registers of
//! `$D000` as written).
//!
//! Ported so far:
//!
//! | original | function | what |
//! |---|---|---|
//! | `$3B80..$3C1A` | [`block_hits`] | the stage-end block, treasure-room doors, dissolving ground under the feet, then [`pickups_and_bumps`] |
//! | `$3C1B..$3E91` (with `$392A..$3968`) | [`pickups_and_bumps`] | diamonds at the sides, blocks hit from below by the head (bonus blocks, bricks, the item and the repeated-hit block) |
//! | `$3E92..$3EC0` | [`time_tick`] | the time: one BCD second every 41 frames, time up, the repeated-hit block's counter |
//! | `$8BDB..$8C02` (and `$8BD9`) | [`score_add`] | BCD score add with saturation at 999999 |
//! | `$8C03..$8C0D` | [`extra_life`] | one life more (BCD) |
//! | `$3F58..$3F8B`, `$41D1..$41F2` | [`room_entry`] | entering a treasure room through a door: what the way back needs |
//! | `$26E8..$27A6` (with `$2377..$23A5`, `$2671..$26CD`, `$7A3A`) | [`hud`] | the status bar (main loop): item icon, lives, score, diamonds, time |
//!
//! Per frame the game runs [`block_hits`] after the tile probe and before [`time_tick`] (`$3137`); while the logic is
//! frozen (`$20 != 0`) with `$02E0` set, only the probe and [`pickups_and_bumps`] run (`$311D`). The block animations
//! the hits start (`$389D`, `$38F8`, `$3991`, `$3B3E`) run in the column step and are ported in [`crate::scroll`]; a
//! diamond under an item starts its fall with [`crate::scroll::start_fall_above`] (`$392A`).

use crate::layout::C30aLayout;

/// NMOS 6502 `ADC` in decimal mode (the result and the carry), also for operands that are not BCD.
pub fn adc_decimal(a: u8, m: u8, c: bool) -> (u8, bool) {
    let mut lo = (a & 15) as u16 + (m & 15) as u16 + c as u16;
    if lo > 9 {
        lo += 6;
    }
    let mut hi = (a >> 4) as u16 + (m >> 4) as u16 + (lo > 15) as u16;
    if hi > 9 {
        hi += 6;
    }
    (((hi << 4) | (lo & 15)) as u8, hi > 15)
}

/// NMOS 6502 `SBC` in decimal mode (the result and the carry = no borrow), also for operands that are not BCD.
pub fn sbc_decimal(a: u8, m: u8, c: bool) -> (u8, bool) {
    let bin = a as i16 - m as i16 - (!c) as i16;
    let mut lo = (a & 15) as i16 - (m & 15) as i16 - (!c) as i16;
    let mut hi = (a >> 4) as i16 - (m >> 4) as i16;
    if lo < 0 {
        lo -= 6;
        hi -= 1;
    }
    if hi < 0 {
        hi -= 6;
    }
    (((hi << 4) | (lo & 15)) as u8, bin >= 0)
}

/// `$8BDB`: adds the BCD amount `amount` to the score (`$CBF0..$CBF2`, low byte first); past 999999 the score stays at
/// 999999. (`$8BD9` is the same with 50 points.) Returns `A` as the original leaves it (the top byte; `Y` is 0).
pub fn score_add(ram: &mut [u8], l: &C30aLayout, amount: u8) -> u8 {
    let s = l.score as usize;
    let (b0, c) = adc_decimal(ram[s], amount, false);
    let (b1, c) = adc_decimal(ram[s + 1], 0, c);
    let (b2, c) = adc_decimal(ram[s + 2], 0, c);
    let bytes = if c { [0x99; 3] } else { [b0, b1, b2] };
    ram[s..s + 3].copy_from_slice(&bytes);
    bytes[2]
}

/// `$8C03`: one life more (`$FB`, BCD). Returns `A` as the original leaves it (8, the extra-life sound).
pub fn extra_life(ram: &mut [u8], l: &C30aLayout) -> u8 {
    ram[l.lives as usize] = adc_decimal(ram[l.lives as usize], 1, false).0;
    8
}

/// `$3E92`: one frame of the time. Every 41 frames (the counter `$C9` runs 40..0) a second comes off the BCD time
/// `$CA/$CB`; at 0 it stays and `$99 = $28` starts the "time up" death instead. On the frame the counter passes 20 and
/// on each second, the repeated-hit block's counter `$0202` (positive values) counts down by one.
pub fn time_tick(ram: &mut [u8], l: &C30aLayout) {
    let mut x = ram[l.time_frames as usize].wrapping_sub(1);
    let repeat = if x & 0x80 != 0 {
        x = 0x28;
        let (lo, c) = sbc_decimal(ram[l.time as usize], 1, true);
        let (hi, c) = sbc_decimal(ram[l.time as usize + 1], 0, c);
        if !c {
            ram[l.dying as usize] = x;
            return;
        }
        ram[l.time as usize + 1] = hi;
        ram[l.time as usize] = lo;
        true
    } else {
        x == 0x14
    };
    if repeat {
        let v = ram[l.repeat_hits as usize];
        if v & 0x80 == 0 && v != 0 {
            ram[l.repeat_hits as usize] = v - 1;
        }
    }
    ram[l.time_frames as usize] = x;
}

/// The memory the block code touches: RAM, colour RAM where the I/O window shows it, the VIC registers it saves.
struct Mem<'a> {
    ram: &'a mut [u8],
    color: &'a mut [u8],
    l: &'a C30aLayout,
}

impl Mem<'_> {
    fn r(&self, a: u16) -> u8 {
        self.ram[a as usize]
    }

    fn w(&mut self, a: u16, v: u8) {
        self.ram[a as usize] = v;
    }

    /// A 16-bit pointer, low byte first.
    fn ptr(&self, a: u16) -> u16 {
        u16::from_le_bytes([self.r(a), self.r(a + 1)])
    }

    fn set_ptr(&mut self, a: u16, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        self.w(a, lo);
        self.w(a + 1, hi);
    }

    /// A read through a pointer (`LDA (zp),Y`): colour RAM reads with the high nibble set, as the C64 gives it.
    fn get(&self, a: u16) -> u8 {
        match a {
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] | 0xF0,
            _ => self.r(a),
        }
    }

    fn put(&mut self, a: u16, v: u8) {
        match a {
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] = v & 15,
            // the pointers of the block code are always on the screen; nothing else of the I/O window is modelled
            _ => self.w(a, v),
        }
    }

    /// The screen pointer `$0E/$0F`.
    fn screen(&self) -> u16 {
        self.ptr(self.l.scratch_ptr)
    }

    fn set_screen(&mut self, v: u16) {
        self.set_ptr(self.l.scratch_ptr, v);
    }

    /// `$3E72`: the colour RAM pointer `$10/$11` of the screen pointer.
    fn colour_ptr(&mut self) -> u16 {
        let s = self.screen();
        let c = u16::from_le_bytes([s as u8, (s >> 8) as u8 | 0x10]);
        self.set_ptr(self.l.colour_ptr, c);
        c
    }

    /// `$3E36`: the used block (characters `$4C..$51`, colour 8) at the screen pointer, 3 x 2 cells.
    fn used_block(&mut self) {
        let (s, c) = (self.screen(), self.colour_ptr());
        for (i, o) in [0u16, 1, 2, 0x28, 0x29, 0x2A].into_iter().enumerate() {
            self.put(s.wrapping_add(o), 0x4C + i as u8);
            self.put(c.wrapping_add(o), 8);
        }
    }

    /// `$3E7D`: colour `a` into the 3 x 2 cells at the colour pointer.
    fn colour_block(&mut self, a: u8) {
        let c = self.ptr(self.l.colour_ptr);
        for o in [0u16, 1, 2, 0x28, 0x29, 0x2A] {
            self.put(c.wrapping_add(o), a);
        }
    }

    /// The highest free slot (state 0) of an animation table with `n` slots.
    fn free_slot(&self, states: u16, n: u16) -> Option<u16> {
        (0..n).rev().find(|&x| self.r(states + x) == 0)
    }

    /// Whether a new animation at the screen pointer would repeat the last one started (`count != 0` and the last start
    /// is at the pointer's low byte or one more).
    fn repeats(&self, count: u16, last: u16) -> bool {
        let lo = self.r(self.l.scratch_ptr);
        self.r(count) != 0 && (self.r(last) == lo || self.r(last).wrapping_sub(1) == lo)
    }

    /// The pointer `$0E` as an animation slot's address.
    fn store_slot(&mut self, lo: u16, hi: u16, x: u16) {
        let s = self.screen().to_le_bytes();
        self.w(lo + x, s[0]);
        self.w(hi + x, s[1]);
    }
}

/// `$3B80`: the block code of one frame, after the tile probe (`a` = the probe's middle cell `$3D`, which the caller
/// has just loaded). In this order:
/// - the stage-end block (tile `$F8`): touched standing on it or hit from below while rising: freezes the logic
///   (`$20 = 1`, only when standing) and bumps it once (`$02D3`);
/// - the frame counter of the time is cut to 3 when Giana stands in tile property `$11`;
/// - a treasure-room door (tile `$8F`) at her feet: [`room_entry`] and nothing else;
/// - ground that dissolves (property bit 3) under the feet starts a `$3B3E` animation (not in a room with `$C8 = 1`);
/// - then [`pickups_and_bumps`].
///
/// `vic` = the VIC registers (`$D000..$D03F` as written); they are only read, when a door is entered.
pub fn block_hits(ram: &mut [u8], color: &mut [u8], vic: &[u8], l: &C30aLayout, a: u8) {
    let mut m = Mem { ram, color, l };
    let vdir = m.r(l.vert_dir);
    let end = if a == 0xF8 && vdir == 0 {
        m.w(l.game_state, 1);
        true
    } else if m.r(l.probe_tiles + 1) == 0xF8 {
        if vdir != 1 {
            return;
        }
        true
    } else {
        false
    };
    if end {
        if m.r(l.end_bump) != 0 {
            return;
        }
        m.w(l.end_bump, 0xFF);
        let p = m.ptr(l.probe_above);
        m.set_screen(p);
        m.w(l.probe_col, m.r(l.probe_col).wrapping_add(2));
        bump(&mut m, 0);
        return;
    }
    // $3BB7
    if m.r(l.probe_props + 4) == 0x11 && m.r(l.time_frames) >= 3 {
        m.w(l.time_frames, 3);
    }
    if (6..9).any(|i| m.r(l.probe_tiles + i) == 0x8F) {
        room_entry(m.ram, vic, l);
        return;
    }
    // $3BD8: the first of the three feet cells whose property has bit 3
    if let Some(i) = (0..3u8).find(|&i| m.r(l.probe_props + 6 + i as u16) & 8 != 0) {
        if m.r(l.in_room) == 1 {
            return;
        }
        let Some(y) = m.free_slot(l.anim_3b3e_state, 16) else { return };
        let (s, c1) = i.overflowing_add(m.r(l.probe_col));
        let (s2, c2a) = s.overflowing_add(m.r(l.probe_ptr4));
        let (s2, c2b) = s2.overflowing_add(c1 as u8);
        let hi = m.r(l.probe_ptr4 + 1).wrapping_add((c2a || c2b) as u8);
        m.w(l.anim_3b3e_lo + y, s2);
        m.w(l.anim_3b3e_hi + y, hi);
        if !(m.r(l.anim_3b3e_count) != 0 && s2 == m.r(l.anim_3b3e_last)) {
            m.w(l.anim_3b3e_last, s2);
            m.w(l.anim_3b3e_count, m.r(l.anim_3b3e_count).wrapping_add(1));
            m.w(l.anim_3b3e_state + y, 1);
        }
    }
    pickups(&mut m);
}

/// `$3C1B`: diamonds and blocks hit from below. A diamond (the 2 x 2 tiles `$A6..$A9`) in one of the probe's four side
/// cells is taken when the playfield is not in the middle of a column step: its cells are cleared, an item above it
/// starts falling, and it counts like every diamond (sound 2, `$FE` + 1 with an extra life at 100, 25 points). Else,
/// rising with the head under a block: with a power-up, bricks break (`$38F8`, 50 points), item blocks drop their item
/// (`$3991`, 50 points) and the repeated-hit block bumps; without, or for the other blocks, the bonus block bumps
/// (a diamond) and the `$81` block turns into a used block (sound 9, `$02DC`).
pub fn pickups_and_bumps(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    pickups(&mut Mem { ram, color, l });
}

fn pickups(m: &mut Mem) {
    let l = m.l;
    let p = m.ptr(l.probe_above).wrapping_sub(1);
    m.set_screen(p);
    let step = m.r(l.scroll_step);
    if step & 0x80 == 0 && step != 0 {
        return head(m);
    }
    // the side cells in the original's order: (property, tile, offset of the cell from the pointer)
    let side = [(0u16, 0u16, 0u8), (3, 3, 0x50), (2, 2, 2), (5, 5, 0x52)]
        .into_iter()
        .find(|&(prop, _, _)| m.r(l.probe_props + prop) & 0x80 != 0);
    let Some((_, tile, y0)) = side else { return head(m) };
    let x = m.r(l.probe_tiles + tile);
    // the diamond's top-left cell from the part that was touched
    let y = match x {
        0xA9 => y0,
        0xA7 => y0 + 1,
        0xA8 => y0 + 0x28,
        0xA6 => y0 + 0x29,
        _ => return head(m),
    };
    m.w(l.effect, 2);
    let p = m.screen().wrapping_add(y as u16);
    m.set_screen(p);
    if m.get(p.wrapping_add(0x28)) == 0xA7 {
        m.put(p.wrapping_add(0x28), 0);
        m.put(p, 0);
    } else if m.get(p) == 0xA6 {
        m.put(p, 0);
    }
    m.put(p.wrapping_add(1), 0);
    m.put(p.wrapping_add(0x29), 0);
    m.w(l.fall_start_state, 3);
    crate::scroll::start_fall_above(m.ram, l, 2);
    diamond(m);
}

/// `$3CA8`: a diamond: `$FE` + 1 (BCD; at 100 an extra life, its sound and a redraw of the lives), 25 points.
fn diamond(m: &mut Mem) {
    let l = m.l;
    let (d, carry) = adc_decimal(m.r(l.diamonds), 1, false);
    m.w(l.diamonds, d);
    if carry {
        let a = extra_life(m.ram, l);
        m.w(l.effect, a);
        m.w(l.hud_redraw, 0xFF);
    }
    score_add(m.ram, l, 0x25);
}

/// `$3E18`: the pointer moves `y` cells right to the block's left edge, the probe column to the cell after it.
fn to_block(m: &mut Mem, y: u8) {
    let l = m.l;
    let p = m.screen().wrapping_add(y as u16);
    m.set_screen(p);
    m.w(l.probe_col, m.r(l.probe_col).wrapping_add(y + 1));
}

/// `$3E2A`: the sum of the block's three top cells (with the carries of the 6502's additions), its signature.
fn signature(m: &Mem) -> u8 {
    let p = m.screen();
    let (s, c) = m.get(p).overflowing_add(m.get(p.wrapping_add(1)));
    s.wrapping_add(m.get(p.wrapping_add(2))).wrapping_add(c as u8)
}

/// `$3CC1`: the head under a block while rising.
fn head(m: &mut Mem) {
    let l = m.l;
    if m.r(l.vert_dir) != 1 || m.r(l.probe_props + 10) & 1 != 0 {
        return;
    }
    let head = m.r(l.probe_tiles + 1);
    if m.r(l.power) != 0 {
        if let Some(y) = [0x45u8, 0x44, 0x43].iter().position(|&t| t == head) {
            to_block(m, y as u8);
            match signature(m) {
                0xCC => item_block(m),
                0x73 => bump(m, 0xFF),
                0x74 => brick(m),
                _ => {}
            }
            return;
        }
    }
    let Some(y) = [0x4Bu8, 0x4A, 0x49].iter().position(|&t| t == head) else { return };
    to_block(m, y as u8);
    match signature(m) {
        0xD5 => bump(m, 0),
        0x81 => {
            m.w(l.bonus_flag, 0xFF);
            m.w(l.effect, 9);
            m.used_block();
        }
        _ => {}
    }
}

/// `$3CF6`: a brick breaks (`$38F8` animation, sound 6, 50 points).
fn brick(m: &mut Mem) {
    let l = m.l;
    let Some(x) = m.free_slot(l.anim_38f8_state, 4) else { return };
    if m.repeats(l.anim_38f8_count, l.anim_38f8_last) {
        return;
    }
    m.w(l.anim_38f8_count, m.r(l.anim_38f8_count).wrapping_add(1));
    m.w(l.anim_38f8_last, m.r(l.scratch_ptr));
    m.store_slot(l.anim_38f8_lo, l.anim_38f8_hi, x);
    m.w(l.anim_38f8_state + x, 1);
    m.w(l.effect, 6);
    score_add(m.ram, l, 0x50);
}

/// `$3D2F`: an item block drops its item (`$3991` animation, 50 points).
fn item_block(m: &mut Mem) {
    let l = m.l;
    let Some(x) = m.free_slot(l.anim_3991_state, 8) else { return };
    if m.repeats(l.anim_3991_count, l.anim_3991_last) {
        return;
    }
    m.w(l.anim_3991_count, m.r(l.anim_3991_count).wrapping_add(1));
    m.w(l.anim_3991_state + x, 1);
    m.w(l.anim_3991_last, m.r(l.scratch_ptr));
    m.store_slot(l.anim_3991_lo, l.anim_3991_hi, x);
    score_add(m.ram, l, 0x50);
}

/// `$3D8D`: a block bumps (`$389D` animation of `kind`: 0 = bonus block, `$FF` = the repeated-hit block, which only
/// bumps once per head hit `$0201` and turns into a used block when its counter `$0202` has run out). The bump sprite
/// of the slot goes to the block, and every bump counts as a diamond.
fn bump(m: &mut Mem, kind: u8) {
    let l = m.l;
    let Some(x) = m.free_slot(l.anim_389d_state, 3) else { return };
    m.w(l.anim_389d_kind + x, kind);
    if kind == 0 {
        if m.repeats(l.anim_389d_count, l.anim_389d_last) {
            return;
        }
    } else {
        if m.r(l.head_bump) == 0 {
            return;
        }
        m.w(l.head_bump, m.r(l.head_bump).wrapping_add(1));
        let v = match m.r(l.repeat_hits) {
            0 => {
                m.used_block();
                0
            }
            v if v & 0x80 != 0 => 0x0E,
            v => v,
        };
        m.w(l.repeat_hits, v.wrapping_sub(1));
    }
    // $3DC8
    m.w(l.anim_389d_last, m.r(l.scratch_ptr));
    if m.r(l.end_bump) != 0 {
        m.used_block();
    }
    m.colour_ptr();
    m.w(l.anim_389d_count, m.r(l.anim_389d_count).wrapping_add(1));
    m.w(l.sprite_split, 1);
    m.w(l.anim_389d_state + x, 1);
    m.colour_block(1);
    m.w(l.effect, 2);
    m.store_slot(l.anim_389d_lo, l.anim_389d_hi, x);
    let row = m.r(l.probe_row);
    m.w(l.bump_y + x, m.r(l.row_sprite_y + row as u16));
    let col = m.r(l.probe_col);
    let msb = m.r(l.bump_msb);
    let msb = if col & 0x20 != 0 { msb | m.r(l.bit_set + x) } else { msb & m.r(l.bit_clear + x) };
    m.w(l.bump_msb, msb);
    m.w(l.bump_x + x, (col << 3).wrapping_add(m.r(l.fine_scroll)));
    m.w(l.bump_timer + x, 0x18);
    diamond(m);
}

/// `$3F58`: Giana enters a treasure room through a door. Outside a room (`$C8 = 0`): sound 8, `$C8 = $FF` (the main
/// loop builds the room, `$3FA1`), and what the way back needs: the stage `$0129`, the fine scroll `$012A`, the sprite
/// variables `$70..$88` (`$0110..$0128`), the sprite expansion, multicolour and colour registers `$D01B`, `$D01C`,
/// `$D02B..$D02E` (`$012B..$0130`, as the C64 reads them back), the probe column `$0200` and the pointer `$027F/$0280`.
/// In stage `$53` (no such stage in this preview) it sets `$98 = $FF` instead.
pub fn room_entry(ram: &mut [u8], vic: &[u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    if r(ram, l.in_room) != 0 {
        return;
    }
    if r(ram, l.stage) == 0x53 {
        ram[l.stage_end as usize] = 0xFF;
        return;
    }
    ram[l.effect as usize] = 8;
    ram[l.in_room as usize] = 0xFF;
    ram[l.room_return as usize] = r(ram, l.stage);
    ram[l.room_return as usize + 1] = r(ram, l.fine_scroll);
    let (src, dst) = (l.player_x as usize, l.room_sprites as usize);
    ram.copy_within(src..src + 0x19, dst);
    let v = l.room_vic as usize;
    ram[v] = vic[0x1B];
    ram[v + 1] = vic[0x1C];
    for i in 0..4 {
        ram[v + 2 + i] = vic[0x2B + i] | 0xF0;
    }
    ram[l.obj_behind as usize] = r(ram, l.probe_col);
    ram[l.room_pointer as usize] = r(ram, l.probe_ptr4);
    ram[l.room_pointer as usize + 1] = r(ram, l.probe_ptr4 + 1);
}

/// `$26E8`: the status bar of a frame (main loop). By `$02DA`: `0` = all of it (the item icon of the power-up level, the
/// sprite colours of the power-up objects, then as `$FF`), `$FF` = the lives, then as `1` (and `$02DA = 1`), `1` = score,
/// diamonds and time; the time's leading zeros are blanked, and below 20 seconds its cells blink.
///
/// `vic` = the VIC registers (`$D000..$D03F` as written): the full redraw sets the colours (`$D02B..$D02E`) and the
/// multicolour bits (`$D01C`) of power-up objects.
pub fn hud(ram: &mut [u8], color: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram, color, l };
    let redraw = m.r(l.hud_redraw);
    if redraw == 0 {
        hud_item(&mut m, vic);
    }
    if redraw == 0 || redraw & 0x80 != 0 {
        // $272B
        let lives = m.r(l.lives);
        print_bcd_at(&mut m, 0x16, 1, lives);
        m.w(l.hud_redraw, 1);
    }
    // $2739
    for (i, col) in [(2u16, Some(0u8)), (1, None), (0, None)] {
        let v = m.r(l.score + i);
        match col {
            Some(c) => print_bcd_at(&mut m, c, 1, v),
            None => print_bcd(&mut m, v),
        }
    }
    m.w(l.print_col, 0x10);
    let d = m.r(l.diamonds);
    print_bcd(&mut m, d);
    let (lo, hi) = (m.r(l.time), m.r(l.time + 1));
    print_bcd_at(&mut m, 0x28, 0x18, hi);
    print_bcd(&mut m, lo);
    // $2766: the four digits printed past the screen go to the status bar, leading zeros as blanks (the last digit is
    // always copied: the loop stops at the first non-zero digit or after the third)
    let src = l.time_digits;
    let dst = l.hud_time;
    let mut x = 0u16;
    while x < 3 && m.r(src + x) == 0x30 {
        m.w(dst + x, 0);
        x += 1;
    }
    for x in x..4 {
        let v = m.r(src + x);
        m.w(dst + x, v);
    }
    if hi == 0 && lo < 0x20 {
        let c = m.r(l.time_blink + u16::from((m.r(l.frame_counter) & 15) >> 1));
        for a in [0xD822u16, 0xD823, 0xD824, 0xD825, 0xD84C, 0xD84D] {
            m.put(a, c);
        }
    }
}

/// `$2377`: two BCD digits (screen codes `$30..$39`) at column `col` of the print row `row` (`$24/$25`, the row
/// through `$8DE3/$8E01`), the column moves on by two.
fn print_bcd_at(m: &mut Mem, col: u8, row: u8, v: u8) {
    m.w(m.l.print_col, col);
    m.w(m.l.print_col + 1, row);
    print_bcd(m, v);
}

/// `$237A`: two BCD digits at the print position.
fn print_bcd(m: &mut Mem, v: u8) {
    let l = m.l;
    let row = m.r(l.print_col + 1) as u16;
    let (lo, hi) = (m.r(l.row_lo + 1 + row), m.r(l.row_hi + 1 + row));
    m.w(l.print_ptr, lo);
    m.w(l.print_ptr + 1, hi);
    let p = u16::from_le_bytes([lo, hi]);
    let y = m.r(l.print_col);
    m.put(p.wrapping_add(y as u16), v >> 4 | 0x30);
    m.put(p.wrapping_add(y.wrapping_add(1) as u16), (v & 15) | 0x30);
    m.w(l.print_col, y.wrapping_add(2));
}

/// `$26EF..$272A`: the item of the power-up level `$FD` (the sprite shape `$EB0F` and the status bar's icon characters,
/// with their colours), the colours of the power-up objects and of Giana's second sprite layer.
fn hud_item(m: &mut Mem, vic: &mut [u8]) {
    let l = m.l;
    let power = m.r(l.power);
    // $2671: the sprite shape of the item (48 bytes, pointers at `$26CF/$26D8`)
    let src = u16::from_le_bytes([m.r(l.item_ptr_lo + 1 + power as u16), m.r(l.item_ptr_hi + 1 + power as u16)]);
    m.w(l.pointer2, src as u8);
    m.w(l.pointer2 + 1, (src >> 8) as u8);
    for y in (0..0x30u16).rev() {
        let v = m.get(src.wrapping_add(y));
        m.put(l.item_sprite + y, v);
    }
    for x in (4..8u16).rev() {
        let s = m.r(l.object_anim + x);
        if (3..0x13).contains(&s) {
            power_object(m, vic, x);
        }
    }
    let (a, x) = hud_operands(m.ram, l);
    m.w(l.hud_d023_op, a);
    m.w(l.hud_d016_op, x);
    let c = m.r(l.item_colours + power as u16);
    for a in [0xD808u16, 0xD809, 0xD80A, 0xD830, 0xD831, 0xD832] {
        m.put(a, c);
    }
    // $269D: the icon's characters (48 bytes, pointers at `$26CE/$26D7`)
    let src = u16::from_le_bytes([m.r(l.item_ptr_lo + power as u16), m.r(l.item_ptr_hi + power as u16)]);
    m.w(l.pointer2, src as u8);
    m.w(l.pointer2 + 1, (src >> 8) as u8);
    let mut y = 0x2Fu16;
    for chars in [[0xC080u16, 0xC058, 0xC050], [0xC040, 0xC0D0, 0xC018]] {
        for x in (0..8u16).rev() {
            for c in chars {
                let v = m.get(src.wrapping_add(y));
                m.put(c + x, v);
                y = y.wrapping_sub(1);
            }
        }
    }
}

/// `$26F2`: the status bar's `$D023` and `$D016` for the power-up level (the code operands `$3301` and `$32F7` of the
/// `$FB` IRQ): level 1 `(7, $17)`, level 4 `(5, $17)`, else the stage header's `$010B` and 7.
pub fn hud_operands(ram: &[u8], l: &C30aLayout) -> (u8, u8) {
    match ram[l.power as usize] {
        1 => (7, 0x17),
        4 => (5, 0x17),
        _ => (ram[l.stage_header as usize + 11], 7),
    }
}

/// `$7A3A`: a power-up object (class `$AC` in `$020E,X`) gets the colour of the current level (`$7A6C`), as sprite
/// colour (`$0277,X`, `$D027,X`) and multicolour bit (`$D01C`, bit 7 of the colour = hi-res); above level 0 its two
/// other class bytes become `$AC` too. Not in a treasure room.
fn power_object(m: &mut Mem, vic: &mut [u8], x: u16) {
    let l = m.l;
    if m.r(l.in_room) != 0 || m.r(l.object_class + x) != 0xAC {
        return;
    }
    let power = m.r(l.power);
    if power != 0 {
        m.w(l.object_class + 5 + x, 0xAC);
        m.w(l.object_class + 10 + x, 0xAC);
    }
    let c = m.r(l.powerup_colours + power as u16);
    m.w(l.obj_colour + x, c);
    vic[0x27 + x as usize] = c;
    vic[0x1C] = if c & 0x80 != 0 { m.r(l.bit_clear + x) & vic[0x1C] } else { m.r(l.bit_set + x) | vic[0x1C] };
}
