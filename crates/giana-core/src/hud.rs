//! Text and status bar of the original: sub_9881 (strings), sub_98D9 (BCD numbers), sub_1CD7 (status bar update),
//! sub_0E66 (status bar frame graphics) and the texts of the stage banner.
//!
//! All of it writes screen RAM (`$C800 + 40 * row + column`) and colour RAM (white, `$07`).

use crate::image::{img, img_word};
use crate::state::GameState;

/// Stores of sub_0E66 (16 + 10 screen cells, 120 colour cells).
pub const STATUS_FRAME_STORES: usize = 146;

impl GameState {
    pub(crate) fn text_cell(&mut self, index: usize, ch: u8) {
        if index < 1000 {
            self.put_cell(false, index, ch);
            self.put_cell(true, index, 0x07);
        }
    }

    /// One store into screen RAM (`colour` false) or colour RAM. With a `store_budget` (the sample cuts the routine: see
    /// `StageTiming::header_cut`) the stores after the budget wait in `store_deferred` for `header_rest`.
    pub(crate) fn put_cell(&mut self, colour: bool, index: usize, v: u8) {
        if let Some(log) = self.store_log.as_mut() {
            log.push((colour, index, v));
        }
        if let Some(b) = self.store_budget.as_mut() {
            if *b == 0 {
                self.store_deferred.push((colour, index, v));
                return;
            }
            *b -= 1;
        }
        if colour {
            self.color_ram[index] = v;
        } else {
            self.screen_ram[index] = v;
        }
    }

    /// The stores of the status bar header that the cut left for the next chain.
    pub fn header_rest(&mut self) {
        for (colour, index, v) in std::mem::take(&mut self.store_deferred) {
            if colour {
                self.color_ram[index] = v;
            } else {
                self.screen_ram[index] = v;
            }
        }
    }

    /// The stores of the status bar header `sub_0DE7` in the order of the 6502 (for the harness: where the sample cut it).
    pub fn header_stores_for_test(&mut self) -> Vec<(bool, usize, u8)> {
        self.store_log = Some(Vec::new());
        self.timing.tile_progress = vec![0];
        self.music_paused = 0;
        self.status_bar_header_l0de7();
        self.store_log.take().unwrap_or_default()
    }

    /// sub_9881 ($9881..$98D8): print the string with number `string` of the table at `$9BD2` at row `row`, offset `x`
    pub fn print_string_sub_9881(&mut self, string: usize, x: usize, row: usize) {
        let mut p = img_word(0x9BD2 + 2 * string);
        self.print_string_at_sub_9881(&mut p, x, row);
    }

    /// sub_9881 with the carry set: the string pointer is in `$04/$05`
    pub fn print_string_at_sub_9881(&mut self, p: &mut usize, x: usize, row: usize) {
        let mut idx = row * 40 + x;
        loop {
            let b = img(*p);
            if b == 0xFF {
                return;
            }
            self.text_cell(idx, b);
            idx += 1;
            *p += 1;
        }
    }

    /// sub_98D9 ($98D9..$991C): print the bytes `bytes[0]` (= memory at `$02/$03 + X`) down to `bytes[X]` (= `$02/$03`) as two
    /// digits each at screen row `row` (row pointer table `$9FD3`), starting at column `col`
    pub fn print_bcd_sub_98d9(&mut self, bytes: &[u8], row: usize, col: usize) {
        let base = if self.engine_b.is_some() {
            // family B (`$98E7..$98F1`): the row address is split in two tables, low bytes `$9BA0`, high bytes `$9FD3`
            ((img(0x9FD3 + row) as usize) << 8 | img(0x9BA0 + row) as usize).wrapping_sub(0xC800)
        } else {
            img_word(0x9FD3 + 2 * row).wrapping_sub(0xC800)
        };
        let mut idx = base + col;
        for &b in bytes {
            self.text_cell(idx, ((b >> 4) & 0x0F) | 0x30);
            self.text_cell(idx + 1, (b & 0x0F) | 0x30);
            idx += 2;
        }
    }

    /// sub_1CD7 ($1CD7..$1D09): update score, diamonds and time in the status bar
    pub fn status_bar_sub_1cd7(&mut self) {
        let score = [self.score_bcd[2], self.score_bcd[1], self.score_bcd[0]];
        self.print_bcd_sub_98d9(&score, 0, 0x2A);
        self.print_bcd_sub_98d9(&[self.diamonds_bcd], 0, 0x36);
        if self.engine_b.is_some() {
            // family B (`$1CF9..$1D07`): four digits, the high byte first, from column $4A
            self.print_bcd_sub_98d9(&[self.timer_seconds_high, self.timer_seconds_low], 0, 0x4A);
            return;
        }
        self.print_bcd_sub_98d9(&[self.timer_seconds_low], 0, 0x4B);
    }

    /// The stores of sub_0E66 (`$0E6C..$0E9C`, `$1E == 0`) in the order of the 6502: (colour RAM?, offset, value).
    pub fn status_bar_frame_stores_for_test(&self) -> Vec<(bool, usize, u8)> {
        self.status_bar_frame_stores()
    }

    fn status_bar_frame_stores(&self) -> Vec<(bool, usize, u8)> {
        let mut v = Vec::with_capacity(STATUS_FRAME_STORES);
        // L0E6C: Y = 3..0: $C851,Y and $C887,Y from $93C8,Y; $C879,Y and $C8AF,Y from $93CC,Y
        for y in (0..4usize).rev() {
            let (a, b) = (img(0x93C8 + y), img(0x93CC + y));
            v.extend([(false, 0x51 + y, a), (false, 0x87 + y, a), (false, 0x79 + y, b), (false, 0xAF + y, b)]);
        }
        // L0E83: Y = 4..0: $C86A,Y from $93D3,Y and $C892,Y from $93D8,Y
        for y in (0..5usize).rev() {
            v.extend([(false, 0x6A + y, img(0x93D3 + y)), (false, 0x92 + y, img(0x93D8 + y))]);
        }
        // L0E96: Y = $78..1: $D84F,Y = 1
        for y in (1..=0x78usize).rev() {
            v.push((true, 0x4F + y, 1));
        }
        v
    }

    /// sub_0E66 ($0E66..$0EAA): the frame graphics of the status bar (cleared when `$1E != 0`)
    pub fn status_bar_frame_sub_0e66(&mut self) {
        self.status_bar_frame_steps(0, usize::MAX);
    }

    /// The stores `from..to` of sub_0E66 (all of them in the `$1E != 0` case, which is one loop)
    pub fn status_bar_frame_steps(&mut self, from: usize, to: usize) {
        if self.zp_1e != 0 {
            if from == 0 {
                for y in 1..=0x78usize {
                    self.screen_ram[0x4F + y] = 0;
                    self.color_ram[0x4F + y] = 0;
                }
            }
            return;
        }
        let stores = self.status_bar_frame_stores();
        let to = to.min(stores.len());
        if from >= to {
            return;
        }
        for &(colour, at, v) in &stores[from..to] {
            if colour {
                self.color_ram[at] = v;
            } else {
                self.screen_ram[at] = v;
            }
        }
    }

    /// `$0A61..$0A93` of L09DA: the stage banner texts
    pub fn banner_text_l0a61(&mut self) {
        // $0A5E: JSR sub_0E39 has just set $D01A = 1 (raster interrupt on, vectors and $D012 for the first handler)
        self.vic_irq_enable = 1;
        self.print_string_sub_9881(self.player as usize, 0x0F, 7); // string `$1568`: GIANA or MARIA
        self.print_string_sub_9881(2, 0x0E, 9);
        self.print_string_sub_9881(3, 0x0E, 0x0B);
        // $0A56 comes right before the text in the 6502 (no wait between): the harness may place the text a chain earlier
        self.stage_number_l0a56();
        // $0A80: $02/$03 = $0466; Y = $0B; X = 0; A = $15
        self.print_bcd_sub_98d9(&[self.death_arc_idx], 0x0B, 0x15);
        self.request_music_sub_150c(6);
    }

    /// `$0DE7..$0E1C` of sub_0D54: status bar header (only while `$154D == 0`)
    pub fn status_bar_header_l0de7(&mut self) {
        self.print_string_sub_9881(self.player as usize, 1, 0);
        self.print_string_sub_9881(4, 8, 0);
        // $0DFC: LDA #$F7; STA $C833; LDA #$01; STA $D833
        self.put_cell(false, 0x33, 0xF7);
        self.put_cell(true, 0x33, 0x01);
        // $0E06: LDX $20; LDA $9FAE,X; STA $043D
        self.warp_stage_id = self.bcd_table(self.stage as usize);
        self.print_bcd_sub_98d9(&[self.warp_stage_id], 0, 0x45);
        // $0E1F: the code falls into sub_0E1F (lives counter)
        self.draw_lives_sub_0e1f();
        // The first main loop pass (`$0BF6` -> sub_1CD7) follows directly and, in the recording, lies before the sample
        // point of the first gameplay frame: do it here so that this frame already shows score, diamonds and time.
        // (measured restarts do this when the main loop starts: `restart_first_pass`)
        if self.hazard_hit == 0 && self.timing.tile_progress.is_empty() {
            self.status_bar_sub_1cd7();
        }
    }
}
