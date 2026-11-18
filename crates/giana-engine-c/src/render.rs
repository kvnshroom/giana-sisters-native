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
//! Sprites that change during the frame (Giana's new position at line `player y - 4` while the bump sprites are up, the
//! object multiplexer `$3183` that moves a finished sprite 21 lines down): [`FrameState::render_rows`] draws the sprite
//! rows each raster line showed, as [`crate::sprites::SpriteUnit`] records them from the frame's register writes; the
//! same unit gives the sprite-sprite collisions `$D01E`.

use crate::sprites::SpriteRow;
use giana_render::framebuffer::{Framebuffer, WIDTH};
use giana_render::palette::c64_color;
use giana_render::vic::{render_vic_bands, render_vic_bands_rows, BandSpec, SpriteRowDraw, VicView, GEOMETRY_30A};

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

    /// Draws the frame with the sprites given row by row (the rows of raster lines 51..250 that the frame showed,
    /// recorded by [`crate::sprites::SpriteUnit`]) instead of from the registers of the frame start. `$D025/$D026` come
    /// from `vic`.
    pub fn render_rows(&'a self, fb: &mut Framebuffer, rows: &[SpriteRow]) {
        let mc = [self.vic[0x25] & 15, self.vic[0x26] & 15];
        let draw: Vec<SpriteRowDraw> = rows
            .iter()
            .map(|r| SpriteRowDraw {
                line: r.line as isize,
                sprite: r.sprite,
                x: r.x as isize,
                bits: r.bits,
                multi: r.multicolor,
                expand_x: r.expand_x,
                behind: r.behind,
                color: r.color,
                mc,
            })
            .collect();
        render_vic_bands_rows(fb, &self.view(), self.bands(), GEOMETRY_30A, &draw);
    }

    /// Draws a frame of the game flow's screens (menus and text pages, the title, the high-score screen) raster line
    /// by raster line: `regs` are the VIC registers of each raster line 0..311 of the picture
    /// ([`crate::frame::Core::picture_regs`]); every line gets its border colour, background colours, text mode
    /// (multicolour or hi-res, `$D016` bit 4), fine scroll and 38/40 columns (`$D016` bit 3); the screen is blank
    /// (border colour) when `$D011` bit 4 is clear at line `$30`. The raster bars are `$D021` changes, so a line's text
    /// shows the bar colour of that line. Sprites are the rows of `rows` (raster lines 51..250).
    pub fn render_screen(&self, fb: &mut Framebuffer, regs: &[[u8; 47]], rows: &[SpriteRow]) {
        self.render_lines(fb, regs, rows, &|_| 0, &[(0, 0); 8]);
    }

    /// [`FrameState::render_screen`] with the text of raster line `L` moved right by `shift(L)` pixels beyond its fine
    /// scroll (the total may leave 0..7: pixels left of the first cell show the background) and sprite `s` moved by
    /// `sprite_shift[s]` (x, y): the picture between two frames ([`crate::game::Shot::render_between`]).
    pub fn render_lines(&self, fb: &mut Framebuffer, regs: &[[u8; 47]], rows: &[SpriteRow], shift: &dyn Fn(u16) -> isize, sprite_shift: &[(isize, isize); 8]) {
        let regs_at = |line: u16| regs[line as usize % regs.len()];
        let den = regs_at(0x30)[0x11] & 0x10 != 0;
        let mut fg = vec![false; 320 * 200];
        let mut windows = [(0isize, 0isize); 200];
        for y in 0..272usize {
            let raster = (y + 16) as u16;
            let v = regs_at(raster);
            let border = c64_color(v[0x20]);
            fb.pixels[y * WIDTH..(y + 1) * WIDTH].fill(border);
            let (d011, d016) = (v[0x11], v[0x16]);
            let top = 0x30 + (d011 & 7) as u16;
            let (first, last) = if d011 & 8 != 0 { (51, 251) } else { (55, 247) };
            if !den || raster < first || raster >= last || raster < top {
                continue;
            }
            let ty = (raster - top) as usize;
            if ty >= 200 {
                continue;
            }
            let (row, line) = (ty / 8, ty % 8);
            let xs = (d016 & 7) as isize + shift(raster);
            let (left, right) = if d016 & 8 != 0 { (0isize, 320isize) } else { (7, 311) };
            let py = raster as usize - 51;
            if py < 200 {
                windows[py] = (left, right);
            }
            let multi = d016 & 0x10 != 0;
            let bg = [v[0x21] & 15, v[0x22] & 15, v[0x23] & 15];
            for x in left..right {
                let sx = x - xs;
                let (colr, front) = if sx < 0 {
                    (bg[0], false)
                } else {
                    let (col, p) = (sx as usize / 8, sx as usize % 8);
                    let i = row * 40 + col.min(39);
                    let ch = self.ram[0xC800 + i] as usize;
                    let cc = self.color[i] & 15;
                    let bits = self.ram[0xC000 + ch * 8 + line];
                    if multi && cc & 8 != 0 {
                        let pair = (bits >> (6 - 2 * (p / 2))) & 3;
                        let c = match pair {
                            0 => bg[0],
                            1 => bg[1],
                            2 => bg[2],
                            _ => cc & 7,
                        };
                        (c, pair >= 2)
                    } else {
                        let on = bits >> (7 - p) & 1 != 0;
                        (if on { if multi { cc & 7 } else { cc } } else { bg[0] }, on)
                    }
                };
                fb.pixels[y * WIDTH + 32 + x as usize] = c64_color(colr);
                if py < 200 {
                    fg[py * 320 + x as usize] = front;
                }
            }
        }
        // the sprites, number 7 first so that 0 is in front (from the registers when no rows were recorded)
        let mc = [self.vic[0x25] & 15, self.vic[0x26] & 15];
        let fixed;
        let rows = if rows.is_empty() {
            fixed = self.sprite_rows();
            &fixed[..]
        } else {
            rows
        };
        let mut order: Vec<(&SpriteRow, isize)> = rows
            .iter()
            .map(|r| (r, r.line as isize + sprite_shift[r.sprite as usize & 7].1))
            .filter(|&(_, line)| (51..251).contains(&line))
            .collect();
        order.sort_by_key(|r| std::cmp::Reverse(r.0.sprite));
        for (r, line) in order {
            let dy = line as usize - 51;
            let sx = sprite_shift[r.sprite as usize & 7].0;
            let (left, right) = windows[dy];
            if left == right {
                continue;
            }
            let w = if r.expand_x { 2 } else { 1 };
            let mut put = |px: isize, n: isize, c: Option<u8>| {
                let Some(c) = c else { return };
                for k in 0..n * w {
                    let dx = r.x as isize + sx - 24 + px * w + k;
                    if dx < left || dx >= right || (r.behind && fg[dy * 320 + dx as usize]) {
                        continue;
                    }
                    fb.pixels[(dy + 35) * WIDTH + 32 + dx as usize] = c64_color(c);
                }
            };
            for (b, &byte) in r.bits.iter().enumerate() {
                if r.multicolor {
                    for p in 0..4isize {
                        let c = match (byte >> (6 - 2 * p)) & 3 {
                            0 => None,
                            1 => Some(mc[0]),
                            2 => Some(r.color),
                            _ => Some(mc[1]),
                        };
                        put(b as isize * 8 + p * 2, 2, c);
                    }
                } else {
                    for p in 0..8isize {
                        put(b as isize * 8 + p, 1, (byte >> (7 - p) & 1 != 0).then_some(r.color));
                    }
                }
            }
        }
    }

    /// The registers of each raster line of a frame of play as the band renderer ([`FrameState::render_rows`]) draws
    /// it: the status bar (text lines 0..15: `$D016 = $17`, `$D022/$D023 = $0A/$02`), the cloud band (16..38: its fine
    /// scroll, hi-res when the `$42` IRQ says so) and the playfield (from 39), with the background colours of
    /// [`FrameState::bands`]; for [`FrameState::render_lines`].
    pub fn play_regs(&self) -> Vec<[u8; 47]> {
        let bands = self.bands();
        (0..312u16)
            .map(|raster| {
                let mut v = self.vic;
                let y = raster as isize - 51;
                let b = if y < 16 { bands[0] } else if y < 39 { bands[1] } else { bands[2] };
                v[0x11] = (self.vic[0x11] & 0xE0) | 0x1B;
                v[0x16] = (b.xscroll & 7) | if b.hires { 0 } else { 0x10 };
                v[0x20] = 0;
                v[0x21..0x24].copy_from_slice(&b.bg);
                v
            })
            .collect()
    }

    /// The rows of the sprites as [`FrameState::vic`] places them (no change during the frame): Y (and its expansion),
    /// the shapes at the pointers `$CBF8..`, X with `$D010`, colours, multicolour, X expansion and priority.
    pub fn sprite_rows(&self) -> Vec<SpriteRow> {
        let v = &self.vic;
        let mut out = Vec::new();
        for s in 0..8usize {
            if v[0x15] >> s & 1 == 0 {
                continue;
            }
            let base = 0xC000 + self.ram[0xCBF8 + s] as usize * 64;
            let tall = v[0x17] >> s & 1 != 0;
            let x = v[2 * s] as u16 | ((v[0x10] >> s & 1) as u16) << 8;
            for k in 0..if tall { 42 } else { 21 } {
                let r = if tall { k / 2 } else { k };
                let mut bits = [0u8; 3];
                bits.copy_from_slice(&self.ram[base + r * 3..base + r * 3 + 3]);
                out.push(SpriteRow {
                    line: (v[2 * s + 1] as u16 + 1 + k as u16) % 312,
                    sprite: s as u8,
                    x,
                    bits,
                    color: v[0x27 + s] & 15,
                    multicolor: v[0x1C] >> s & 1 != 0,
                    expand_x: v[0x1D] >> s & 1 != 0,
                    behind: v[0x1B] >> s & 1 != 0,
                });
            }
        }
        out
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
