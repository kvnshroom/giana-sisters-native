//! Column scrolling of the original engine: raster IRQ $1326 and sub_9600, plus the title demo.
//!
//! sub_9600 shifts the playfield one character column to the left and fetches the new right column from the
//! map. It starts in the $1326 IRQ (DEC $14 -> $FE, INC $21, CLI) and its copy loops run longer than the
//! rest of the frame, so the oracle snapshot still shows `$14 = $FE`. The stores of the loops are therefore made
//! in the next chain (`column_scroll_finish_sub_9600`).

use crate::state::{ControllerInput, GameState};


/// The copy loops of sub_9600 in their order: (colour RAM, first row, last row). Each loop copies the cell to the right for
/// columns 0..38, all its rows per column, then the column 39 of its rows (from the map, colour from `$AB00`).
const SCROLL_LOOPS: [(bool, usize, usize); 8] = [
    (false, 5, 5),    // L9610, $961B
    (false, 6, 7),    // L9623, $9636..
    (false, 8, 11),   // L9644, $9661..
    (true, 5, 11),    // L967B, $96AA..
    (false, 12, 19),  // L96F2, $9727..
    (true, 12, 19),   // L975B, $9790..
    (false, 20, 24),  // L97E2, $9805..
    (true, 20, 24),   // L9827, $984A..
];

/// Stores of the copy of sub_9600: 20 rows of 40 screen cells and 20 rows of 40 colour cells.
pub const SCROLL_STORES: usize = 1600;

/// Store `k` of the copy: (colour RAM, screen row 5..24, column 0..39).
pub fn scroll_store(k: usize) -> (bool, usize, usize) {
    let mut k = k;
    for (colour, first, last) in SCROLL_LOOPS {
        let rows = last - first + 1;
        if k < rows * 40 {
            return if k < rows * 39 { (colour, first + k % rows, k / rows) } else { (colour, first + k - rows * 39, 39) };
        }
        k -= rows * 40;
    }
    (true, 24, 39)
}

const SLOT_BASE: usize = 0x9B02;
/// (low byte table, high byte table) pairs decremented as 16-bit words by the $1326 handler.
const SLOT_WORDS: [(usize, usize); 4] = [(0x9B0A, 0x9B12), (0x9B40, 0x9B48), (0x9B58, 0x9B60), (0x9B70, 0x9B78)];

impl GameState {
    /// sub_0D1F ($0D1F..$0D38): Clear the state bytes of the animated tile slots
    pub fn clear_entity_slots_sub_0d1f(&mut self) {
        // $0D1F: LDY #$07; LDA #$00; loop: STA $9B02,Y / $9B38,Y / $9B50,Y / $9B68,Y / $9B90,Y / $9B98,Y
        for y in (0..8usize).rev() {
            for base in [0x9B02usize, 0x9B38, 0x9B50, 0x9B68, 0x9B90, 0x9B98] {
                self.particle_slots[base + y - SLOT_BASE] = 0;
            }
        }
    }

    /// $1326 handler ($1347..$138A): shift animated tile pointers and start sub_9600 when `$14 == $FF`
    pub fn scroll_irq_l1326(&mut self) {
        // $1347: LDA $14; CMP #$FF; BNE L138A
        if self.scroll_wrap_flag != 0xFF {
            return;
        }
        // $134D: LDX #$07 ... DEX; BPL: 16-bit decrement of four pointer tables
        for x in (0..8usize).rev() {
            for (lo, hi) in SLOT_WORDS {
                let lo_i = lo + x - SLOT_BASE;
                let hi_i = hi + x - SLOT_BASE;
                // LDA lo,X; BNE skip; DEC hi,X; skip: DEC lo,X
                if self.particle_slots[lo_i] == 0 {
                    self.particle_slots[hi_i] = self.particle_slots[hi_i].wrapping_sub(1);
                }
                self.particle_slots[lo_i] = self.particle_slots[lo_i].wrapping_sub(1);
            }
        }
        // $137E: LDA $154D; CMP #$80; BNE L1387; INC $21
        if self.music_paused == 0x80 {
            self.map_column = self.map_column.wrapping_add(1);
        }
        // L1387: JSR sub_9600
        self.column_scroll_start_sub_9600();
    }

    /// sub_9600 ($9600..$9607 and $9607..$960C): entry part executed inside the IRQ
    pub fn column_scroll_start_sub_9600(&mut self) {
        // $9600: LDA $14; CMP #$FF; BEQ L9607; RTS
        if self.scroll_wrap_flag != 0xFF {
            return;
        }
        // L9607: DEC $14; CLI; INC $21
        self.scroll_wrap_flag = 0xFE;
        self.map_column = self.map_column.wrapping_add(1);
        self.column_scroll_pending = true;
    }

    /// sub_9600 ($960C..$9880): copy loops, then `$14 = 0`.
    ///
    /// The copy runs in the main line from the `$1326` handler on and is still going when the next chain starts: the port
    /// makes its stores (`SCROLL_STORES`, in the order of the original loops) at the start of that chain up to
    /// `scroll_split_tiles`, after the handler `$1249` up to `scroll_split`, and the rest after the handler `$12A3`. Both are
    /// all stores by default; a recording shows where the copy stood when the animated tiles of `$1249` wrote their cells
    /// (stage_31_run frame 233: `sub_19CF` stores `$2E` to `$CB59` at line 70, the loop `$97E2` of rows 20..24 starts at line 74)
    /// and when the tile lookup `sub_9DF4` and the block interaction `sub_1A17` of `$12A3` read the screen (stage_31_run frame
    /// 1310: the loop `$97E2` is at column 18 when `sub_9DF4` reads row 21).
    pub fn column_scroll_finish_sub_9600(&mut self) {
        if !self.column_scroll_pending {
            return;
        }
        self.column_scroll_pending = false;
        self.scroll_copy_done = 0;
        let to = self.scroll_split_tiles.min(self.scroll_split);
        self.scroll_copy_to(to);
    }

    /// The stores of the copy that came after the handler `$1249` and before `$12A3` (see `column_scroll_finish_sub_9600`).
    pub fn column_scroll_after_tiles_sub_9600(&mut self) {
        let to = self.scroll_split;
        self.scroll_copy_to(to);
    }

    /// The stores of the copy that came after the handler `$12A3` of the chain (see `column_scroll_finish_sub_9600`).
    pub fn column_scroll_rest_sub_9600(&mut self) {
        self.scroll_copy_to(SCROLL_STORES as u16);
    }

    /// Makes the stores of the running copy up to `to`; the last one ends sub_9600 (`$987C`: `$14 = 0`).
    fn scroll_copy_to(&mut self, to: u16) {
        let to = to.min(SCROLL_STORES as u16);
        while self.scroll_copy_done < to {
            let (colour, row, col) = scroll_store(self.scroll_copy_done as usize);
            let i = row * 40 + col;
            if colour {
                // colour RAM: the cell to the right, column 39 from the new character (`$AB00`)
                self.color_ram[i] = if col < 39 {
                    self.color_ram[i + 1]
                } else if let Some(c) = self.wide_scroll_in(self.map_column, row - 5, true) {
                    // WIDESCREEN ON: a cell of the right extra area that the game changed comes in with its colour
                    c
                } else {
                    self.char_colour(self.screen_ram[i])
                };
            } else if col < 39 {
                if col == 0 {
                    // the cell that leaves the screen (WIDESCREEN ON keeps it)
                    self.wide_note_shift(row, row + 1);
                }
                self.screen_ram[i] = self.screen_ram[i + 1];
            } else {
                // LDA $AC00+256*(row-5),X ; STA $C8EF + 40*(row-5)
                // (WIDESCREEN ON: or the cell of the right extra area as the game changed it)
                self.screen_ram[i] = match self.wide_scroll_in(self.map_column, row - 5, false) {
                    Some(ch) => ch,
                    None => self.current_map.get((row - 5) * 256 + self.map_column as usize).copied().unwrap_or(0),
                };
            }
            self.scroll_copy_done += 1;
            if self.scroll_copy_done == SCROLL_STORES as u16 {
                // $987C: LDA #$00; STA $14
                self.scroll_wrap_flag = 0;
            }
        }
    }

    /// Title demo (original frames between the stage-2 attract screen and the fire press): the IRQ chain runs
    /// with `$154D != 0`, so sub_0EAB only auto-scrolls the playfield by table step $24 (3 px per frame).
    /// Afterwards L0940/L09B6/L09DA start Stage 1 (`start_stage`), which keeps the slot table.
    pub fn run_title_demo(&mut self, frames: usize) {
        self.music_paused = 0xFF; // $08C5: STA $154D
        self.flag_0431 = 0; // $093D: STA $0431 = 0
        let input = ControllerInput::new();
        for _ in 0..frames {
            self.irq_frame_core(&input);
        }
        self.column_scroll_finish_sub_9600();
        self.column_scroll_rest_sub_9600();
        // $096F: STA $154D = 0 on fire; L09B6: sub_0D1F; L09DA: stage setup
        self.music_paused = 0;
        self.clear_entity_slots_sub_0d1f();
        // family T (`$0996: JSR $7AC0`): the start menu comes first, the stage starts after the choice
        if self.tnw_menu_begin() {
            return;
        }
        self.start_stage(self.stage);
    }
}
