//! `sub_1E19` ($1E19..$1FE1) and `sub_1FE2`: the statue. Touching it ($0457 = $FF) swaps the playfield with a bonus room
//! (stage `$22..$26`), Giana walks out of it at the end and the stage comes back (checkpoint at `$0458..$0460`).
//!
//! The routine keeps the main line for many frames (decompression, row swap, busy waits). `MainLine::Bonus` models those
//! waits; their lengths are measured from the recording like the other waits (`BonusTiming`).

use crate::image::img;
use crate::mainloop::MainLine;
use crate::state::{BonusTiming, GameState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BonusPhase {
    /// L1E2B: waits until the animated tile slots are idle
    SlotWait,
    /// `sub_9936` of the bonus room runs: chains left
    Decomp(usize),
    /// `sub_1FE2`: swapping the 20 rows of 40 cells, `cells` done; `back` = on the way back to the stage
    Swap { chain: usize, back: bool },
    /// `sub_13F0` waits for the sprite copy of one chain (L1E8A)
    Sync,
    /// L1F04: Giana walks up out of the room, one step per chain
    Rise,
    /// the busy wait and stage set-up between the walk and the swap on the way back
    Prep,
    /// `sub_9936` of the stage runs on the way back: chains left
    DecompBack(usize),
    /// `sub_20A3` after the sprites were given back (L1FBC): waits for the sprite copy of one chain
    FinSync,
    /// L1F5A.. restored, `sub_9A4F` with Y = 0: chains left until `$0457` drops to 0
    Fin(usize),
}

/// cells swapped by `sub_1FE2` per chain: the loop at `$2006` needs 46 cycles per cell, the chain leaves it about 14.4k of
/// the 19.7k cycles of a frame (measured: 314 cells between the samples of stage_30_run, `$045A` drops by 8 rows)
pub const SWAP_CELLS_PER_CHAIN: usize = 314;
const SWAP_CELLS: usize = 20 * 40;

impl GameState {
    pub(crate) fn bonus_timing_front(&self) -> BonusTiming {
        self.bonus_timing.front().cloned().unwrap_or_default()
    }

    /// sub_1E19 as called by `$0C86`, one call per pass of the main loop
    pub fn sub_1e19(&mut self) {
        // $1E19: LDA $0464; BEQ L1E21; JMP L1F04
        if self.st_0464 != 0 {
            self.main = MainLine::Bonus(BonusPhase::Rise);
            self.bonus_rise_step();
            return;
        }
        // $1E21: LDA $0457; CMP #$FF; BEQ L1E2B; JMP L1EE0
        if self.st_0457 == 0xFF {
            self.main = MainLine::Bonus(BonusPhase::SlotWait);
            self.bonus_step(BonusPhase::SlotWait);
            return;
        }
        // L1EE0: the walk-out trigger
        if self.player_y_c64 == 0xD3 && self.sprite_x_msb & 1 != 0 && self.player_x_low >= 0x0B {
            self.player_momentum = 0; // $1EF6
            self.st_0464 = 0xFF;
            self.flag_0431 = 0x80;
            // the main loop is short: it comes round to $1E19 again within the same gap between two interrupts and
            // takes the first step of the walk ($1F04: the video chip still shows the current Y, the wait is over)
            self.main = MainLine::Bonus(BonusPhase::Rise);
            self.bonus_rise_step();
        }
    }

    /// Chains of the bonus visit that run in the time-based slot of the main line
    pub fn bonus_timed(&mut self, phase: BonusPhase) {
        self.bonus_step(phase);
    }

    fn bonus_step(&mut self, phase: BonusPhase) {
        let mut phase = phase;
        loop {
            match phase {
                BonusPhase::SlotWait => {
                    // L1E2D: spin until the four slot tables are zero for every slot
                    let busy = (0..8usize).any(|y| {
                        [0x9B02usize, 0x9B38, 0x9B50, 0x9B68].iter().any(|&b| self.particle_slots[b + y - 0x9B02] != 0)
                    });
                    if busy {
                        self.main = MainLine::Bonus(BonusPhase::SlotWait);
                        return;
                    }
                    // $1E41: JSR sub_0D1F; $1E44: LDA #$80; STA $0457
                    self.clear_entity_slots_sub_0d1f();
                    self.st_0457 = 0x80;
                    // $1E49: LDA $21; STA $045C; $1E4E: LDX #$FF; STX $0431; INX; STX $040A
                    self.chk[4] = self.map_column;
                    self.flag_0431 = 0xFF;
                    self.player_momentum = 0;
                    // $1E57: the room: $21 for stage $20, else $045B counts up through $22..$26
                    let a = if self.engine_b.is_some() {
                        // family B (`$1E55..$1E72`): rooms `$65..$78`; the hack compares the map column `$21` (not the stage)
                        // with the operand of `CMP #imm` at `$1E57`; limits are the operands at `$1E65`, `$1E68`, `$1E6E`
                        if self.map_column == img(0x1E58) {
                            img(0x1E5C)
                        } else {
                            self.stage_cycle_counter = self.stage_cycle_counter.wrapping_add(1);
                            if self.stage_cycle_counter >= img(0x1E66) || self.stage_cycle_counter < img(0x1E6F) {
                                self.stage_cycle_counter = img(0x1E6A);
                            }
                            self.stage_cycle_counter
                        }
                    } else if self.stage == self.tnw_room_numbers().0 {
                        self.tnw_room_numbers().1 as u8
                    } else {
                        let (_, _, limit, first) = self.tnw_room_numbers();
                        self.stage_cycle_counter = self.stage_cycle_counter.wrapping_add(1);
                        if self.stage_cycle_counter >= limit || self.stage_cycle_counter < first {
                            self.stage_cycle_counter = first;
                        }
                        self.stage_cycle_counter
                    };
                    self.stage = a as u32;
                    // $1E76: JSR sub_9936 (it sets `$21` = $27 only when it reaches the end of the macro list at L9965)
                    let column = self.map_column;
                    self.decompress_stage_layout();
                    self.map_column = column;
                    phase = BonusPhase::Decomp(self.bonus_timing_front().decomp);
                }
                BonusPhase::Decomp(n) => {
                    if n > 0 {
                        self.main = MainLine::Bonus(BonusPhase::Decomp(n - 1));
                        return;
                    }
                    // the end of sub_9936 (L9965)
                    self.map_column = 0x27;
                    // $1E79: JSR sub_0D39; JSR sub_0E66; $1E7F: LDA #$07; STA $0404; STA $0469
                    self.stage_colours_sub_0d39();
                    self.status_bar_frame_sub_0e66();
                    self.scroll_fine = 7;
                    self.prev_scroll = 7;
                    // $1E87: JSR sub_1FE2
                    self.flag_0431 = 0xFF;
                    self.chk[2] = 0x13; // $045A
                    self.swap_state = (0, 0, 0, 0);
                    self.swap_chain_sub_1fe2(0, false);
                    self.main = MainLine::Bonus(BonusPhase::Swap { chain: 1, back: false });
                    return;
                }
                BonusPhase::Swap { chain, back } => {
                    if !self.swap_chain_sub_1fe2(chain, back) {
                        self.main = MainLine::Bonus(BonusPhase::Swap { chain: chain + 1, back });
                        return;
                    }
                    // $203A: LDA #$00; STA $0431
                    self.flag_0431 = 0;
                    if back {
                        // $1F4B: JSR sub_9936
                        self.decompress_stage_layout();
                        phase = BonusPhase::DecompBack(self.bonus_timing_front().ret_decomp);
                        continue;
                    }
                    // $1E8A: JSR sub_13F0 up to the sprite sync
                    self.start_stage_sprites();
                    self.main = MainLine::Bonus(BonusPhase::Sync);
                    return;
                }
                BonusPhase::Sync => {
                    // $1438: sprites on; $1E8D..$1EDF: the sparkle sprites 5..7 and Giana's colours
                    self.setup_sprites_finish();
                    for (i, p) in [(5usize, 0xC7u8), (6, 0xC8), (7, 0xC9)] {
                        self.set_sptr(i, p);
                    }
                    self.en.vic_mdp &= 0x07;
                    self.en.vic_mmc &= 0x07;
                    self.en.vic_col[5] = 0x0E; // $D02C..$D02E
                    self.en.vic_col[6] = 0x0E;
                    self.en.vic_col[7] = 0x0E;
                    self.sprite_x_msb |= 0xE0;
                    self.set_spr(0x0A, 0x0E); // $05FE
                    self.set_spr(0x0C, 0x26); // $0600
                    self.set_spr(0x0E, 0x3E); // $0602
                    for off in [0x0Busize, 0x0D, 0x0F] {
                        self.set_spr(off, 0xE9); // $05FF, $0601, $0603
                    }
                    self.en.vic_me |= 0xE0;
                    self.en.vic_frame.me = self.en.vic_me;
                    let c = self.vic_sprite_collisions();
                    self.en.vic_d01e |= c;
                    self.main = MainLine::Loop;
                    return;
                }
                BonusPhase::Rise => {
                    self.bonus_rise_step();
                    return;
                }
                BonusPhase::Prep => {
                    // $1F37: LDA $0458; STA $20; LDA $0459; STA $0404; sub_0D39; sub_0E66; sub_1FE2
                    self.stage = self.chk[0] as u32;
                    self.scroll_fine = self.chk[1];
                    self.stage_colours_sub_0d39();
                    self.status_bar_frame_sub_0e66();
                    self.flag_0431 = 0xFF;
                    self.chk[2] = 0x13;
                    self.swap_state = (0, 0, 0, 0);
                    self.swap_chain_sub_1fe2(0, true);
                    // the first chain is over: the rest of the swap goes on in the next one
                    self.main = MainLine::Bonus(BonusPhase::Swap { chain: 1, back: true });
                    return;
                }
                BonusPhase::DecompBack(n) => {
                    if n > 0 {
                        self.main = MainLine::Bonus(BonusPhase::DecompBack(n - 1));
                        return;
                    }
                    // $1F4E: LDA $045C; STA $21; $1F5A: LDA #$00; STA $044B; LDA #$01; STA $0457
                    self.map_column = self.chk[4];
                    // family T (`$1F4E: JSR $60C0`): the colours of the stage that was just loaded again
                    self.tnw_colours();
                    // $1F53: CMP #$FF; BNE L1F5A; LDA #$FF; BIT $00A9: with $21 = $FF the `LDA #$00` of L1F5A is skipped
                    // (it is the operand of the BIT), so `$044B` gets $FF instead of 0
                    self.time_warning = if self.chk[4] == 0xFF { 0xFF } else { 0 };
                    self.st_0457 = 1;
                    if !self.statue_replace_l1f77() {
                        // $1F80: RTS (no statue tile in the next 128 bytes): the routine ends here with `$0457` = 1; the
                        // sprites stay as they are and the main line is free again
                        self.bonus_timing.pop_front();
                        self.main = MainLine::Loop;
                        return;
                    }
                    // $1F9D: sub_213A, sub_20DC, Giana's place, sub_20A3, sub_1132, flags
                    self.restore_sprites_sub_213a();
                    self.clear_zero_page_sub_20dc();
                    let x = self.chk[5];
                    self.player_x_low = x;
                    self.sprite_head_x = x;
                    self.sprite_dress_x = x;
                    self.player_y_c64 = 0xDB;
                    self.sprite_head_y = 0xDC;
                    self.sprite_dress_y = 0xDD;
                    self.main = MainLine::Bonus(BonusPhase::FinSync);
                    return;
                }
                BonusPhase::FinSync => {
                    self.joystick_enabled = false; // sub_1132 patches $9F77 to RTS: the joystick is not read during the wait
                    self.flag_0431 = 0;
                    self.joystick_fire = 0;
                    self.sfx_queue = 8;
                    self.joystick_directions = 1;
                    phase = BonusPhase::Fin(self.bonus_timing_front().fin_wait);
                }
                BonusPhase::Fin(n) => {
                    if n > 0 {
                        self.main = MainLine::Bonus(BonusPhase::Fin(n - 1));
                        return;
                    }
                    // $1FD9: JSR sub_112C (patches $9F77 back to LDA: reading again); $1FDE: LDA #$00; STA $0457
                    self.joystick_enabled = true;
                    self.st_0457 = 0;
                    self.bonus_timing.pop_front();
                    self.main = MainLine::Loop;
                    return;
                }
            }
        }
    }

    /// L1F04..L1F34: one step of Giana's walk out of the room
    pub fn bonus_rise_step(&mut self) {
        // $1F04: JSR sub_20A3 (wait for the video chip to show the current Y)
        if self.en.vic_frame.xy[1] != self.player_y_c64 {
            self.main = MainLine::Bonus(BonusPhase::Rise);
            return;
        }
        // $1F07: LDA #$02; STA $0428; JSR sub_9F2C
        self.move_delta = 2;
        self.sub_player_y_sub_9f2c(2);
        // $1F0F: LDA $05FF; SEC; SBC #$02; STA $05FF, $0601, $0603
        let v = self.spr(0x0B).wrapping_sub(2);
        for off in [0x0Busize, 0x0D, 0x0F] {
            self.set_spr(off, v);
        }
        // $1F1E: LDA $05F5; CMP #$40; BCS L1F03
        if self.player_y_c64 >= 0x40 {
            self.main = MainLine::Bonus(BonusPhase::Rise);
            return;
        }
        // $1F25: sprites off, $0464 = 0, $040A = 0, $14 = 0
        self.en.vic_me = 0;
        self.st_0464 = 0;
        self.player_momentum = 0;
        self.scroll_wrap_flag = 0;
        // $1F32: LDY #$08; JSR sub_9A4F (short busy wait): the rest runs in the next chain
        self.main = MainLine::Bonus(BonusPhase::Prep);
    }

    /// The chain that finishes the swap runs in the slot after the interrupt chain
    pub fn bonus_phase_is_last(&self, p: &BonusPhase) -> bool {
        match p {
            BonusPhase::Swap { chain, back } => {
                let t = self.bonus_timing_front();
                let steps = if *back { &t.ret_swap } else { &t.swap };
                // the chain that finishes the swap runs between the handlers unless it ended before the chain (`swap_early`)
                let early = if *back { t.ret_swap_early } else { t.swap_early };
                !early && steps.get(*chain).map_or(false, |&(c, _)| c >= SWAP_CELLS)
            }
            // the set-up after the walk happens between the interrupts $12A3 and $1393
            BonusPhase::Prep => true,
            BonusPhase::DecompBack(0) => !self.bonus_timing_front().ret_late && !self.bonus_timing_front().ret_early,
            // the end of the final wait: between the interrupts $12A3 and $1393 if the chain of $1393 already sees `$0457` = 0
            BonusPhase::Fin(0) => self.bonus_timing_front().fin_early,
            _ => false,
        }
    }

    /// One chain of sub_1FE2 ($1FE2..$203F): advances to the progress measured for this chain (or the default pace).
    /// Returns true when all 20 x 40 cells are done.
    pub fn swap_chain_sub_1fe2(&mut self, chain: usize, back: bool) -> bool {
        let t = self.bonus_timing_front();
        let steps = if back { &t.ret_swap } else { &t.swap };
        let target = steps.get(chain).copied().unwrap_or(if steps.is_empty() {
            ((chain + 1) * SWAP_CELLS_PER_CHAIN, 0)
        } else {
            (SWAP_CELLS, 0)
        });
        self.swap_advance_sub_1fe2(target.0.min(SWAP_CELLS), target.1);
        self.swap_state.0 >= SWAP_CELLS
    }

    /// The loop at `$2006`, store by store: A = ($04),Y (map); PHA; A = ($06),Y (screen); STA ($04),Y; PLA; STA ($06),Y;
    /// TAX; STA ($08),Y = `$AB00[X]` (colour). Each cell goes row by row from column 39 down to 0.
    fn swap_advance_sub_1fe2(&mut self, cells: usize, part: u8) {
        loop {
            let (done, st, hold_m, hold_s) = self.swap_state;
            if done >= SWAP_CELLS || (done, st) >= (cells, part) {
                break;
            }
            let (r, x) = (done / 40, 39 - done % 40);
            let mi = r * 256 + x;
            let si = (r + 5) * 40 + x;
            match st {
                0 => {
                    let m = self.current_map[mi];
                    let s = self.screen_ram[si];
                    self.current_map[mi] = s;
                    self.swap_state = (done, 1, m, s);
                }
                1 => {
                    self.screen_ram[si] = hold_m;
                    self.swap_state = (done, 2, hold_m, hold_s);
                }
                _ => {
                    self.color_ram[si] = self.char_colour(hold_m);
                    self.swap_state = (done + 1, 0, 0, 0);
                }
            }
        }
        // $2035: DEC $045A per finished row (counts $13 down to $FF)
        self.chk[2] = 0x13u8.wrapping_sub((self.swap_state.0 / 40) as u8);
    }

    /// L1F77..L1F9C: statue tiles ($8F) around the checkpoint are replaced by the next tile of the counter at `$2040`
    fn statue_replace_l1f77(&mut self) -> bool {
        // ($04) = $045F/$0460 (screen address), ($06) = colour RAM at the same offset
        let mut p = self.chk[7] as usize | (self.chk[8] as usize) << 8;
        let mut y = 0usize;
        loop {
            let at = |s: &GameState, i: usize| -> u8 {
                let a = p + i;
                if (0xC800..0xC800 + 1000).contains(&a) { s.screen_ram[a - 0xC800] } else { 0 }
            };
            if at(self, y) == 0x8F {
                break;
            }
            y += 1;
            if y >= 0x80 {
                return false;
            }
        }
        loop {
            // L1F81: INC $2040; wrap at $54 to $52
            self.statue_tile_2040 = self.statue_tile_2040.wrapping_add(1);
            if self.statue_tile_2040 >= 0x54 {
                self.statue_tile_2040 = 0x52;
            }
            let a = p + y;
            if (0xC800..0xC800 + 1000).contains(&a) {
                self.screen_ram[a - 0xC800] = self.statue_tile_2040;
                self.color_ram[a - 0xC800] = 0x08;
            }
            y += 1;
            let b = p + y;
            let next = if (0xC800..0xC800 + 1000).contains(&b) { self.screen_ram[b - 0xC800] } else { 0 };
            if next != 0x8F {
                break;
            }
        }
        let _ = &mut p;
        true
    }

    /// sub_213A ($213A..$2168): give back the sprite registers saved by sub_210B
    pub fn restore_sprites_sub_213a(&mut self) {
        for i in 0..16 {
            let v = self.saved_sprites[i];
            self.set_spr(i, v);
        }
        self.sprite_x_msb = self.saved_sprites[16];
        self.en.vic_mdp = self.saved_vic[0];
        self.en.vic_mmc = self.saved_vic[1];
        self.en.vic_me = self.saved_vic[2];
        for i in 0..8 {
            self.en.vic_col[i] = self.saved_colours[i];
            self.set_sptr(i, self.saved_ptrs[i]);
        }
        let _ = img(0);
    }
}

impl GameState {
    /// The last chain of a swap that ended in the gap before the interrupts of its frame
    pub fn bonus_swap_ends_early(&self, p: &BonusPhase) -> bool {
        match p {
            BonusPhase::Swap { chain, back } => {
                let t = self.bonus_timing_front();
                let steps = if *back { &t.ret_swap } else { &t.swap };
                let early = if *back { t.ret_swap_early } else { t.swap_early };
                early && steps.get(*chain).map_or(false, |&(c, _)| c >= SWAP_CELLS)
            }
            _ => false,
        }
    }
}
