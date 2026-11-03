//! The video chip of the slice: registers, raster beam, bad lines and a line renderer. The picture is 384 x 272 pixels,
//! raster lines 16..287 of the PAL frame, with the 40 column display window at x = 32.
//!
//! Rendering happens when a raster line is complete (`Vic::end_line`). Colour changes of `$D020` and `$D021` inside a
//! line are placed at the pixel the write cycle corresponds to (raster tricks like rainbow bars); every other register
//! counts with the value it has at the end of the line.

pub const WIDTH: usize = 384;
pub const HEIGHT: usize = 272;
pub const LINES: u16 = 312;
pub const CYCLES: u16 = 63;
/// first raster line of the picture
pub const FIRST_LINE: u16 = 16;
/// Pixel column (in picture pixels) where a write in cycle 1 takes effect, see `Vic::x_of_cycle`.
pub const X_OF_CYCLE1: i32 = -104;

pub struct Vic {
    pub regs: [u8; 64],
    pub line: u16,
    /// cycle in the line, 1..=63 (as in the VIC-II documentation)
    pub cycle: u16,
    pub irq_flags: u8,
    pub frame: u64,
    pub vcbase: u16,
    pub vc: u16,
    pub rc: u8,
    pub display: bool,
    pub den_seen: bool,
    pub badline: bool,
    pub vborder: bool,
    pub matrix: [u8; 40],
    pub colors: [u8; 40],
    /// colour register changes inside the current line: (cycle, register 0x20..=0x2E, value)
    events: Vec<(u16, u8, u8)>,
    line_start: [u8; 64],
    /// the picture: C64 colour indices
    pub pic: Vec<u8>,
    /// set when the last line of the frame was drawn
    pub frame_done: bool,
    collide_ss: u8,
    collide_sb: u8,
    pub sprite_dma: [bool; 8],
}

impl Vic {
    pub fn new() -> Self {
        Vic {
            regs: [0; 64],
            line: 0,
            cycle: 1,
            irq_flags: 0,
            frame: 0,
            vcbase: 0,
            vc: 0,
            rc: 0,
            display: false,
            den_seen: false,
            badline: false,
            vborder: true,
            matrix: [0; 40],
            colors: [0; 40],
            events: Vec::new(),
            line_start: [0; 64],
            pic: vec![0; WIDTH * HEIGHT],
            frame_done: false,
            collide_ss: 0,
            collide_sb: 0,
            sprite_dma: [false; 8],
        }
    }

    pub fn raster_compare(&self) -> u16 {
        self.regs[0x12] as u16 | ((self.regs[0x11] as u16 & 0x80) << 1)
    }

    pub fn irq_line(&self) -> bool {
        self.irq_flags & self.regs[0x1A] & 0x0F != 0
    }

    pub fn read(&mut self, r: u8) -> u8 {
        match r {
            0x11 => (self.regs[0x11] & 0x7F) | ((self.line >> 1) as u8 & 0x80),
            0x12 => self.line as u8,
            0x16 => self.regs[0x16] | 0xC0,
            0x18 => self.regs[0x18] | 0x01,
            0x19 => self.irq_flags | 0x70 | if self.irq_line() { 0x80 } else { 0 },
            0x1A => self.regs[0x1A] | 0xF0,
            0x1E => std::mem::take(&mut self.collide_ss),
            0x1F => std::mem::take(&mut self.collide_sb),
            0x20..=0x2E => self.regs[r as usize] | 0xF0,
            0x2F..=0x3F => 0xFF,
            _ => self.regs[r as usize],
        }
    }

    pub fn write(&mut self, r: u8, v: u8) {
        match r {
            0x19 => self.irq_flags &= !(v & 0x0F),
            0x1E | 0x1F => {}
            0x20..=0x2E => {
                self.regs[r as usize] = v & 0x0F;
                self.events.push((self.cycle, r, v & 0x0F));
            }
            0x2F..=0x3F => {}
            _ => self.regs[r as usize] = v,
        }
    }

    /// Is the current line a bad line (evaluated with the registers as they are now)?
    pub fn is_badline(&self) -> bool {
        let y = self.line;
        (0x30..=0xF7).contains(&y) && self.den_seen && (y & 7) == (self.regs[0x11] as u16 & 7)
    }

    /// Picture pixel at which a colour written in cycle `c` is first visible.
    pub fn x_of_cycle(c: u16) -> i32 {
        c as i32 * 8 + X_OF_CYCLE1 + 0
    }

    /// Called when a new cycle starts. Handles line-start work and the bad line decision. Returns true if a raster IRQ
    /// condition appeared.
    pub fn start_cycle(&mut self) {
        let c = self.cycle;
        if c == 2 && self.line != 0 || c == 3 && self.line == 0 {
            if self.raster_compare() == self.line {
                self.irq_flags |= 1;
            }
        }
        if c == 1 {
            let (top, bottom) = if self.regs[0x11] & 8 != 0 { (51, 251) } else { (55, 247) };
            if self.line == bottom {
                self.vborder = true;
            }
            if self.line == top && self.regs[0x11] & 0x10 != 0 {
                self.vborder = false;
            }
            self.line_start = self.regs;
            self.events.clear();
            if self.line == 0x30 && self.regs[0x11] & 0x10 != 0 {
                self.den_seen = true;
            }
            if self.line == 0 {
                self.den_seen = false;
                self.vcbase = 0;
            }
        }
        if c == 12 {
            if self.line == 0x30 && self.regs[0x11] & 0x10 != 0 {
                self.den_seen = true;
            }
            self.badline = self.is_badline();
        }
        if c == 14 {
            self.vc = self.vcbase;
            if self.badline {
                self.rc = 0;
                self.display = true;
            }
        }
    }

    /// The c-accesses of a bad line (cycles 15..54): reads the video matrix and colour RAM.
    pub fn c_access(&mut self, ram: &[u8], color: &[u8], bank: usize) {
        let vm = bank + ((self.regs[0x18] as usize >> 4) << 10);
        for i in 0..40 {
            let a = vm + ((self.vc as usize + i) & 0x3FF);
            self.matrix[i] = ram[a & 0xFFFF];
            self.colors[i] = color[(self.vc as usize + i) & 0x3FF] & 0x0F;
        }
    }

    /// End of a line: draws it and advances the vertical state. Returns true at the end of a frame.
    pub fn end_line(&mut self, ram: &[u8], color: &[u8], bank: usize) -> bool {
        let line = self.line;
        if (FIRST_LINE..FIRST_LINE + HEIGHT as u16).contains(&line) {
            self.draw_line(ram, color, bank, (line - FIRST_LINE) as usize);
        }
        // cycle 58 logic
        if self.rc == 7 {
            self.display = false;
            self.vcbase = self.vc;
        }
        if self.display {
            self.rc = (self.rc + 1) & 7;
        }
        self.line += 1;
        self.cycle = 1;
        if self.line >= LINES {
            self.line = 0;
            self.frame += 1;
            self.frame_done = true;
            return true;
        }
        false
    }

    /// Colour of register `r` (0x20..=0x2E) at picture pixel `x` of the current line.
    fn reg_at(&self, r: u8, x: i32) -> u8 {
        let mut v = self.line_start[r as usize] & 0x0F;
        for &(c, rr, val) in &self.events {
            if rr == r && x >= Self::x_of_cycle(c) {
                v = val;
                // the 6569 shows light grey for one pixel while a colour register changes ("grey dot")
                if x == Self::x_of_cycle(c) {
                    return 15;
                }
            }
        }
        v
    }

    fn draw_line(&mut self, ram: &[u8], _color: &[u8], bank: usize, y: usize) {
        let regs = self.regs;
        let r11 = regs[0x11];
        let r16 = regs[0x16];
        let ecm = r11 & 0x40 != 0;
        let bmm = r11 & 0x20 != 0;
        let mcm = r16 & 0x10 != 0;
        let xscroll = (r16 & 7) as usize;
        let cols40 = r16 & 8 != 0;
        let row = &mut [0u8; WIDTH]; // colour index, 0x10 = D021, 0x11 = D022, 0x12 = D023, 0x13 = D024
        let fg = &mut [false; WIDTH]; // pixel counts as foreground for sprite priority
        for p in row.iter_mut() {
            *p = 0x10;
        }
        let in_window = !self.vborder;
        // graphics
        if in_window {
            let base_x = 32 + xscroll;
            if self.display || self.badline {
                let cb = bank + (((regs[0x18] as usize >> 1) & 7) << 11);
                let vc = self.vc as usize;
                for i in 0..40 {
                    let ch = self.matrix[i] as usize;
                    let col = self.colors[i];
                    let rc = self.rc as usize;
                    let (byte, a_addr) = if bmm {
                        let bm = bank + (((regs[0x18] as usize >> 3) & 1) << 13);
                        let a = bm + (((vc + i) & 0x3FF) << 3) + rc;
                        (ram[a & 0xFFFF], a)
                    } else {
                        let cha = cb + ((if ecm { ch & 0x3F } else { ch }) << 3) + rc;
                        (ram[cha & 0xFFFF], cha)
                    };
                    let _ = a_addr;
                    let x0 = base_x + i * 8;
                    if bmm {
                        if mcm {
                            for k in 0..4 {
                                let bits = (byte >> (6 - 2 * k)) & 3;
                                let c = match bits {
                                    0 => 0x10,
                                    1 => ch as u8 >> 4,
                                    2 => ch as u8 & 15,
                                    _ => col,
                                };
                                for j in 0..2 {
                                    let x = x0 + k * 2 + j;
                                    if x < WIDTH {
                                        row[x] = c;
                                        fg[x] = bits >= 2;
                                    }
                                }
                            }
                        } else {
                            for k in 0..8 {
                                let bit = (byte >> (7 - k)) & 1;
                                let x = x0 + k;
                                if x < WIDTH {
                                    row[x] = if bit == 1 { ch as u8 >> 4 } else { ch as u8 & 15 };
                                    fg[x] = bit == 1;
                                }
                            }
                        }
                    } else if mcm && col & 8 != 0 {
                        for k in 0..4 {
                            let bits = (byte >> (6 - 2 * k)) & 3;
                            let c = match bits {
                                0 => 0x10,
                                1 => 0x11,
                                2 => 0x12,
                                _ => col & 7,
                            };
                            for j in 0..2 {
                                let x = x0 + k * 2 + j;
                                if x < WIDTH {
                                    row[x] = c;
                                    fg[x] = bits >= 2;
                                }
                            }
                        }
                    } else {
                        for k in 0..8 {
                            let bit = (byte >> (7 - k)) & 1;
                            let x = x0 + k;
                            if x < WIDTH {
                                row[x] = if bit == 1 {
                                    col
                                } else if ecm {
                                    0x10 + (ch >> 6) as u8
                                } else {
                                    0x10
                                };
                                fg[x] = bit == 1;
                            }
                        }
                    }
                }
                self.vc = (self.vc + 40) & 0x3FF;
            } else {
                // idle state: the byte at $3FFF (or $39FF) as black pixels
                let a = bank + if ecm { 0x39FF } else { 0x3FFF };
                let byte = ram[a & 0xFFFF];
                for i in 0..40 * 8 {
                    let bit = if mcm && (bmm || false) { (byte >> (6 - 2 * ((i % 8) / 2))) & 2 != 0 } else { (byte >> (7 - i % 8)) & 1 != 0 };
                    let x = base_x + i;
                    if x < WIDTH {
                        row[x] = if bit { 0 } else { 0x10 };
                        fg[x] = bit;
                    }
                }
            }
            // window clipping: columns outside the 38/40 column window show background
            let (lx, rx) = if cols40 { (32, 352) } else { (39, 345) };
            for x in 0..WIDTH {
                if x < lx || x >= rx {
                    row[x] = 0x10;
                    fg[x] = false;
                }
            }
        }
        // resolve background colour per segment
        let mut out = [0u8; WIDTH];
        for x in 0..WIDTH {
            out[x] = match row[x] {
                0x10 => self.reg_at(0x21, x as i32),
                0x11 => self.reg_at(0x22, x as i32),
                0x12 => self.reg_at(0x23, x as i32),
                0x13 => self.reg_at(0x24, x as i32),
                c => c,
            };
        }
        // sprites
        if in_window || regs[0x15] != 0 {
            self.draw_sprites(ram, bank, &mut out, fg, y);
        }
        // border
        let cols40 = r16 & 8 != 0;
        let (lx, rx) = if cols40 { (32, 352) } else { (39, 345) };
        for x in 0..WIDTH {
            if self.vborder || (x as i32) < lx || (x as i32) >= rx {
                out[x] = self.reg_at(0x20, x as i32);
            }
        }
        let dst = &mut self.pic[y * WIDTH..(y + 1) * WIDTH];
        dst.copy_from_slice(&out);
    }

    fn draw_sprites(&mut self, ram: &[u8], bank: usize, out: &mut [u8; WIDTH], fg: &[bool; WIDTH], _y: usize) {
        let regs = self.regs;
        let line = self.line as i32;
        let vm = bank + ((regs[0x18] as usize >> 4) << 10);
        // per pixel: which sprite drew it (lowest number wins)
        let mut owner = [0xFFu8; WIDTH];
        let mut ss = 0u8;
        let mut sb = 0u8;
        for s in 0..8usize {
            if regs[0x15] & (1 << s) == 0 {
                continue;
            }
            let sy = regs[1 + s * 2] as i32;
            let yexp = regs[0x17] & (1 << s) != 0;
            let h = if yexp { 42 } else { 21 };
            // the sprite starts in the line after the one whose number equals its Y register (the compare is on the low 8 bits)
            let d = (line - sy - 1) & 0xFF;
            if !(0..h).contains(&d) {
                continue;
            }
            let srow = if yexp { d / 2 } else { d } as usize;
            let ptr = ram[(vm + 0x3F8 + s) & 0xFFFF] as usize;
            let da = bank + ptr * 64 + srow * 3;
            let bytes = [ram[da & 0xFFFF], ram[(da + 1) & 0xFFFF], ram[(da + 2) & 0xFFFF]];
            let xexp = regs[0x1D] & (1 << s) != 0;
            let mc = regs[0x1C] & (1 << s) != 0;
            let xpos = regs[s * 2] as i32 | if regs[0x10] & (1 << s) != 0 { 256 } else { 0 };
            // sprite x coordinate 24 is the first pixel of the display window (picture x 32)
            let px0 = xpos - 24 + 32;
            let behind = regs[0x1B] & (1 << s) != 0;
            let col = regs[0x27 + s] & 0x0F;
            let bits: u32 = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | bytes[2] as u32;
            let n = if mc { 12 } else { 24 };
            for k in 0..n {
                let (val, pw) = if mc { ((bits >> (22 - 2 * k)) & 3, 2) } else { ((bits >> (23 - k)) & 1, 1) };
                if val == 0 {
                    continue;
                }
                let c = if mc {
                    match val {
                        1 => regs[0x25] & 0x0F,
                        2 => col,
                        _ => regs[0x26] & 0x0F,
                    }
                } else {
                    col
                };
                let w = pw * if xexp { 2 } else { 1 };
                for j in 0..w {
                    let x = px0 + (k as i32) * w as i32 + j as i32;
                    if !(0..WIDTH as i32).contains(&x) {
                        continue;
                    }
                    let x = x as usize;
                    if fg[x] {
                        sb |= 1 << s;
                    }
                    if owner[x] != 0xFF {
                        ss |= 1 << s | 1 << owner[x];
                        continue;
                    }
                    owner[x] = s as u8;
                    if behind && fg[x] {
                        continue;
                    }
                    out[x] = c;
                }
            }
        }
        self.collide_ss |= ss;
        self.collide_sb |= sb;
    }
}
