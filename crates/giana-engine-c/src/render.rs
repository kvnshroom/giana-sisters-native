//! Rendering a frame of the 30A preview with `giana-render`.
//!
//! The frame uses the same band structure as the original's, which the measurements of `docs/rom-hack-family-c.md`
//! (section 1.4) confirm, so the existing per-band renderer needs no change for the picture itself:
//!
//! | text rows | raster lines | `$D016` (fine scroll) | source |
//! |---|---|---|---|
//! | 0-1 (status bar) | `$33..$42` | `$17` = 0 | fixed, written at `$FB` |
//! | 2-4 (cloud band) | `$43..$5A` | `$68 >> 1` | written cycle-exactly by the IRQ at `$30A4` |
//! | 5-24 (playfield) | `$5B..$FB` | `$67 \| $10` | written by the IRQ at `$31F4` |
//!
//! Bank 3: characters `$C000`, screen `$C800`, sprite pointers `$CBF8`, sprite shapes anywhere in `$C000..$FFFF`.
//!
//! Which state a frame shows: screen RAM and colour RAM **as of the end of the frame** (the logic in the `$5A` IRQ rewrites
//! rows below the beam before it arrives), sprite registers and the fine scroll values `$67/$68` as of the frame start.
//!
//! What is *not* modelled yet (work package W): the two sprite changes during the frame (Giana's new position at line
//! `player y - 4` when `$94` is set; the object multiplexer `$3183` that moves a finished sprite 21 lines down), the
//! sprite-sprite collision register `$D01E`, and sprite priority against characters beyond what `giana-render` has.

use giana_render::framebuffer::Framebuffer;
use giana_render::vic::{render_vic_bands, BandSpec, VicView, GEOMETRY_30A};

/// Everything of the machine that one frame of the preview needs (state rules in the module documentation).
pub struct FrameState<'a> {
    /// all 64 KB of RAM
    pub ram: &'a [u8],
    /// VIC registers `$D000..$D02E`
    pub vic: [u8; 47],
    /// colour RAM (1000 cells)
    pub color: &'a [u8],
}

impl<'a> FrameState<'a> {
    /// Scroll value of the playfield (`$67`, low three bits of `$D016`).
    pub fn play_scroll(&self) -> u8 {
        self.ram[0x67] & 7
    }

    /// Scroll value of the cloud band (the IRQ at `$30A4` writes `$68 >> 1` to `$D016`).
    pub fn sky_scroll(&self) -> u8 {
        (self.ram[0x68] >> 1) & 7
    }

    /// The three bands: status bar (`$D016 = $17`, `$D022/$D023 = $0A/$02` written by the `$FB` IRQ), cloud band and
    /// playfield (`$D022/$D023` from the stage header copy `$010A/$010B`, written by the `$42` IRQ). The cloud band's text
    /// mode is bit 4 of what that IRQ writes to `$D016` (`$68 >> 1`): the clouds of some stages are hi-res text.
    pub fn bands(&self) -> [BandSpec; 3] {
        let d021 = self.vic[0x21] & 15;
        let stage = [d021, self.ram[0x010A] & 15, self.ram[0x010B] & 15];
        [
            BandSpec { xscroll: 7, bg: [d021, 0x0A, 0x02], hires: false },
            BandSpec { xscroll: self.sky_scroll(), bg: stage, hires: self.ram[0x68] & 0x20 == 0 },
            BandSpec { xscroll: self.play_scroll(), bg: stage, hires: false },
        ]
    }

    /// Draws the frame into `fb` (384x272 with border, like every other frame of the port).
    pub fn render(&'a self, fb: &mut Framebuffer) {
        render_vic_bands(fb, &self.view(), self.bands(), GEOMETRY_30A);
    }

    /// The `giana-render` view of this frame.
    pub fn view(&'a self) -> VicView<'a> {
        let mut xy = [0u8; 17];
        xy.copy_from_slice(&self.vic[..17]);
        let mut ptr = [0u8; 8];
        ptr.copy_from_slice(&self.ram[0xCBF8..0xCC00]);
        let mut spr_col = [0u8; 8];
        spr_col.copy_from_slice(&self.vic[0x27..0x2F]);
        VicView {
            mem: &self.ram[0xC000..],
            screen: &self.ram[0xC800..0xCBE8],
            chars: &self.ram[0xC000..0xC800],
            clouds: &self.ram[0xC800 + 80..0xC800 + 200],
            color: self.color,
            bg: [self.vic[0x21] & 15, self.vic[0x22] & 15, self.vic[0x23] & 15],
            xscroll: self.play_scroll(),
            sky_xscroll: self.sky_scroll(),
            me: self.vic[0x15],
            mmc: self.vic[0x1C],
            mdp: self.vic[0x1B],
            xy,
            ptr,
            spr_col,
            spr_mc: [self.vic[0x25] & 15, self.vic[0x26] & 15],
            wide: None,
        }
    }
}
