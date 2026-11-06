//! The hooks of engine B in `GameState`: small methods the original routines of the port call behind an
//! `engine_b.is_some()` check. See the table in `engine_b/mod.rs`.

use crate::image::{image, img};
use crate::state::GameState;

/// `$8F99`: decimal to BCD table of the hack image (index 0..=99), the place of the original's `$9FAE`.
const BCD_TABLE: usize = 0x8F99;

impl GameState {
    /// True when engine B (the 7A3 construction-set engine) runs this game.
    pub fn is_engine_b(&self) -> bool {
        self.engine_b.is_some()
    }

    /// Stage numbers are `1..=33` in the original; engine B has up to 99 stage files and the rooms from `$64` on.
    pub(crate) fn clamp_stage(&self, stage: u32) -> u32 {
        if self.engine_b.is_some() || self.tnw.is_some() {
            stage.min(0xFF) // family T has 32 stages per set and 64 in the grand loop
        } else {
            stage.clamp(1, 33)
        }
    }

    /// The original's decimal-to-BCD table `$9FAE` (engine B: `$8F99`) at `n`.
    pub(crate) fn bcd_table(&self, n: usize) -> u8 {
        img(if self.engine_b.is_some() { BCD_TABLE } else if self.tnw.is_some() { 0x6F00 } else { 0x9FAE } + n)
    }

    /// Tile class (`$9C20`, bit 0 = solid ...) of a character; engine B changes a few entries per stage.
    #[inline]
    pub fn tile_flags(&self, ch: u8) -> u8 {
        match &self.engine_b {
            Some(b) => b.classes[ch as usize],
            None => Self::tile_lookup_sub_9f40(ch),
        }
    }

    /// Colour attribute (`$AB00`) of a character as it goes into colour RAM; engine B changes some per stage.
    #[inline]
    pub(crate) fn char_colour(&self, ch: u8) -> u8 {
        match &self.engine_b {
            Some(b) => b.colours[ch as usize],
            None => crate::tables::TABLE_AB00[ch as usize],
        }
    }

    /// The colour that a character gets when the scroll brings it into colour RAM (read only; the widescreen view uses it for
    /// the columns beyond the C64 picture).
    pub fn tile_colour(&self, ch: u8) -> u8 {
        self.char_colour(ch)
    }

    /// Engine B (`$0924..$092C`): the hack's image holds the first 64 characters inverted (`$C000 != 0`) and the start-up
    /// code inverts them once (`$32C9`).
    pub(crate) fn b_init_charset(&mut self) {
        if self.engine_b.is_some() && self.sprite_mem[0] != 0 {
            for b in &mut self.sprite_mem[..0x200] {
                *b ^= 0xFF;
            }
        }
    }

    /// Engine B: `$9936` (with the hook and the loader): builds the map for `self.stage`.
    pub(crate) fn b_decompress_stage(&mut self) {
        let Some(mut b) = self.engine_b.take() else { return };
        let built = b.build(image(), &mut self.sprite_mem[..0x800], self.stage as u8);
        self.stage = built.stage as u32;
        if let Some(p) = built.min_power {
            // Gem Nightmare's builder hook (`$9A38`: `LDX #2; STX $22`): at least this power-up level
            self.anim_counter = self.anim_counter.max(p);
        }
        if let Some(l) = built.lives {
            self.lives_c64 = l;
            self.lives = l as u32;
        }
        if let Some((addr, data)) = built.sprites {
            let at = (addr as usize).wrapping_sub(0xC000);
            if at < self.sprite_mem.len() {
                let n = data.len().min(self.sprite_mem.len() - at);
                self.sprite_mem[at..at + n].copy_from_slice(&data[..n]);
            }
        }
        self.current_map = built.map;
        // the end of the builder (`$9965`)
        self.map_column = 0x27;
        self.engine_b = Some(b);
    }

    /// Engine B: `$0D2E`, the stage palette from the header and the diamond colours.
    pub(crate) fn b_stage_colours(&mut self) {
        let Some(b) = self.engine_b.as_mut() else { return };
        let h = &b.current.header;
        self.zp_1e = h.underground;
        self.en.vic_bg0 = h.bg[0] & 0x0F;
        self.en.vic_bg1 = h.bg[1] & 0x0F;
        self.vic_bg2 = h.bg[2] & 0x0F;
        if b.gem.is_some() {
            // Gem Nightmare (`$0D3F`, `$0D52`): the diamond colours are the high nibbles of the background colour bytes
            let (d0, d2) = (h.bg[0] >> 4, h.bg[2] >> 4);
            b.colours[0xA9] = d0;
            b.colours[0xA7] = d2;
            b.colours[0xA8] = d2;
        } else {
            let d = giana_engine_b::hooks::diamond_colours(image(), h.diamond_scheme);
            b.colours[0xA6..=0xA9].copy_from_slice(&d);
        }
    }

    /// Engine B: `$0AD9..$0AF0` after the builder: the 4-digit time of the header and the stage number for the banner.
    pub(crate) fn b_stage_start_values(&mut self) {
        let Some(b) = self.engine_b.as_ref() else { return };
        let t = b.current.header.time_bcd;
        self.timer_seconds_low = t as u8;
        self.timer_seconds_high = (t >> 8) as u8;
        self.time = (self.timer_seconds_high as u32 >> 4 & 15) * 1000
            + (self.timer_seconds_high as u32 & 15) * 100
            + (self.timer_seconds_low as u32 >> 4 & 15) * 10
            + (self.timer_seconds_low as u32 & 15);
        self.death_arc_idx = self.bcd_table(self.stage as usize);
    }

    /// Engine B: Giana's start position `(x, y)` from the header (`$1426`: bytes `$900C + $0456` and the next one).
    pub(crate) fn b_start_position(&self) -> Option<(u8, u8)> {
        let b = self.engine_b.as_ref()?;
        let s = b.current.header.start;
        let raw = [s[0][0], s[0][1], s[1][0], s[1][1]];
        let y = self.spawn_variant as usize;
        Some((raw.get(y).copied().unwrap_or(0), raw.get(y + 1).copied().unwrap_or(0)))
    }

    /// Engine B: the music track of the stage (`$0B37`: `LDA $9003`), `None` for the original.
    pub(crate) fn b_music_track(&self) -> Option<u8> {
        self.engine_b.as_ref().map(|b| b.current.header.music)
    }

    /// Engine B: true when the stage after this one has no stage file, i.e. the game is over after this stage (the
    /// original ends after stage 33).
    pub(crate) fn b_is_last_stage(&self) -> bool {
        match &self.engine_b {
            Some(b) => {
                let next = self.stage + 1;
                next >= 100 || !b.has_stage(image(), next as u8)
            }
            None if self.tnw.is_some() => self.tnw_is_last_stage(),
            None => self.stage >= 33,
        }
    }

    /// Engine B: the place of the hack's sprite program (`$776E`: spawn, move, collide, shoot, bosses). Called once per
    /// frame where the original calls its spawner, collision and movement routines (`enemy_main_sub_2194`, while the game
    /// runs). The next work packages fill it (E3 and later, see `docs/rom-hack-family-b.md`); today it does nothing, so no
    /// object of a stage appears.
    pub(crate) fn b_sprite_frame(&mut self) {
        self.b_sprite_frame_impl();
    }
}
