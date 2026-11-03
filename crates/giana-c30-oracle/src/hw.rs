//! The memory map and the raster-line VIC of the oracle machine.

use giana_module::cpu::Bus;

/// A write to a VIC register, with the raster position at which it happened (feeds the sprite-hardware work package).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VicWrite {
    pub line: u16,
    /// cycle within the line at the start of the writing instruction (0..62)
    pub cycle: u8,
    /// register number `0..=0x2E`
    pub reg: u8,
    pub val: u8,
}

/// Per sprite DMA state of the VIC-II (Christian Bauer's "The MOS 6567/6569 video controller", section 3.8).
#[derive(Clone, Copy, Default, Debug)]
pub struct SpriteDma {
    pub dma: bool,
    pub display: bool,
    pub mcbase: u8,
    /// MC as loaded at cycle 58 of the previous line: the shape row shown in the current line is `mc / 3`
    pub mc: u8,
    pub exp_ff: bool,
}

/// A recorded RAM write (see [`Hw::watch`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchHit {
    pub line: u16,
    /// address of the writing instruction
    pub pc: u16,
    pub addr: u16,
    pub val: u8,
}

/// How many CPU cycles the VIC takes, and where in the line (tuned against VICE, see the tests).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// cycles lost on a bad line
    pub badline_cost: u32,
    /// cycle of the line at which they are lost
    pub badline_at: u32,
    /// cycles lost per sprite with DMA on in a line
    pub sprite_cost: u32,
    /// extra cycles when any sprite has DMA on in a line (the BA lead-in)
    pub sprite_extra: u32,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing { badline_cost: 43, badline_at: 15, sprite_cost: 2, sprite_extra: 3 }
    }
}

pub const LINES: u16 = 312;
pub const CYCLES: u32 = 63;

/// Everything outside the CPU: RAM, colour RAM, VIC, CIAs, SID, joystick and the raster counter.
#[derive(Clone)]
pub struct Hw {
    pub ram: Box<[u8; 65536]>,
    pub color: [u8; 1024],
    /// VIC register image (what was written; `$D011/$D012/$D019/$D01E/$D01F` are computed on read)
    pub vic: [u8; 64],
    pub cia1: [u8; 16],
    pub cia2: [u8; 16],
    pub sid: [u8; 32],
    /// `$DC00` as read: joystick port 2, active low
    pub joy: u8,
    /// keyboard matrix, one byte per column (`$DC00` bit n low selects column n), a low bit = key down
    pub keys: [u8; 8],
    pub line: u16,
    pub cycle: u32,
    /// completed raster frames since the machine was created
    pub frames: u64,
    pub d019: u8,
    pub d01e: u8,
    pub d01f: u8,
    pub spr: [SpriteDma; 8],
    pub vic_log: Vec<VicWrite>,
    /// reads of `$D01E` as `(line, value the game got)`
    pub coll_reads: Vec<(u16, u8)>,
    /// writes to SID registers as `(line, register, value)`
    pub sid_log: Vec<(u16, u8, u8)>,
    /// log VIC and SID writes and `$D01E` reads (cleared by [`crate::Machine::frame`]); costs a little time
    pub logging: bool,
    /// RAM write watch: inclusive `(lo, hi)` ranges; every write into one is recorded in [`Hw::watch_log`]
    pub watch: Vec<(u16, u16)>,
    pub watch_log: Vec<WatchHit>,
    /// address of the instruction being executed (set by the machine before each step)
    pub cur_pc: u16,
    pub(crate) steal: bool,
    /// The text rows as the VIC fetched them at their bad lines in the current frame (screen codes, colour RAM nibbles):
    /// what is *on screen*, as opposed to what screen RAM holds at some other moment. Row `r` is valid when
    /// `fetched_rows > r`; the counter restarts at raster line 0.
    pub fetched_screen: [[u8; 40]; 25],
    pub fetched_color: [[u8; 40]; 25],
    pub fetched_rows: u8,
    /// VIC cycle stealing parameters (used when stealing is on)
    pub timing: Timing,
    stage: u8,
}

impl Hw {
    pub fn new(ram: &[u8]) -> Hw {
        assert_eq!(ram.len(), 65536);
        let mut r = Box::new([0u8; 65536]);
        r.copy_from_slice(ram);
        Hw {
            ram: r,
            color: [0; 1024],
            vic: [0; 64],
            cia1: [0; 16],
            cia2: [0; 16],
            sid: [0; 32],
            joy: 0xFF,
            keys: [0xFF; 8],
            line: 0,
            cycle: 0,
            frames: 0,
            d019: 0,
            d01e: 0,
            d01f: 0,
            spr: [SpriteDma::default(); 8],
            vic_log: Vec::new(),
            coll_reads: Vec::new(),
            sid_log: Vec::new(),
            logging: true,
            watch: Vec::new(),
            watch_log: Vec::new(),
            cur_pc: 0,
            steal: true,
            fetched_screen: [[0; 40]; 25],
            fetched_color: [[0; 40]; 25],
            fetched_rows: 0,
            timing: Timing::default(),
            stage: 0,
        }
    }

    #[inline]
    pub fn io_visible(&self) -> bool {
        (self.ram[1] & 4) != 0 && (self.ram[1] & 3) != 0
    }

    /// 16 KB bank the VIC sees (CIA 2 port A, inverted).
    pub fn vic_bank(&self) -> usize {
        (!self.cia2[0] & 3) as usize * 0x4000
    }

    /// Address of the sprite pointers (`screen + $3F8`).
    pub fn sprite_pointers(&self) -> usize {
        self.vic_bank() + ((self.vic[0x18] >> 4) as usize) * 0x400 + 0x3F8
    }

    /// Start address of a sprite's 63 shape bytes.
    pub fn sprite_shape(&self, i: usize) -> usize {
        let p = self.ram[self.sprite_pointers() + i] as usize;
        self.vic_bank() + p * 64
    }

    /// Raster compare value of `$D012` + bit 8 of `$D011`.
    pub fn raster_compare(&self) -> u16 {
        self.vic[0x12] as u16 | ((self.vic[0x11] as u16 & 0x80) << 1)
    }

    pub fn irq_pending(&self) -> bool {
        self.d019 & self.vic[0x1A] & 15 != 0
    }

    // ---- raster -----------------------------------------------------------------------------------------------

    /// Advances the raster by `cy` CPU cycles, running the per-line VIC work at the right places.
    pub fn advance(&mut self, cy: u32) {
        self.cycle += cy;
        loop {
            if self.stage == 0 && self.cycle >= self.timing.badline_at {
                self.stage = 1;
                if self.is_badline() {
                    self.fetch_row();
                    if self.steal {
                        self.cycle += self.timing.badline_cost;
                    }
                }
            }
            if self.stage == 1 && self.cycle >= 30 {
                self.stage = 2;
                self.collide_line();
            }
            if self.stage == 2 && self.cycle >= 55 {
                self.stage = 3;
                self.cycle55();
                if self.steal {
                    let n = self.spr.iter().filter(|s| s.dma).count() as u32;
                    if n > 0 {
                        self.cycle += n * self.timing.sprite_cost + self.timing.sprite_extra;
                    }
                }
            }
            if self.stage == 3 && self.cycle >= 58 {
                self.stage = 4;
                self.cycle58();
            }
            if self.cycle >= CYCLES {
                self.cycle -= CYCLES;
                self.line += 1;
                if self.line >= LINES {
                    self.line = 0;
                    self.frames += 1;
                }
                if self.line == 0 {
                    self.fetched_rows = 0;
                }
                self.stage = 0;
                self.begin_line();
                continue;
            }
            break;
        }
    }

    fn begin_line(&mut self) {
        let n = self.line;
        if n == self.raster_compare() {
            self.d019 |= 1;
        }
        // cycles 15/16: MCBASE advances (by 3) while the expansion flip-flop is set; at 63 the DMA ends
        for s in self.spr.iter_mut() {
            if s.exp_ff {
                s.mcbase = s.mcbase.wrapping_add(3);
                if s.mcbase == 63 {
                    s.dma = false;
                }
            }
        }
    }

    /// The c-accesses of a bad line: 40 screen codes and 40 colour nibbles of the next text row.
    fn fetch_row(&mut self) {
        let r = self.fetched_rows as usize;
        if r < 25 {
            let base = self.vic_bank() + ((self.vic[0x18] >> 4) as usize) * 0x400 + r * 40;
            for i in 0..40 {
                self.fetched_screen[r][i] = self.ram[(base + i) & 0xFFFF];
                self.fetched_color[r][i] = self.color[r * 40 + i] & 15;
            }
        }
        self.fetched_rows = self.fetched_rows.saturating_add(1);
    }

    fn is_badline(&self) -> bool {
        let n = self.line;
        (0x30..=0xF7).contains(&n) && self.vic[0x11] & 0x10 != 0 && (n & 7) as u8 == self.vic[0x11] & 7
    }

    /// Cycle 55: expansion flip-flops toggle, sprites whose Y matches the line start their DMA.
    fn cycle55(&mut self) {
        let n = (self.line & 0xFF) as u8;
        for i in 0..8 {
            let ye = self.vic[0x17] & (1 << i) != 0;
            let s = &mut self.spr[i];
            if ye {
                s.exp_ff = !s.exp_ff;
            } else {
                s.exp_ff = true;
            }
            if self.vic[0x15] & (1 << i) != 0 && self.vic[1 + 2 * i] == n && !s.dma {
                s.dma = true;
                s.mcbase = 0;
                if ye {
                    s.exp_ff = false;
                }
            }
        }
    }

    /// Cycle 58: MC := MCBASE; a sprite whose DMA is on and whose Y matches starts displaying; DMA off ends display.
    fn cycle58(&mut self) {
        let n = (self.line & 0xFF) as u8;
        for i in 0..8 {
            let y = self.vic[1 + 2 * i];
            let s = &mut self.spr[i];
            s.mc = s.mcbase;
            if s.dma && y == n {
                s.display = true;
            }
            if !s.dma {
                s.display = false;
            }
        }
    }

    /// The pixels of sprite `i` in the current line as `(x, bits)`: bit `k` of `bits` is the pixel at `x + k`.
    pub fn sprite_line_pixels(&self, i: usize) -> (u16, u64) {
        let s = &self.spr[i];
        let row = (s.mc / 3) as usize;
        let at = self.sprite_shape(i) + row * 3;
        let w = (self.ram[at & 0xFFFF] as u32) << 16 | (self.ram[(at + 1) & 0xFFFF] as u32) << 8 | self.ram[(at + 2) & 0xFFFF] as u32;
        let mc = self.vic[0x1C] & (1 << i) != 0;
        let xe = self.vic[0x1D] & (1 << i) != 0;
        let mut px = 0u64;
        for j in 0..24 {
            let set = if mc {
                (w >> (22 - (j & !1))) & 3 != 0
            } else {
                (w >> (23 - j)) & 1 != 0
            };
            if set {
                if xe {
                    px |= 3 << (2 * j);
                } else {
                    px |= 1 << j;
                }
            }
        }
        let x = self.vic[2 * i] as u16 | if self.vic[0x10] & (1 << i) != 0 { 256 } else { 0 };
        (x, px)
    }

    /// Sprite-sprite collision of the current line, ORed into `$D01E`.
    fn collide_line(&mut self) {
        let mut shown = [(0u16, 0u64); 8];
        let mut mask = 0u8;
        for i in 0..8 {
            if self.spr[i].display {
                shown[i] = self.sprite_line_pixels(i);
                mask |= 1 << i;
            }
        }
        if mask.count_ones() < 2 {
            return;
        }
        let mut hit = 0u8;
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
                    hit |= 1 << a | 1 << b;
                }
            }
        }
        if hit != 0 {
            if self.d01e == 0 {
                self.d019 |= 4;
            }
            self.d01e |= hit;
        }
    }

    // ---- bus --------------------------------------------------------------------------------------------------

    fn io_read(&mut self, a: u16) -> u8 {
        match a {
            0xD000..=0xD3FF => {
                let r = ((a - 0xD000) & 63) as usize;
                match r {
                    0x11 => (self.vic[0x11] & 0x7F) | ((self.line >> 1) as u8 & 0x80),
                    0x12 => self.line as u8,
                    0x19 => self.d019 | 0x70 | if self.irq_pending() { 0x80 } else { 0 },
                    0x1A => self.vic[0x1A] | 0xF0,
                    0x1E => {
                        let v = self.d01e;
                        self.d01e = 0;
                        if self.logging {
                            self.coll_reads.push((self.line, v));
                        }
                        v
                    }
                    0x1F => std::mem::take(&mut self.d01f),
                    0x20..=0x2E => self.vic[r] | 0xF0,
                    0x2F..=0x3F => 0xFF,
                    _ => self.vic[r],
                }
            }
            0xD400..=0xD7FF => 0,
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] | 0xF0,
            0xDC00..=0xDCFF => match a & 15 {
                0 => self.joy,
                1 => {
                    let sel = self.cia1[0];
                    let mut v = 0xFF;
                    for c in 0..8 {
                        if sel & (1 << c) == 0 {
                            v &= self.keys[c];
                        }
                    }
                    v
                }
                r => self.cia1[r as usize],
            },
            0xDD00..=0xDDFF => self.cia2[(a & 15) as usize],
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, a: u16, v: u8) {
        match a {
            0xD000..=0xD3FF => {
                let r = ((a - 0xD000) & 63) as usize;
                if r == 0x19 {
                    self.d019 &= !v;
                    return;
                }
                if r == 0x1E || r == 0x1F {
                    return;
                }
                if r <= 0x2E && self.logging {
                    self.vic_log.push(VicWrite { line: self.line, cycle: self.cycle as u8, reg: r as u8, val: v });
                }
                self.vic[r] = v;
            }
            0xD400..=0xD7FF => {
                let r = ((a - 0xD400) & 31) as usize;
                if self.logging {
                    self.sid_log.push((self.line, r as u8, v));
                }
                self.sid[r] = v;
            }
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] = v & 15,
            0xDC00..=0xDCFF => self.cia1[(a & 15) as usize] = v,
            0xDD00..=0xDDFF => self.cia2[(a & 15) as usize] = v,
            _ => {}
        }
    }
}

impl Bus for Hw {
    #[inline]
    fn read(&mut self, a: u16) -> u8 {
        if a == 0 {
            return 0x2F;
        }
        if (0xD000..0xE000).contains(&a) && self.io_visible() {
            return self.io_read(a);
        }
        self.ram[a as usize]
    }

    #[inline]
    fn write(&mut self, a: u16, v: u8) {
        if a == 0 {
            return;
        }
        if (0xD000..0xE000).contains(&a) && self.io_visible() {
            return self.io_write(a, v);
        }
        if !self.watch.is_empty() && self.watch.iter().any(|&(lo, hi)| (lo..=hi).contains(&a)) {
            self.watch_log.push(WatchHit { line: self.line, pc: self.cur_pc, addr: a, val: v });
        }
        self.ram[a as usize] = v;
    }
}
