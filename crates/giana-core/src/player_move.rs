//! Player movement group of the original in-game engine, ported 1:1 from `re/ingame.asm`.
//!
//! Routines: sub_11C6, sub_1200 (move gate), sub_1138, sub_0EAB (incl. tails L0FC5, L0FD2, L0FD7,
//! L1006, L1012, L10FD, L1110) and sub_1078. Flag conventions follow the 6502 code: `$FF` / `$80`
//! state bytes are kept as raw bytes, 6502 carry / negative flags are made explicit in comments.

use crate::state::GameState;
use crate::tables::{TABLE_9A56, TABLE_9A9A, TABLE_9ADE};

/// Offset of $9AF9 inside `TABLE_9ADE` (fall table read by sub_0EAB).
const FALL_TABLE_BASE: usize = 0x1B;

impl GameState {
    /// $0457 as seen by sub_0EAB: non-zero while dying or respawning.
    fn state_0457_nonzero(&self) -> bool {
        self.st_0457 != 0
    }

    /// sub_11C6 ($11C6..$11FF): Player ground contact & landing check
    pub fn check_ground_collision_sub_11c6(&mut self) {
        let f = self.surrounding_tile_flags;
        // $11C6: LDA $05F5; AND #$07; CMP #$07; BEQ L11EA
        if (self.player_y_c64 & 0x07) != 0x07 {
            // $11CF: LDA $0426; LSR A; BCS L119D (rts)
            if f[7] & 1 != 0 {
                return;
            }
            // $11D5: LDA $0422; ORA $0424; LSR A; BCS L11EA
            if (f[3] | f[5]) & 1 == 0 {
                // $11DE: LDA $0425; ORA $0426; ORA $0427; LSR A; BCS rts
                if (f[6] | f[7] | f[8]) & 1 != 0 {
                    return;
                }
            }
        }
        // L11EA: $0400 &= $FE; $0428 = 4; $1C = $FF; $1F = $FF; JMP sub_9F1F
        self.joystick_directions &= 0xFE;
        self.move_delta = 0x04;
        self.airborne_flag = 0xFF;
        self.land_flag = 0xFF;
        self.add_player_y_sub_9f1f(self.move_delta);
    }

    /// sub_1200 ($1200..$1248): Gate that stops horizontal movement against walls / ledges
    pub fn move_gate_sub_1200(&mut self) {
        let f = self.surrounding_tile_flags;
        // $1200: LDA $15; BNE L1205; RTS
        if self.move_state == 0 {
            return;
        }
        let stop = if self.move_state == 0xFF {
            // $1209: LDA $0424; LSR A; BCC L1219 (else L120F)
            if f[5] & 1 != 0 {
                true
            } else {
                // L1219: LDA $0427; LSR A; BCC rts
                if f[8] & 1 == 0 {
                    return;
                }
                // $121F: LDA $0426; ORA $0425; LSR A; BCS rts
                if (f[7] | f[6]) & 1 != 0 {
                    return;
                }
                // L1228: LDA $05F5; AND #$07; CMP #$03; BNE L120F
                (self.player_y_c64 & 0x07) != 0x03
            }
        } else {
            // L1232: LDA $0422; LSR A; BCS L120F
            if f[3] & 1 != 0 {
                true
            } else {
                // $1238: LDA $0425; LSR A; BCC rts
                if f[6] & 1 == 0 {
                    return;
                }
                // $123E: LDA $0426; ORA $0427; LSR A; BCS rts
                if (f[7] | f[8]) & 1 != 0 {
                    return;
                }
                // $1247: BCC L1228 (carry is clear here)
                (self.player_y_c64 & 0x07) != 0x03
            }
        };
        if stop {
            // L120F: $15 = 0; $16 = 0; $040A = 0
            self.move_state = 0;
            self.move_edge_flag = 0;
            self.player_momentum = 0;
        }
    }

    /// sub_1138 ($1138..$11C5): Player tile collision / jump state transitions
    pub fn check_player_tile_collision_sub_1138(&mut self) {
        let f = self.surrounding_tile_flags;
        // $1138: LDA $0431; BEQ L113E; RTS
        if self.flag_0431 != 0 {
            return;
        }
        // L113E: JSR sub_1200
        self.move_gate_sub_1200();
        // $1141: LDA $17; BNE L1148; JMP sub_11C6
        if self.jump_state == 0 {
            self.check_ground_collision_sub_11c6();
            return;
        }
        // L1148: LDA $05F5; AND #$07; CMP #$03; BNE rts
        if (self.player_y_c64 & 0x07) != 0x03 {
            return;
        }
        if self.jump_state == 0xFF {
            // L119E
            if self.jump_phase == 0x80 {
                return;
            }
            // $11A4: LDA $0426; LSR A; BCS L11BF
            if f[7] & 1 == 0 {
                // $11AA: LDA $0422; ORA $0424; LSR A; BCS rts
                if (f[3] | f[5]) & 1 != 0 {
                    return;
                }
                // $11B3: LDA $0425; ORA $0426; ORA $0427; LSR A; BCC rts
                if (f[6] | f[7] | f[8]) & 1 == 0 {
                    return;
                }
            }
            // L11BF: $17 = 0; $18 = 0
            self.jump_state = 0;
            self.jump_phase = 0;
        } else if self.jump_state == 0x80 {
            // L1162
            self.side_tile_flags = 0;
            // $1167: CLC; LDA $041F; ADC $0422; CMP #$02; BEQ L1186
            let mut skip = f[0].wrapping_add(f[3]) == 2;
            if !skip {
                // $1172: CLC; LDA $0421; ADC $0424; CMP #$02; BEQ L1186
                skip = f[2].wrapping_add(f[5]) == 2;
            }
            if !skip {
                // $117D: LDA $041F; ORA $0421; STA $0414
                self.side_tile_flags = f[0] | f[2];
            }
            // L1186: LDA $0420; ORA $0414; LSR A; BCC rts
            if (f[1] | self.side_tile_flags) & 1 != 0 {
                // $118F: $17 = $FF; if $18 == $80 then $18 = $FF
                self.jump_state = 0xFF;
                if self.jump_phase == 0x80 {
                    self.jump_phase = 0xFF;
                }
            }
        }
        // any other $17 value: RTS
    }

    /// L1012 ($1012..$106D): Scroll playfield / sky by table step X
    fn scroll_playfield_l1012(&mut self, x: usize) {
        // $1012: SEC; LDA $0404; SBC $9A56,X; TAY; AND #$07; STA $0404
        let y = self.scroll_fine.wrapping_sub(TABLE_9A56[x]);
        self.scroll_fine = y & 0x07;
        // $101F: SEC; LDA $0405; SBC $9A9A,X; TAX; AND #$07; STA $0405
        let xs = self.sky_scroll_fine.wrapping_sub(TABLE_9A9A[x]);
        self.sky_scroll_fine = xs & 0x07;
        // $102C: TXA; ASL A; BCC L1065 (sky scroll underflow -> shift 3 screen rows left)
        if xs & 0x80 != 0 {
            for row_base in [0x50usize, 0x78, 0xA0] {
                for i in 0..0x27usize {
                    self.screen_ram[row_base + i] = self.screen_ram[row_base + i + 1];
                }
            }
            // $104E: C877 = C850; C89F = C878; C8C7 = C8A0
            self.screen_ram[0x77] = self.screen_ram[0x50];
            self.screen_ram[0x9F] = self.screen_ram[0x78];
            self.screen_ram[0xC7] = self.screen_ram[0xA0];
        }
        // L1065: TYA; ASL A; BCC rts; LDA #$FF; STA $14
        if y & 0x80 != 0 {
            self.scroll_wrap_flag = 0xFF;
        }
    }

    /// L0FD2/L0FD7 tail of sub_0EAB: compute delta from $9A56 and apply movement or scroll
    fn move_tail_l0fd7(&mut self) {
        // $0FD7: CLC; LDA $040A; ADC $0408; TAX; LDA $9A56,X; STA $0428
        let x = self.player_momentum.wrapping_add(self.scroll_subpixel) as usize;
        self.move_delta = TABLE_9A56[x];
        // $0FE5: ASL A; ASL A; STA $0429
        self.momentum_friction = self.move_delta << 2;
        // $0FEA: LDA $0457; BNE L1006
        let scroll = if self.state_0457_nonzero() {
            false
        } else {
            // $0FEF: LDA $21; BEQ L0FF6 -> $044B = $FF, else $044B = 0
            self.time_warning = if self.map_column != 0 { 0x00 } else { 0xFF };
            // $0FFB: LDA $044B; BMI L1006; LDA $16; CMP #$FF; BEQ L1012
            (self.time_warning & 0x80) == 0 && self.move_edge_flag == 0xFF
        };
        if scroll {
            self.scroll_playfield_l1012(x);
        } else if self.move_state == 0xFF {
            // L1006: LDA $15; CMP #$FF; BEQ L100F -> JMP L9EC3
            self.add_player_x_sub_9ec3(self.move_delta);
        } else {
            // JMP L9EF1
            self.sub_player_x_sub_9ef1(self.move_delta);
        }
    }

    /// L0FD2: reset sub-pixel fraction, then continue at L0FD7
    fn move_tail_l0fd2(&mut self) {
        self.scroll_subpixel = 0;
        self.move_tail_l0fd7();
    }

    /// sub_0EAB ($0EAB..$106D): Player movement, jump curve & horizontal inertia
    pub fn update_player_physics_sub_0eab(&mut self) {
        // $0EAB: LDA $154D; BEQ L0EB5; LDX #$24; JMP L1012
        if self.music_paused != 0 {
            self.scroll_playfield_l1012(0x24);
            return;
        }

        // ---- vertical part: falling / jumping state machine ----
        // L0EB5: LDA $18; BEQ L0EF4
        if self.jump_phase != 0 {
            if self.jump_phase != 0x80 {
                // $0EBD: DEC $042B; BMI L0EF0
                self.jump_descent_idx = self.jump_descent_idx.wrapping_sub(1);
                if self.jump_descent_idx & 0x80 == 0 {
                    // $0EC2: LDX $042B; LDA $9AF9,X; STA $0428; JSR sub_9F1F; JMP L0F6A
                    self.move_delta = TABLE_9ADE[FALL_TABLE_BASE + self.jump_descent_idx as usize];
                    self.add_player_y_sub_9f1f(self.move_delta);
                    return self.horizontal_part_l0f6a();
                }
                // L0EF0: $18 = 0 (fall through to L0EF4)
                self.jump_phase = 0;
            } else {
                // L0ED1: LDA $042B; TAX; CMP #$07; BEQ L0EE8
                let x = self.jump_descent_idx;
                if x == 0x07 {
                    // L0EE8: $17 = $FF; $18 = $FF; BNE L0F6A
                    self.jump_state = 0xFF;
                    self.jump_phase = 0xFF;
                } else {
                    // $0ED9: INC $042B; LDA $9AF9,X; STA $0428; JSR sub_9F2C
                    self.jump_descent_idx = x.wrapping_add(1);
                    self.move_delta = TABLE_9ADE[FALL_TABLE_BASE + x as usize];
                    self.sub_player_y_sub_9f2c(self.move_delta);
                }
                return self.horizontal_part_l0f6a();
            }
        }

        // L0EF4: LDA $17; CMP #$FF; BNE L0F18
        if self.jump_state == 0xFF {
            // $0EFA: DEC $042A; LDX $042A; BMI L0F0E
            self.jump_ascent_idx = self.jump_ascent_idx.wrapping_sub(1);
            if self.jump_ascent_idx & 0x80 == 0 {
                // $0F02: LDA $9ADE,X; STA $0428; JSR sub_9F1F
                self.move_delta = TABLE_9ADE[self.jump_ascent_idx as usize];
                self.add_player_y_sub_9f1f(self.move_delta);
            } else {
                // L0F0E: $17 = 0; JSR sub_11C6
                self.jump_state = 0;
                self.check_ground_collision_sub_11c6();
            }
        } else if self.jump_state == 0x80 {
            // L0F18/L0F1C: LDA $0400; LSR A; BCC L0F39
            let mut hold = true;
            if self.joystick_directions & 0x01 != 0 {
                // $0F22: LDA $042A; TAX; CMP #$1B; BEQ L0F39
                let x = self.jump_ascent_idx;
                if x != 0x1B {
                    // $0F2A: INC $042A; LDA $9ADE,X; STA $0428; JSR sub_9F2C
                    self.jump_ascent_idx = x.wrapping_add(1);
                    self.move_delta = TABLE_9ADE[x as usize];
                    self.sub_player_y_sub_9f2c(self.move_delta);
                    hold = false;
                }
            }
            if hold {
                // L0F39: $18 = $80; $042B = 0
                self.jump_phase = 0x80;
                self.jump_descent_idx = 0;
            }
        } else {
            // L0F44: LDA $0400; TAX; LSR A; BCC L0F6A (jump button not pressed)
            let joy = self.joystick_directions;
            // $0F4C: TXA; ASL A; BCS L0F6A (jump latch bit 7 already set)
            if joy & 0x01 != 0 && joy & 0x80 == 0 {
                // $0F4F: ORA #$80; STA $0400; $042A = 0; $17 = $80
                self.joystick_directions = joy | 0x80;
                self.jump_ascent_idx = 0;
                self.jump_state = 0x80;
                // $0F5E: LDA sub_9F77; CMP #$60; BEQ L0F6A; else $154C = 0
                if self.joystick_enabled {
                    self.sfx_queue = 0x00;
                }
            }
        }
        self.horizontal_part_l0f6a();
    }

    /// L0F6A ($0F6A..$1006): horizontal movement, momentum and scroll decision
    fn horizontal_part_l0f6a(&mut self) {
        let joy = self.joystick_directions;
        // $0F6E: AND #$0C; BNE L0F75; JMP L10FD
        if joy & 0x0C == 0 {
            // L10FD: SEC; LDA $040A; SBC #$01; BMI L110B
            let m = self.player_momentum.wrapping_sub(1);
            if m & 0x80 != 0 {
                // L110B: $15 = 0
                self.move_state = 0;
            } else {
                self.player_momentum = m;
                self.move_tail_l0fd2();
            }
            return;
        }

        if joy & 0x04 == 0 {
            // right: $042C = 0
            self.facing_right = true;
            // $0F7F: LDA $040A; BEQ L0F8D; LDA $15; CMP #$80; BNE L0F8D; JMP L1110
            if self.player_momentum != 0 && self.move_state == 0x80 {
                return self.brake_l1110();
            }
            // L0F8D: $1B = 0; $15 = $FF; $16 = ($05F4 >= $A0) ? $FF : 0
            self.brake_flag = 0;
            self.move_state = 0xFF;
            self.move_edge_flag = if self.player_x_low >= 0xA0 { 0xFF } else { 0x00 };
        } else {
            // left: $042C = $FF
            self.facing_right = false;
            // $0FAB: LDA $040A; BEQ L0FB9; LDA $15; CMP #$FF; BNE L0FB9; JMP L1110
            if self.player_momentum != 0 && self.move_state == 0xFF {
                return self.brake_l1110();
            }
            // L0FB9: $1B = 0; $15 = $80; $16 = 0
            self.brake_flag = 0;
            self.move_state = 0x80;
            self.move_edge_flag = 0;
        }

        // L0FC5: CLC; LDA $040A; ADC #$01; CMP #$21; BCS L0FD7
        let m = self.player_momentum.wrapping_add(1);
        if m >= 0x21 {
            self.move_tail_l0fd7();
        } else {
            self.player_momentum = m;
            self.move_tail_l0fd2();
        }
    }

    /// L1110 ($1110..$112B): Direction change while moving - brake momentum by 2
    fn brake_l1110(&mut self) {
        // $1110: $1B = $FF; SEC; LDA $040A; SBC #$02; BMI L1122
        self.brake_flag = 0xFF;
        let m = self.player_momentum.wrapping_sub(2);
        if m & 0x80 != 0 {
            // L1122: $040A = 0; $15 = 0; $1B = 0
            self.player_momentum = 0;
            self.move_state = 0;
            self.brake_flag = 0;
        } else {
            self.player_momentum = m;
            self.move_tail_l0fd2();
        }
    }

    /// sub_1078 ($1078..$10DE): Player sprite animation sequence selector
    pub fn update_player_animation_sub_1078(&mut self) {
        let facing_left = !self.facing_right; // $042C != 0
        // $1078: LDA $1C; BNE L1080; LDA $17; BEQ L1093
        if self.airborne_flag != 0 || self.jump_state != 0 {
            // L1080: $1A = 0; $1C = 0; A = $042C ? $1B : $18; JMP sub_10ED
            self.anim_walking = 0;
            self.airborne_flag = 0;
            self.set_sprite_pointers(if facing_left { 0x1B } else { 0x18 });
            return;
        }
        // L1093: LDA $15; BEQ sub_10DF
        if self.move_state == 0 {
            self.apply_standing_animation_sub_10df();
            return;
        }
        // $1097: LDA $1B; BNE L10CF
        if self.brake_flag != 0 {
            // L10CF: $1A = 0; A = $042C ? $24 : $27; JMP sub_10ED
            self.anim_walking = 0;
            self.set_sprite_pointers(if facing_left { 0x24 } else { 0x27 });
            return;
        }
        // $109B: LDA $1A; BEQ L10B6
        if self.anim_walking != 0 {
            // $109F: DEC $042E; BEQ L10A5; RTS
            self.anim_subtimer = self.anim_subtimer.wrapping_sub(1);
            if self.anim_subtimer != 0 {
                return;
            }
            // L10A5: DEC $042D; BEQ L10B6
            self.anim_step_timer = self.anim_step_timer.wrapping_sub(1);
            if self.anim_step_timer != 0 {
                // JSR sub_106E; LDA $0605; CLC; ADC #$03; JMP L10F0
                self.clear_momentum_sub_106e();
                self.set_sprite_pointers_l10f0(self.sprite_0_frame.wrapping_add(3));
                return;
            }
        }
        // L10B6: JSR sub_106E; $1A = $FF; $042D = 4; A = $042C ? $0C : 0; JMP sub_10ED
        self.clear_momentum_sub_106e();
        self.anim_walking = 0xFF;
        self.anim_step_timer = 4;
        self.set_sprite_pointers(if facing_left { 0x0C } else { 0x00 });
    }
}
