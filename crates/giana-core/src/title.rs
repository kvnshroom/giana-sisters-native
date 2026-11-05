//! The title sequence of the original, native: the Time Warp screen with its raster bars, the title picture, and the
//! hand-over to the game. No 6502 code runs here.
//!
//! The raster bars of the original are drawn by an interrupt routine that writes the background colour once per raster
//! line, with delay loops that count cycles (a bad line steals 40 of them). So the picture only comes out right when the
//! write lands in the right cycle of the right line. This module therefore keeps a beam (raster line and cycle of the
//! 6569) and spends, instruction by instruction, the cycles the original's routines spend: the interrupt entry, the
//! colour table passes, the table rotation, the wait loop with its own length. The colour of a bar and its place in the
//! line are a result of that, not of a table of positions. The routines are described in `docs/authentic-mode.md`
//! (section 2); `giana-authentic` (the cycle exact C64 slice) is the reference these frames are compared with.
//!
//! The result of a frame are the raster lines handed to the `draw` callback (see `LineState`); `giana-render` turns them
//! into pixels (`giana_render::title`).

/// Colour data the title program carries: the logo of the Time Warp screen and the colour bar table. Derived from the
/// player's own `f_2.prg` (see `giana_extract::derive::title_assets`), never part of the program.
#[derive(Clone)]
pub struct TitleAssets {
    /// multicolour bitmap of the Time Warp screen (8000 bytes)
    pub bitmap: Vec<u8>,
    /// video matrix (1000 bytes) and colour RAM (1000 bytes)
    pub screen: Vec<u8>,
    pub color: Vec<u8>,
    /// the two colour sequences the bars are cut from (`$0963..$09A2`, 32 entries each)
    pub bars: [u8; 64],
}

impl TitleAssets {
    /// From the unpacked 64K memory of the title program after its set-up code has run (it stops at `$0AB2`).
    pub fn from_setup_memory(mem: &[u8]) -> TitleAssets {
        let mut bars = [0u8; 64];
        bars.copy_from_slice(&mem[0x0963..0x09A3]);
        TitleAssets { bitmap: mem[0xE000..0xFF40].to_vec(), screen: mem[0xDC00..0xDFE8].to_vec(), color: mem[0xD800..0xDBE8].to_vec(), bars }
    }
}

pub const WIDTH: usize = 384;
pub const HEIGHT: usize = 272;
const LINES: u16 = 312;
const CYCLES: u16 = 63;
/// first raster line of the picture
pub const FIRST_LINE: u16 = 16;
/// picture pixel at which a colour written in cycle 1 would take effect
const X_OF_CYCLE1: i32 = -104;
/// VIC bank of the title program (`$DD00 = 0`: bank 3)
pub const BANK: usize = 0xC000;

/// Picture pixel at which a colour written in `cycle` is first visible.
pub fn x_of_cycle(cycle: u16) -> i32 {
    cycle as i32 * 8 + X_OF_CYCLE1
}

/// What the video chip knows about a raster line when it is drawn.
pub struct LineState<'a> {
    pub d011: u8,
    pub d016: u8,
    pub d018: u8,
    /// border colour (never changes in the title sequence)
    pub border: u8,
    pub vborder: bool,
    /// the display state is on, or this is a bad line: graphics come from memory, else the idle state
    pub fetch: bool,
    pub rc: u8,
    pub vc: u16,
    pub matrix: &'a [u8; 40],
    pub colors: &'a [u8; 40],
    /// `$D021` at the start of the line, and the writes during it: (cycle, value)
    pub d021_start: u8,
    pub d021_events: &'a [(u16, u8)],
    /// RAM of the VIC bank (offsets from `BANK`: 16K)
    pub ram: &'a [u8],
}

/// Where the program is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prog {
    /// the set-up code is running (seven black frames)
    Boot,
    /// the wait loop of a stage: instruction 0 `LDA $DC01`, 1 `AND #$10`, 2 `BNE`, 3 `JMP $0AB2` (once, after the set-up)
    Wait(u8, u8),
    /// after fire: `SEI`, then the loop that waits for raster line 250
    PollRaster(u8),
    /// straight-line stretches of the program as cycle counts (see `Step`)
    Script,
    Done,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    /// cycles with no visible effect
    Burn(u32),
    D011(u8),
    D01A(u8),
    D021(u8),
    /// the title picture is in the video memory (the copy loops are through)
    ShowPicture,
    /// the wait loop of the title picture starts
    EnterWait(u8),
    /// the program jumps into the loader: the title sequence is over
    Finish,
    /// a cycle that writes to something without a visible effect
    Write,
    SetI(bool),
    /// acknowledge the raster interrupt
    ClearFlag,
    /// continue with this part of the program
    Goto(Prog),
}

pub struct TitleScreen {
    assets: TitleAssets,
    koala: Vec<u8>,
    ram: Vec<u8>,
    color: Vec<u8>,
    // video chip
    d011: u8,
    d016: u8,
    d018: u8,
    d021: u8,
    d01a: u8,
    compare: u16,
    irq_flags: u8,
    irq_hist: u8,
    line: u16,
    cycle: u16,
    frame: u64,
    frame_done: bool,
    vcbase: u16,
    vc: u16,
    rc: u8,
    display: bool,
    den_seen: bool,
    badline: bool,
    vborder: bool,
    matrix: [u8; 40],
    colors: [u8; 40],
    events: Vec<(u16, u8)>,
    line_start_d021: u8,
    // program
    prog: Prog,
    script: std::collections::VecDeque<Step>,
    i_flag: bool,
    fire: bool,
    /// the loop's last read of `$DC01` had fire pressed
    fired: bool,
    boot_frames: u8,
    part: u8,
    // state of the colour bar routine: `$09A3`, `$09A4`, `$09A5` and the 16 colours at `$0953`
    pass_counter: u8,
    phase: u8,
    pos: u8,
    table: [u8; 16],
    /// when set, every write to `$D021` is recorded here: (raster line, cycle, value)
    pub d021_log: Option<Vec<(u16, u16, u8)>>,
    draw: Box<dyn FnMut(usize, &LineState)>,
}

/// Delay loop counters of the 16 colour writes of a pass (`LDX #n` of the routine).
const DELAYS: [u32; 16] = [10, 10, 10, 10, 10, 10, 2, 10, 10, 10, 8, 10, 10, 10, 2, 4];

impl TitleScreen {
    /// Starts the title program. `koala` is the title picture (10001 bytes, Koala layout). `draw` gets every raster line of the
    /// picture (raster lines 16..287 as `y` = 0..271) when the line is complete.
    pub fn new(assets: TitleAssets, koala: &[u8], draw: Box<dyn FnMut(usize, &LineState)>) -> Self {
        let mut ram = vec![0u8; 65536];
        ram[0xE000..0xE000 + 8000].copy_from_slice(&assets.bitmap);
        ram[0xDC00..0xDC00 + 1000].copy_from_slice(&assets.screen);
        let mut color = vec![0u8; 1024];
        color[..1000].copy_from_slice(&assets.color);
        TitleScreen {
            assets,
            koala: koala.to_vec(),
            ram,
            color,
            d011: 0,
            d016: 0,
            d018: 0,
            d021: 0,
            d01a: 0,
            compare: 0,
            irq_flags: 0,
            irq_hist: 0,
            line: 0,
            cycle: 1,
            frame: 0,
            frame_done: false,
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
            line_start_d021: 0,
            prog: Prog::Boot,
            script: Default::default(),
            i_flag: false,
            fire: false,
            fired: false,
            boot_frames: 0,
            part: 1,
            pass_counter: 0,
            phase: 0,
            pos: 0x0F,
            table: [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
            d021_log: None,
            draw,
        }
    }

    /// Joystick port 1 fire button (the title program reads nothing else).
    pub fn set_fire(&mut self, pressed: bool) {
        self.fire = pressed;
    }

    /// The title program has jumped into the loader: the game would be loaded now.
    pub fn done(&self) -> bool {
        self.prog == Prog::Done
    }

    /// Which part is showing: 1 the Time Warp screen, 2 the title picture, 0 black (the program is copying).
    pub fn part(&self) -> u8 {
        self.part
    }

    // ---- video chip ---------------------------------------------------------------------------------------------

    fn raster_compare(&self) -> u16 {
        self.compare
    }

    fn irq_line(&self) -> bool {
        self.irq_flags & self.d01a & 0x0F != 0
    }

    fn is_badline(&self) -> bool {
        let y = self.line;
        (0x30..=0xF7).contains(&y) && self.den_seen && (y & 7) == (self.d011 as u16 & 7)
    }

    fn start_cycle(&mut self) {
        let c = self.cycle;
        if ((c == 2 && self.line != 0) || (c == 3 && self.line == 0))
            && self.raster_compare() == self.line {
                self.irq_flags |= 1;
            }
        if c == 1 {
            let (top, bottom) = if self.d011 & 8 != 0 { (51, 251) } else { (55, 247) };
            if self.line == bottom {
                self.vborder = true;
            }
            if self.line == top && self.d011 & 0x10 != 0 {
                self.vborder = false;
            }
            self.line_start_d021 = self.d021;
            self.events.clear();
            if self.line == 0x30 && self.d011 & 0x10 != 0 {
                self.den_seen = true;
            }
            if self.line == 0 {
                self.den_seen = false;
                self.vcbase = 0;
            }
        }
        if c == 12 {
            if self.line == 0x30 && self.d011 & 0x10 != 0 {
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
            let st = LineState {
                d011: self.d011,
                d016: self.d016,
                d018: self.d018,
                border: 0,
                vborder: self.vborder,
                fetch,
                rc: self.rc,
                vc: self.vc,
                matrix: &self.matrix,
                colors: &self.colors,
                d021_start: self.line_start_d021,
                d021_events: &self.events,
                ram: &self.ram[BANK..],
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
            self.frame += 1;
            self.frame_done = true;
        }
    }

    fn tick(&mut self) {
        self.cycle += 1;
        if self.cycle > CYCLES {
            self.end_line();
        }
        self.start_cycle();
        self.irq_hist = self.irq_hist << 1 | self.irq_line() as u8;
        if self.cycle == 15 && self.badline {
            let vm = BANK + ((self.d018 as usize >> 4) << 10);
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

    // ---- cycles of the program ----------------------------------------------------------------------------------

    fn read(&mut self) {
        self.stall(false);
        self.tick();
    }

    fn write_with(&mut self, f: impl FnOnce(&mut Self)) {
        self.stall(true);
        f(self);
        self.tick();
    }

    /// Spends an instruction: `r` is a cycle that reads, `w` one that writes.
    fn cycles(&mut self, pattern: &str) {
        for ch in pattern.bytes() {
            if ch == b'w' {
                self.write_with(|_| {});
            } else {
                self.read();
            }
        }
    }

    fn reads(&mut self, n: u32) {
        for _ in 0..n {
            self.read();
        }
    }

    /// `STA abs` of a colour to `$D021`: three reading cycles, then the write.
    #[allow(dead_code)] // kept reference of the `STA abs` to $D021 timing
    fn store_d021(&mut self, v: u8) {
        self.reads(3);
        self.write_with(|s| {
            s.d021 = v & 15;
            let c = s.cycle;
            s.events.push((c, v & 15));
            let l = s.line;
            if let Some(log) = s.d021_log.as_mut() {
                log.push((l, c, v & 15));
            }
        });
    }

    fn irq_taken(&self) -> bool {
        !self.i_flag && self.irq_hist & 2 != 0
    }

    // ---- the program --------------------------------------------------------------------------------------------

    /// Runs until the video chip has finished a frame.
    pub fn run_frame(&mut self) {
        self.frame_done = false;
        if self.prog == Prog::Boot {
            if self.boot_frames < 7 {
                // the set-up code clears and fills the video memory: seven black frames
                self.boot_frames += 1;
                self.frame += 1;
                return;
            }
            self.boot_end();
        }
        while !self.frame_done && self.prog != Prog::Done {
            self.step();
        }
    }

    /// The end of the set-up code, in frame 7 at raster line 304: the video registers are set, `CLI` follows. The raster
    /// flag has been set since line 0 of the frame (the compare value was still 0), so the first interrupt comes at once.
    fn boot_end(&mut self) {
        self.line = 304;
        self.cycle = 50;
        self.d01a = 1;
        self.d011 = 0x3B;
        self.compare = 0x3C;
        self.d018 = 0x78;
        self.d016 = 0x18;
        self.irq_flags = 1;
        self.irq_hist = 3;
        self.cycles("rr"); // CLI
        self.i_flag = false;
        self.prog = Prog::Wait(1, 3);
    }

    fn step(&mut self) {
        match self.prog {
            Prog::Wait(stage, ip) => {
                if self.irq_taken() {
                    self.start_irq(Prog::Wait(stage, ip));
                    return;
                }
                match ip {
                    0 => {
                        self.reads(3);
                        self.read();
                        self.fired = self.fire;
                        self.prog = Prog::Wait(stage, 1);
                    }
                    1 => {
                        self.cycles("rr");
                        self.prog = Prog::Wait(stage, 2);
                    }
                    2 => {
                        if self.fired {
                            self.cycles("rr");
                            self.i_flag = true; // SEI
                            self.cycles("rr");
                            self.prog = Prog::PollRaster(stage);
                        } else {
                            self.cycles("rrr");
                            self.prog = Prog::Wait(stage, 0);
                        }
                    }
                    _ => {
                        self.cycles("rrr"); // JMP $0AB2
                        self.prog = Prog::Wait(stage, 0);
                    }
                }
            }
            Prog::PollRaster(stage) => {
                // LDA $D012 / CMP #$FA / BNE
                self.reads(3);
                self.stall(false);
                let line = self.line as u8;
                self.tick();
                self.cycles("rr");
                if line == 0xFA {
                    self.cycles("rr");
                    self.begin_exit(stage);
                } else {
                    self.cycles("rrr");
                }
            }
            Prog::Script => self.script_step(),
            Prog::Boot | Prog::Done => {}
        }
    }

    /// The colour bar interrupt (`$0846`): entry, passes over the colour table, rotating the table, return. The cycles it
    /// spends become a script (so a frame may end in the middle of it); the colours are taken from the table as it is now.
    fn start_irq(&mut self, back: Prog) {
        use Step::*;
        let mut s: Vec<Step> = Vec::new();
        // hardware entry and the KERNAL prologue: 36 cycles
        s.extend([Burn(2), Write, Write, Write, SetI(true), Burn(31)]);
        // LDA #1 / STA $D019: acknowledges the raster interrupt, then four NOPs
        s.extend([Burn(2 + 3), ClearFlag, Burn(8)]);
        let mut passes = 1;
        let mut counter = self.pass_counter;
        loop {
            for k in 0..16 {
                s.push(Burn(4)); // LDA table,k
                s.push(Burn(3));
                s.push(D021(self.table[k]));
                let n = DELAYS[k];
                s.push(Burn(2 + (2 * (n + 1) + 3 * n + 2))); // LDX #n, DEX / BPL
            }
            s.push(Burn(2)); // LDX #2
            s.push(Burn(4));
            s.push(Write);
            s.push(Write); // DEC $09A3
            let negative = counter == 0;
            counter = counter.wrapping_sub(1);
            if negative {
                s.push(Burn(3)); // BMI taken
                break;
            }
            s.push(Burn(2 + 3)); // BMI not taken, JMP $084F
            passes += 1;
        }
        let _ = passes;
        self.pass_counter = 4;
        // $0909: back to black, restart the counter, rotate the colour table
        s.extend([Burn(2 + 3), D021(0), Burn(2 + 3), Write, Burn(4 + 2 + 3), Write]);
        self.phase ^= 1;
        let base = if self.phase != 0 { 0 } else { 32 };
        // BEQ (not taken 2, taken 3), LDX #$0F, LDY $09A5
        s.push(Burn(if self.phase != 0 { 2 } else { 3 } + 2 + 4));
        let mut y = self.pos as usize;
        for x in (0..16).rev() {
            self.table[x] = self.assets.bars[base + y];
            // LDA abs,Y (4), STA abs,X (5), DEY, DEX (4), BPL (3 taken, 2 not)
            s.push(Burn(4 + 4 + 4));
            s.push(Write);
            s.push(Burn(if x > 0 { 3 } else { 2 }));
            y = y.wrapping_sub(1);
        }
        if self.phase != 0 {
            s.push(Burn(3)); // JMP $0950
        } else {
            s.push(Burn(4 + 2)); // LDA $09A5, CMP #$1F
            if self.pos < 0x1F {
                s.push(Burn(3 + 4));
                s.push(Write);
                s.push(Write); // BCC taken, INC $09A5
                self.pos += 1;
            } else {
                s.push(Burn(2 + 2 + 3)); // BCC not taken, LDA #$10, STA $09A5 (reads)
                s.push(Write);
                s.push(Burn(3)); // JMP $0950
                self.pos = 0x10;
            }
        }
        // JMP $EA81 and the KERNAL's way out of the interrupt
        s.extend([Burn(3 + 19), SetI(false), Goto(back)]);
        self.script = s.into();
        self.prog = Prog::Script;
    }

    /// Fire was seen and raster line 250 is reached: the program blanks the screen, copies the next part into place.
    fn begin_exit(&mut self, stage: u8) {
        use Step::*;
        let mut s: Vec<Step> = Vec::new();
        if stage == 1 {
            // LDA #0; STA $D011; STA $D01A; STA $D021 (the colour is written in the 4th cycle of each store)
            s.extend([Burn(5), D011(0), Burn(3), D01A(0), Burn(3), D021(0)]);
            // KERNAL vector set-up: LDA/STA $DC0D, vectors, pointers
            let mut n = 6 + 12 + 10 + 10 + 2;
            // copy $2000.. to $E000..: 32 pages of 256 bytes (LDA (zp),Y / STA (zp),Y / INY / BNE; INC, INC, BNE per page)
            n += 31 * 4108 + 4107;
            // SEI / LDX #0 and the loop that copies the colours and screen matrix (256 passes, page crossings of the indexed loads)
            n += 4 + 256 * 84 + 4 * 64 + 4 * 40 + 255 * 4 + 2;
            // set up the source/destination pointers for the music and the driver ($4C00.. to $9000..)
            n += 22;
            n += 60 * 4110 + 4109;
            // JSR $C000 / JMP $C012 / SEI / vectors / JSR $9B00 (36 cycles) / CLI / RTS / LDA #$3B / STA $D011
            n += 6 + 3 + 2 + 6 + 6 + 6 + 36 + 2 + 6 + 2 + 3;
            s.push(Burn(n));
            s.push(ShowPicture);
            s.push(D011(0x3B));
            s.push(SetI(false));
            s.push(EnterWait(2));
        } else {
            // LDA #0; SEI; STA $DD0E; STA $D011; STA $D418
            s.extend([Burn(2 + 2 + 4 + 3), D011(0), Burn(4)]);
            let mut n = 26;
            n += 256 * 38 + 32 * 3 + 31 + 255 * 3 + 2;
            n += 39 * 16 + 15;
            n += 3;
            s.push(Burn(n));
            s.push(Finish);
        }
        self.script = s.into();
        self.prog = Prog::Script;
    }

    fn script_step(&mut self) {
        let Some(step) = self.script.front().copied() else {
            return;
        };
        match step {
            Step::Burn(n) => {
                self.read();
                if n <= 1 {
                    self.script.pop_front();
                } else {
                    self.script[0] = Step::Burn(n - 1);
                }
            }
            Step::D011(v) => {
                self.write_with(|s| s.d011 = v);
                self.part = if v == 0 { 0 } else { 2 };
                // the trace of $D011=0 in the last burn cycle: the write is the 4th cycle of the store
                self.script.pop_front();
            }
            Step::D01A(v) => {
                self.write_with(|s| s.d01a = v);
                self.script.pop_front();
            }
            Step::D021(v) => {
                self.write_with(|s| {
                    s.d021 = v;
                    let c = s.cycle;
                    s.events.push((c, v));
                    let l = s.line;
                    if let Some(log) = s.d021_log.as_mut() {
                        log.push((l, c, v));
                    }
                });
                self.script.pop_front();
            }
            Step::ShowPicture => {
                self.show_picture();
                self.script.pop_front();
            }
            Step::EnterWait(stage) => {
                self.script.pop_front();
                self.prog = Prog::Wait(stage, 0);
            }
            Step::Write => {
                self.write_with(|_| {});
                self.script.pop_front();
            }
            Step::SetI(v) => {
                self.i_flag = v;
                self.script.pop_front();
            }
            Step::ClearFlag => {
                self.write_with(|s| s.irq_flags &= !1);
                self.script.pop_front();
            }
            Step::Goto(p) => {
                self.script.pop_front();
                self.prog = p;
            }
            Step::Finish => {
                self.script.pop_front();
                self.prog = Prog::Done;
            }
        }
    }

    /// The copy loops have put the title picture (Koala layout) in the video memory.
    fn show_picture(&mut self) {
        self.ram[0xE000..0xE000 + 8000].copy_from_slice(&self.koala[..8000]);
        self.ram[0xDC00..0xDC00 + 1000].copy_from_slice(&self.koala[8000..9000]);
        self.color[..1000].copy_from_slice(&self.koala[9000..10000]);
    }
}
