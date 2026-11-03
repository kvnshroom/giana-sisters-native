//! Column scrolling of the original engine: raster IRQ $1326 and sub_9600, plus the title demo.
//!
//! sub_9600 shifts the playfield one character column to the left and fetches the new right column from the
//! map. It starts in the $1326 IRQ (DEC $14 -> $FE, INC $21, CLI) and its copy loops run longer than the
//! rest of the frame, so the oracle snapshot (raster line 0) still shows `$14 = $FE`. The loops are therefore
//! executed at the beginning of the next frame, before the next IRQ chain (`column_scroll_finish_sub_9600`).

use crate::state::{ControllerInput, GameState};


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

    /// sub_9600 ($960C..$9880): copy loops, then `$14 = 0`
    pub fn column_scroll_finish_sub_9600(&mut self) {
        if !self.column_scroll_pending {
            return;
        }
        self.column_scroll_pending = false;
        let x = self.map_column as usize;
        // Rows 5..24 of screen RAM ($C8C8..$CBE7): shift left by one column, new column 39 from the map
        for r in 5..25usize {
            let base = r * 40;
            for y in 0..39usize {
                self.screen_ram[base + y] = self.screen_ram[base + y + 1];
            }
            // LDA $AC00+256*(r-5),X ; STA $C8EF + 40*(r-5)
            self.screen_ram[base + 39] = self.current_map.get((r - 5) * 256 + x).copied().unwrap_or(0);
        }
        // Colour RAM: same shift; new colour = $AB00[new screen character]
        for r in 5..25usize {
            let base = r * 40;
            for y in 0..39usize {
                self.color_ram[base + y] = self.color_ram[base + y + 1];
            }
            self.color_ram[base + 39] = self.char_colour(self.screen_ram[base + 39]);
        }
        // $987C: LDA #$00; STA $14
        self.scroll_wrap_flag = 0;
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
