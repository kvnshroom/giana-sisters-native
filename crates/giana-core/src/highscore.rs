//! The high score screen of the original, native: game over -> table with the colour bars -> name entry -> F1 saves.
//!
//! The routines are `$325D..$3611` of the game program (the screen, the comparison with the tables, the insertion of a
//! new record, the name entry, the save, the colour bar interrupt), `$9F45..$9FAD` (clear the screen, read the joystick)
//! and the sound driver `$41C0..$4A79` (the high score tune, played from the same interrupt). `highscore_code.rs`
//! holds them as a static translation that spends the bus cycles of every instruction (`tools/recompile_timed.py`),
//! the way `sound_driver.rs` holds the sound driver. This file is the runtime around it:
//!
//! * a beam (raster line and cycle of the 6569) that advances with every cycle, bad lines that take the bus from the
//!   CPU, the raster interrupt, the video registers, and the raster lines handed to `draw` as `title::LineState`
//!   (`giana_render::highscore` turns them into pixels). The colour bars come out of when the interrupt routine's
//!   writes to `$D021` land, not out of a table of positions;
//! * what the KERNAL did for the program: the interrupt entry and exit (`$EA31`, with the keyboard scan), `GETIN`,
//!   `IOINIT`, and the `SAVE` of the table (as a returned `.prg`, there is no disk);
//! * the keyboard matrix and joystick, held for a whole frame.
//!
//! The entry is `Hs::enter` (the game over code reaches `$325D`), the exit is `Hs::finished` (the routine's `RTS` to
//! `$0C64`). The reference of every frame is `giana-authentic` (the slice that runs the original code).

use crate::title::LineState;

pub const LINES: u16 = 312;
pub const CYCLES: u16 = 63;
/// first raster line of the picture
pub const FIRST_LINE: u16 = 16;
pub const HEIGHT: usize = 272;

/// Where the game over code returns to when the high score routine is done (`JSR $325D` at `$0C61`).
pub const RETURN_PC: u16 = 0x0C64;

/// What the video chip knows when the high score routine starts (right after its first instruction, `SEI` at `$325D`).
#[derive(Clone)]
pub struct VicState {
    pub regs: [u8; 64],
    pub irq_flags: u8,
    /// raster line and cycle of the beam
    pub line: u16,
    pub cycle: u16,
    pub vcbase: u16,
    pub vc: u16,
    pub rc: u8,
    pub display: bool,
    pub den_seen: bool,
    pub badline: bool,
    pub vborder: bool,
    pub matrix: [u8; 40],
    pub colors: [u8; 40],
    /// the interrupt line of the last cycles (bit 0 now, bit 1 the cycle before)
    pub irq_hist: u8,
}

impl VicState {
    /// The state at the top of a frame (line 0), as the game leaves its screen: `regs` are the video registers.
    pub fn at_frame_start(regs: [u8; 64]) -> VicState {
        VicState { regs, irq_flags: 0, line: 0, cycle: 1, vcbase: 0, vc: 0, rc: 7, display: false, den_seen: false, badline: false, vborder: true, matrix: [0; 40], colors: [0; 40], irq_hist: 0 }
    }
}

/// The keys and the joystick for one frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    /// joystick port 2: bit 0 up, 1 down, 2 left, 3 right, 4 fire
    pub joy: u8,
    /// keyboard matrix: bit `row` of byte `col` is set while the key is down
    pub keys: [u8; 8],
}

pub struct Hs {
    // the processor
    pub(crate) a: u8,
    pub(crate) x: u8,
    pub(crate) y: u8,
    pub(crate) sp: u8,
    pub(crate) pc: u16,
    pub(crate) c: bool,
    pub(crate) z: bool,
    pub(crate) n: bool,
    pub(crate) v: bool,
    pub(crate) i: bool,
    // memory
    pub ram: Vec<u8>,
    pub color: Vec<u8>,
    pub sid: [u8; 32],
    // the video chip
    regs: [u8; 64],
    pub line: u16,
    pub cycle: u16,
    irq_flags: u8,
    vcbase: u16,
    vc: u16,
    rc: u8,
    display: bool,
    den_seen: bool,
    badline: bool,
    vborder: bool,
    matrix: [u8; 40],
    colors: [u8; 40],
    irq_hist: u8,
    events: Vec<(u16, u8)>,
    line_start_d021: u8,
    frame_done: bool,
    pub frames: u64,
    pub clock: u64,
    // the CIAs and the outside
    cia1: [u8; 16],
    cia2: [u8; 16],
    input: Input,
    kbd_prev: u8,
    cmd: Vec<u8>,
    name_ptr: (u16, u8),
    saves: Vec<Vec<u8>>,
    done: bool,
    error: Option<String>,
    /// when set, every write to `$D021` is recorded: (raster line, cycle, value)
    pub d021_log: Option<Vec<(u16, u16, u8)>>,
    draw: Box<dyn FnMut(usize, &LineState)>,
}

impl Hs {
    /// Starts the high score routine at its second instruction (`SEI` has run). `ram` is the 64K of the game program as it is
    /// when the game is over (screen, character set, sound driver variables, the high score tables), `color` the colour RAM.
    /// `draw` gets every raster line of the picture (raster lines 16..287 as `y` = 0..271) when the line is complete.
    pub fn enter(ram: Vec<u8>, color: Vec<u8>, vic: VicState, cia2_0: u8, draw: Box<dyn FnMut(usize, &LineState)>) -> Hs {
        assert_eq!(ram.len(), 65536);
        let mut color = color;
        color.resize(1024, 0);
        let mut cia1 = [0u8; 16];
        cia1[2] = 0xFF;
        let mut cia2 = [0u8; 16];
        cia2[0] = cia2_0;
        cia2[2] = 0x3F;
        let mut h = Hs {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xF0,
            pc: 0x325E,
            c: false,
            z: false,
            n: false,
            v: false,
            i: true,
            ram,
            color,
            sid: [0; 32],
            regs: vic.regs,
            line: vic.line,
            cycle: vic.cycle,
            irq_flags: vic.irq_flags,
            vcbase: vic.vcbase,
            vc: vic.vc,
            rc: vic.rc,
            display: vic.display,
            den_seen: vic.den_seen,
            badline: vic.badline,
            vborder: vic.vborder,
            matrix: vic.matrix,
            colors: vic.colors,
            irq_hist: vic.irq_hist,
            events: Vec::new(),
            line_start_d021: vic.regs[0x21] & 15,
            frame_done: false,
            frames: 0,
            clock: 0,
            cia1,
            cia2,
            input: Input::default(),
            kbd_prev: 64,
            cmd: Vec::new(),
            name_ptr: (0, 0),
            saves: Vec::new(),
            done: false,
            error: None,
            d021_log: None,
            draw,
        };
        // the return address of `JSR $325D` at `$0C61`
        h.ram[0x1F1] = (RETURN_PC - 1) as u8;
        h.ram[0x1F2] = ((RETURN_PC - 1) >> 8) as u8;
        h.sp = 0xF0;
        h
    }

    // ---- the program is done / failed ---------------------------------------------------------------------------

    /// The routine has returned to the game over code (`RTS` of `$325D` to `$0C64`): the screen is over.
    pub fn finished(&self) -> bool {
        self.done
    }

    /// What the program did that this model cannot do (an address outside the translated code, an unknown KERNAL entry).
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// SID registers `$D400..$D418` now.
    pub fn sid_regs(&self) -> [u8; 25] {
        let mut r = [0u8; 25];
        r.copy_from_slice(&self.sid[..25]);
        r
    }

    /// The `.prg` files the program saved (`SAVE "ATG"`, F1) since the last call.
    pub fn take_saves(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.saves)
    }

    /// The program waits at "PRESS F1 TO SAVE HIGHSCORES" (the loop at `$33EA`, which spends most of its time in the joystick
    /// routine `$9F77` it calls).
    pub fn waiting_for_save_key(&self) -> bool {
        if (0x33EA..0x3400).contains(&self.pc) {
            return true;
        }
        let sp = self.sp as usize;
        (0x9F77..0x9FAE).contains(&self.pc) && sp < 0xFD && self.ram[0x101 + sp] == 0xEC && self.ram[0x102 + sp] == 0x33
    }

    /// Joystick port 2 and the keyboard for the next frame.
    pub fn set_input(&mut self, input: Input) {
        self.input = input;
    }

    fn vic_bank(&self) -> usize {
        ((!self.cia2[0]) & 3) as usize * 0x4000
    }

    // ---- the video chip ------------------------------------------------------------------------------------------

    fn raster_compare(&self) -> u16 {
        self.regs[0x12] as u16 | ((self.regs[0x11] as u16 & 0x80) << 1)
    }

    fn irq_line(&self) -> bool {
        self.irq_flags & self.regs[0x1A] & 0x0F != 0
    }

    fn is_badline(&self) -> bool {
        let y = self.line;
        (0x30..=0xF7).contains(&y) && self.den_seen && (y & 7) == (self.regs[0x11] as u16 & 7)
    }

    fn start_cycle(&mut self) {
        let c = self.cycle;
        if (c == 2 && self.line != 0) || (c == 3 && self.line == 0) {
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
            self.line_start_d021 = self.regs[0x21] & 15;
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

    fn end_line(&mut self) {
        let line = self.line;
        if (FIRST_LINE..FIRST_LINE + HEIGHT as u16).contains(&line) {
            let fetch = self.display || self.badline;
            let in_window = !self.vborder;
            let bank = self.vic_bank();
            let st = LineState {
                d011: self.regs[0x11],
                d016: self.regs[0x16],
                d018: self.regs[0x18],
                border: self.regs[0x20] & 15,
                vborder: self.vborder,
                fetch,
                rc: self.rc,
                vc: self.vc,
                matrix: &self.matrix,
                colors: &self.colors,
                d021_start: self.line_start_d021,
                d021_events: &self.events,
                ram: &self.ram[bank..bank + 0x4000],
            };
            (self.draw)((line - FIRST_LINE) as usize, &st);
            if in_window && fetch {
                self.vc = (self.vc + 40) & 0x3FF;
            }
        }
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
            self.frames += 1;
            self.frame_done = true;
        }
    }

    fn tick(&mut self) {
        self.clock += 1;
        self.cycle += 1;
        if self.cycle > CYCLES {
            self.end_line();
        }
        self.start_cycle();
        self.irq_hist = self.irq_hist << 1 | self.irq_line() as u8;
        if self.cycle == 15 && self.badline {
            let vm = self.vic_bank() + ((self.regs[0x18] as usize >> 4) << 10);
            for i in 0..40 {
                let a = vm + ((self.vc as usize + i) & 0x3FF);
                self.matrix[i] = self.ram[a & 0xFFFF];
                self.colors[i] = self.color[(self.vc as usize + i) & 0x3FF] & 0x0F;
            }
        }
    }

    /// A bad line takes the bus from the CPU: no reads in cycles 12..=54, no writes in 15..=54.
    fn stall(&mut self, write: bool) {
        if self.badline {
            let lo = if write { 15 } else { 12 };
            if (lo..=54).contains(&self.cycle) {
                while self.cycle <= 54 {
                    self.tick();
                }
            }
        }
    }

    // ---- bus cycles of the program --------------------------------------------------------------------------------

    fn port(&self) -> u8 {
        (self.ram[1] & self.ram[0]) | !self.ram[0]
    }

    fn kernal_visible(&self) -> bool {
        self.port() & 2 != 0
    }

    fn io_visible(&self) -> bool {
        let p = self.port();
        p & 3 != 0 && p & 4 != 0
    }

    /// A cycle that reads and does not use what it reads.
    #[inline]
    pub(crate) fn r(&mut self) {
        self.stall(false);
        self.tick();
    }

    /// A cycle that reads.
    pub(crate) fn rd(&mut self, a: u16) -> u8 {
        self.stall(false);
        let v = if (0xD000..0xE000).contains(&a) && self.io_visible() {
            self.io_read(a)
        } else if a == 1 {
            self.port()
        } else {
            self.ram[a as usize]
        };
        self.tick();
        v
    }

    /// A cycle that writes.
    pub(crate) fn wr(&mut self, a: u16, v: u8) {
        self.stall(true);
        if (0xD000..0xE000).contains(&a) && self.io_visible() {
            self.io_write(a, v);
        } else {
            self.ram[a as usize] = v;
        }
        self.tick();
    }

    fn io_read(&mut self, a: u16) -> u8 {
        match a {
            0xD000..=0xD3FF => {
                let r = (a & 0x3F) as usize;
                match r {
                    0x11 => (self.regs[0x11] & 0x7F) | ((self.line >> 1) as u8 & 0x80),
                    0x12 => self.line as u8,
                    0x16 => self.regs[0x16] | 0xC0,
                    0x18 => self.regs[0x18] | 0x01,
                    0x19 => self.irq_flags | 0x70 | if self.irq_line() { 0x80 } else { 0 },
                    0x1A => self.regs[0x1A] | 0xF0,
                    0x1E | 0x1F => 0,
                    0x20..=0x2E => self.regs[r] | 0xF0,
                    0x2F..=0x3F => 0xFF,
                    _ => self.regs[r],
                }
            }
            0xD400..=0xD7FF => match a & 0x1F {
                0x1B | 0x1C => 0,
                r => self.sid[r as usize],
            },
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] | 0xF0,
            0xDC00..=0xDCFF => match a & 0xF {
                0 => (self.cia1[0] | !self.cia1[2]) & !(self.input.joy & 0x1F),
                0xD => 0,
                r => self.cia1[r as usize],
            },
            0xDD00..=0xDDFF => match a & 0xF {
                0 => self.cia2[0] | !self.cia2[2],
                0xD => 0,
                r => self.cia2[r as usize],
            },
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, a: u16, v: u8) {
        match a {
            0xD000..=0xD3FF => {
                let r = (a & 0x3F) as usize;
                match r {
                    0x19 => self.irq_flags &= !(v & 0x0F),
                    0x1E | 0x1F | 0x2F..=0x3F => {}
                    0x20..=0x2E => {
                        self.regs[r] = v & 0x0F;
                        if r == 0x21 {
                            let c = self.cycle;
                            self.events.push((c, v & 0x0F));
                            let l = self.line;
                            if let Some(log) = self.d021_log.as_mut() {
                                log.push((l, c, v & 0x0F));
                            }
                        }
                    }
                    _ => self.regs[r] = v,
                }
            }
            0xD400..=0xD7FF => self.sid[(a & 0x1F) as usize] = v,
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] = v & 0x0F,
            0xDC00..=0xDCFF => self.cia1[(a & 0xF) as usize] = v,
            0xDD00..=0xDDFF => self.cia2[(a & 0xF) as usize] = v,
            _ => {}
        }
    }

    // ---- registers --------------------------------------------------------------------------------------------------

    pub(crate) fn nz(&mut self, v: u8) -> u8 {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
        v
    }

    pub(crate) fn adc(&mut self, m: u8) {
        let s = self.a as u16 + m as u16 + self.c as u16;
        self.v = (!(self.a ^ m) & (self.a ^ s as u8) & 0x80) != 0;
        self.c = s > 0xFF;
        self.a = self.nz(s as u8);
    }

    pub(crate) fn flags(&self, brk: bool) -> u8 {
        (self.n as u8) << 7 | (self.v as u8) << 6 | 0x20 | (brk as u8) << 4 | (self.i as u8) << 2 | (self.z as u8) << 1 | self.c as u8
    }

    pub(crate) fn set_flags(&mut self, f: u8) {
        self.n = f & 0x80 != 0;
        self.v = f & 0x40 != 0;
        self.i = f & 0x04 != 0;
        self.z = f & 0x02 != 0;
        self.c = f & 0x01 != 0;
    }

    pub(crate) fn push(&mut self, v: u8) {
        self.wr(0x100 + self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub(crate) fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.rd(0x100 + self.sp as u16)
    }

    // ---- the KERNAL's part ---------------------------------------------------------------------------------------------

    /// Cycles without a bus access of interest.
    fn burn(&mut self, n: u32) {
        for _ in 0..n {
            self.r();
        }
    }

    /// Pulls a byte from the stack without spending a cycle (the cycles are burnt separately).
    fn pull_free(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.ram[0x100 + self.sp as usize]
    }

    fn rts_free(&mut self) {
        let lo = self.pull_free() as u16;
        let hi = self.pull_free() as u16;
        self.pc = (lo | hi << 8).wrapping_add(1);
        self.burn(6);
    }

    /// Takes the pending interrupt if the processor allows it: hardware entry (7 cycles) and the KERNAL's prologue at `$FF48`
    /// (29 cycles) that ends in `JMP ($0314)`.
    fn maybe_irq(&mut self) {
        if self.i || self.irq_hist & 2 == 0 {
            return;
        }
        // the first five cycles of the hardware entry
        self.r();
        self.r();
        let pc = self.pc;
        self.push((pc >> 8) as u8);
        self.push(pc as u8);
        let f = self.flags(false);
        self.push(f);
        self.i = true;
        self.burn(2);
        for v in [self.a, self.x, self.y] {
            let s = self.sp;
            self.ram[0x100 + s as usize] = v;
            self.sp = s.wrapping_sub(1);
        }
        self.pc = self.ram[0x314] as u16 | (self.ram[0x315] as u16) << 8;
        self.burn(29);
    }

    /// Stand-ins for the KERNAL entries the program uses.
    fn kernal(&mut self, pc: u16) -> Result<(), String> {
        match pc {
            // IRQ: scan the keyboard, then the way out of the interrupt
            0xEA31 => {
                self.scan_keyboard();
                self.burn(100);
                self.pc = 0xEA81;
            }
            0xEA81 => {
                self.y = self.pull_free();
                self.x = self.pull_free();
                self.a = self.pull_free();
                let f = self.pull_free();
                self.set_flags(f);
                let lo = self.pull_free() as u16;
                let hi = self.pull_free() as u16;
                self.pc = lo | hi << 8;
                self.burn(6 + 3 + 3 + 3 + 4);
            }
            // GETIN from the keyboard queue
            0xE124 => {
                let n = self.ram[0xC6];
                if n == 0 {
                    self.a = 0;
                    self.z = true;
                    self.n = false;
                } else {
                    let ch = self.ram[0x277];
                    for i in 0..9 {
                        self.ram[0x277 + i] = self.ram[0x278 + i];
                    }
                    self.ram[0xC6] = n - 1;
                    self.a = ch;
                    self.z = ch == 0;
                    self.n = ch & 0x80 != 0;
                    self.i = false;
                }
                self.c = false;
                self.rts_free();
            }
            // IOINIT
            0xFDA3 => {
                self.cia1[0xD] = 0x7F;
                self.cia2[0xD] = 0x7F;
                self.cia1[0] = 0x7F;
                self.cia1[2] = 0xFF;
                self.cia1[3] = 0;
                self.cia2[0] = 0x03;
                self.cia2[2] = 0x3F;
                self.sid = [0; 32];
                self.ram[1] = 0x37;
                self.burn(120);
                self.rts_free();
            }
            // LISTEN, SECOND, UNLSN: open the command channel
            0xFFB1 => {
                self.cmd.clear();
                self.rts_free();
            }
            0xFF93 | 0xFFAE | 0xFFBA => self.rts_free(),
            // CIOUT
            0xFFA8 => {
                self.cmd.push(self.a);
                self.c = false;
                self.rts_free();
            }
            // SETNAM: A = length, X/Y = address
            0xFFBD => {
                self.name_ptr = (self.x as u16 | (self.y as u16) << 8, self.a);
                self.rts_free();
            }
            // SAVE: A = zero page pointer to the start address, X/Y = end address
            0xFFD8 => {
                let zp = self.a as usize;
                let start = self.ram[zp] as u16 | (self.ram[zp + 1] as u16) << 8;
                let end = self.x as u16 | (self.y as u16) << 8;
                let mut prg = vec![start as u8, (start >> 8) as u8];
                prg.extend_from_slice(&self.ram[start as usize..end as usize]);
                self.saves.push(prg);
                self.c = false;
                self.rts_free();
            }
            _ => return Err(format!("unhandled address ${pc:04X}")),
        }
        Ok(())
    }

    /// SCNKEY: matrix to PETSCII, one new key per interrupt into the queue at `$0277`.
    fn scan_keyboard(&mut self) {
        const TABLE: [u8; 64] = [
            0x14, 0x0D, 0x1D, 0x88, 0x85, 0x86, 0x87, 0x11, 0x33, 0x57, 0x41, 0x34, 0x5A, 0x53, 0x45, 0xFF, 0x35, 0x52, 0x44, 0x36, 0x43, 0x46, 0x54, 0x58, 0x37, 0x59, 0x47, 0x38, 0x42, 0x48, 0x55, 0x56, 0x39, 0x49, 0x4A, 0x30,
            0x4D, 0x4B, 0x4F, 0x4E, 0x2B, 0x50, 0x4C, 0x2D, 0x2E, 0x3A, 0x40, 0x2C, 0x5C, 0x2A, 0x3B, 0x13, 0xFF, 0x3D, 0x5E, 0x2F, 0x31, 0x5F, 0xFF, 0x32, 0x20, 0xFF, 0x51, 0x03,
        ];
        let mut code = 64u8;
        'outer: for col in 0..8usize {
            for row in 0..8usize {
                if self.input.keys[col] & (1 << row) != 0 {
                    let c = (col * 8 + row) as u8;
                    // shift keys (left 15, right 52) are modifiers, not keys
                    if c == 15 || c == 52 || c == 58 || c == 61 {
                        continue;
                    }
                    code = c;
                    break 'outer;
                }
            }
        }
        self.ram[0xCB] = code;
        if code != 64 && code != self.kbd_prev {
            let n = self.ram[0xC6] as usize;
            if n < 10 {
                self.ram[0x277 + n] = TABLE[code as usize];
                self.ram[0xC6] = (n + 1) as u8;
            }
        }
        self.kbd_prev = code;
        self.ram[0xC5] = code;
    }

    // ---- running ---------------------------------------------------------------------------------------------------------

    /// One instruction (or one KERNAL stand-in). Returns false when the program cannot go on.
    fn step(&mut self) -> bool {
        if self.error.is_some() || self.done {
            return false;
        }
        self.maybe_irq();
        if self.pc == RETURN_PC && self.sp == 0xF2 {
            self.done = true;
            return false;
        }
        if self.exec() {
            return true;
        }
        let pc = self.pc;
        if pc >= 0xE000 && self.kernal_visible() {
            if let Err(e) = self.kernal(pc) {
                self.error = Some(e);
                return false;
            }
            return true;
        }
        self.error = Some(format!("the program left the translated code at ${pc:04X}"));
        false
    }

    /// Runs one frame (until the beam has wrapped to line 0). Returns false when the program has finished or stopped.
    pub fn run_frame(&mut self) -> bool {
        self.frame_done = false;
        while !self.frame_done {
            if !self.step() {
                return false;
            }
        }
        true
    }
}
