//! Sprite hardware and the raster IRQ chain (work package W of `docs/rom-hack-family-c.md`, 7.4): native ports of the
//! preview's sprite register code and of its raster IRQs, a native model of the VIC's sprite unit (the sprite DMA, the
//! sprite-sprite collisions `$D01E` and the sprite rows a raster line shows), each verified against the original code
//! running in the oracle machine (`crates/giana-c30-oracle/tests/sprites.rs`).
//!
//! # The frame (section 1.4)
//!
//! | line | IRQ | ported | what it does for the sprites |
//! |---|---|---|---|
//! | `$41` | `$3488` | [`irq_41`] | next IRQ at `$42`; `CLI` and a NOP slide, so the `$42` IRQ comes at a fixed cycle |
//! | `$42` | `$30A4` | [`irq_42`] | cloud band `$D016`, cycle exact: the delay before the write is the branch offset `$30B2`, which [`object_pointers`] sets from how many object sprites steal cycles on these lines |
//! | `$5A` | `$31F4` | [`irq_5a`] | playfield `$D016`; Giana's sprites ([`giana_sprites`]), or, while the bump sprites are up (`$94`) and Giana is low enough (`y >= $60`), the **split**: an IRQ at line `y - 4` and the frame logic with IRQs on |
//! | `y - 4` | `$32B4` | [`irq_split`] | Giana's sprites below the bump sprites |
//! | `$63` | `$3183` | [`irq_mux`] | the object multiplexer: moves a finished object sprite 21 lines down, once or more |
//! | `$FB` | `$32DC` | [`irq_fb`], [`object_sprites`], [`shimmer`], [`fb_tail`], [`object_pointers`] | status bar registers, `$D015`; with IRQs on: objects (O1), Giana's position (P), the object sprites' registers, the bump sprites ([`bump_sprites`]) or Giana's, the tile shimmer, the object pointers |
//!
//! The functions work on the game's 64 KB memory image and the VIC registers through [`Io`], which also records every
//! write whose **raster position** matters (VIC registers, sprite pointers, the IRQ vector) in program order: a native
//! frame feeds them into its [`SpriteUnit`] at the line where they happen, the oracle test applies them at the instruction
//! where the original makes them.
//!
//! # Collisions
//!
//! The game reads `$D01E` (sprite-sprite) at `$7CB7` (Giana against the objects, every frame), `$7DBE` (objects against
//! each other) and `$4057`; it never reads `$D01F` (sprite-background: the only `$1F $D0` byte pair in the code is an
//! operand at `$2D25`, `LDA $1F`). [`SpriteUnit`] computes `$D01E` per raster line from the sprite shapes in RAM like the
//! oracle's VIC model, which agrees with VICE on every contact of the reference traces.

use crate::layout::C30aLayout;

/// Raster lines of the IRQ chain (immediate operands of the IRQ code that nothing rewrites).
pub const LINE_42: u8 = 0x42;
pub const LINE_5A: u8 = 0x5A;
pub const LINE_FB: u8 = 0xFB;
/// The multiplexer's last IRQ hands over to the `$FB` IRQ at this line (`$31D5`).
pub const LINE_MUX_END: u8 = 0xFC;
/// Giana must be at least this low (sprite y) for the split (`$320C`).
pub const SPLIT_MIN_Y: u8 = 0x60;
/// Lines of the `$42` IRQ an object sprite steals cycles from (sprite y `$2D..$41`, `$343B`/`$343F`).
pub const CLOUD_SPRITE_Y: (u8, u8) = (0x2D, 0x42);

/// The VIC registers and RAM as the sprite code sees them.
pub struct Io<'a> {
    pub ram: &'a mut [u8],
    /// VIC registers `$D000..$D02E` as written (`$D019` writes are acknowledgements and do not change it)
    pub vic: &'a mut [u8],
    /// writes whose raster position matters, in program order: `($D0xx, value)` for VIC registers (with `$D019`),
    /// `($CBF8..$CBFF, value)` for sprite pointers and `($FFFE/$FFFF, value)` for the IRQ vector
    pub log: Vec<(u16, u8)>,
    l: &'a C30aLayout,
}

impl<'a> Io<'a> {
    pub fn new(ram: &'a mut [u8], vic: &'a mut [u8], l: &'a C30aLayout) -> Io<'a> {
        Io { ram, vic, log: Vec::new(), l }
    }

    #[inline]
    pub fn r(&self, a: u16) -> u8 {
        self.ram[a as usize]
    }

    /// A RAM write; sprite pointers and the IRQ vector are logged.
    pub fn w(&mut self, a: u16, v: u8) {
        self.ram[a as usize] = v;
        if is_timed_ram(self.l, a) {
            self.log.push((a, v));
        }
    }

    pub fn vic_r(&self, reg: u8) -> u8 {
        self.vic[reg as usize]
    }

    pub fn vic_w(&mut self, reg: u8, v: u8) {
        if reg != 0x19 {
            self.vic[reg as usize] = v;
        }
        self.log.push((0xD000 + reg as u16, v));
    }

    /// The next raster IRQ: compare line `$D012` and the vector, in the original's order (line first).
    fn next_irq(&mut self, line: u8, vector: u16) {
        self.vic_w(0x12, line);
        self.w(self.l.irq_vector, vector as u8);
        self.w(self.l.irq_vector + 1, (vector >> 8) as u8);
    }
}

/// RAM whose writes take effect at a raster position: the sprite pointers and the IRQ vector.
pub fn is_timed_ram(l: &C30aLayout, a: u16) -> bool {
    (l.sprite_pointers..l.sprite_pointers + 8).contains(&a) || a == l.irq_vector || a == l.irq_vector + 1
}

// ---- the IRQ chain --------------------------------------------------------------------------------------------------

/// `$3488` (line `$41`): acknowledges, next IRQ at line `$42` (`$30A4`). The original then enables IRQs and runs a slide
/// of 17 `NOP`s, in which the `$42` IRQ lands at a fixed cycle; `RTI`.
pub fn irq_41(io: &mut Io) {
    io.vic_w(0x19, 1);
    let v = io.l.irq_42;
    io.next_irq(LINE_42, v);
}

/// `$30A4..$30D7` (line `$42`): the cloud band's `$D016` (`$68 >> 1`; the write comes after a delay of `$30B2` cycles,
/// see [`object_pointers`]), next IRQ at line `$5A` (`$31F4`), `$D022/$D023` from the stage header (`$010A/$010B`).
/// Returns `(A, X, Y)` at `$30D8` (where the block animations follow).
pub fn irq_42(io: &mut Io) -> (u8, u8, u8) {
    let l = io.l;
    let d016 = io.r(l.sky_scroll) >> 1;
    io.vic_w(0x19, 1);
    io.vic_w(0x16, d016);
    io.vic_w(0x12, LINE_5A);
    io.w(l.irq_vector, l.irq_5a as u8);
    io.w(l.irq_vector + 1, (l.irq_5a >> 8) as u8);
    let (c2, c3) = (io.r(l.band_colours), io.r(l.band_colours + 1));
    io.vic_w(0x22, c2);
    io.vic_w(0x23, c3);
    (c3, 1, LINE_5A)
}

/// What the `$5A` IRQ chose for Giana's sprites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// written now (or not at all: no bump sprites, Giana is drawn by the `$FB` IRQ's [`giana_sprites`])
    No,
    /// the split: an IRQ at this line ([`irq_split`]) writes them; the frame logic runs with IRQs on until then
    At(u8),
}

/// `$31F4..$323D` (line `$5A`): the playfield's `$D016` (`$67 | $10`), then Giana's sprites: while the bump sprites are up
/// (`$94`) and Giana is at sprite y `$60` or lower, the split at line `y - 4` ([`irq_split`], with `CLI`: the frame logic
/// that follows runs with IRQs on); with the bump sprites up and Giana higher, [`giana_sprites`] now. Without the split
/// the next IRQ is the multiplexer's ([`mux_next`]) or the one at `$FB`. Returns the choice and `X` at `$323E` (`$61`
/// when the code loaded it; the original keeps the caller's `X` on the split path).
pub fn irq_5a(io: &mut Io, x: u8) -> (Split, u8) {
    let l = io.l;
    let d016 = io.r(l.fine_scroll) | 0x10;
    io.vic_w(0x16, d016);
    io.vic_w(0x19, 1);
    if io.r(l.sprite_split) != 0 {
        let y = io.r(l.player_sprite_y);
        if y >= SPLIT_MIN_Y {
            let line = y - 4;
            let v = l.irq_split;
            io.next_irq(line, v);
            return (Split::At(line), x);
        }
        giana_sprites(io);
    }
    (Split::No, irq_next(io))
}

/// `$3226..$323D` / `$32C0..$32D7`: the multiplexer's IRQ when `$61` is set and its line is above `$F9`, else the `$FB`
/// IRQ. Returns `X` (`$61`).
fn irq_next(io: &mut Io) -> u8 {
    let l = io.l;
    let x = io.r(l.mux_first);
    if x != 0 && mux_next(io) {
        return x;
    }
    let v = l.irq_fb;
    io.next_irq(LINE_FB, v);
    x
}

/// `$31DE`: when `$63 < $F9`, the next IRQ is the multiplexer's at line `$63` (`$3183`); returns `true` then (the
/// original's carry is clear), `false` (carry set) otherwise.
pub fn mux_next(io: &mut Io) -> bool {
    let l = io.l;
    let line = io.r(l.mux_line);
    if line >= 0xF9 {
        return false;
    }
    let v = l.irq_mux;
    io.next_irq(line, v);
    true
}

/// `$32B4..$32D7` (line `y - 4`): Giana's sprites ([`giana_sprites`]), then the multiplexer's IRQ or the one at `$FB`.
/// `RTI` after it (A and X restored).
pub fn irq_split(io: &mut Io) {
    io.vic_w(0x19, 1);
    giana_sprites(io);
    irq_next(io);
}

/// `$3183..$31DD` (line `$63`): the object multiplexer. If a second sprite (register offset `$62`) is about to start at
/// this line, it is held off by 21 lines, or the line moves up 3 to fit it; the sprite at offset `$30` moves 21 lines
/// down, and so does the line; past `$F8` (or on a carry) the next IRQ is the one at `$FB`, at line `$FC`, and `$63` gets
/// its start value `$64` back.
pub fn irq_mux(io: &mut Io) {
    let l = io.l;
    io.vic_w(0x19, 1);
    let y2 = io.r(l.mux_other);
    if y2 != 0 {
        let reg = 1 + y2;
        let line = io.r(l.mux_line);
        let sy = io.vic_r(reg);
        let d = sy.wrapping_sub(line);
        // `$3196`: `BCS` with the carry of `SBC` (no borrow: the sprite is at or below the line)
        let close = if sy >= line {
            d < 3
        } else if d >= 0xEB {
            // `$319C`: `ADC #$14` with the carry of `CMP #$EB` (set): 21 lines further down
            let (v, carry) = sy.overflowing_add(0x15);
            io.vic_w(reg, v);
            !carry && v < 3
        } else {
            false
        };
        if close {
            // `$31AA`: `SBC #$02` with the carry clear: three lines higher
            io.w(l.mux_line, line.wrapping_sub(3));
        }
    }
    let reg = 1 + io.r(l.mux_slot);
    let (v, c) = io.vic_r(reg).overflowing_add(0x15);
    io.vic_w(reg, v);
    let line = io.r(l.mux_line) as u16 + 0x15 + c as u16;
    io.w(l.mux_line, line as u8);
    if line > 0xFF || line as u8 >= 0xF9 {
        let v = l.irq_fb;
        io.w(l.irq_vector, v as u8);
        io.w(l.irq_vector + 1, (v >> 8) as u8);
        let start = io.r(l.mux_line_start);
        io.w(l.mux_line, start);
        io.vic_w(0x12, LINE_MUX_END);
    } else {
        io.vic_w(0x12, line as u8);
    }
}

/// `$32DC..$330F` (line `$FB`): acknowledges, the next frame's first IRQ (line and vector are code operands, `$32E8`,
/// `$32ED`, `$32F2`), the status bar's `$D016`, `$D022`, `$D023` (code operands too), `$D011 <- $D7`, `$D015 <- $7A`.
/// The original then enables IRQs and calls the object code (`$3310`); `A` is `$7A` there.
pub fn irq_fb(io: &mut Io) -> u8 {
    let l = io.l;
    io.vic_w(0x19, 1);
    let (line, lo, hi) = (io.r(l.irq_fb_line_op), io.r(l.irq_fb_vector_lo_op), io.r(l.irq_fb_vector_hi_op));
    io.vic_w(0x12, line);
    io.w(l.irq_vector, lo);
    io.w(l.irq_vector + 1, hi);
    for (reg, op) in [(0x16, l.hud_d016_op), (0x22, l.hud_d022_op), (0x23, l.hud_d023_op)] {
        let v = io.r(op);
        io.vic_w(reg, v);
    }
    let d011 = io.r(l.screen_mode);
    io.vic_w(0x11, d011);
    let e = io.r(l.sprite_enable);
    io.vic_w(0x15, e);
    e
}

// ---- sprite registers -----------------------------------------------------------------------------------------------

/// `$3340..$3383` (in the `$FB` IRQ, after Giana's position `$3313`): positions of the object sprites 3..7 (`$73..$77`,
/// `$7B..$7F`; their x high bits are in `$88`, written with Giana's), `$D015 <- $7A`; then the bump sprites
/// ([`bump_sprites`]) while `$94` is set, else Giana's sprites ([`giana_sprites`]).
pub fn object_sprites(io: &mut Io) {
    let l = io.l;
    for n in 0..5u16 {
        let (x, y) = (io.r(l.object_sprite_x + n), io.r(l.object_sprite_y + n));
        io.vic_w(6 + 2 * n as u8, x);
        io.vic_w(7 + 2 * n as u8, y);
    }
    let e = io.r(l.sprite_enable);
    io.vic_w(0x15, e);
    if io.r(l.sprite_split) != 0 {
        bump_sprites(io);
    } else {
        giana_sprites(io);
    }
}

/// `$353C`: Giana's three sprites: y `$79` (sprite 0), `+ $3543` (sprite 2) and that `+ $3548` (sprite 1; the second add
/// keeps the first one's carry), the x high bits `$72` (`$D010`, all sprites), x `$71`, pointers `$80..$82`, colours
/// `$DC`/`$DD` (sprites 0 and 2) and, when `$1D` is 0, the multicolour bit of sprite 2.
pub fn giana_sprites(io: &mut Io) {
    let l = io.l;
    let y = io.r(l.player_sprite_y);
    io.vic_w(0x01, y);
    let (y2, c) = y.overflowing_add(io.r(l.layer_offset_ops));
    io.vic_w(0x05, y2);
    let y1 = y2.wrapping_add(io.r(l.layer_offset_ops + 5)).wrapping_add(c as u8);
    io.vic_w(0x03, y1);
    let msb = io.r(l.player_sprite_msb);
    io.vic_w(0x10, msb);
    let x = io.r(l.player_sprite_x);
    for reg in [0x00, 0x02, 0x04] {
        io.vic_w(reg, x);
    }
    for i in 0..3u16 {
        let p = io.r(l.player_frames + i);
        io.w(l.sprite_pointers + i, p);
    }
    let (c0, c2) = (io.r(l.layer0_colour), io.r(l.sprite_color));
    io.vic_w(0x27, c0);
    io.vic_w(0x29, c2);
    if io.r(l.layer_hires) == 0 {
        let mc = io.vic_r(0x1C) | 4;
        io.vic_w(0x1C, mc);
    }
}

/// `$34B1`: sprites 0..2 as the bump sprites (what a bumped block throws up, `$3DDB`): sprite 0's y is set to 0 first;
/// each bump sprite with frames left (`$91..$93`) rises along the arc `$3582` from its block (`$8C..$8E`, x `$89..$8B`)
/// and ends when it would reach Giana's y - 29 (`$F5`). While one is up: x high bits `$88 & $F8 | $90`, pointer `$AB`,
/// white, hi-res. When none was up (and Giana's `$78 >= $1D`, the carry of the first subtraction), `$3538` clears `$94`
/// and the sprites are Giana's again ([`giana_sprites`]).
pub fn bump_sprites(io: &mut Io) {
    let l = io.l;
    io.vic_w(0x01, 0);
    let (limit, mut carry) = sbc(io.r(l.player_y), 0x1D, true);
    io.w(l.bump_limit, limit);
    for n in 0..3u16 {
        let mut left = io.r(l.bump_timer + n);
        if left == 0 {
            continue;
        }
        left -= 1;
        let (y, _) = sbc(io.r(l.bump_y + n), io.r(l.bump_arc + left as u16), true);
        io.vic_w(1 + 2 * n as u8, y);
        if y >= limit {
            left = 0;
        }
        // `CMP $F5` clears the carry, or `CLC` does
        carry = false;
        let x = io.r(l.bump_x + n);
        io.vic_w(2 * n as u8, x);
        io.w(l.bump_timer + n, left);
    }
    if carry {
        io.w(l.sprite_split, 0);
        giana_sprites(io);
        return;
    }
    let msb = io.r(l.sprite_x_msb) & 0xF8 | io.r(l.bump_msb);
    io.vic_w(0x10, msb);
    for i in 0..3u16 {
        io.w(l.sprite_pointers + i, BUMP_POINTER);
    }
    io.vic_w(0x27, 1);
    io.vic_w(0x29, 1);
    let mc = io.vic_r(0x1C) & 0xF8;
    io.vic_w(0x1C, mc);
}

/// Shape pointer of the bump sprites (`$351C`).
pub const BUMP_POINTER: u8 = 0xAB;

/// 6502 `SBC` in binary mode: result and carry (= no borrow).
fn sbc(a: u8, m: u8, c: bool) -> (u8, bool) {
    let r = a as i16 - m as i16 - (!c) as i16;
    (r as u8, r >= 0)
}

// ---- the rest of the `$FB` IRQ --------------------------------------------------------------------------------------

/// `$3387..$3415`: the tile shimmer. `$D8` bit 7 toggles each frame; on the frames it is set, the masks' operands
/// (`$33B8`, `$33C1`) move on by one, else the frame counter in bits 0..6 counts 1..9 (wrapping to 1 resets the masks to
/// `$182D`/`$1845`). Then 16 characters bytes at `$C4E0` are the source bytes `$180D`/`$181D` masked by the moving
/// operands, from the frame counter on; and three of the six entries of `$3470..$3487` (5..3 or 2..0, `$346F`) flip
/// bits in a character row at `$9E/$9F` and move their row up or down by one.
pub fn shimmer(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let phase = r(ram, l.shimmer_phase) ^ 0x80;
    ram[l.shimmer_phase as usize] = phase;
    let (op1, op2) = (l.shimmer_op1 as usize, l.shimmer_op2 as usize);
    let start = if phase & 0x80 != 0 {
        ram[op1] = ram[op1].wrapping_add(1);
        ram[op2] = ram[op2].wrapping_add(1);
        phase & 0x7F
    } else if phase >= 9 {
        ram[op1] = 0x2D;
        ram[op2] = 0x45;
        ram[l.shimmer_phase as usize] = 2;
        1
    } else {
        ram[l.shimmer_phase as usize] = phase + 1;
        phase
    };
    let word = |ram: &[u8], a: usize| u16::from_le_bytes([ram[a], ram[a + 1]]);
    let (m1, m2) = (word(ram, op1), word(ram, op2));
    let mut x = start;
    for y in (0..8u16).rev() {
        let at = |base: u16| base.wrapping_add(x as u16) as usize;
        ram[(l.shimmer_chars + y) as usize] = ram[at(l.shimmer_source)] & ram[at(m1)];
        ram[(l.shimmer_chars + 8 + y) as usize] = ram[at(l.shimmer_source + 0x10)] & ram[at(m2)];
        x = x.wrapping_add(1);
    }
    let mut x = r(ram, l.shimmer_cursor);
    loop {
        let xi = x as u16;
        ram[l.shimmer_ptr as usize] = r(ram, l.shimmer_lo + xi);
        let base = u16::from_le_bytes([ram[l.shimmer_ptr as usize], ram[l.shimmer_ptr as usize + 1]]);
        let state = r(ram, l.shimmer_state + xi);
        let mut y = state & 0x7F;
        if y != 8 {
            let a = base.wrapping_add(y as u16) as usize;
            ram[a] ^= r(ram, l.shimmer_lo + 12 + xi);
        }
        y = y.wrapping_sub(1);
        let a = base.wrapping_add(y as u16) as usize;
        ram[a] ^= r(ram, l.shimmer_lo + 6 + xi);
        let s = (l.shimmer_state + xi) as usize;
        let turn = if state & 0x80 != 0 {
            y = y.wrapping_sub(1);
            y & 0x80 != 0
        } else {
            y == 7
        };
        if turn {
            ram[s] = state ^ 0x80;
        } else if state & 0x80 != 0 {
            ram[s] = state.wrapping_sub(1);
        } else {
            ram[s] = state.wrapping_add(1);
        }
        if x == 0 {
            x = 5;
            break;
        }
        x -= 1;
        if x == 2 {
            break;
        }
    }
    ram[l.shimmer_cursor as usize] = x;
}

/// Where the `$FB` IRQ goes on after the shimmer (`$3416..$3422`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FbTail {
    /// automatic scroll (`$3705` negative): straight to the tile animation (`$3453`)
    TileAnim,
    /// a column step is running (`$14`): straight to the music (`$3456`)
    Music,
    /// `JSR $5C35` with `Y` = this sound effect (`$D5`), then `$D5 = $FF` and [`object_pointers`]
    Effect(u8),
    /// [`object_pointers`] (`$342A`, `Y` = `$D5`)
    Pointers(u8),
}

/// `$3416..$3422`: see [`FbTail`].
pub fn fb_tail(ram: &[u8], l: &C30aLayout) -> FbTail {
    if ram[l.auto_scroll as usize] & 0x80 != 0 {
        FbTail::TileAnim
    } else if ram[l.scroll_step as usize] != 0 {
        FbTail::Music
    } else {
        let e = ram[l.effect as usize];
        if e & 0x80 == 0 {
            FbTail::Effect(e)
        } else {
            FbTail::Pointers(e)
        }
    }
}

/// `$342A..$3452`: the object sprites' pointers (`$83..$87` to `$CBFB..$CBFF`, sprite 7 first), then the delay of the next
/// `$42` IRQ: which of the object sprites 4..7 are on and at y `$2D..$41` (their DMA steals cycles on the IRQ's lines) is
/// a 4-bit index into `$345F`, the branch offset written to `$30B2`. Returns `(A, X)` at `$3453`.
pub fn object_pointers(io: &mut Io) -> (u8, u8) {
    let l = io.l;
    for n in (0..5u16).rev() {
        let p = io.r(l.object_frames + n);
        io.w(l.sprite_pointers + 3 + n, p);
    }
    let mut mask = 0u8;
    for n in 1..5u16 {
        let y = io.r(l.object_sprite_y + n);
        mask >>= 1;
        if (CLOUD_SPRITE_Y.0..CLOUD_SPRITE_Y.1).contains(&y) {
            mask |= 0x80;
        }
    }
    let x = (mask & io.r(l.sprite_enable)) >> 4;
    let a = io.r(l.cloud_delays + x as u16);
    io.w(l.cloud_delay_op, a);
    (a, x)
}

// ---- the VIC's sprite unit ------------------------------------------------------------------------------------------

/// DMA state of one sprite (Christian Bauer's "The MOS 6567/6569 video controller", 3.8, as the oracle models it).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SpriteDma {
    pub dma: bool,
    pub display: bool,
    pub mcbase: u8,
    /// MC as loaded at cycle 58 of the previous line: the shape row shown in the current line is `mc / 3`
    pub mc: u8,
    pub exp_ff: bool,
}

/// One sprite row as a raster line shows it (taken at cycle 30 of the line, where the collisions are computed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteRow {
    pub line: u16,
    pub sprite: u8,
    /// x position (9 bits)
    pub x: u16,
    /// the three shape bytes of the row
    pub bits: [u8; 3],
    pub color: u8,
    pub multicolor: bool,
    pub expand_x: bool,
    /// behind the characters' foreground (`$D01B`)
    pub behind: bool,
}

/// Raster lines and cycles of the PAL C64.
pub const LINES: u16 = 312;
pub const CYCLES: u8 = 63;

/// The sprite unit of the VIC-II: sprite DMA, display, the rows each line shows and the sprite-sprite collisions
/// (`$D01E`), driven by the raster position. Feed it the frame's register writes in time order: `run_to(line, cycle)`
/// first (everything of the raster up to that position happens with the registers as they are), then the write. The
/// per-line steps are the ones of the oracle's VIC model: MCBASE at the start of the line, the collisions at cycle 30,
/// Y compare and DMA start at 55, display start at 58.
#[derive(Clone, Debug)]
pub struct SpriteUnit {
    pub spr: [SpriteDma; 8],
    /// collision latch (cleared by [`SpriteUnit::read_collisions`])
    pub d01e: u8,
    pub line: u16,
    /// what of the current line is done: 0 line start, 1 collisions, 2 cycle 55, 3 cycle 58
    stage: u8,
    /// rows shown, when `record` is on
    pub rows: Vec<SpriteRow>,
    pub record: bool,
    /// the 16 KB bank the VIC sees and the address of the sprite pointers (`screen + $3F8`)
    pub bank: usize,
    pub pointers: usize,
}

impl SpriteUnit {
    /// A unit at `line`, before any of its steps, with the given DMA state.
    pub fn new(spr: [SpriteDma; 8], line: u16, cycle: u8, l: &C30aLayout) -> SpriteUnit {
        let stage = match cycle {
            0..=29 => 0,
            30..=54 => 1,
            55..=57 => 2,
            _ => 3,
        };
        SpriteUnit {
            spr,
            d01e: 0,
            line,
            stage,
            rows: Vec::new(),
            record: false,
            bank: (l.sprite_pointers & 0xC000) as usize,
            pointers: l.sprite_pointers as usize,
        }
    }

    /// Runs the raster to `(line, cycle)`: every step of the lines in between whose cycle is at or before `cycle` on the
    /// last one. A `line` below the current one is in the next frame.
    pub fn run_to(&mut self, line: u16, cycle: u8, vic: &[u8], ram: &[u8]) {
        while self.line != line {
            self.finish_line(vic, ram);
            self.line = (self.line + 1) % LINES;
            self.begin_line();
        }
        if cycle >= 30 && self.stage < 1 {
            self.stage = 1;
            self.collide_line(vic, ram);
        }
        if cycle >= 55 && self.stage < 2 {
            self.stage = 2;
            self.cycle55(vic);
        }
        if cycle >= 58 && self.stage < 3 {
            self.stage = 3;
            self.cycle58(vic);
        }
    }

    /// `$D01E` read: the latched collisions, then the latch is clear.
    pub fn read_collisions(&mut self) -> u8 {
        std::mem::take(&mut self.d01e)
    }

    fn finish_line(&mut self, vic: &[u8], ram: &[u8]) {
        if self.stage < 1 {
            self.collide_line(vic, ram);
        }
        if self.stage < 2 {
            self.cycle55(vic);
        }
        if self.stage < 3 {
            self.cycle58(vic);
        }
    }

    fn begin_line(&mut self) {
        self.stage = 0;
        for s in self.spr.iter_mut() {
            if s.exp_ff {
                s.mcbase = s.mcbase.wrapping_add(3);
                if s.mcbase == 63 {
                    s.dma = false;
                }
            }
        }
    }

    fn cycle55(&mut self, vic: &[u8]) {
        let n = (self.line & 0xFF) as u8;
        for i in 0..8 {
            let ye = vic[0x17] & (1 << i) != 0;
            let s = &mut self.spr[i];
            s.exp_ff = if ye { !s.exp_ff } else { true };
            if vic[0x15] & (1 << i) != 0 && vic[1 + 2 * i] == n && !s.dma {
                s.dma = true;
                s.mcbase = 0;
                if ye {
                    s.exp_ff = false;
                }
            }
        }
    }

    fn cycle58(&mut self, vic: &[u8]) {
        let n = (self.line & 0xFF) as u8;
        for i in 0..8 {
            let s = &mut self.spr[i];
            s.mc = s.mcbase;
            if s.dma && vic[1 + 2 * i] == n {
                s.display = true;
            }
            if !s.dma {
                s.display = false;
            }
        }
    }

    /// The row sprite `i` shows in the current line.
    pub fn row(&self, i: usize, vic: &[u8], ram: &[u8]) -> SpriteRow {
        let p = ram[self.pointers + i] as usize;
        let at = self.bank + p * 64 + (self.spr[i].mc / 3) as usize * 3;
        let b = |k: usize| ram[(at + k) & 0xFFFF];
        SpriteRow {
            line: self.line,
            sprite: i as u8,
            x: vic[2 * i] as u16 | if vic[0x10] & (1 << i) != 0 { 256 } else { 0 },
            bits: [b(0), b(1), b(2)],
            color: vic[0x27 + i] & 15,
            multicolor: vic[0x1C] & (1 << i) != 0,
            expand_x: vic[0x1D] & (1 << i) != 0,
            behind: vic[0x1B] & (1 << i) != 0,
        }
    }

    /// Cycle 30: the sprite-sprite collisions of the line (and the rows it shows, when recording).
    fn collide_line(&mut self, vic: &[u8], ram: &[u8]) {
        let mut shown = [(0u16, 0u64); 8];
        let mut mask = 0u8;
        for i in 0..8 {
            if self.spr[i].display {
                let row = self.row(i, vic, ram);
                shown[i] = (row.x, row_pixels(&row));
                mask |= 1 << i;
                if self.record {
                    self.rows.push(row);
                }
            }
        }
        if mask.count_ones() < 2 {
            return;
        }
        for a in 0..8 {
            for b in a + 1..8 {
                if mask & (1 << a) == 0 || mask & (1 << b) == 0 {
                    continue;
                }
                let ((xa, pa), (xb, pb)) = (shown[a], shown[b]);
                let overlap = if xa <= xb {
                    let d = (xb - xa) as u32;
                    d < 48 && (pa >> d) & pb != 0
                } else {
                    let d = (xa - xb) as u32;
                    d < 48 && (pb >> d) & pa != 0
                };
                if overlap {
                    self.d01e |= 1 << a | 1 << b;
                }
            }
        }
    }
}

/// The opaque pixels of a row: bit `k` is the pixel at `x + k` (multicolour pairs count where either bit is set; an
/// expanded sprite is 48 pixels wide).
pub fn row_pixels(row: &SpriteRow) -> u64 {
    let w = (row.bits[0] as u32) << 16 | (row.bits[1] as u32) << 8 | row.bits[2] as u32;
    let mut px = 0u64;
    for j in 0..24 {
        let set = if row.multicolor { (w >> (22 - (j & !1))) & 3 != 0 } else { (w >> (23 - j)) & 1 != 0 };
        if set {
            if row.expand_x {
                px |= 3 << (2 * j);
            } else {
                px |= 1 << j;
            }
        }
    }
    px
}
