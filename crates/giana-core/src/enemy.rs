//! Enemy subsystem of the original ($2194 and everything it calls).
//!
//! The original keeps its state in fixed RAM areas, so the port keeps the same layout instead of an object
//! model: the enemy property columns (`$071C + 8 * column + slot`), the spawner/mover scratch bytes
//! (`$0470..$049F`), the sprite helper block (`$3200..$325F`) and the sprite registers. Sprite slots 4..7
//! ($05F4,X with X = 8..14) are the enemies, slot 3 is the fireball, slots 0..2 are Giana.
//!
//! Hardware the original touches is modelled as shadow registers (`$D015`, `$D01B`, `$D01C`, `$D027..`) and
//! the sprite/sprite collision latch `$D01E` is computed from the sprite data the video chip would have seen
//! (`vic_sprite_collisions`).

use crate::image::{img, img_word};
use crate::state::GameState;

/// First address of the enemy property columns.
const COL_BASE: usize = 0x071C;
/// First address of the scratch block (`$044E..$049F`): tile animation counters, spawner and mover variables.
const V_BASE: usize = 0x044E;
/// First address of the sprite helper block.
const D_BASE: usize = 0x3200;

/// Registers of the video chip as the chip itself saw them while it drew the last frame.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VicFrame {
    pub me: u8,
    pub xy: [u8; 17],
    pub ptr: [u8; 8],
    pub mmc: u8,
    /// Video chip registers actually used to draw the picture on screen (from previous frame)
    pub draw_xscroll: u8,
    /// `$0405`, which the handler at line `$41` writes to `$D016` for the cloud band
    pub draw_sky_xscroll: u8,
    pub draw_me: u8,
    pub draw_mmc: u8,
    pub draw_mdp: u8,
    pub draw_xy: [u8; 17],
    pub draw_ptr: [u8; 8],
    pub draw_col: [u8; 8],
    /// Cloud rows (`screen_ram[80..200]`, `$C850..$C8C7`): sub_0EAB rotates them in the middle of the frame, after the beam
    /// has drawn them, so the picture shows the state of the previous frame
    pub draw_clouds: Vec<u8>,
    /// Character set `$C000..$C7FF` as it stood before `sub_163C` (animated characters) ran in the handler at line `$FC`,
    /// after the beam had drawn the picture
    pub draw_chars: Vec<u8>,
}

impl Default for VicFrame {
    fn default() -> Self {
        Self {
            me: 0,
            xy: [0; 17],
            ptr: [0; 8],
            mmc: 0,
            draw_xscroll: 7,
            draw_sky_xscroll: 0,
            draw_me: 0,
            draw_mmc: 0,
            draw_mdp: 0,
            draw_xy: [0; 17],
            draw_ptr: [0; 8],
            draw_col: [0; 8],
            draw_clouds: vec![0; 120],
            draw_chars: vec![0; 0x800],
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EnemyState {
    /// `$071C..$07BB`: 20 property columns of 8 slots each.
    #[serde(with = "giana_engine_b::snap::arr")]
    pub col: [u8; 160],
    /// `$044E..$049F`
    #[serde(with = "giana_engine_b::snap::arr")]
    pub v: [u8; 0x52],
    /// `$3200..$325F`
    #[serde(with = "giana_engine_b::snap::arr")]
    pub d: [u8; 0x60],
    /// `$05FA..$0603`: X/Y of sprites 3..7
    pub spr_xy: [u8; 10],
    /// `$0608..$060C`: frame pointers of sprites 3..7
    pub spr_ptr: [u8; 5],
    /// `$D015`
    pub vic_me: u8,
    /// `$D01B`
    pub vic_mdp: u8,
    /// `$D01C`
    pub vic_mmc: u8,
    /// `$D027..$D02E`
    pub vic_col: [u8; 8],
    /// `$D021`, `$D022`
    pub vic_bg0: u8,
    pub vic_bg1: u8,
    /// `$D418` (volume)
    pub sid_volume: u8,
    /// Accumulated `$D01E` latch
    pub vic_d01e: u8,
    /// Keyboard matrix, one byte per `$DC00` column (bit = 0: key pressed)
    pub kbd: [u8; 8],
    /// Last value written to `$DC00`
    pub cia_pra: u8,
    /// What the video chip drew with
    pub vic_frame: VicFrame,
    /// `$157D`: pause flag
    pub pause_157d: u8,
    /// `$157E`
    pub flag_157e: u8,
    /// `$0497` palette flash counter lives in `v`; the colour save area is `d[0x34]`.
    pub dummy: u8,
}

impl EnemyState {
    pub fn new() -> Self {
        let mut d = [0u8; 0x60];
        for (i, b) in d.iter_mut().enumerate() {
            *b = img(D_BASE + i);
        }
        // RAM is cleared at boot; the title demo scrolls the enemy sprites 3..7 (sub_20E7) from X = 0
        let spr_xy = [0u8; 10];
        Self {
            col: [0; 160],
            v: [0; 0x52],
            d,
            spr_xy,
            spr_ptr: [0; 5],
            vic_me: 0,
            vic_mdp: 0,
            vic_mmc: 0,
            vic_col: [0; 8],
            vic_bg0: 0,
            vic_bg1: 0,
            sid_volume: 0,
            vic_d01e: 0,
            kbd: [0xFF; 8],
            cia_pra: 0xFF,
            vic_frame: VicFrame::default(),
            pause_157d: 0,
            flag_157e: 0,
            dummy: 0,
        }
    }
}

impl Default for EnemyState {
    fn default() -> Self {
        Self::new()
    }
}

/// 6502 `ADC` in decimal mode for valid BCD operands. Returns (result, carry out).
pub fn bcd_adc(a: u8, b: u8, carry: bool) -> (u8, bool) {
    let mut lo = (a & 0x0F) + (b & 0x0F) + carry as u8;
    if lo > 9 {
        lo += 6;
    }
    let mut hi = (a >> 4) + (b >> 4) + (lo > 0x0F) as u8;
    lo &= 0x0F;
    if hi > 9 {
        hi += 6;
    }
    (((hi & 0x0F) << 4) | lo, hi > 0x0F)
}

impl GameState {
    // ---- raw RAM style accessors -------------------------------------------------------------------------

    #[inline]
    pub fn ep(&self, addr: usize) -> u8 {
        self.en.col[addr - COL_BASE]
    }
    #[inline]
    pub fn set_ep(&mut self, addr: usize, v: u8) {
        self.en.col[addr - COL_BASE] = v;
    }
    #[inline]
    pub fn v47(&self, addr: usize) -> u8 {
        self.en.v[addr - V_BASE]
    }
    #[inline]
    pub fn set_v47(&mut self, addr: usize, v: u8) {
        self.en.v[addr - V_BASE] = v;
    }
    #[inline]
    pub fn d32(&self, addr: usize) -> u8 {
        self.en.d[addr - D_BASE]
    }
    #[inline]
    pub fn set_d32(&mut self, addr: usize, v: u8) {
        self.en.d[addr - D_BASE] = v;
    }

    /// `$05F4 + off` (off 0..15): sprite X (even) / Y (odd) registers of the software buffer.
    pub fn spr(&self, off: usize) -> u8 {
        match off {
            0 => self.player_x_low,
            1 => self.player_y_c64,
            2 => self.sprite_head_x,
            3 => self.sprite_head_y,
            4 => self.sprite_dress_x,
            5 => self.sprite_dress_y,
            6..=15 => self.en.spr_xy[off - 6],
            _ => 0,
        }
    }
    pub fn set_spr(&mut self, off: usize, v: u8) {
        match off {
            0 => self.player_x_low = v,
            1 => self.player_y_c64 = v,
            2 => self.sprite_head_x = v,
            3 => self.sprite_head_y = v,
            4 => self.sprite_dress_x = v,
            5 => self.sprite_dress_y = v,
            6..=15 => self.en.spr_xy[off - 6] = v,
            _ => {}
        }
    }

    /// `$0605 + i`: sprite frame pointers.
    pub fn sptr(&self, i: usize) -> u8 {
        match i {
            0 => self.sprite_0_frame,
            1 => self.sprite_1_frame,
            2 => self.sprite_2_frame,
            3..=7 => self.en.spr_ptr[i - 3],
            _ => 0,
        }
    }
    pub fn set_sptr(&mut self, i: usize, v: u8) {
        match i {
            0 => self.sprite_0_frame = v,
            1 => self.sprite_1_frame = v,
            2 => self.sprite_2_frame = v,
            3..=7 => self.en.spr_ptr[i - 3] = v,
            _ => {}
        }
    }

    /// Screen RAM byte at a C64 address ($C800..).
    fn scr(&self, addr: usize) -> u8 {
        if (0xC800..0xC800 + 1000).contains(&addr) {
            self.screen_ram[addr - 0xC800]
        } else {
            img(addr)
        }
    }

    /// `$DC01` for the current `$DC00` column selection.
    fn cia_read_pb(&self) -> u8 {
        let mut r = 0xFFu8;
        for col in 0..8 {
            if self.en.cia_pra & (1 << col) == 0 {
                r &= self.en.kbd[col];
            }
        }
        r
    }

    /// sub_9F40 + LSR A: carry = bit 0 of the tile classification
    #[inline]
    fn tile_solid(ch: u8) -> bool {
        GameState::tile_lookup_sub_9f40(ch) & 1 != 0
    }

    // ---- sub_9936: stage map decompressor -------------------------------------------------------------------------

    /// sub_9936 ($9936..$9A4E): expand the stage stream at `$6000` into the map `$AC00..$BFFF`
    ///
    /// A stream record has five bytes: block number, destination offset (low, high), horizontal and vertical repeat
    /// count; `$FF` ends the stream. A block (`$9200` table) is `width - 1`, `height - 1` and `width * height` tiles.
    pub fn decompress_stage_sub_9936(&mut self) {
        // $9936..$9948: clear $AC00..$BFFF
        self.current_map = vec![0; 5120];
        let mut rec = match self.tnw_stream() {
            Some(s) => s,
            None => img_word(0x6000 + ((self.stage as usize).wrapping_sub(1) << 1)),
        };
        loop {
            // L995D
            let id = self.stage_byte(rec);
            if id == 0xFF {
                self.map_column = 0x27;
                return;
            }
            let mut src = img_word(0x9200 + ((id as usize) << 1));
            self.dec[0] = self.stage_byte(rec + 3); // $040C
            self.dec[1] = self.stage_byte(rec + 3); // $040D
            self.dec[2] = self.stage_byte(rec + 4); // $040E
            self.dec[3] = self.stage_byte(rec + 4); // $040F
            self.dec[4] = img(src); // $0410
            self.dec[5] = img(src); // $0411
            self.dec[6] = img(src + 1); // $0412
            self.dec[7] = img(src + 1); // $0413
            src += 2;
            let mut dest = 0xAC00usize + self.stage_byte(rec + 1) as usize + ((self.stage_byte(rec + 2) as usize) << 8);
            let row_lo = dest & 0xFF;
            let mut row_hi = dest >> 8;
            let src_start = src;
            loop {
                // L99D0
                let w1 = self.dec[4] as usize;
                // TYA; CLC; ADC $06; LDA $07; ADC #0; CMP #$C0; BCC L99E3
                let carry = ((w1 & 0xFF) + (dest & 0xFF) > 0xFF) as usize;
                if (dest >> 8) + carry >= 0xC0 {
                    self.en.vic_bg0 = self.en.vic_bg0.wrapping_add(1); // INC $D021
                    return;
                }
                for y in (0..=w1).rev() {
                    let a = dest + y;
                    if (0xAC00..0xC000).contains(&a) {
                        self.current_map[a - 0xAC00] = img(src + y);
                    }
                }
                dest += w1 + 1;
                // DEC $040C; BNE L99D0
                self.dec[0] = self.dec[0].wrapping_sub(1);
                if self.dec[0] != 0 {
                    continue;
                }
                self.dec[0] = self.dec[1];
                src += w1 + 1;
                // INC $09: next row
                row_hi += 1;
                dest = (row_hi << 8) | row_lo;
                // DEC $0412; BPL L99D0
                self.dec[6] = self.dec[6].wrapping_sub(1);
                if self.dec[6] & 0x80 == 0 {
                    continue;
                }
                src = src_start;
                self.dec[6] = self.dec[7];
                // DEC $040E; BNE L99D0
                self.dec[2] = self.dec[2].wrapping_sub(1);
                if self.dec[2] != 0 {
                    continue;
                }
                break;
            }
            rec += 5;
        }
    }

    // ---- sub_991D --------------------------------------------------------------------------------------------

    /// sub_991D ($991D..$9935): add the BCD value A (low) / X (high) to the score at $0435..$0437
    pub fn add_score_sub_991d(&mut self, a: u8, x: u8) {
        let (r0, c) = bcd_adc(a, self.score_bcd[0], false);
        self.score_bcd[0] = r0;
        let (r1, c) = bcd_adc(x, self.score_bcd[1], c);
        self.score_bcd[1] = r1;
        let (r2, _) = bcd_adc(self.score_bcd[2], 0, c);
        self.score_bcd[2] = r2;
    }

    // ---- sub_98D9 / sub_0E1F -----------------------------------------------------------------------------------

    /// sub_0E1F ($0E1F..$0E38): show the lives counter (`$9FAE[$043C]` converted to BCD) in the status bar
    pub fn draw_lives_sub_0e1f(&mut self) {
        let x = self.lives_c64 as usize;
        let v = self.bcd_table(x);
        self.stage_text_157b = v;
        self.print_bcd_sub_98d9(&[v], 0, 0x3D);
    }

    /// sub_20E7 ($20E7..$210A): subtract the scroll step `$0468` from the X of sprites 3..7 and fix their MSBs
    pub fn shift_sprites_sub_20e7(&mut self) {
        let mut x: u8 = 0;
        let mut y = 14usize;
        while y != 4 {
            let (v, borrow) = self.spr(y).overflowing_sub(self.transform_step);
            self.set_spr(y, v);
            // TXA; ROL A; TAX: the carry (no borrow) is rolled into X
            x = (x << 1) | (!borrow) as u8;
            y -= 2;
        }
        // TXA; ASL A; ASL A; ASL A; EOR #$FF; AND #$F8; EOR $0604; STA $0604
        let m = ((x << 3) ^ 0xFF) & 0xF8;
        self.sprite_x_msb ^= m;
    }

    /// sub_1D67 ($1D67..$1DB6): decide whether Giana dies this frame
    pub fn death_check_sub_1d67(&mut self) {
        if self.hazard_hit != 0 {
            return;
        }
        if self.st_0457 == 0xFF {
            // $1D72..: dying: switch the sprites off once she has fallen out of the top part of the screen
            if self.player_y_c64 >= 0x1E {
                return;
            }
            self.en.vic_me = 0;
            return;
        }
        if self.st_0457 != 0 {
            return;
        }
        // L1D87
        let ch = self.surrounding_tiles[7];
        let hit = if self.player_y_c64 >= 0xFA || ch == 0xA2 || ch == 0xA3 {
            true
        } else {
            let f = self.surrounding_tile_flags[3] | self.surrounding_tile_flags[4] | self.surrounding_tile_flags[5];
            if f & 0x40 == 0 {
                return; // L1DB6
            }
            // f bit 5 set and $22 >= 7: harmless
            !(f & 0x20 != 0 && self.anim_counter >= 7)
        };
        if hit && self.jump_state != 0x80 {
            self.player_hit_l1db7();
        }
    }

    // ---- sub_2194 -----------------------------------------------------------------------------------------------

    /// sub_2194 ($2194..$2275): enemy/sprite handler of the last raster IRQ
    pub fn enemy_main_sub_2194(&mut self) {
        // L2198: LDA $0497; BEQ L21B7
        if self.v47(0x497) != 0 {
            // DEC $0497; LDY $0497; LDA $3234,Y; STA $D021
            let y = self.v47(0x497).wrapping_sub(1);
            self.set_v47(0x497, y);
            self.en.vic_bg0 = self.d32(0x3234usize.wrapping_add(y as usize).min(0x325F)) & 0x0F;
            // TYA; CLC; ADC #$04; TAX; JSR sub_29B2
            self.kill_enemy_sub_29b2((y.wrapping_add(4)) as usize);
            // LDA #$0D; STA $154C
            self.sfx_queue = 0x0D;
            return;
        }
        // L21B7: LDA $1D; BNE L2233
        if self.zp_1d == 0 {
            // keyboard scan: $DC00 = $FD; CMP #$7F
            self.en.cia_pra = 0xFD;
            if self.cia_read_pb() == 0x7F {
                // L21F1
                self.en.pause_157d = 0xFF;
                self.en.sid_volume = 0;
                return;
            }
            self.en.cia_pra = 0xE9;
            if self.cia_read_pb() == 0x69 {
                if self.engine_b.is_some() {
                    // family B (`$7789`: `CMP #$69; BEQ $77A6`): the original's stage skip (`$046D = $FF`) is gone in every 7A3 hack.
                    // Junior replaced the routine by `INC $D020; INC $D021; RTS` (it runs in the raster IRQ: the sprite program of the
                    // frame, i.e. the enemies, is skipped while the keys are held, Giana moves on, the colours count up: checked in VICE), the others print "HAHAHA THE ARMINMODE WAS KILLED BY MERLIN" and hang in
                    // `JMP $77B6` (here: the sprite program is skipped as long as the keys are held; the app has no such key combination).
                    if img(0x77A6) == 0xEE {
                        self.en.vic_bg0 = self.en.vic_bg0.wrapping_add(1); // INC $D021 (the border, `$D020`, is not modelled)
                    }
                    return;
                }
                // L21EB
                self.st_046d = 0xFF;
                return;
            }
            self.en.cia_pra = 0x7F;
            let k = self.cia_read_pb();
            let mut skip_clear = false;
            if k == 0xEF {
                match self.anim_counter {
                    5 => {
                        // L2241
                        self.anim_counter = 4;
                        self.set_v47(0x494, 4);
                        self.set_v47(0x495, 0);
                        self.enemy_clear_hit_slots_l224d();
                        return;
                    }
                    6 => {
                        // L2261
                        self.anim_counter = 4;
                        self.set_v47(0x497, 4);
                        self.sfx_queue = 0x0E;
                        // LDA $D021: the upper nibble of a colour register reads as 1
                        self.set_d32(0x3234, self.en.vic_bg0 | 0xF0);
                        self.enemy_clear_hit_slots_l224d();
                        return;
                    }
                    _ => skip_clear = true, // BNE L2200
                }
            }
            if !skip_clear {
                // L21FB: LDA #$00; STA $157D
                self.en.pause_157d = 0;
            }
            // L2200
            if self.scroll_wrap_flag == 0 {
                if self.st_0457 == 0 {
                    if !self.joystick_enabled {
                        // LDA sub_9F77 == $60 -> JMP L2D41
                        self.cutscene_walk_l2d41();
                    } else if self.engine_b.is_some() {
                        // family B (`$776E`): the new sprite program replaces the original's routines
                        if self.flag_0431 == 0 && self.music_paused == 0 {
                            self.b_sprite_frame();
                        }
                    } else if self.flag_0431 == 0 && self.music_paused == 0 {
                        // L2213..: CLI; JSR sub_2276, 288A, 255A, 24FA, 251F, 2A69
                        self.spawner_sub_2276();
                        self.collision_handler_sub_288a();
                        self.enemy_movement_sub_255a();
                        self.clip_sprites_sub_24fa();
                        self.clip_sprites_sub_251f();
                        self.fire_sub_2a69();
                        // L2230
                        self.fireball_sub_2ad5();
                    }
                }
            } else {
                // L2230 (BNE L2230 when $14 != 0); family B (`$7801`): the shot flies on
                if self.engine_b.is_some() {
                    self.b_shot_flight();
                } else {
                    self.fireball_sub_2ad5();
                }
            }
        }
        // L2233: LDA $1D; BEQ L223C; LDX #0; JSR sub_2F6E
        if self.zp_1d != 0 {
            self.sprite_colour_cycle_sub_2f6e(0);
        }
        // L223C: LDA #0; STA $1F
        self.land_flag = 0;
    }

    /// L224D: LDX #7 .. CPX #4: sprites of type 2 get column $071C cleared
    fn enemy_clear_hit_slots_l224d(&mut self) {
        let mut x = 7usize;
        loop {
            if self.ep(0x744 + x) == 2 {
                self.set_ep(0x71C + x, 0);
            }
            if x == 0 {
                break;
            }
            x -= 1;
            if x < 4 {
                break;
            }
        }
    }

    // ---- sub_2276: spawner -----------------------------------------------------------------------------------------

    /// sub_2276 ($2276..$24F9): read the stage object list and create an enemy when the map column is reached
    pub fn spawner_sub_2276(&mut self) {
        // $2276: LDA $20; CMP $0484; BEQ L2285
        let stage = self.stage as u8;
        if stage != self.v47(0x484) {
            self.set_v47(0x484, stage);
            self.set_v47(0x474, 0);
        }
        // L2285: LDA $21; CMP $0475; BEQ L22CC
        if self.map_column == self.v47(0x475) {
            return;
        }
        self.set_v47(0x475, self.map_column);
        // LDA $D015; CMP #$FF; BEQ L22CC
        if self.en.vic_me == 0xFF {
            return;
        }
        // $2296: LDY $20; DEY; TYA; ASL A; TAY; pointer from $3D20,Y
        let y = ((stage.wrapping_sub(1)) << 1) as usize;
        let list = match self.tnw_enemy_list() {
            Some(l) => l,
            None => img_word(0x3D20 + y),
        };
        // The index is 8 bits and steps by 3, so after 86 steps it has seen every entry: a list without its $FF end (a
        // broken or hostile module's stage file) would loop forever, which a real list never needs.
        for _ in 0..86 {
            // L22A6
            let y = self.v47(0x474) as usize;
            let a = self.stage_byte(list + y);
            if a == 0xFF {
                return;
            }
            // SEC; SBC #$01; CMP $21
            let t = a.wrapping_sub(1);
            if t == self.map_column {
                self.spawn_match_l22cd(list, y);
                return;
            }
            if t >= self.map_column {
                return;
            }
            // CLC; LDA $0474; ADC #$03; STA $0474; LDA $0475; ADC #$00; STA $0475
            let (n, c) = self.v47(0x474).overflowing_add(3);
            self.set_v47(0x474, n);
            let h = self.v47(0x475).wrapping_add(c as u8);
            self.set_v47(0x475, h);
        }
    }

    fn spawn_match_l22cd(&mut self, list: usize, y: usize) {
        // L22CD: advance $0474 by 3 (carry into $0475)
        let (n, c) = self.v47(0x474).overflowing_add(3);
        self.set_v47(0x474, n);
        let h = self.v47(0x475).wrapping_add(c as u8);
        self.set_v47(0x475, h);
        // $3221 = list[y], $3222 = list[y+1], third byte = enemy kind ($0B -> $3032,$22)
        self.set_d32(0x3221, self.stage_byte(list + y));
        self.set_d32(0x3222, self.stage_byte(list + y + 1));
        let mut a = self.stage_byte(list + y + 2);
        if a == 0x0B {
            a = img(0x3032 + self.anim_counter as usize);
        }
        self.set_d32(0x3223, a);
        // ASL A; TAX; pointer from $303B,X
        let rec = img_word(0x303B + ((a << 1) as usize));
        // $2304: LDY #$07; LDA $D015; scan for a free sprite slot 7..4
        let mut free = None;
        let mut me = self.en.vic_me;
        let mut y = 7usize;
        loop {
            let carry = me & 0x80 != 0;
            me <<= 1;
            if !carry {
                free = Some(y);
                break;
            }
            y -= 1;
            if y < 4 {
                break;
            }
        }
        let slot = match free {
            Some(s) => s,
            None => return, // L2311: RTS (object is lost)
        };
        self.set_v47(0x476, slot as u8);
        let mut x = slot;
        // L2315: LDY #$03; LDA ($12),Y; CMP #$03; BCC L2335
        if img(rec + 3) >= 3 {
            self.set_d32(0x3252, 0);
            self.set_ep(0x7AB, 0);
            self.set_ep(0x7AA, 0);
            self.set_ep(0x7A9, 0);
            self.set_d32(0x3259, 0x28);
            x = 7;
            self.set_v47(0x476, 7);
        }
        // L2335: copy 20 property bytes (stride 8)
        self.set_v47(0x477, 0x14);
        let mut xx = x;
        for yy in 3usize..23 {
            self.set_ep(COL_BASE + xx, img(rec + yy));
            xx += 8;
        }
        self.set_v47(0x477, 0);
        // L234D
        let kind = self.ep(COL_BASE + x);
        if kind < 3 {
            self.spawn_small_l2444(rec, x);
        } else if kind < 4 {
            self.spawn_boss_a_l235a(rec, x);
        } else {
            self.spawn_boss_b_l23d1(rec);
        }
    }

    /// L235A..$23D0 (type 3)
    fn spawn_boss_a_l235a(&mut self, rec: usize, x: usize) {
        self.set_d32(0x3251, 4);
        let base = img(rec);
        self.en.spr_ptr[4] = base; // $060C
        self.en.spr_ptr[3] = base.wrapping_add(1); // $060B
        self.en.spr_ptr[2] = base.wrapping_add(2); // $060A
        self.en.vic_mmc |= 0xE0;
        self.en.vic_mdp &= 0x0F;
        let c = img(rec + 2);
        self.en.vic_col[5] = c; // $D02C
        self.en.vic_col[6] = c; // $D02D
        self.en.vic_col[7] = c; // $D02E
        self.sprite_x_msb |= 0xE0;
        self.en.spr_xy[0x0602 - 0x05FA] = 0x4F;
        self.en.spr_xy[0x0600 - 0x05FA] = 0x67;
        self.en.spr_xy[0x05FE - 0x05FA] = 0x7F;
        let y = (self.d32(0x3205 + self.d32(0x3222) as usize)).wrapping_add(0x1D);
        self.en.spr_xy[0x05FF - 0x05FA] = y;
        self.en.spr_xy[0x0601 - 0x05FA] = y;
        self.en.spr_xy[0x0603 - 0x05FA] = y;
        let _ = x;
        self.en.vic_me |= self.ep(0x744 + 7);
        self.set_d32(0x3253, 0);
        self.set_d32(0x3256, 0);
        self.set_d32(0x325A, 0);
    }

    /// L23D1..$2443 (type >= 4)
    fn spawn_boss_b_l23d1(&mut self, rec: usize) {
        let base = img(rec);
        self.en.spr_ptr[4] = base;
        self.en.spr_ptr[3] = base.wrapping_add(1);
        self.en.spr_ptr[2] = base.wrapping_add(2);
        self.en.spr_ptr[1] = base.wrapping_add(3); // $0609
        self.en.vic_mmc &= 0x0F;
        self.en.vic_mdp &= 0x1F;
        self.en.vic_col[4] = 0x07; // $D02B
        self.en.vic_col[5] = 0x07;
        self.en.vic_col[6] = 0x01;
        self.en.vic_col[7] = 0x01;
        self.sprite_x_msb |= 0xF0;
        self.en.spr_xy[0x0602 - 0x05FA] = 0x4F;
        self.en.spr_xy[0x0600 - 0x05FA] = 0x4F;
        self.en.spr_xy[0x05FE - 0x05FA] = 0x4F;
        self.en.spr_xy[0x05FC - 0x05FA] = 0x4F;
        let y0 = self.d32(0x3205 + self.d32(0x3222) as usize);
        self.en.spr_xy[0x05FF - 0x05FA] = y0;
        self.en.spr_xy[0x0603 - 0x05FA] = y0;
        let y1 = y0.wrapping_add(0x15);
        self.en.spr_xy[0x0601 - 0x05FA] = y1;
        self.en.spr_xy[0x05FD - 0x05FA] = y1;
        self.en.vic_me |= 0xF0;
        self.set_d32(0x3253, 0);
        self.set_d32(0x3256, 0);
        self.set_d32(0x325A, 0);
    }

    /// L2444..$24F9: normal enemy
    fn spawn_small_l2444(&mut self, rec: usize, x: usize) {
        self.set_sptr(x, img(rec));
        self.set_ep(0x794 + x, 1);
        self.set_ep(0x7A4 + x, 0);
        let v = self.ep(0x784 + x);
        self.set_ep(0x79C + x, v);
        let v = self.ep(0x77C + x);
        self.set_ep(0x78C + x, v);
        let mask = self.d32(0x3224 + x);
        let nmask = self.d32(0x322C + x);
        if self.ep(0x744 + x) == 2 {
            self.set_v47(0x46C, 0);
            self.set_v47(0x492, img(rec + 1));
            self.set_v47(0x493, img(rec + 2));
            self.en.vic_mmc &= nmask;
            self.en.vic_mdp |= mask;
            self.en.vic_col[x] = self.en.vic_bg1;
            self.set_ep(0x794 + x, 0);
        } else {
            let b = img(rec + 1);
            if b & 1 != 0 {
                self.en.vic_mmc &= nmask;
            } else {
                self.en.vic_mmc |= mask;
            }
            if b & 2 != 0 {
                self.en.vic_mdp |= mask;
            } else {
                self.en.vic_mdp &= nmask;
            }
            let c = img(rec + 2);
            self.set_ep(0x7AC + x, c);
            self.en.vic_col[x] = c & 0x7F;
        }
        // L24D4
        self.sprite_x_msb |= mask;
        self.set_spr(2 * x, 0x4F);
        let ty = self.d32(0x3205 + self.d32(0x3222) as usize).wrapping_add(0x1D);
        self.set_spr(2 * x + 1, ty);
        self.en.vic_me |= mask;
    }

    // ---- sub_24FA / sub_251F / sub_253C / sub_2547 ----------------------------------------------------------------

    /// sub_2547: disable sprite Y (VIC enable + X MSB)
    fn sprite_off_sub_2547(&mut self, y: usize) {
        let m = self.d32(0x322C + y);
        self.en.vic_me &= m;
        self.sprite_x_msb &= m;
    }

    /// sub_253C ($253C..$2546)
    fn sprite_off_sub_253c(&mut self, y: usize) {
        if self.ep(0x723) < 3 || y == 3 {
            self.sprite_off_sub_2547(y);
        }
    }

    /// sub_24FA ($24FA..$251E): switch off sprites whose X (with MSB) left the right edge
    pub fn clip_sprites_sub_24fa(&mut self) {
        let mut y: u8 = 7;
        let mut a = self.sprite_x_msb;
        loop {
            // L24FF: ASL A; BCS L2508
            let carry = a & 0x80 != 0;
            a <<= 1;
            if carry {
                // L2508
                let x = (y as usize) * 2;
                let xa = self.spr(x);
                if (0x5A..0xF0).contains(&xa) {
                    self.sprite_off_sub_253c(y as usize);
                }
                y = y.wrapping_sub(1);
            } else {
                y = y.wrapping_sub(1);
                if y < 3 {
                    return;
                }
            }
        }
    }

    /// sub_251F ($251F..$2546): switch off enemy sprites outside X = $1E..$F9
    pub fn clip_sprites_sub_251f(&mut self) {
        let mut x = 0x0Fusize;
        loop {
            let a = self.spr(x);
            if !(0x1E..0xFA).contains(&a) {
                self.sprite_off_sub_2547(x >> 1);
            }
            x -= 2;
            if x < 7 {
                return;
            }
        }
    }

    // ---- sub_255A: movement ---------------------------------------------------------------------------------------------

    /// sub_255A ($255A..$269C): move the enemies
    pub fn enemy_movement_sub_255a(&mut self) {
        // $255A: LDA $0494; BEQ L2569; DEC $0495; BNE rts; LDA #0; STA $0494
        if self.v47(0x494) != 0 {
            let n = self.v47(0x495).wrapping_sub(1);
            self.set_v47(0x495, n);
            if n != 0 {
                return;
            }
            self.set_v47(0x494, 0);
        }
        // L2569
        let mut y: usize = 7;
        let mut a = self.en.vic_me;
        loop {
            // L256E: ASL A; BCS L2577
            let carry = a & 0x80 != 0;
            a <<= 1;
            if carry {
                // L2577
                self.set_v47(0x479, a);
                self.set_v47(0x47A, y as u8);
                self.set_v47(0x47B, (y * 2) as u8);
                let kind = self.ep(COL_BASE + y);
                if kind >= 3 {
                    if kind == 4 {
                        return;
                    }
                    self.boss_move_l2dd3();
                    return;
                }
                self.move_one_enemy_l2590();
                a = self.v47(0x479);
                y = self.v47(0x47A) as usize;
            }
            // L2571
            if y == 0 {
                return;
            }
            y -= 1;
            if y < 4 {
                return;
            }
        }
    }

    fn move_one_enemy_l2590(&mut self) {
        // L2590: JSR sub_27D3; BCC L2598; JMP L2691
        if self.activate_sub_27d3() {
            self.enemy_colour_l2691();
            return;
        }
        if self.stunned_sub_27af() {
            self.enemy_colour_l2691();
            return;
        }
        let x = self.v47(0x47A) as usize;
        let xs = self.v47(0x47B) as usize;
        if self.ep(COL_BASE + x) == 0 {
            // L25FE
            self.probe_tiles_sub_26ed();
            let x = self.v47(0x47A) as usize;
            let mode = self.ep(0x784 + x);
            let mut goto_265f = false;
            if mode == 0 {
                // L2638
                if Self::tile_solid(self.v47(0x47D)) || Self::tile_solid(self.v47(0x47E)) || Self::tile_solid(self.v47(0x47F)) {
                    goto_265f = true;
                }
            } else if mode == 2 {
                goto_265f = true;
            } else if Self::tile_solid(self.v47(0x47E)) {
                // LDA $047E; JSR sub_9F40; LSR A; BCC L2653 -> carry set here: fall on
                if self.ep(0x794 + x) == 0 {
                    // L2629
                    if !Self::tile_solid(self.v47(0x47F)) {
                        self.toggle_dir_sub_269d();
                    }
                    goto_265f = true;
                } else if !Self::tile_solid(self.v47(0x47D)) {
                    // L2632
                    self.toggle_dir_sub_269d();
                    goto_265f = true;
                } else {
                    goto_265f = true;
                }
            }
            if !goto_265f {
                // L2653: fall one pixel pair: Y += 4
                let v = self.spr(xs + 1).wrapping_add(4);
                self.set_spr(xs + 1, v);
            }
            // L265F
            let x = self.v47(0x47A) as usize;
            let t = if self.ep(0x794 + x) != 0 { self.v47(0x47C) } else { self.v47(0x481) };
            if Self::tile_solid(t) {
                self.toggle_dir_sub_269d();
            }
            // L2679
            let y = self.v47(0x47A) as usize;
            let xs = self.v47(0x47B) as usize;
            self.set_v47(0x482, self.ep(0x774 + y));
            self.set_v47(0x483, self.ep(0x794 + y));
            self.move_x_sub_26b7(xs, y);
        } else if self.ep(0x794 + x) == 0 {
            // L25AD: jumping up
            let n = self.ep(0x79C + x).wrapping_sub(1);
            self.set_ep(0x79C + x, n);
            if n == self.ep(0x784 + x) {
                self.toggle_dir_sub_269d();
            } else {
                let d = img(0x9ADE + n as usize);
                let yy = self.v47(0x47A) as usize;
                let v = d.wrapping_add(self.spr(xs + 1)).wrapping_add(self.ep(0x774 + yy));
                self.set_spr(xs + 1, v);
            }
        } else {
            // L25D6
            let n = self.ep(0x79C + x).wrapping_add(1);
            self.set_ep(0x79C + x, n);
            let d = img(0x9ADE + n as usize);
            self.set_v47(0x488, d);
            if d == 0xFF {
                self.toggle_dir_sub_269d();
            } else {
                let yy = self.v47(0x47A) as usize;
                let v = self.spr(xs + 1).wrapping_sub(d).wrapping_sub(self.ep(0x774 + yy));
                self.set_spr(xs + 1, v);
            }
        }
        // L268E
        self.animate_sub_2831();
        self.enemy_colour_l2691();
    }

    /// L2691: JSR sub_2F5C
    fn enemy_colour_l2691(&mut self) {
        self.sprite_colour_check_sub_2f5c();
    }

    /// sub_269D ($269D..$26B6): turn around
    fn toggle_dir_sub_269d(&mut self) {
        let x = self.v47(0x47A) as usize;
        let v = self.ep(0x794 + x) ^ 1;
        self.set_ep(0x794 + x, v);
        let p = if v == 0 { self.ep(0x724 + x) } else { self.ep(0x734 + x) };
        self.set_sptr(x, p);
    }

    /// sub_26B7 ($26B7..$26E0): move sprite (offset X, mask index Y) horizontally by $0482 (direction $0483)
    fn move_x_sub_26b7(&mut self, xs: usize, y: usize) {
        let amount = self.v47(0x482);
        
        let toggle = if self.v47(0x483) != 0 {
            let (v, borrow) = self.spr(xs).overflowing_sub(amount);
            self.set_spr(xs, v);
            borrow
        } else {
            let (v, carry) = self.spr(xs).overflowing_add(amount);
            self.set_spr(xs, v);
            carry
        };
        if toggle {
            // L26C8: LDA $322C,Y; EOR #$FF; EOR $0604; STA $0604
            self.sprite_x_msb ^= !self.d32(0x322C + y);
        }
    }

    /// sub_26ED ($26ED..$2798): look at the 5 tiles around enemy `$047A`
    fn probe_tiles_sub_26ed(&mut self) {
        let y = self.v47(0x47A) as usize;
        let xs = self.v47(0x47B) as usize;
        let ypos = self.spr(xs + 1);
        if ypos >= 0xE1 {
            // L26E1
            self.set_v47(0x47D, 0);
            self.set_v47(0x47E, 0);
            self.set_v47(0x47F, 0);
            return;
        }
        // $0A:$0B = Y - $23
        let w0a = ((ypos as i32) - 0x23) as i16 as u16;
        let r0a = (w0a & 0xFF) as u8;
        // $0C:$0D = X (9 bit) - scroll
        let hi = if (!self.d32(0x322C + y)) & self.sprite_x_msb != 0 { 1u8 } else { 0 };
        let xl = self.spr(xs);
        let (r0c, b) = xl.overflowing_sub(self.scroll_fine);
        let r0d = hi.wrapping_sub(b as u8);
        if r0d == 0 {
            if r0c < 0x15 {
                return;
            }
        } else if r0c >= 0x4C {
            return;
        }
        // L2740: subtract $15
        let (r0c, b) = r0c.overflowing_sub(0x15);
        let r0d = r0d.wrapping_sub(b as u8);
        // LSR $0D; ROR A; LSR A; LSR A
        let carry = r0d & 1;
        let r0d = r0d >> 1;
        let a = ((carry << 7) | (r0c >> 1)) >> 2;
        let x = (r0a >> 3) as usize;
        // $0C += $9BA0,X ; $0D += $9BB9,X + carry ; then + $C800
        let (lo, c) = a.overflowing_add(img(0x9BA0 + x));
        let hi2 = r0d.wrapping_add(img(0x9BB9 + x)).wrapping_add(c as u8);
        let hi3 = hi2.wrapping_add(0xC8);
        let ptr = ((hi3 as usize) << 8) | lo as usize;
        self.set_v47(0x47C, self.scr(ptr));
        self.set_v47(0x481, self.scr(ptr + 2));
        self.set_v47(0x47D, self.scr(ptr + 0x28));
        self.set_v47(0x47E, self.scr(ptr + 0x29));
        self.set_v47(0x47F, self.scr(ptr + 0x2A));
    }

    /// sub_27AF ($27AF..$27D2): enemy hit by a fireball (flipped over) falls down; returns the carry
    fn stunned_sub_27af(&mut self) -> bool {
        let x = self.v47(0x47A) as usize;
        if self.ep(0x7A4 + x) == 0 {
            return false;
        }
        self.probe_tiles_sub_26ed();
        if !Self::tile_solid(self.v47(0x47E)) {
            let xs = self.v47(0x47B) as usize;
            let v = self.spr(xs + 1).wrapping_add(4);
            self.set_spr(xs + 1, v);
        }
        true
    }

    /// sub_27D3 ($27D3..$2830): activate hidden kind-2 enemies; returns the carry
    fn activate_sub_27d3(&mut self) -> bool {
        let x = self.v47(0x47A) as usize;
        let k = self.ep(0x744 + x);
        if k == 0x11 {
            let m = self.d32(0x322C + x);
            self.en.vic_mdp &= m;
        } else if k >= 0x12 {
            self.set_v47(0x46C, 0);
            return false;
        } else if k < 2 {
            return false;
        }
        {
            if self.v47(0x46C) == 0 {
                return true;
            }
            let a = if self.v47(0x492) & 1 == 0 {
                self.d32(0x3224 + x) | self.en.vic_mmc
            } else {
                self.d32(0x322C + x) & self.en.vic_mmc
            };
            self.en.vic_mmc = a;
            let c = self.v47(0x493);
            self.set_ep(0x7AC + x, c);
            self.en.vic_col[x] = c & 0x7F;
            let n = self.ep(0x744 + x).wrapping_add(1);
            self.set_ep(0x744 + x, n);
            let ys = self.v47(0x47B) as usize;
            let v = self.spr(ys + 1).wrapping_sub(2);
            self.set_spr(ys + 1, v);
        }
        true
    }

    /// sub_2831 ($2831..$2889): advance the walking animation
    fn animate_sub_2831(&mut self) {
        let x = self.v47(0x47A) as usize;
        let n = self.ep(0x78C + x).wrapping_sub(1);
        self.set_ep(0x78C + x, n);
        if n & 0x80 == 0 {
            return;
        }
        let r = self.ep(0x77C + x);
        self.set_ep(0x78C + x, r);
        let (y, a) = if self.ep(0x794 + x) != 0 {
            (self.ep(0x734 + x), self.ep(0x73C + x))
        } else {
            (self.ep(0x724 + x), self.ep(0x72C + x))
        };
        self.set_v47(0x485, y);
        self.set_v47(0x486, a);
        if y <= a {
            // L2876: INC $0605,X
            let p = self.sptr(x).wrapping_add(1);
            self.set_sptr(x, p);
            if p > a {
                self.set_sptr(x, y);
            }
        } else {
            let p = self.sptr(x).wrapping_sub(1);
            self.set_sptr(x, p);
            if p < a {
                self.set_sptr(x, y);
            }
        }
    }

    // ---- sub_2F5C / sub_2F6E ----------------------------------------------------------------------------------------------

    /// sub_2F5C ($2F5C..$2F6D)
    fn sprite_colour_check_sub_2f5c(&mut self) {
        let x = self.v47(0x47A) as usize;
        if self.ep(0x7AC + x) & 0x80 == 0 {
            return;
        }
        if self.en.vic_col[x] != self.en.vic_bg1 {
            self.sprite_colour_cycle_sub_2f6e(x);
        }
    }

    /// sub_2F6E ($2F6E..$2F83): cycle through the 20 colours at $2F84
    pub fn sprite_colour_cycle_sub_2f6e(&mut self, x: usize) {
        let mut y = self.d32(0x325B).wrapping_add(1);
        self.set_d32(0x325B, y);
        if y >= 0x14 {
            y = 0;
            self.set_d32(0x325B, 0);
        }
        // the sprite colour registers hold 4 bits
        self.en.vic_col[x] = img(0x2F84 + y as usize) & 0x0F;
    }

    // ---- sub_288A: collisions ----------------------------------------------------------------------------------------------------

    /// sub_29B2 ($29B2..$29E7): flip an enemy over after a fireball/stomp hit and award points
    pub fn kill_enemy_sub_29b2(&mut self, x: usize) {
        if x > 7 || self.ep(0x744 + x) != 0 {
            return;
        }
        let mut y = 1u8;
        let a;
        if self.ep(0x794 + x) != 0 {
            if self.ep(0x75C + x) != 0 {
                a = self.ep(0x75C + x);
            } else {
                y += 1;
                a = self.ep(0x76C + x);
            }
        } else if self.ep(0x754 + x) != 0 {
            a = self.ep(0x754 + x);
        } else {
            y = 2;
            a = self.ep(0x764 + x);
        }
        self.set_sptr(x, a);
        self.set_ep(0x7A4 + x, y);
        self.sfx_queue = 0x05;
        let pts = self.ep(0x74C + x);
        self.add_score_sub_991d(pts, 0);
    }

    /// sub_29E8 ($29E8..$2A68): is an enemy within 24 x 21 pixels of (`$048A`, `$048B`, `$0490`)? Returns the slot.
    fn hit_test_sub_29e8(&mut self) -> Option<usize> {
        let mut x = 7usize;
        loop {
            let bit = self.d32(0x3224 + x);
            if self.en.vic_me & bit != 0 && self.ep(0x7A4 + x) == 0 {
                let y = x * 2;
                let ex = self.spr(y);
                let ehi = if self.sprite_x_msb & bit != 0 { 1u8 } else { 0 };
                let ey = self.spr(y + 1);
                self.set_v47(0x48C, ex);
                self.set_v47(0x48D, ehi);
                self.set_v47(0x491, ey);
                let (mut lo, b) = self.v47(0x48A).overflowing_sub(ex);
                let mut hi = self.v47(0x48B).wrapping_sub(ehi).wrapping_sub(b as u8);
                // carry clear (borrow) -> negate
                let borrow = (self.v47(0x48B) as i16 - ehi as i16 - b as i16) < 0;
                if borrow {
                    lo = !lo;
                    hi = !hi;
                    lo = lo.wrapping_add(1);
                    if lo == 0 {
                        hi = hi.wrapping_add(1);
                    }
                }
                self.set_v47(0x48E, lo);
                self.set_v47(0x48F, hi);
                if hi == 0 && lo < 0x18 {
                    let (d, b) = self.v47(0x490).overflowing_sub(ey);
                    let d = if b { (!d).wrapping_add(1) } else { d };
                    if d < 0x15 {
                        return Some(x);
                    }
                }
            }
            x -= 1;
            if x < 4 {
                return None;
            }
        }
    }

    /// sub_288A ($288A..$29B1): react to sprite collisions
    pub fn collision_handler_sub_288a(&mut self) {
        let latch = self.en.vic_d01e;
        self.en.vic_d01e = 0;
        if latch == 0 {
            return;
        }
        let m = latch & 0xF8;
        self.set_v47(0x489, m);
        if m == 0 {
            return;
        }
        if m == 8 {
            // L2958
            if self.d32(0x3246) == 1 {
                self.en.spr_ptr[0] = 0xF8;
                self.set_d32(0x3246, 2);
            }
            return;
        }
        // L28AE
        let (xv, msb, yv);
        if m & 8 != 0 {
            xv = self.en.spr_xy[0];
            msb = (self.sprite_x_msb >> 3) & 1;
            yv = self.en.spr_xy[1];
        } else {
            let p = self.en.spr_ptr[4];
            if p == 0xC7 {
                self.cutscene_walk_l2d41();
                return;
            }
            if p == 0xDF {
                self.exit_check_l301a();
                return;
            }
            xv = self.player_x_low;
            msb = self.sprite_x_msb & 1;
            yv = self.player_y_c64;
        }
        self.set_v47(0x48A, xv);
        self.set_v47(0x48B, msb);
        self.set_v47(0x490, yv);
        let x = match self.hit_test_sub_29e8() {
            Some(x) => x,
            None => return,
        };
        // L28F1
        if m & 8 != 0 {
            if self.en.spr_ptr[0] < 0xF8 {
                self.en.spr_ptr[0] = 0xF8;
            }
            if self.ep(0x723) >= 3 {
                let v = self.d32(0x3259);
                if v & 0x80 == 0 {
                    let n = v.wrapping_sub(1);
                    self.set_d32(0x3259, n);
                    if n & 0x80 == 0 {
                        return;
                    }
                    self.set_d32(0x325A, 0x0F);
                }
                // L291B
                self.ck_boss_bonus(); // fan-hack families: `$291B` hook
                let b = self.ep(0x76B);
                if b != 0 {
                    self.en.spr_ptr[4] = b;
                    self.en.spr_ptr[3] = b.wrapping_add(1);
                    self.en.spr_ptr[2] = b.wrapping_add(2);
                }
                self.set_ep(0x7AB, 1);
                self.set_ep(0x7AA, 1);
                self.set_ep(0x7A9, 1);
                return;
            }
            // L2937
            if self.ep(0x744 + x) == 0 {
                self.kill_enemy_sub_29b2(x);
            } else {
                self.en.vic_col[3] = self.en.vic_col[3].wrapping_add(1);
            }
            return;
        }
        // L2940: Giana touched an enemy
        if self.ep(0x723) >= 3 {
            self.player_hit_l1db7();
            return;
        }
        let k = self.ep(0x744 + x);
        if k == 0 {
            // L29A3
            if self.jump_state_1f_is_ff() {
                self.kill_enemy_sub_29b2(x);
            } else {
                self.player_hit_l1db7();
            }
        } else if k == 1 {
            self.player_hit_l1db7();
        } else if k >= 0x12 {
            // L296A: power-up pick-up
            self.anim_counter = self.anim_counter.wrapping_add(1);
            
            let sfx = if self.anim_counter >= 8 {
                self.anim_counter = 4;
                self.lives_c64 = self.lives_c64.wrapping_add(1);
                self.draw_lives_sub_0e1f();
                8
            } else {
                3
            };
            self.sfx_queue = sfx;
            self.redraw_pending = 1;
            self.en.vic_me &= self.d32(0x322C + x);
            self.add_score_sub_991d(0x50, 0);
        }
    }

    /// L29A3: `$1F == $FF` or `$17 == $FF`
    fn jump_state_1f_is_ff(&self) -> bool {
        self.land_flag == 0xFF || self.jump_state == 0xFF
    }

    /// L1DB7 ($1DB7..$1DD0): Giana is hit
    pub fn player_hit_l1db7(&mut self) {
        // Debug cheat F2 (invulnerable): enemies and hazards do nothing. Off = the original code path, unchanged.
        if self.god_mode {
            return;
        }
        self.hazard_hit = 0xFF;
        self.flag_0431 = 0xFF;
        let mut x = 2u8;
        if self.map_column != 0 {
            x = 0;
            if self.map_column >= 0x80 {
                x = 2;
            }
        }
        self.spawn_variant = x;
    }

    // ---- L2D41 / L301A -----------------------------------------------------------------------------------------------------------------

    /// L2D41 ($2D41..$2DD2): scripted walk to the door
    pub fn cutscene_walk_l2d41(&mut self) {
        if self.joystick_enabled {
            // LDA sub_9F77 != $60
            if self.land_flag == 0xFF || self.jump_state == 0xFF {
                // L2D5D
                let d = self.en.spr_xy[0x0603 - 0x05FA].wrapping_sub(self.player_y_c64);
                let borrow = self.en.spr_xy[0x0603 - 0x05FA] < self.player_y_c64;
                if borrow || d < 8 {
                    self.move_state = 0;
                    self.move_edge_flag = 0;
                    self.jump_state = 0;
                    return;
                }
                self.joystick_enabled = false; // STA sub_9F77 = $60
                let x = self.en.spr_xy[0x0603 - 0x05FA].wrapping_sub(0x17);
                self.player_y_c64 = x;
                self.sprite_head_y = x.wrapping_add(1);
                self.sprite_dress_y = x.wrapping_add(2);
                let px = self.en.spr_xy[0x0602 - 0x05FA].wrapping_add(4);
                self.player_x_low = px;
                self.sprite_head_x = px;
                self.sprite_dress_x = px;
                self.apply_standing_animation_sub_10df();
                self.move_state = 0;
                self.move_edge_flag = 0;
                self.jump_state = 0;
                self.land_flag = 0;
                self.flag_0431 = 0x80;
            } else {
                // L2D54
                self.move_state = 0;
                self.move_edge_flag = 0;
                self.jump_state = 0;
                return;
            }
        }
        // L2DA2
        self.move_delta = 2;
        self.add_player_y_sub_9f1f(2);
        for _ in 0..2 {
            self.en.spr_xy[0x05FF - 0x05FA] = self.en.spr_xy[0x05FF - 0x05FA].wrapping_add(1);
            self.en.spr_xy[0x0601 - 0x05FA] = self.en.spr_xy[0x0601 - 0x05FA].wrapping_add(1);
            self.en.spr_xy[0x0603 - 0x05FA] = self.en.spr_xy[0x0603 - 0x05FA].wrapping_add(1);
        }
        if self.en.spr_xy[0x0603 - 0x05FA] >= 0xFC {
            self.joystick_enabled = true; // STA sub_9F77 = $AD
            self.st_046d = 1;
            self.flag_0431 = 0;
        }
    }

    /// L301A ($301A..$3031)
    pub fn exit_check_l301a(&mut self) {
        let (d, borrow) = self.en.spr_xy[0x0602 - 0x05FA].overflowing_sub(self.player_x_low);
        let _ = d;
        if borrow && self.sprite_x_msb & 0x80 == 0 {
            self.flag_0431 = 0xFF;
            if self.ck_final_bonus() {
                return; // fan-hack families: `$302E` hook
            }
            self.en.flag_157e = 0xFF;
        }
    }

    // ---- L2DD3: big enemy -----------------------------------------------------------------------------------------------------------------

    fn boss_move_l2dd3(&mut self) {
        let v = self.d32(0x3259);
        if v & 0x80 != 0 {
            let n = self.d32(0x325A).wrapping_sub(1);
            self.set_d32(0x325A, n);
            if n == 0 {
                return;
            }
            if n & 0x80 != 0 {
                // L2DE7
                self.set_d32(0x3255, 2);
                self.boss_shift_pos_l2f3d();
            } else {
                self.set_d32(0x3257, 2);
                self.boss_shift_neg_l2f1e();
            }
            return;
        }
        // L2DEF
        self.boss_script_sub_2e6e();
        let n = self.d32(0x3251).wrapping_sub(1);
        self.set_d32(0x3251, n);
        if n & 0x80 == 0 {
            return;
        }
        self.set_d32(0x3251, 4);
        if self.d32(0x3252) != 0 {
            self.boss_anim_down_l2e41();
        } else {
            self.boss_anim_up_l2e02();
        }
    }

    fn boss_anim_up_l2e02(&mut self) {
        loop {
            let p = self.sptr(7);
            let hi = self.ep(0x73C + 7);
            if p == hi {
                return;
            }
            if p < hi {
                // L2E28
                let p = p.wrapping_add(self.ep(0x784 + 7));
                self.set_sptr(7, p);
                self.set_sptr(6, p.wrapping_add(1));
                self.set_sptr(5, p.wrapping_add(2));
                return;
            }
            // BCS: A >= $073C,X
            let d = self.ep(0x724 + 7).wrapping_sub(self.ep(0x72C + 7));
            let borrow = self.ep(0x724 + 7) < self.ep(0x72C + 7);
            if !borrow {
                self.set_d32(0x3252, 0xFF);
                self.boss_anim_down_l2e41();
                return;
            }
            let _ = d;
            let v = self.ep(0x734 + 7);
            self.set_sptr(7, v);
        }
    }

    fn boss_anim_down_l2e41(&mut self) {
        let p = self.sptr(7);
        let lim = self.ep(0x734 + 7);
        if p <= lim {
            self.set_d32(0x3252, 0);
            self.boss_anim_up_l2e02();
            return;
        }
        // L2E55
        let p = p.wrapping_sub(self.ep(0x784 + 7));
        self.set_sptr(7, p);
        self.set_sptr(6, p.wrapping_add(1));
        self.set_sptr(5, p.wrapping_add(2));
    }

    /// L2F1E / L2F3D: shift the three body sprites vertically
    fn boss_shift_neg_l2f1e(&mut self) {
        let d = self.d32(0x3257);
        for off in [0x05FFusize, 0x0601, 0x0603] {
            let i = off - 0x05FA;
            self.en.spr_xy[i] = self.en.spr_xy[i].wrapping_sub(d);
        }
    }

    fn boss_shift_pos_l2f3d(&mut self) {
        let d = self.d32(0x3255);
        for off in [0x05FFusize, 0x0601, 0x0603] {
            let i = off - 0x05FA;
            self.en.spr_xy[i] = self.en.spr_xy[i].wrapping_add(d);
        }
    }

    /// sub_2E6E ($2E6E..$2F5B): scripted movement of the big enemy
    fn boss_script_sub_2e6e(&mut self) {
        let ptr = ((self.ep(0x783) as usize) << 8) | self.ep(0x77B) as usize;
        let n = self.d32(0x3253).wrapping_sub(1);
        self.set_d32(0x3253, n);
        if n & 0x80 != 0 {
            loop {
                let y = self.d32(0x3256) as usize;
                let a = img(ptr + y);
                if a == 0xFF {
                    self.set_d32(0x3256, 0);
                    continue;
                }
                self.set_d32(0x3253, a);
                self.set_d32(0x3254, img(ptr + y + 1));
                self.set_d32(0x3255, img(ptr + y + 2));
                self.set_d32(0x3256, (y + 3) as u8);
                break;
            }
        }
        // L2EA1
        let mut speed = self.d32(0x3254);
        let mut dir;
        if speed & 0x80 != 0 {
            speed = !speed;
            dir = 0u8; // LDA #0; BIT $01A9 skips the next LDA #1
        } else {
            dir = 1;
        }
        self.set_d32(0x3258, speed);
        self.set_v47(0x483, dir);
        if self.stage == 0x21 {
            // L2EBC: face Giana
            let bx = self.player_x_low;
            let bm = self.sprite_x_msb & 1;
            let ex = self.en.spr_xy[0x0602 - 0x05FA];
            let em = (self.sprite_x_msb >> 7) & 1;
            let (_, borrow1) = bx.overflowing_sub(ex);
            let diff_hi = (bm as i16) - (em as i16) - borrow1 as i16;
            dir = if diff_hi < 0 { 1 } else { 0 };
            self.set_v47(0x483, dir);
        }
        // L2EF9
        let amount = self.d32(0x3258);
        self.set_v47(0x482, amount);
        self.move_x_sub_26b7(0x0E, 7);
        self.move_x_sub_26b7(0x0C, 6);
        self.move_x_sub_26b7(0x0A, 5);
        let a = self.d32(0x3255);
        if a & 0x80 != 0 {
            self.set_d32(0x3257, !a);
            self.boss_shift_neg_l2f1e();
        } else {
            self.boss_shift_pos_l2f3d();
        }
    }

    // ---- sub_2A69 / sub_2AD5 / sub_2CDC: fireball ----------------------------------------------------------------------------------------------------

    /// sub_2A69 ($2A69..$2AD4): shoot a fireball
    pub fn fire_sub_2a69(&mut self) {
        if self.joystick_fire == 0 || self.en.vic_me & 8 != 0 {
            return;
        }
        self.set_d32(0x3248, 0);
        if self.anim_counter < 2 {
            return;
        }
        if self.anim_counter >= 4 {
            self.target_search_sub_2cdc();
        }
        self.set_d32(0x3246, 0);
        self.sfx_queue = 0x0A;
        self.set_d32(0x3244, 0);
        let f = if self.facing_right { 0 } else { 0xFF };
        self.set_d32(0x3243, f);
        self.en.vic_col[3] = 1; // $D02A
        self.en.spr_ptr[0] = 0xF7;
        self.en.spr_xy[0] = self.player_x_low;
        let m = self.sprite_x_msb;
        let b = (m << 1) & 8;
        self.sprite_x_msb = (m & 0xF7) | b;
        self.en.spr_xy[1] = self.player_y_c64.wrapping_sub(0x0F);
        self.en.vic_me |= 8;
        self.fireball_sub_2ad5();
    }

    /// sub_2CDC ($2CDC..$2D40): pick the target enemy in front of Giana
    fn target_search_sub_2cdc(&mut self) {
        let mut y = 7usize;
        let mut a = self.en.vic_me;
        loop {
            let carry = a & 0x80 != 0;
            a <<= 1;
            if carry && self.ep(0x744 + y) == 0 && self.ep(0x7A4 + y) == 0 {
                self.set_d32(0x3247, y as u8);
                let x = y * 2;
                let bit = self.d32(0x3224 + y);
                let ex = self.spr(x);
                let (lo, b) = self.player_x_low.overflowing_sub(ex);
                let ehi = if self.sprite_x_msb & bit != 0 { 1u8 } else { 0 };
                let phi = self.sprite_x_msb & 1;
                let borrow = (phi as i16 - ehi as i16 - b as i16) < 0;
                self.set_v47(0x48A, lo);
                self.set_v47(0x48B, phi.wrapping_sub(ehi).wrapping_sub(b as u8));
                self.set_v47(0x48C, ex);
                self.set_v47(0x48D, ehi);
                let facing = if self.facing_right { 0u8 } else { 0xFF };
                if borrow != (facing != 0) {
                    return; // $2D3A, $2D40: the enemy is in front of Giana
                }
                // Back to L2CE4 with $042C in A, not the rest of $D015: the search goes on with that value
                a = facing;
            }
            if y == 0 {
                break;
            }
            y -= 1;
            if y < 4 {
                break;
            }
        }
        self.set_d32(0x3248, 1);
    }

    /// sub_2AD5 ($2AD5..$2CDB): move the fireball
    pub fn fireball_sub_2ad5(&mut self) {
        if self.en.vic_me & 8 == 0 {
            return;
        }
        let p = self.en.spr_ptr[0];
        if p == 0xF7 {
            // L2AF8: first frame
            self.set_v47(0x47A, 3);
            self.set_v47(0x47B, 6);
            self.probe_tiles_sub_26ed();
            if Self::tile_solid(self.v47(0x47C)) && Self::tile_solid(self.v47(0x481)) {
                self.en.spr_ptr[0] = 0xF8;
            }
        } else if p < 0xF8 {
            return;
        } else if p < 0xFB {
            self.en.spr_ptr[0] = p.wrapping_add(1);
            return;
        } else {
            self.en.vic_me &= 0xF7;
            return;
        }
        // L2B1C
        if self.d32(0x3248) == 0 && self.anim_counter >= 4 {
            self.homing_fireball_l2bae();
            return;
        }
        // L2B2A
        let dir = self.d32(0x3243);
        if dir == 0xFF {
            if Self::tile_solid(self.v47(0x47C)) {
                self.fireball_wall_l2b3a();
                return;
            }
        } else if Self::tile_solid(self.v47(0x481)) {
            self.fireball_wall_l2b3a();
            return;
        }
        // L2B4F
        self.set_v47(0x482, 8);
        self.set_v47(0x483, dir);
        self.move_x_sub_26b7(6, 3);
        if self.d32(0x3244) != 0 {
            if Self::tile_solid(self.v47(0x47C)) {
                self.fireball_bounce_l2b90();
                return;
            }
            let n = self.d32(0x3245).wrapping_sub(1);
            self.set_d32(0x3245, n);
            if n & 0x80 != 0 {
                self.fireball_bounce_l2b90();
                return;
            }
            if n < 3 {
                return;
            }
            // DEC $05FB twice
            self.en.spr_xy[1] = self.en.spr_xy[1].wrapping_sub(2);
            return;
        }
        // L2B82
        if Self::tile_solid(self.v47(0x47E)) {
            self.set_d32(0x3245, 0x0E);
            self.fireball_bounce_l2b90();
            return;
        }
        // INC $05FB twice
        self.en.spr_xy[1] = self.en.spr_xy[1].wrapping_add(2);
    }

    fn fireball_bounce_l2b90(&mut self) {
        let v = self.d32(0x3244) ^ 1;
        self.set_d32(0x3244, v);
    }

    fn fireball_wall_l2b3a(&mut self) {
        if self.anim_counter >= 3 {
            self.set_d32(0x3246, 1);
            let v = !self.d32(0x3243);
            self.set_d32(0x3243, v);
        } else {
            self.en.spr_ptr[0] = 0xF8;
        }
    }

    /// L2BAE..$2CDB: fireball that homes on the enemy chosen by sub_2CDC
    fn homing_fireball_l2bae(&mut self) {
        let t = self.d32(0x3247) as usize;
        if self.en.vic_me & self.d32(0x3224 + t) == 0 {
            self.en.spr_ptr[0] = 0xF8;
            return;
        }
        let fx = self.en.spr_xy[0];
        let fhi = (self.sprite_x_msb >> 3) & 1;
        let fy = self.en.spr_xy[1];
        let ex = self.spr(t * 2);
        let ehi = if self.sprite_x_msb & self.d32(0x3224 + t) != 0 { 1u8 } else { 0 };
        let ey = self.spr(t * 2 + 1).wrapping_sub(0x0A);
        // $324B:$324C = fireball X - enemy X
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
            0u8 // LDA #0; BIT skips LDA #$FF
        } else {
            0xFF
        };
        self.set_d32(0x3243, dirx);
        // $324D = fireball Y - enemy Y (absolute)
        let (mut dy, b) = fy.overflowing_sub(ey);
        let diry;
        if b {
            dy = !dy;
            diry = 0u8;
        } else {
            diry = 0xFF;
        }
        self.set_d32(0x3244, diry);
        // $324B:$324C -= $324D
        let (dx2, b2) = dx.overflowing_sub(dy);
        let dxh2 = dxh.wrapping_sub(b2 as u8);
        let ge = (dxh as i16 - b2 as i16) >= 0;
        let (mut sx, mut sy): (u8, u8) = if !ge { (1, 3) } else { (3, 1) };
        // $2C6B: LDA $324B; ROR $324C; LSR A five times. The carry of the ROR is shifted away: what reaches the ADC is bit 4.
        let c = (dx2 >> 4) & 1;
        let idx = (dx2 >> 5) as usize;
        let t3239 = self.d32(0x3239 + idx);
        // PHA; ADC $3249; CLC; STA $3249; PLA; CLC; ADC $324A; STA $324A
        sx = t3239.wrapping_add(sx).wrapping_add(c);
        sy = t3239.wrapping_add(sy);
        self.set_d32(0x324B, dx2);
        self.set_d32(0x324C, (dxh2 >> 1) | ((ge as u8) << 7));
        self.set_d32(0x324D, dy);
        self.set_d32(0x324E, ex);
        self.set_d32(0x324F, ehi);
        self.set_d32(0x3250, ey);
        // LDA $3243; BEQ L2C95
        let tile = if dirx != 0 { self.v47(0x47D) } else { self.v47(0x47F) };
        if Self::tile_solid(tile) {
            sy = sx;
            sx = 0;
            self.set_d32(0x3244, 0xFF);
        }
        self.set_d32(0x3249, sx);
        self.set_d32(0x324A, sy);
        self.set_v47(0x482, sx);
        self.set_v47(0x483, dirx);
        self.move_x_sub_26b7(6, 3);
        let v = self.d32(0x3244);
        let ny = if v != 0 { self.en.spr_xy[1].wrapping_sub(sy) } else { self.en.spr_xy[1].wrapping_add(sy) };
        self.en.spr_xy[1] = ny;
    }

    // ---- video chip sprite collisions ---------------------------------------------------------------------------------------------------------------------------

    /// Pixel mask of a sprite: one `u32` per row (bit 23 = leftmost pixel), as used for collisions.
    fn sprite_rows(&self, ptr: u8, multicolor: bool) -> [u32; 21] {
        let base = ptr as usize * 64;
        let mut rows = [0u32; 21];
        for (r, row) in rows.iter_mut().enumerate() {
            let b0 = self.sprite_mem[base + r * 3] as u32;
            let b1 = self.sprite_mem[base + r * 3 + 1] as u32;
            let b2 = self.sprite_mem[base + r * 3 + 2] as u32;
            let bits = (b0 << 16) | (b1 << 8) | b2;
            if multicolor {
                let mut m = 0u32;
                for p in 0..12 {
                    let pair = (bits >> (22 - 2 * p)) & 3;
                    if pair != 0 {
                        m |= 3 << (22 - 2 * p);
                    }
                }
                *row = m;
            } else {
                *row = bits;
            }
        }
        rows
    }

    /// `$D01E` as the video chip would have latched it while drawing the last frame.
    pub fn vic_sprite_collisions(&self) -> u8 {
        let f = &self.en.vic_frame;
        let mut rows: Vec<Option<[u32; 21]>> = Vec::with_capacity(8);
        for s in 0..8 {
            if f.me & (1 << s) != 0 {
                rows.push(Some(self.sprite_rows(f.ptr[s], f.mmc & (1 << s) != 0)));
            } else {
                rows.push(None);
            }
        }
        let pos = |s: usize| -> (i32, i32) {
            let x = f.xy[2 * s] as i32 | if f.xy[16] & (1 << s) != 0 { 0x100 } else { 0 };
            (x, f.xy[2 * s + 1] as i32)
        };
        let mut latch = 0u8;
        for a in 0..8 {
            for b in (a + 1)..8 {
                let (Some(ra), Some(rb)) = (&rows[a], &rows[b]) else { continue };
                let (xa, ya) = pos(a);
                let (xb, yb) = pos(b);
                let dy = yb - ya;
                let dx = xb - xa;
                if dx <= -24 || dx >= 24 || dy <= -21 || dy >= 21 {
                    continue;
                }
                let mut hit = false;
                for ry in 0..21i32 {
                    let rby = ry - dy;
                    if !(0..21).contains(&rby) {
                        continue;
                    }
                    let ma = ra[ry as usize];
                    let mb = rb[rby as usize];
                    // sprite b is shifted right by dx pixels relative to a
                    let shifted = if dx >= 0 { mb >> dx } else { (mb << (-dx)) & 0xFF_FFFF };
                    if ma & shifted != 0 {
                        hit = true;
                        break;
                    }
                }
                if hit {
                    latch |= (1 << a) | (1 << b);
                }
            }
        }
        latch
    }

    /// $13BF..$13C8: copy the software sprite coordinates to the video chip
    pub fn vic_copy_xy_l13bf(&mut self) {
        for i in 0..16 {
            self.en.vic_frame.xy[i] = self.spr(i);
        }
        self.en.vic_frame.xy[16] = self.sprite_x_msb;
    }

    /// $13D1..$13DA: copy the sprite frame pointers
    pub fn vic_copy_pointers_l13d1(&mut self) {
        for i in 0..8 {
            self.en.vic_frame.ptr[i] = self.sptr(i);
        }
    }

    /// End of the last IRQ handler: the video chip draws the next frame with the registers it now holds.
    pub fn vic_frame_end(&mut self) {
        self.en.vic_frame.me = self.en.vic_me;
        self.en.vic_frame.mmc = self.en.vic_mmc;
        let c = self.vic_sprite_collisions();
        self.en.vic_d01e |= c;
    }
}
