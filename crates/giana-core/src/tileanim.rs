//! Animated tiles of the original: hit blocks, debris, rising diamonds, crumbling bridges and the block
//! interaction routine that starts them ($1249 handler, sub_1A17 and everything they call).
//!
//! The state lives in the slot tables at `$9B02..$9B9F` (`GameState::particle_slots`), which the column scroll
//! of `$1326` also shifts. Screen and colour RAM are written through the zero-page pointers `$0E` (screen) and
//! `$10` (colour = screen + `$1000`), kept in `zp_0e`/`zp_10`/`zp_12`.

use crate::image::img;
use crate::state::GameState;

const SLOT_BASE: usize = 0x9B02;

/// Colour RAM lies `$1000` behind the screen RAM page.
const COLOUR_OFFSET: u16 = 0x1000;

impl GameState {
    // ---- slot table access --------------------------------------------------------------------------------------

    #[inline]
    fn slot(&self, addr: usize) -> u8 {
        self.particle_slots[addr - SLOT_BASE]
    }
    #[inline]
    fn set_slot(&mut self, addr: usize, v: u8) {
        self.particle_slots[addr - SLOT_BASE] = v;
    }

    // ---- screen / colour RAM through the zero-page pointers ----------------------------------------------------------

    /// `LDA ($0E),Y`
    fn rd0e(&self, y: u16) -> u8 {
        let a = self.zp_0e.wrapping_add(y);
        if (0xC800..0xC800 + 1000).contains(&a) {
            self.screen_ram[(a - 0xC800) as usize]
        } else {
            img(a as usize)
        }
    }
    /// `STA ($0E),Y`
    fn wr0e(&mut self, y: u16, v: u8) {
        let a = self.zp_0e.wrapping_add(y);
        if (0xC800..0xC800 + 1000).contains(&a) {
            self.screen_ram[(a - 0xC800) as usize] = v;
        }
    }
    /// `LDA ($10),Y`
    fn rd10(&self, y: u16) -> u8 {
        let a = self.zp_10.wrapping_add(y);
        if (0xD800..0xD800 + 1000).contains(&a) {
            self.color_ram[(a - 0xD800) as usize]
        } else {
            0
        }
    }
    /// `STA ($10),Y`
    fn wr10(&mut self, y: u16, v: u8) {
        let a = self.zp_10.wrapping_add(y);
        if (0xD800..0xD800 + 1000).contains(&a) {
            self.color_ram[(a - 0xD800) as usize] = v;
        }
    }

    /// sub_1D46 ($1D46..$1D51): colour pointer = screen pointer + `$1000`
    pub fn colour_pointer_sub_1d46(&mut self) {
        self.zp_10 = self.zp_0e.wrapping_add(COLOUR_OFFSET);
    }

    /// sub_1D52 ($1D52..$1D66): fill the 3 x 2 colour cells at `$10`
    pub fn colour_fill_sub_1d52(&mut self, a: u8) {
        for y in [0u16, 1, 2, 0x28, 0x29, 0x2A] {
            self.wr10(y, a);
        }
    }

    /// sub_19BD ($19BD): three cells `Y..Y+2` set to A
    fn fill_19bd(&mut self, a: u8, y: u16) {
        self.wr0e(y, a);
        self.fill_19c0(a, y + 1);
    }
    /// sub_19C0 ($19C0): two cells `Y..Y+1` set to A
    fn fill_19c0(&mut self, a: u8, y: u16) {
        self.wr0e(y, a);
        self.wr0e(y + 1, a);
    }
    /// sub_19BA ($19BA): clear the top row of three cells
    fn clear_19ba(&mut self) {
        self.fill_19bd(0, 0);
    }
    /// sub_19B3 ($19B3): clear a block of 3 x 2 cells
    fn clear_19b3(&mut self) {
        self.fill_19bd(0, 0x28);
        self.clear_19ba();
    }
    /// sub_19C6 ($19C6): put character `x` with colour 9 at cell Y and count X up
    fn put_19c6(&mut self, y: u16, x: &mut u8) {
        self.wr0e(y, *x);
        self.wr10(y, 0x09);
        *x = x.wrapping_add(1);
    }

    // ---- handler $1249 --------------------------------------------------------------------------------------------------

    /// Raster handler `$1249` ($127C..$1297): animate blocks, debris and bonus items
    pub fn tile_animation_handler_l127c(&mut self) {
        // $127C: LDA $157D; BNE L129A; LDA $0431; CMP #$FF; BEQ L129A
        if self.en.pause_157d != 0 || self.flag_0431 == 0xFF {
            return;
        }
        self.debris_sub_184c();
        self.bump_sub_17fc();
        self.rising_tiles_sub_16d9();
        self.debris_sub_184c();
        self.bump_sub_17fc();
        self.bonus_sub_19cf();
    }

    // ---- sub_184C: debris of a broken block ---------------------------------------------------------------------------

    /// sub_184C ($184C..$19B2)
    pub fn debris_sub_184c(&mut self) {
        // DEC $0450; BPL L1856; LDA #$07; STA $0450
        let mut i = self.v47(0x450).wrapping_sub(1);
        if i & 0x80 != 0 {
            i = 7;
        }
        self.set_v47(0x450, i);
        let x = i as usize;
        let st = self.slot(0x9B02 + x);
        if st == 0 {
            return;
        }
        self.zp_0e = (self.slot(0x9B12 + x) as u16) << 8 | self.slot(0x9B0A + x) as u16;
        let mut cx: u8;
        match st {
            1 => cx = 0x1C,
            2 => cx = 0x22,
            3 | 4 | 5 => cx = 0x28,
            6 => {
                self.debris_state_6_l1896();
                return;
            }
            _ => {
                self.set_slot(0x9B02 + x, 0);
                return;
            }
        }
        // L190D: CLC; LDA $9B02,Y; ADC #$01; STA $9B02,Y
        self.set_slot(0x9B02 + i as usize, st.wrapping_add(1));
        self.debris_draw_l1919(&mut cx);
    }

    /// L1919: draw six characters, and at state 6 continue downwards
    fn debris_draw_l1919(&mut self, cx: &mut u8) {
        loop {
            self.colour_pointer_sub_1d46();
            for y in [0u16, 1, 2, 0x28, 0x29, 0x2A] {
                self.put_19c6(y, cx);
            }
            let i = self.v47(0x450) as usize;
            if self.slot(0x9B02 + i) != 6 {
                return; // L19B2
            }
            // $1944: $0E:$0F -= $52
            self.zp_0e = self.zp_0e.wrapping_sub(0x52);
            let mut x = 0u16;
            let mut y = 2u16;
            loop {
                if self.rd0e(y) == 0x2D {
                    break;
                }
                x += 1;
                y += 1;
                if y == 7 {
                    return;
                }
            }
            // L1962
            self.zp_0e = self.zp_0e.wrapping_add(x).wrapping_sub(0x28);
            let t = self.tile_flags(self.rd0e(0x50))
                | self.tile_flags(self.rd0e(0x51))
                | self.tile_flags(self.rd0e(0x52));
            if t & 1 != 0 {
                return;
            }
            self.clear_19ba();
            self.zp_0e = self.zp_0e.wrapping_add(0x28);
            *cx = 0x28;
        }
    }

    /// L1896: the debris reached its lowest frame; check the tiles underneath
    fn debris_state_6_l1896(&mut self) {
        let t1 = self.tile_flags(self.rd0e(0x50));
        let t2 = self.tile_flags(self.rd0e(0x51));
        let t3 = self.tile_flags(self.rd0e(0x52));
        let sum = t1.wrapping_add(t2).wrapping_add(t3);
        self.set_v47(0x451, sum);
        if sum == 0 {
            self.clear_19ba(); // L18EB
        } else if sum == 1 {
            if self.tile_flags(self.rd0e(0x50)) & 1 != 0 {
                self.clear_19b3();
                self.zp_0e = self.zp_0e.wrapping_add(1);
            } else {
                self.clear_19b3(); // L18DD
                self.zp_0e = self.zp_0e.wrapping_sub(1);
            }
        } else {
            // L18D6: INC $9B02,X
            let i = self.v47(0x450) as usize;
            let v = self.slot(0x9B02 + i).wrapping_add(1);
            self.set_slot(0x9B02 + i, v);
            return;
        }
        // L18EE: $0E:$0F += $28; store the new position in the slot; X = $28; JMP L1919
        self.zp_0e = self.zp_0e.wrapping_add(0x28);
        let i = self.v47(0x450) as usize;
        self.set_slot(0x9B0A + i, (self.zp_0e & 0xFF) as u8);
        self.set_slot(0x9B12 + i, (self.zp_0e >> 8) as u8);
        let mut cx = 0x28u8;
        self.debris_draw_l1919(&mut cx);
    }

    // ---- sub_17FC: bumped block ---------------------------------------------------------------------------------------------

    /// sub_17FC ($17FC..$184B)
    pub fn bump_sub_17fc(&mut self) {
        let x = self.v47(0x450) as usize;
        let st = self.slot(0x9B38 + x);
        if st == 0 {
            return;
        }
        self.zp_0e = (self.slot(0x9B48 + x) as u16) << 8 | self.slot(0x9B40 + x) as u16;
        if st == 7 {
            self.set_slot(0x9B38 + x, 0);
            self.clear_19b3();
            return;
        }
        self.set_slot(0x9B38 + x, st + 1);
        let k = st as usize - 1;
        let chars = [
            (0u16, 0x9DC5usize),
            (1, 0x9DCB),
            (2, 0x9DD1),
            (0x28, 0x9DD7),
            (0x29, 0x9DDD),
            (0x2A, 0x9DE3),
        ];
        for (y, table) in chars {
            self.wr0e(y, img(table + k));
        }
    }

    // ---- sub_19CF: crumbling bridge tile --------------------------------------------------------------------------------------

    /// sub_19CF ($19CF..$1A16)
    pub fn bonus_sub_19cf(&mut self) {
        let mut i = self.v47(0x454).wrapping_sub(1);
        if i & 0x80 != 0 {
            i = 7;
        }
        self.set_v47(0x454, i);
        let x = i as usize;
        let st = self.slot(0x9B50 + x);
        if st == 0 {
            return;
        }
        self.zp_0e = (self.slot(0x9B60 + x) as u16) << 8 | self.slot(0x9B58 + x) as u16;
        let a = match st {
            1 => 0x2E,
            2 => 0x2F,
            3 => 0x00,
            _ => {
                self.set_slot(0x9B50 + x, 0);
                return;
            }
        };
        self.wr0e(0, a);
        self.set_slot(0x9B50 + x, st + 1);
    }

    // ---- sub_16D9 / sub_16A7 / sub_16FE: rising items ----------------------------------------------------------------------------------

    /// sub_16D9 ($16D9..$16FD)
    pub fn rising_tiles_sub_16d9(&mut self) {
        let n = self.v47(0x44F).wrapping_sub(1);
        self.set_v47(0x44F, n);
        if n & 0x80 == 0 {
            return;
        }
        self.set_v47(0x44F, 2);
        self.restore_tiles_sub_16a7();
        self.set_v47(0x44E, 2);
        self.rising_slot_sub_16fe(2);
        let n = self.v47(0x44E).wrapping_sub(1);
        self.set_v47(0x44E, n);
        self.rising_slot_sub_16fe(n as usize);
        let n = self.v47(0x44E).wrapping_sub(1);
        self.set_v47(0x44E, n);
        self.rising_slot_sub_16fe(n as usize);
    }

    /// sub_16A7 ($16A7..$16D8): put back the tile a rising item covered
    pub fn restore_tiles_sub_16a7(&mut self) {
        for x in 0..3usize {
            if self.slot(0x9B90 + x) == 0 {
                continue;
            }
            self.zp_0e = (self.slot(0x9B78 + x) as u16) << 8 | self.slot(0x9B70 + x) as u16;
            self.colour_pointer_sub_1d46();
            let y = self.slot(0x9B88 + x) as u16;
            let ch = self.slot(0x9B80 + x);
            self.wr0e(y, ch);
            self.wr10(y, self.char_colour(ch));
            self.set_slot(0x9B90 + x, 0);
        }
    }

    /// sub_16FE ($16FE..$17FB): one step of rising slot X
    pub fn rising_slot_sub_16fe(&mut self, x: usize) {
        let st = self.slot(0x9B68 + x);
        if st == 0 {
            return;
        }
        self.zp_0e = (self.slot(0x9B78 + x) as u16) << 8 | self.slot(0x9B70 + x) as u16;
        let mut a = st;
        if a >= 1 && a < 6 {
            // $174A: PHA; TAY; $0E:$0F += $A0; sub_1D46; sub_1D52($9D22,Y); $0E:$0F -= $A0; PLA
            self.zp_0e = self.zp_0e.wrapping_add(0xA0);
            self.colour_pointer_sub_1d46();
            self.colour_fill_sub_1d52(img(0x9D22 + a as usize));
            self.zp_0e = self.zp_0e.wrapping_sub(0xA0);
        }
        if a >= 6 && a == 7 {
            // L1704
            self.zp_0e = self.zp_0e.wrapping_add(0xA0);
            if self.slot(0x9B98 + x) != 0 {
                self.colour_pointer_sub_1d46();
                self.colour_fill_sub_1d52(8);
            } else {
                self.block_graphic_sub_1c9b();
            }
            self.zp_0e = self.zp_0e.wrapping_sub(0xA0);
            a = 2;
        }
        if a == 9 {
            // L1778
            let x = self.v47(0x44E) as usize;
            if self.slot(0x9B98 + x) != 0 && self.transform_timer == 0 {
                self.transform_timer = self.transform_timer.wrapping_sub(1);
                self.zp_0e = self.zp_0e.wrapping_add(0xA0);
                self.block_graphic_sub_1c9b();
            }
            let x = self.v47(0x44E) as usize;
            self.set_slot(0x9B68 + x, 0);
            return;
        }
        // L17A1
        let off = img(0x9D28 + a as usize);
        self.set_slot(0x9B88 + x, off);
        let target = self.zp_0e.wrapping_add(off as u16);
        // $12:$13 = target; compare with $C8C8 (the first row below the status bar)
        if target > 0xC8C8 {
            // L17DB: remember the tile, replace it by the rising item
            let y = off as u16;
            let covered = self.rd0e(y);
            self.set_slot(0x9B80 + x, covered);
            self.set_slot(0x9B90 + x, 1);
            self.wr0e(y, 0xF7);
            self.colour_pointer_sub_1d46();
            self.wr10(y, 1);
        } else {
            self.set_slot(0x9B90 + x, 0);
        }
        // L17F5: INC $9B68,X (X = $044E)
        let x = self.v47(0x44E) as usize;
        let v = self.slot(0x9B68 + x).wrapping_add(1);
        self.set_slot(0x9B68 + x, v);
    }

    /// sub_1C9B ($1C9B..$1CD6): draw the empty (used) block
    pub fn block_graphic_sub_1c9b(&mut self) {
        self.colour_pointer_sub_1d46();
        let cells = [(0u16, 0x4Cu8), (1, 0x4D), (2, 0x4E), (0x28, 0x4F), (0x29, 0x50), (0x2A, 0x51)];
        for (y, ch) in cells {
            self.wr0e(y, ch);
            self.wr10(y, 0x08);
        }
    }

    // ---- sub_1A17: block interaction ------------------------------------------------------------------------------------------------

    /// sub_1C21 ($1C21): `$0E:$0F += Y`
    fn add_y_sub_1c21(&mut self, y: u16) {
        self.zp_0e = self.zp_0e.wrapping_add(y);
    }

    /// sub_1C2E ($1C2E): sum of the three cells at `$0E`
    fn sum3_sub_1c2e(&self) -> u8 {
        // CLC; ADC; ADC; ADC: the carry of the first addition goes into the second
        let first = self.rd0e(0) as u16 + self.rd0e(1) as u16;
        ((first & 0xFF) + self.rd0e(2) as u16 + (first >> 8)) as u8
    }

    /// sub_1A17 ($1A17..$1C20): react to the tiles around Giana (bonus blocks, bricks, doors, bridges)
    pub fn block_interaction_sub_1a17(&mut self) {
        let t = self.surrounding_tiles;
        // $1A17: LDA $041A; CMP #$28; BCC; CMP #$2F; BCS; LDA $0448; CMP #3; BCC; LDA #3; STA $0448
        if t[4] >= 0x28 && t[4] < 0x2F && self.timer_subsecond >= 3 {
            self.timer_subsecond = 3;
        }
        // L1A2E: compare with the previous tiles ($043F..$0447)
        let mut y: i32 = 8;
        loop {
            if t[y as usize] != self.prev_tiles[y as usize] {
                break;
            }
            y -= 1;
            if y < 0 {
                return;
            }
        }
        for i in (0..=y as usize).rev() {
            self.prev_tiles[i] = t[i];
        }
        // $1A45: $041C/$041D/$041E == $8F -> L1DD1
        if t[6] == 0x8F || t[7] == 0x8F || t[8] == 0x8F {
            self.checkpoint_l1dd1();
            return;
        }
        // L1A5D
        if t[4] == 0xD9 {
            if self.st_0457 == 0 {
                self.st_046d = 0xFF;
            }
            return;
        }
        // L1A6F
        if t[1] == 0xF8 {
            if self.st_2169 != 0 || self.jump_state != 0x80 {
                return;
            }
            self.st_2169 = 0xFF;
            self.zp_0e = self.zp_0e.wrapping_sub(0xA0);
            self.reveal_l1bd2(0);
            return;
        }
        // L1A9A: bridge tiles $52/$53 below Giana
        for i in 0..3usize {
            if t[6 + i] == 0x52 || t[6 + i] == 0x53 {
                self.bridge_l1c6e(i as u8);
                return;
            }
        }
        self.hit_check_l1ab2();
    }

    /// L1AB2: which tile does Giana's head hit?
    fn hit_check_l1ab2(&mut self) {
        let t = self.surrounding_tiles;
        let f = self.surrounding_tile_flags;
        // $0E:$0F = $0432/$0433 - 1
        self.zp_0e = ((self.head_tile_ptr_high as u16) << 8 | self.head_tile_ptr_low as u16).wrapping_sub(1);
        let (a, y): (u8, u16);
        if f[0] & 0x80 != 0 {
            a = t[0];
            y = 0;
        } else if f[3] & 0x80 != 0 {
            a = t[3];
            y = 0x50;
        } else if f[2] & 0x80 != 0 {
            a = t[2];
            y = 2;
        } else if f[5] & 0x80 != 0 {
            a = t[5];
            y = 0x52;
        } else {
            self.after_hit_l1b21();
            return;
        }
        // L1AFC
        let mut yy = y;
        if a == 0xA9 {
            self.break_block_l1c3a(yy);
            return;
        }
        yy += 1;
        if a == 0xA7 {
            self.break_block_l1c3a(yy);
            return;
        }
        yy += 0x27;
        if a == 0xA8 {
            self.break_block_l1c3a(yy);
            return;
        }
        yy += 1;
        if a == 0xA6 {
            self.break_block_l1c3a(yy);
            return;
        }
        self.after_hit_l1b21();
    }

    /// L1C3A: a bonus block was hit from below; clear it and count a diamond
    fn break_block_l1c3a(&mut self, y: u16) {
        self.sfx_queue = 2;
        self.fill_19c0(0, y);
        // JSR sub_19C0 advanced Y once; TYA; CLC; ADC #$28; TAY; DEY -> second row at Y + $28
        self.fill_19c0(0, y + 0x28);
        self.diamond_l1c4f();
    }

    /// L1C4F: one more diamond (BCD), extra life at 100, score +25
    fn diamond_l1c4f(&mut self) {
        let (v, carry) = crate::enemy::bcd_adc(self.diamonds_bcd, 1, false);
        self.diamonds_bcd = v;
        if carry {
            self.lives_c64 = self.lives_c64.wrapping_add(1);
            self.sfx_queue = 8;
            self.draw_lives_sub_0e1f();
        }
        self.add_score_sub_991d(0x25, 0);
    }

    /// L1B21..: Giana is rising ($17 == $80) below a special block
    fn after_hit_l1b21(&mut self) {
        if self.jump_state != 0x80 {
            return;
        }
        let t = self.surrounding_tiles;
        if self.anim_counter != 0 {
            // L1B2F
            let y: u16;
            if t[1] == 0x43 {
                y = 2;
            } else if t[1] == 0x44 {
                y = 1;
            } else if t[1] == 0x45 {
                y = 0;
            } else {
                self.after_hit_l1bab();
                return;
            }
            self.add_y_sub_1c21(y);
            let a = self.sum3_sub_1c2e();
            if a == 0xCC {
                // L1B89: start a debris slot
                for x in (0..8usize).rev() {
                    if self.slot(0x9B02 + x) == 0 {
                        self.set_slot(0x9B02 + x, 1);
                        self.set_slot(0x9B0A + x, (self.zp_0e & 0xFF) as u8);
                        self.set_slot(0x9B12 + x, (self.zp_0e >> 8) as u8);
                        self.add_score_sub_991d(0x50, 0);
                        return;
                    }
                }
            } else if a == 0x73 {
                // L1B7A
                if self.transform_timer & 0x80 != 0 {
                    self.transform_timer = 3;
                }
                self.reveal_l1bd2(0xFF);
            } else if a == 0x74 {
                // L1B54: start a bump slot
                for x in (0..8usize).rev() {
                    if self.slot(0x9B38 + x) == 0 {
                        self.set_slot(0x9B38 + x, 1);
                        self.sfx_queue = 6;
                        self.set_slot(0x9B40 + x, (self.zp_0e & 0xFF) as u8);
                        self.set_slot(0x9B48 + x, (self.zp_0e >> 8) as u8);
                        self.add_score_sub_991d(0x50, 0);
                        return;
                    }
                }
            }
            return;
        }
        self.after_hit_l1bab();
    }

    /// L1BAB: tiles $49..$4B above Giana
    fn after_hit_l1bab(&mut self) {
        let t = self.surrounding_tiles;
        let y: u16;
        if t[1] == 0x49 {
            y = 2;
        } else if t[1] == 0x4A {
            y = 1;
        } else if t[1] == 0x4B {
            y = 0;
        } else {
            return;
        }
        self.add_y_sub_1c21(y);
        let a = self.sum3_sub_1c2e();
        if a == 0xD5 {
            self.reveal_l1bd2(0);
        } else {
            self.set_v47(0x46C, 0xFF);
            self.block_graphic_sub_1c9b();
        }
    }

    /// L1BD2: start a rising item in a free slot (Y selects what it will be)
    fn reveal_l1bd2(&mut self, y: u8) {
        for x in (0..3usize).rev() {
            if self.slot(0x9B68 + x) != 0 || self.slot(0x9B90 + x) != 0 {
                continue;
            }
            self.set_slot(0x9B98 + x, y);
            self.sfx_queue = 2;
            if self.st_2169 != 0 {
                self.block_graphic_sub_1c9b();
            }
            self.colour_pointer_sub_1d46();
            let c = self.rd10(0) & 0x0F;
            if c != 0x08 && c != 0x0F {
                return;
            }
            self.colour_fill_sub_1d52(1);
            self.set_slot(0x9B68 + x, 1);
            let p = self.zp_0e.wrapping_sub(0xA0);
            self.set_slot(0x9B70 + x, (p & 0xFF) as u8);
            self.set_slot(0x9B78 + x, (p >> 8) as u8);
            self.diamond_l1c4f();
            return;
        }
    }

    /// L1C6E: Giana stands on a bridge tile; start its crumbling
    fn bridge_l1c6e(&mut self, idx: u8) {
        if self.st_0457 == 1 {
            return;
        }
        for x in (0..8usize).rev() {
            if self.slot(0x9B50 + x) == 0 {
                // TYA; CLC; ADC #$A0; ADC $0432 (carry of the first add goes into the second); ...
                let first = idx as u16 + 0xA0;
                let lo = first + self.head_tile_ptr_low as u16;
                self.set_slot(0x9B58 + x, (lo & 0xFF) as u8);
                let hi = self.head_tile_ptr_high as u16 + (lo >> 8);
                self.set_slot(0x9B60 + x, (hi & 0xFF) as u8);
                self.set_slot(0x9B50 + x, 1);
                self.hit_check_l1ab2();
                return;
            }
        }
    }

    /// L1DD1 ($1DD1..$1E18): Giana touched the statue ($8F): checkpoint or end of the stage
    pub fn checkpoint_l1dd1(&mut self) {
        if self.st_0457 != 0 {
            return;
        }
        if self.stage == 0x20 {
            self.st_046d = 0xFF;
            return;
        }
        self.sfx_queue = 8;
        self.st_0457 = 0xFF;
        self.chk[0] = self.stage as u8; // $0458
        self.chk[1] = self.scroll_fine; // $0459
        self.chk[5] = self.player_x_low; // $045D
        self.chk[6] = self.player_y_c64; // $045E
        self.save_sprites_sub_210b();
        let p = ((self.head_tile_ptr_high as u16) << 8 | self.head_tile_ptr_low as u16).wrapping_add(0x98);
        self.chk[7] = (p & 0xFF) as u8; // $045F
        self.chk[8] = (p >> 8) as u8; // $0460
    }

    /// sub_210B ($210B..$2139): save the sprite registers (restored by sub_213A after a death)
    pub fn save_sprites_sub_210b(&mut self) {
        let mut saved = [0u8; 17];
        for i in 0..16 {
            saved[i] = self.spr(i);
        }
        saved[16] = self.sprite_x_msb;
        self.saved_sprites = saved;
        self.saved_vic = [self.en.vic_mdp, self.en.vic_mmc, self.en.vic_me];
        self.saved_colours = self.en.vic_col;
        for i in 0..8 {
            self.saved_ptrs[i] = self.sptr(i);
        }
    }

    /// sub_163C ($163C..$16A6): animated characters (water, fire ...): counts down per animation and copies the next frame of
    /// 8 bytes into the character set at `$C000..`
    pub fn animate_chars_sub_163c(&mut self) {
        let img = crate::image::img;
        loop {
            // $163C: DEC $9D32; BPL L1647; LDA #$07; STA $9D32; RTS
            self.anim_9d[0] = self.anim_9d[0].wrapping_sub(1);
            if self.anim_9d[0] & 0x80 != 0 {
                self.anim_9d[0] = 7;
                return;
            }
            // L1647: Y = $9D32; X = Y * 8
            let y = self.anim_9d[0] as usize;
            let x = (y << 3) & 0xFF;
            // $164F: LDA $060D,X; SEC; SBC #1; STA $060D,X; BPL sub_163C
            let t = self.anim_060d[x].wrapping_sub(1);
            self.anim_060d[x] = t;
            if t & 0x80 == 0 {
                continue;
            }
            let tab = |s: &Self, a: usize| s.anim_9d[a - 0x9D32];
            self.anim_060d[x] = tab(self, 0x9D33 + x);
            let dest = (tab(self, 0x9D6B + y) as u16) | (tab(self, 0x9D72 + y) as u16) << 8;
            let list = (tab(self, 0x9D39 + x) as usize) | (tab(self, 0x9D3A + x) as usize) << 8;
            self.zp_0e = dest;
            self.zp_10 = list as u16;
            // $1674: frame counter $060E,X counts down and reloads from $9D34,X
            let mut f = self.anim_060d[x + 1].wrapping_sub(1);
            if f & 0x80 != 0 {
                f = tab(self, 0x9D34 + x);
            }
            self.anim_060d[x + 1] = f;
            // $1685: ASL A; TAY; ($10),Y -> $12/$13
            let yy = (f as usize) << 1;
            let src = (img(list + yy) as usize) | (img(list + yy + 1) as usize) << 8;
            self.zp_12 = src as u16;
            // $1690: Y = $9D35,X; copy Y+1 bytes (Y down to 0)
            let mut cnt = tab(self, 0x9D35 + x) as i32;
            while cnt >= 0 {
                let d = dest as usize + cnt as usize;
                if d >= 0xC000 {
                    self.sprite_mem[d - 0xC000] = img(src + cnt as usize);
                }
                cnt -= 1;
            }
            // $169B: copy the 8 bytes at $C238 to $C790
            for i in (0..8usize).rev() {
                self.sprite_mem[0x790 + i] = self.sprite_mem[0x238 + i];
            }
            return;
        }
    }
}
