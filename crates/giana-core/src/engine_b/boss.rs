//! Engine B, work package E6: weapons and bosses. Ports of the hack's routines (addresses of the Ridley X image):
//!
//! - the composite bosses (kind 3 = three sprites 5..7, the dragon `$14` also breathes with sprite 4, kind 4 = the four sprite
//!   goal of the last stage): spawn `$797A..$7A81`, movement `$8611/$86AC`, hits `$80D2`, breath `$8CB9`, touch `$8EB7`;
//! - chains of sprites (record byte 19 > 0: a head and 1..3 followers that are placed relative to the head): link
//!   building `$7B66..$7B98`, placement `$8A68`, per frame `$8BAE` (called from `$2D2D`);
//! - the homing shot of power level 4: target search `$8519`, flight `$83EB`.
//!
//! Slot indexes read from the link arrays are masked to 0..7 and the array offsets wrap at 256 like the hack's 8 bit
//! operands, so hostile type records (chain length 255, ...) cannot index out of range.

use super::sprites::Sprites;
use crate::state::GameState;

impl Sprites {
    /// A byte of `$0700..$07FF` addressed by `a` as the 6502 would (the low byte of the sum counts).
    #[inline]
    fn mw(&self, a: usize) -> u8 {
        self.m[a.wrapping_sub(0x700) & 0xFF]
    }
}

impl GameState {
    #[inline]
    fn bmw(&self, a: usize) -> u8 {
        self.eb().sprites.mw(a)
    }
    #[inline]
    fn set_bmw(&mut self, a: usize, v: u8) {
        self.engine_b.as_mut().expect("engine B").sprites.m[a.wrapping_sub(0x700) & 0xFF] = v;
    }

    // ---- boss spawn $797A.. ---------------------------------------------------------------------------------------------------------

    /// The record of a boss (kind 3 or 4) was copied into slot 7: shapes, colours, fixed position and the sprites switched on.
    /// `row` is the object's row (`$3222`).
    pub(super) fn b_boss_spawn(&mut self, rec: &[u8; 21], kind: u8, row: u8) {
        let ty = super::sprites::row_y(row);
        if kind == 3 {
            // `$797F`
            self.set_d32(0x3251, 4);
            let hits = self.bm(0x7A4 + 7);
            self.set_d32(0x3259, hits);
            let base = rec[0];
            self.set_sptr(7, base);
            self.set_sptr(6, base.wrapping_add(1));
            self.set_sptr(5, base.wrapping_add(2));
            if rec[1] & 1 != 0 {
                self.en.vic_mmc &= 0x1F;
            } else {
                self.en.vic_mmc |= 0xF0;
            }
            self.en.vic_mdp &= 0x0F;
            for s in 5..=7 {
                self.en.vic_col[s] = rec[2];
            }
            // `$319A`: the first dragon (type `$14`) also owns sprite 4, the flame
            if self.d32(0x3223) == 0x14 {
                self.set_sptr(4, 0xFB);
                self.sprite_x_msb |= 0xF0;
            } else {
                self.sprite_x_msb |= 0xE0;
            }
            self.set_spr(14, 0x4F);
            self.set_spr(12, 0x67);
            self.set_spr(10, 0x7F);
            let y = ty.wrapping_add(0x1D);
            self.set_spr(11, y);
            self.set_spr(13, y);
            self.set_spr(15, y);
            self.en.vic_me |= 0xE0;
        } else {
            // `$7A0F`: kind 4
            let base = rec[0];
            for (i, s) in [7usize, 6, 5, 4].into_iter().enumerate() {
                self.set_sptr(s, base.wrapping_add(i as u8));
            }
            self.en.vic_mmc &= 0x0F;
            self.en.vic_mdp &= 0x1F;
            self.en.vic_col[4] = 7;
            self.en.vic_col[5] = 7;
            self.en.vic_col[6] = 1;
            self.en.vic_col[7] = 1;
            self.sprite_x_msb |= 0xF0;
            for off in [14, 12, 10, 8] {
                self.set_spr(off, 0x4F);
            }
            self.set_spr(11, ty);
            self.set_spr(15, ty);
            let y2 = ty.wrapping_add(0x15);
            self.set_spr(13, y2);
            self.set_spr(9, y2);
            self.en.vic_me |= 0xF0;
        }
        self.set_d32(0x3253, 0);
        self.set_d32(0x3256, 0);
        self.set_d32(0x325A, 0);
    }

    // ---- boss movement $8611 ------------------------------------------------------------------------------------------------------------

    /// `$8611`: one step of the kind 3 boss (called instead of the slot loop; nothing else moves in that frame).
    pub(super) fn b_boss_move(&mut self) {
        let v = self.d32(0x3259);
        if v & 0x80 != 0 {
            // dying: rises for 15 frames, then sinks (`$325A` counts down through 0)
            let n = self.d32(0x325A).wrapping_sub(1);
            self.set_d32(0x325A, n);
            if n == 0 {
                return;
            }
            if n & 0x80 != 0 {
                self.set_d32(0x3255, 2);
                self.b_boss_shift(true);
            } else {
                self.set_d32(0x3257, 2);
                self.b_boss_shift(false);
            }
            return;
        }
        self.b_boss_script(); // `$86AC`
        let n = self.d32(0x3251).wrapping_sub(1);
        self.set_d32(0x3251, n);
        if n & 0x80 == 0 {
            return;
        }
        self.set_d32(0x3251, 4);
        if self.d32(0x3252) != 0 {
            self.b_boss_anim_down();
        } else {
            self.b_boss_anim_up();
        }
    }

    /// `$8640`: the animation counts up through the range `$0724..$073C`.
    fn b_boss_anim_up(&mut self) {
        // bounded: the hack loops while the range is not valid; at most 256 frames of shapes
        for _ in 0..0x200 {
            let p = self.sptr(7);
            let hi = self.bm(0x73C + 7);
            if p == hi {
                return;
            }
            if p < hi {
                self.b_boss_step_shape(p.wrapping_add(self.bm(0x784 + 7)));
                return;
            }
            if self.bm(0x724 + 7) >= self.bm(0x72C + 7) {
                self.set_d32(0x3252, 0xFF);
                self.b_boss_anim_down();
                return;
            }
            let v = self.bm(0x734 + 7);
            self.set_sptr(7, v);
        }
    }

    /// `$867F`
    fn b_boss_anim_down(&mut self) {
        let p = self.sptr(7);
        let lim = self.bm(0x734 + 7);
        if p <= lim {
            self.set_d32(0x3252, 0);
            self.b_boss_anim_up();
            return;
        }
        self.b_boss_step_shape(p.wrapping_sub(self.bm(0x784 + 7)));
    }

    fn b_boss_step_shape(&mut self, p: u8) {
        self.set_sptr(7, p);
        self.set_sptr(6, p.wrapping_add(1));
        self.set_sptr(5, p.wrapping_add(2));
    }

    /// `$8786` (down by `$3255`) / `$8767` (up by `$3257`): the three body sprites vertically.
    fn b_boss_shift(&mut self, down: bool) {
        for off in [11usize, 13, 15] {
            let v = self.spr(off);
            let n = if down { v.wrapping_add(self.d32(0x3255)) } else { v.wrapping_sub(self.d32(0x3257)) };
            self.set_spr(off, n);
        }
    }

    /// `$86AC`: the movement script of the boss (3 byte steps: frames, dx, dy; `$FF` restarts), pointer in the speed and delay
    /// bytes of slot 7.
    fn b_boss_script(&mut self) {
        let ptr = self.bm(0x774 + 7) as usize | (self.bm(0x77C + 7) as usize) << 8;
        let n = self.d32(0x3253).wrapping_sub(1);
        self.set_d32(0x3253, n);
        if n & 0x80 != 0 {
            // an empty script (the first byte `$FF` ...) would loop for ever in the hack; the port gives up after one round
            let mut rounds = 0;
            loop {
                let y = self.d32(0x3256) as usize;
                let a = self.b_mem(ptr + y);
                if a == 0xFF {
                    self.set_d32(0x3256, 0);
                    rounds += 1;
                    if rounds > 1 {
                        self.set_d32(0x3253, 0);
                        self.set_d32(0x3254, 0);
                        self.set_d32(0x3255, 0);
                        break;
                    }
                    continue;
                }
                self.set_d32(0x3253, a);
                self.set_d32(0x3254, self.b_mem(ptr + y + 1));
                self.set_d32(0x3255, self.b_mem(ptr + y + 2));
                self.set_d32(0x3256, (y + 3) as u8);
                break;
            }
        }
        // `$86DF`
        let mut speed = self.d32(0x3254);
        let mut dir = 1u8;
        if speed & 0x80 != 0 {
            speed = !speed;
            dir = 0;
        }
        self.set_d32(0x3258, speed);
        if self.stage as u8 == crate::image::img(0x86F7) {
            // `$86F6: CMP #imm` (Ridley X `$2A`, the other hacks `0`): the last stage's dragon is blue-grey (3) and faces Giana
            for s in 5..=7 {
                self.en.vic_col[s] = 3;
            }
            // `$9A1B`: Giana's X + 14 with the carry into the MSB
            let (gx, c) = self.spr(0).overflowing_add(0x0E);
            let gm = (self.sprite_x_msb & 1) ^ c as u8;
            let bx = self.spr(14);
            let bm = (self.sprite_x_msb >> 7) & 1;
            let d = gx as i16 - bx as i16;
            let hi = gm as i16 - bm as i16 - (d < 0) as i16;
            dir = if hi < 0 { 1 } else { 0 };
        }
        self.set_v47(0x483, dir);
        self.set_v47(0x482, self.d32(0x3258));
        self.b_move_x(7, 14);
        self.b_move_x(6, 12);
        self.b_move_x(5, 10);
        let a = self.d32(0x3255);
        if a & 0x80 != 0 {
            self.set_d32(0x3257, !a);
            self.b_boss_shift(false);
        } else {
            self.b_boss_shift(true);
        }
    }

    // ---- boss hit, touch ----------------------------------------------------------------------------------------------------------------

    /// `$80D2`: Giana's shot hit a sprite while a boss (kind >= 3) is in slot 7.
    pub(super) fn b_boss_hit(&mut self) {
        if self.bm(0x7CB) != 0 {
            return;
        }
        let v = self.d32(0x3259);
        if v & 0x80 == 0 {
            let n = v.wrapping_sub(1);
            self.set_d32(0x3259, n);
            if n & 0x80 == 0 {
                return;
            }
            self.set_d32(0x325A, 0x0F);
        }
        // `$80E7`: the boss is dead: squashed shape (if it has one), the parts stop being targets, score
        let s = self.bm(0x764 + 7);
        if s != 0 {
            self.set_sptr(7, s);
            self.set_sptr(6, s.wrapping_add(1));
            self.set_sptr(5, s.wrapping_add(2));
        }
        for a in [0x7C9, 0x7CA, 0x7CB] {
            self.set_bm(a, 1);
        }
        self.add_score_sub_991d(self.bm(0x793), self.bm(0x79B));
    }

    /// `$8EB7`: Giana touches the four sprite goal of the last stage (kind 4): once she has passed it, the game ends.
    pub(super) fn b_goal_touch(&mut self) {
        let (_, borrow) = self.spr(14).overflowing_sub(self.spr(0));
        if borrow && self.sprite_x_msb & 0x80 == 0 {
            self.en.flag_157e = 0xFF;
            self.flag_0431 = 0xFF;
        }
    }

    // ---- the dragon's breath $8CB9 ----------------------------------------------------------------------------------------------------

    /// `$8CB9`: the flame of the dragon `$14` (sprite 4) flies left; it starts when the boss script is at the step `(20, 0, 1)`.
    pub(super) fn b_breath(&mut self) {
        if self.bm(0x724 + 7) != 0xBE || self.bm(0x71C + 7) != 3 {
            return;
        }
        if self.en.vic_me & 0x10 != 0 {
            self.en.vic_col[4] = 7;
            self.set_bm(0x744 + 4, 1);
            let (v, borrow) = self.spr(8).overflowing_sub(4);
            self.set_spr(8, v);
            if borrow {
                self.sprite_x_msb ^= 0x10;
            }
            if self.sprite_x_msb & 0x10 == 0 && self.spr(8) < 0x10 {
                self.en.vic_me &= 0xEF;
                return;
            }
        } else {
            if self.d32(0x3253) != 0x14 || self.d32(0x3255) != 1 || self.bm(0x7CB) != 0 {
                return;
            }
            self.en.vic_me |= 0x10;
            let (x, y) = (self.spr(14), self.spr(15));
            self.set_spr(8, x);
            self.set_spr(9, y);
            if self.sprite_x_msb & 0x80 != 0 {
                self.sprite_x_msb |= 0x10;
            } else {
                self.sprite_x_msb &= 0xEF;
            }
        }
        // `$8D30`: the flame flickers through `$FB..$FD`
        if self.bz(0xF0) == 0 {
            let p = self.sptr(4).wrapping_sub(1);
            self.set_sptr(4, p);
            if p < 0xFC {
                self.set_bz(0xF0, p);
            }
        } else {
            let p = self.sptr(4).wrapping_add(1);
            self.set_sptr(4, p);
            if p >= 0xFE {
                self.set_bz(0xF0, 0);
            }
        }
    }

    // ---- chains ---------------------------------------------------------------------------------------------------------------------

    /// `$7B66..$7B98`: a sprite of a chain was created in slot `x`. Returns true when the chain is complete (the followers were
    /// placed), false when the next sprite of it has to be created.
    pub(super) fn b_chain_link(&mut self, x: usize) -> bool {
        if self.bz(0xF4) & 0x80 != 0 {
            // the head: the link operand starts at `$E0`
            self.engine_b.as_mut().expect("engine B").sprites.chain_op = 0xE0; // the operand `$7B83` of the hack
            self.set_bz(0xF3, x as u8);
        }
        let f4 = self.bz(0xF4).wrapping_add(1);
        self.set_bz(0xF4, f4);
        let head = self.bz(0xF3) as usize;
        self.set_bmw(0x7F0 + x, head as u8);
        let op = self.eb().sprites.chain_op;
        self.set_bmw(0x700 + op as usize + head, x as u8);
        self.engine_b.as_mut().expect("engine B").sprites.chain_op = op.wrapping_add(4);
        if f4 >= self.bmw(0x79C + head) {
            self.b_chain_place();
            return true;
        }
        false
    }

    /// `$8A68`: place the followers of the chain whose head is in `$F3` (count in `$F4`) relative to the head.
    pub(super) fn b_chain_place(&mut self) {
        let head = self.bz(0xF3) as usize;
        let mode = self.bmw(0x7A4 + head);
        let hx2 = (2 * head) & 0xFF;
        if mode & 0x80 != 0 {
            self.b_chain_up(head, hx2);
        } else if mode != 0 {
            self.b_chain_block(head, hx2);
        } else {
            self.b_chain_row(head, hx2);
        }
    }

    /// Copy the head's MSB bit into sprite `f`.
    fn b_chain_msb(&mut self, head: usize, f: usize) {
        if self.sprite_x_msb & self.bit(head & 7) != 0 {
            self.sprite_x_msb |= self.bit(f & 7);
        } else {
            self.sprite_x_msb &= self.nbit(f & 7);
        }
    }

    /// `$8A75`: followers side by side, `$18` pixels apart.
    fn b_chain_row(&mut self, head: usize, hx2: usize) {
        let mut op = 0xE4u8;
        let mut off = 0u8;
        loop {
            let f = self.bmw(0x700 + op as usize + head) as usize & 7;
            self.b_chain_msb(head, f);
            let hy = self.spr(hx2 + 1);
            self.set_spr(2 * f + 1, hy);
            let (o2, c1) = op.overflowing_add(4);
            op = o2;
            let sum = off as u16 + 0x18 + c1 as u16;
            off = sum as u8;
            let sum2 = self.spr(hx2) as u16 + off as u16 + (sum > 0xFF) as u16;
            self.set_spr(2 * f, sum2 as u8);
            if sum2 > 0xFF {
                self.sprite_x_msb ^= self.bit(f);
            }
            let n = self.bz(0xF4).wrapping_sub(1);
            self.set_bz(0xF4, n);
            if n == 0 {
                return;
            }
        }
    }

    /// `$8B62`: followers stacked upwards, `$15` pixels apart.
    fn b_chain_up(&mut self, head: usize, hx2: usize) {
        let mut op = 0xE4u8;
        let mut fy = self.spr(hx2 + 1);
        loop {
            let f = self.bmw(0x700 + op as usize + head) as usize & 7;
            self.b_chain_msb(head, f);
            let hx = self.spr(hx2);
            self.set_spr(2 * f, hx);
            fy = fy.wrapping_sub(0x15);
            self.set_spr(2 * f + 1, fy);
            op = op.wrapping_add(4);
            let n = self.bz(0xF4).wrapping_sub(1);
            self.set_bz(0xF4, n);
            if n == 0 {
                return;
            }
        }
    }

    /// `$8AD5`: a block of four, the head at the bottom left.
    fn b_chain_block(&mut self, head: usize, hx2: usize) {
        let f1 = self.bmw(0x7E4 + head) as usize & 7;
        self.b_chain_msb(head, f1);
        let (hx, hy) = (self.spr(hx2), self.spr(hx2 + 1));
        self.set_spr(2 * f1, hx);
        let up = hy.wrapping_sub(0x15);
        self.set_spr(2 * f1 + 1, up);
        let f3 = self.bmw(0x7EC + head) as usize & 7;
        let f2 = self.bmw(0x7E8 + head) as usize & 7;
        self.set_bz(0xF4, f3 as u8);
        self.set_bz(0xF2, f2 as u8);
        if self.sprite_x_msb & self.bit(head & 7) != 0 {
            self.sprite_x_msb |= self.bit(f3) | self.bit(f2);
        } else {
            self.sprite_x_msb &= self.nbit(f3) & self.nbit(f2);
        }
        self.set_spr(2 * f2 + 1, up);
        self.set_spr(2 * f3 + 1, hy);
        let (nx, carry) = hx.overflowing_add(0x18);
        self.set_spr(2 * f2, nx);
        self.set_spr(2 * f3, nx);
        if carry {
            self.sprite_x_msb ^= self.bit(f3) ^ self.bit(f2);
        }
    }

    /// `$8BAE` (via `$2D2D`): per frame, the chains of the slots 7..5: place the followers, let them die with the head and
    /// follow its shape.
    pub(super) fn b_chain_frame(&mut self) {
        if self.bm(0x71C + 7) >= 3 {
            return;
        }
        let mut a = self.en.vic_me;
        let mut y = 7usize;
        loop {
            let carry = a & 0x80 != 0;
            a <<= 1;
            if carry && self.bm(0x7E0 + y) != 0 {
                self.b_chain_one(y);
            }
            y -= 1;
            if y == 4 {
                return;
            }
        }
    }

    fn b_chain_one(&mut self, head: usize) {
        self.set_bz(0xF3, head as u8);
        let len = self.bm(0x79C + head);
        self.set_bz(0xF4, len);
        self.b_chain_place();
        let mut y = self.bz(0xF3) as usize;
        let len = self.bmw(0x79C + y);
        self.set_bz(0xF4, len);
        if self.bmw(0x7C4 + y) != 0 {
            // the head is dying: the followers fall down with it
            for _ in 0..256 {
                let x = self.bmw(0x7E4 + y) as usize;
                self.set_bmw(0x7C4 + x, 1);
                self.set_bmw(0x7E0 + x, 0);
                self.set_bmw(0x75C + x, 0);
                self.set_bmw(0x7F0 + x, 0xFF);
                y = (y + 4) & 0xFF;
                let n = self.bz(0xF4).wrapping_sub(1);
                self.set_bz(0xF4, n);
                if n == 0 {
                    break;
                }
            }
            y = self.bz(0xF3) as usize;
            let len = self.bmw(0x79C + y);
            self.set_bz(0xF4, len);
        }
        // `$8BFC`: the followers show the next shapes of the head's
        let mut p = self.sptr(y & 7);
        let mut c = false;
        for k in 0..3usize {
            let f = self.bmw(0x7E4 + 4 * k + y) as usize;
            let sum = p as u16 + 1 + c as u16;
            p = sum as u8;
            c = sum > 0xFF;
            self.set_sptr(f, p);
            let n = self.bz(0xF4).wrapping_sub(1);
            self.set_bz(0xF4, n);
            if n == 0 {
                break;
            }
        }
    }

    // ---- the homing shot (power level 4) ------------------------------------------------------------------------------------------------

    /// `$8519`: choose the target of the shot: the highest slot with a stompable (class 0 or bit 7 bosses ...) sprite in front of Giana.
    /// `$3247` = the slot; `$3248 = 1` when there is none (the shot flies straight).
    pub(super) fn b_target_search(&mut self) {
        let mut a = self.en.vic_me;
        let mut y = 7usize;
        loop {
            let carry = a & 0x80 != 0;
            a <<= 1;
            // `LDX $0744,Y; DEX; BPL`: class 0 and classes above `$80` pass; then `$07C4,Y` must be 0
            if carry && self.bm(0x744 + y).wrapping_sub(1) & 0x80 != 0 && self.bm(0x7C4 + y) == 0 {
                self.set_d32(0x3247, y as u8);
                let ex = self.spr(2 * y);
                let ehi = (self.sprite_x_msb & self.bit(y) != 0) as u8;
                let phi = self.sprite_x_msb & 1;
                let (lo, b) = self.spr(0).overflowing_sub(ex);
                let hi = phi.wrapping_sub(ehi).wrapping_sub(b as u8);
                self.set_v47(0x48A, lo);
                self.set_v47(0x48B, hi);
                self.set_v47(0x48C, ex);
                self.set_v47(0x48D, ehi);
                let borrow = (phi as i16 - ehi as i16 - b as i16) < 0;
                let facing = if self.facing_right { 0u8 } else { 0xFF };
                if borrow != (facing != 0) {
                    return;
                }
                // the search goes on with `$042C` in A (the hack's quirk)
                a = facing;
            }
            y -= 1;
            if y < 4 {
                self.set_d32(0x3248, 1);
                return;
            }
        }
    }

    /// `$83EB`: the homing flight: steer towards the target in `$3247`.
    pub(super) fn b_homing(&mut self) {
        let t = self.d32(0x3247) as usize & 7;
        if self.en.vic_me & self.bit(t) == 0 {
            self.en.spr_ptr[0] = 0xFB;
            return;
        }
        let fx = self.en.spr_xy[0];
        let fhi = (self.sprite_x_msb >> 3) & 1;
        let fy = self.en.spr_xy[1];
        let ex = self.spr(t * 2);
        let ehi = (self.sprite_x_msb & self.bit(t) != 0) as u8;
        let ey = self.spr(t * 2 + 1).wrapping_sub(0x0A);
        let (mut dx, b) = fx.overflowing_sub(ex);
        let mut dxh = fhi.wrapping_sub(ehi).wrapping_sub(b as u8);
        let neg = (fhi as i16 - ehi as i16 - b as i16) < 0;
        
        let dirx = if neg {
            dx = !dx;
            dxh = !dxh;
            dx = dx.wrapping_add(1);
            if dx == 0 {
                dxh = dxh.wrapping_add(1);
            }
            0u8
        } else {
            0xFF
        };
        self.set_d32(0x3243, dirx);
        let (mut dy, b) = fy.overflowing_sub(ey);
        let diry;
        if b {
            dy = !dy;
            diry = 0u8;
        } else {
            diry = 0xFF;
        }
        self.set_d32(0x3244, diry);
        let (dx2, b2) = dx.overflowing_sub(dy);
        let ge = (dxh as i16 - b2 as i16) >= 0;
        let (mut sx, mut sy): (u8, u8) = if !ge { (1, 3) } else { (3, 1) };
        // `$84A8`: ROR `$324C`, then five LSR: the carry that reaches the ADC is bit 4
        let c = (dx2 >> 4) & 1;
        let step = self.d32(0x3239 + (dx2 >> 5) as usize);
        sx = step.wrapping_add(sx).wrapping_add(c);
        sy = step.wrapping_add(sy);
        let tile = if dirx != 0 { self.v47(0x47D) } else { self.v47(0x47F) };
        if self.b_solid(tile) {
            sy = sx;
            sx = 0;
            self.set_d32(0x3244, 0xFF);
        }
        self.set_d32(0x3249, sx);
        self.set_d32(0x324A, sy);
        self.set_v47(0x482, sx);
        self.set_v47(0x483, dirx);
        self.b_move_x(3, 6);
        let v = self.d32(0x3244);
        let ny = if v != 0 { self.en.spr_xy[1].wrapping_sub(sy) } else { self.en.spr_xy[1].wrapping_add(sy) };
        self.en.spr_xy[1] = ny;
    }
}
