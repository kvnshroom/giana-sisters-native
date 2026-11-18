//! The native frame driver (work package F of `docs/rom-hack-family-c.md`, section 7.9): who runs what at which raster
//! line, so that a whole frame of the preview runs from Rust with the ported routines of the packages P, S, B, O1, O2
//! and W.
//!
//! # Model
//!
//! The original is two threads of 6502 code: a chain of raster IRQs (each handler sets the line and the vector of the
//! next one) and the main program (the game flow of [`crate::flow`]: screens, stage start, the main loop `$1F68`, death,
//! stage clear), which runs whenever no IRQ does. [`Core::run_frame`] runs one video frame from raster line `$41` to the
//! next line `$41` the same way: it takes the raster IRQ that the game has set up (`$D012` and the vector at
//! `$FFFE/$FFFF`, if `$D01A` enables it and the main program has not masked IRQs), runs its native handler as if it
//! took no time at its line, and gives the main program the time between two handlers. The handlers of the play chain:
//!
//! | line | vector | native |
//! |---|---|---|
//! | `$41` | `$3488` | [`sprites::irq_41`] |
//! | `$42` | `$30A4` | [`sprites::irq_42`], then the block animations of package S unless a column step runs |
//! | `$5A` | `$31F4` | [`sprites::irq_5a`] and the frame logic `$30FE` ([`Core::logic`]); a column step requested by the logic starts here and ends in the next frame's `$5A` handler (the original's `$18C4` runs with IRQs on for a whole frame) |
//! | `y - 4` | `$32B4` | [`sprites::irq_split`] |
//! | `$63`.. | `$3183` | [`sprites::irq_mux`] |
//! | `$FB` | `$32DC` | [`sprites::irq_fb`], the object frame `$72EA` ([`Core::objects`]), Giana's position, sprites, frame, tile shimmer, effect, pointers, tile animation, music tick |
//!
//! Every write whose raster position matters goes into the [`SpriteUnit`] at the line of its handler (package W showed
//! that the line is enough); the `$D01E` read of the object code is placed at line 254, where the original makes it.
//!
//! Routines that are not ported yet (packages O3/O4 at the time of writing) are called through [`Original`], which the
//! oracle tests implement with the original code; when a port is merged, its call replaces the fallback here.

use crate::layout::C30aLayout;
use crate::music::Music;
use crate::objects::{self, ClassEnd, Dispatch, TailEnd};
use crate::player::{self, MoveEnd, Regs};
use crate::sprites::{self, Io, Split, SpriteDma, SpriteUnit};
use crate::pickups::{self, ContactEnd};
use crate::{blocks, bosses, classes, scroll};

/// The line where a native frame starts and ends (the play chain's first IRQ).
pub const FRAME_LINE: u16 = 0x41;
/// Raster lines of a PAL frame.
pub const LINES: u16 = 312;
/// Line of the object code's `$D01E` read (`$7CB7`, measured: 254 in every frame).
pub const COLLISION_LINE: u16 = 254;
/// Line of the `$FB` handler's sprite writes after the object code (`$3340`, measured 263..276).
pub const SPRITES_LINE: u16 = 272;
/// The same while the logic is frozen (`$20`; measured 263).
pub const SPRITES_LINE_FROZEN: u16 = 263;

/// Routines of the original that have no native port yet, called by address (`regs` in, registers out). The oracle
/// tests implement it with the original code on a copy of the [`Core`]; the shipped game must not need it.
pub trait Original {
    /// `JSR addr` with IRQs masked.
    fn call(&mut self, core: &mut Core, addr: u16, regs: Regs) -> Regs;
    /// A raster IRQ handler (`vector`) that has no native port, run to its `RTI`.
    fn irq(&mut self, core: &mut Core, vector: u16);
}

/// An [`Original`] for a game without fallbacks: every call is a programming error.
pub struct NoOriginal;

impl Original for NoOriginal {
    fn call(&mut self, _: &mut Core, addr: u16, _: Regs) -> Regs {
        panic!("no native port of ${addr:04X}")
    }
    fn irq(&mut self, _: &mut Core, vector: u16) {
        panic!("no native port of the IRQ ${vector:04X}")
    }
}

/// The machine the native game runs on: the 64 KB memory image (owned by the music driver, which works on the same
/// RAM), colour RAM, the VIC registers as written, the sprite unit, CIA 1 (joystick port 2 and the keyboard) and the
/// raster position of the scheduler.
#[derive(Clone)]
pub struct Core {
    pub l: &'static C30aLayout,
    /// the music driver; `music.driver.mem` **is** the game's RAM, `music.driver.sid` the SID registers
    pub music: Music,
    pub color: Vec<u8>,
    /// VIC registers `$D000..$D03F` as written (`$D019` acknowledgements are not stored)
    pub vic: [u8; 64],
    pub unit: SpriteUnit,
    /// `$DC00` as read: joystick port 2, active low
    pub joy: u8,
    /// keyboard matrix, one byte per column (a low bit = key down), as the oracle's
    pub keys: [u8; 8],
    /// last value written to `$DC00` (the key column select)
    pub cia_pra: u8,
    /// the raster line the scheduler is at
    pub line: u16,
    /// the screen and colour rows as the VIC fetched them in this frame (see [`Core::picture_rows`])
    pub shown: Shown,
    /// completed frames
    pub frames: u64,
    /// raster bars of this frame: `$D021` written per raster line by the menus' and title's IRQs, `(line, colour)`
    pub bars: Vec<(u16, u8)>,
    /// PETSCII keys typed, for the KERNAL's keyboard buffer (`GETIN`, the high-score name entry)
    pub typed: std::collections::VecDeque<u8>,
    /// the key held for the KERNAL's `$CB` (its matrix code, `$40` = none): the high-score screen ends on space
    /// (`$3C`) or F1 (`$04`)
    pub held_key: u8,
    /// the raster IRQs of the current frame: `(line, vector)`
    pub handled: Vec<(u16, u16)>,
    /// when the raster IRQ (`$D012`) was set up, in CPU cycles from the frame start (line `$41`): a compare line the
    /// raster had passed then fires in the next frame
    pub armed: i64,
    /// the VIC's raster IRQ flag (`$D019` bit 0) is set but no IRQ was taken (the raster passed the compare line with
    /// `$D01A` off): enabling `$D01A` with IRQs on gives an IRQ at once (the stage start's and the title's `$284A`)
    pub latch: bool,
    /// a column step runs: the main program gets no time until it ends (the original's `$18C4` runs in the `$5A` IRQ
    /// with IRQs on)
    pub starved: bool,
    /// since when (cycles from line `$41` of this frame; earlier frames negative) the main program has IRQs masked: a
    /// raster line crossed in that time latches its flag, and the IRQ comes at the `CLI`
    pub masked_since: Option<i64>,
    /// the picture's VIC register writes of this frame, `(first raster line that shows it, register, value)` in the
    /// order they were made: `$D011`, `$D016`, `$D018`, `$D020..$D023` (the screens' renderer,
    /// [`crate::render::FrameState::render_screen`])
    pub vic_writes: Vec<(u16, u8, u8)>,
    /// a screen's IRQ (menus, title, high-score screen) ran in this frame: it is drawn line by line
    pub screen_frame: bool,
    /// the previous frame's start registers and writes (the picture's lines above `$41` belong to it)
    pub prev_writes: (Box<[u8; 47]>, Vec<(u16, u8, u8)>),
    /// the sprite rows of raster lines 0..`$40` recorded at the end of this frame: they belong to the next frame's
    /// picture (one raster frame, as VICE shows it; [`SpriteUnit::rows`] has the ones of this picture)
    pub next_top_rows: Vec<crate::sprites::SpriteRow>,
}

/// The first text row the VIC fetches after the frame logic of the `$5A` IRQ (about 1,650 cycles from line `$5A`)
/// is done: its block changes show from this row on in the same frame.
pub const LOGIC_DONE_ROW: usize = 7;

/// The raster line after the last text line: a handler from here on runs after the picture's text.
pub const LAST_TEXT_LINE: u16 = 251;

/// The VIC registers the picture depends on (besides the sprites): `$D011`, `$D016`, `$D018`, `$D020..$D023`.
pub const PICTURE_REGS: [u8; 7] = [0x11, 0x16, 0x18, 0x20, 0x21, 0x22, 0x23];

/// CPU cycles of a frame.
pub const FRAME_CYCLES: i64 = LINES as i64 * 63;

/// Estimated CPU cycles of the native handlers (what the original's take on average; the main program's time is the
/// rest, see [`Core::run_frame`]).
pub fn handler_cycles(core: &Core, vector: u16) -> i64 {
    let l = core.l;
    match vector {
        v if v == l.irq_41 => 80,
        v if v == l.irq_42 => 315,
        // the logic: about 1,650 cycles, 510 while it is frozen (`$20`)
        v if v == l.irq_5a => {
            if core.peek(l.game_state) != 0 {
                510
            } else {
                1650
            }
        }
        v if v == l.irq_split || v == l.irq_mux => 150,
        v if v == l.irq_fb => 4350,
        _ => crate::menus::irq_cycles(core, vector),
    }
}

/// The text rows the frame shows (screen codes and colours), and the VIC registers at the frame start.
#[derive(Clone)]
pub struct Shown {
    /// rows taken in this frame (bit `r`)
    pub taken: u32,
    pub screen: [[u8; 40]; 25],
    pub color: [[u8; 40]; 25],
    pub vic: [u8; 47],
    /// the character set `$C000..$C7FF` as the picture's last text line (250) showed it: the tile animation of the
    /// `$FB` IRQ changes it after the display
    pub chars: Box<[u8; 2048]>,
    /// [`Shown::chars`] taken in this frame
    pub chars_taken: bool,
    /// the fine scrolls `$67` (playfield) and `$68` (cloud band) at the frame start: the `$5A` and `$42` IRQs write
    /// them to `$D016` before the logic changes them (at a column step the logic's new `$67` belongs to the next
    /// frame's picture, together with the shifted playfield)
    pub scroll: [u8; 2],
    /// the status rows 0-1 (screen codes, colours) the picture shows: fetched at lines `$33..$40` before this frame's
    /// start, at the end of the previous native frame (the picture is one raster frame, as VICE shows it)
    pub top: [[[u8; 40]; 2]; 2],
}

impl Core {
    /// A core over the given state (64 KB RAM, colour RAM, VIC registers, SID registers, the sprite DMA state) at raster
    /// line `line`.
    pub fn new(l: &'static C30aLayout, ram: Vec<u8>, color: &[u8], vic: &[u8], sid: &[u8], dma: [SpriteDma; 8], line: u16) -> Core {
        let mut music = Music::new(ram);
        music.driver.sid[..sid.len().min(32)].copy_from_slice(&sid[..sid.len().min(32)]);
        let mut v = [0u8; 64];
        v[..vic.len().min(64)].copy_from_slice(&vic[..vic.len().min(64)]);
        let mut c = vec![0u8; 1024];
        c[..color.len().min(1024)].copy_from_slice(&color[..color.len().min(1024)]);
        let mut shown = Shown { taken: 0, screen: [[0; 40]; 25], color: [[0; 40]; 25], vic: [0; 47], chars: Box::new([0; 2048]), chars_taken: false, scroll: [0; 2], top: [[[0; 40]; 2]; 2] };
        shown.vic.copy_from_slice(&v[..47]);
        let s0 = l.screen as usize;
        for r in 0..2 {
            shown.screen[r].copy_from_slice(&music.driver.mem[s0 + r * 40..s0 + r * 40 + 40]);
            for k in 0..40 {
                shown.color[r][k] = c[r * 40 + k] & 15;
            }
        }
        shown.top = [[shown.screen[0], shown.screen[1]], [shown.color[0], shown.color[1]]];
        Core {
            l,
            music,
            color: c,
            vic: v,
            unit: SpriteUnit::new(dma, line, 0, l),
            joy: 0xFF,
            keys: [0xFF; 8],
            cia_pra: 0xFF,
            line,
            shown,
            frames: 0,
            bars: Vec::new(),
            typed: Default::default(),
            held_key: 0x40,
            handled: Vec::new(),
            armed: -1,
            latch: false,
            starved: false,
            masked_since: None,
            vic_writes: Vec::new(),
            screen_frame: false,
            prev_writes: (Box::new([0; 47]), Vec::new()),
            next_top_rows: Vec::new(),
        }
    }

    #[inline]
    pub fn ram(&mut self) -> &mut [u8] {
        &mut self.music.driver.mem[..]
    }

    #[inline]
    pub fn peek(&self, a: u16) -> u8 {
        self.music.driver.mem[a as usize]
    }

    #[inline]
    pub fn poke(&mut self, a: u16, v: u8) {
        self.music.driver.mem[a as usize] = v;
    }

    /// RAM, colour RAM and VIC registers at once.
    pub fn parts(&mut self) -> (&mut [u8], &mut [u8], &mut [u8]) {
        (&mut self.music.driver.mem[..], &mut self.color[..], &mut self.vic[..])
    }

    /// A 16-bit vector (`lo, hi`) at `a`.
    pub fn set_vector(&mut self, a: u16, v: u16) {
        self.poke(a, v as u8);
        self.poke(a.wrapping_add(1), (v >> 8) as u8);
    }

    /// `$DC01` as read with the column select of the last `$DC00` write.
    pub fn read_keys(&self) -> u8 {
        let mut v = 0xFF;
        for c in 0..8 {
            if self.cia_pra & (1 << c) == 0 {
                v &= self.keys[c];
            }
        }
        v
    }

    /// `LDA #sel; STA $DC00; LDA $DC01`.
    pub fn scan(&mut self, sel: u8) -> u8 {
        self.cia_pra = sel;
        self.read_keys()
    }

    /// The raster compare line (`$D012` and bit 8 in `$D011` bit 7).
    pub fn compare_line(&self) -> u16 {
        self.vic[0x12] as u16 | ((self.vic[0x11] as u16 & 0x80) << 1)
    }

    /// Moves the sprite unit to the end of line `line` (lines before the current one are in the next frame).
    pub fn raster_to(&mut self, line: u16) {
        let (ram, vic) = (&self.music.driver.mem[..], &self.vic[..]);
        self.unit.run_to(line, 62, vic, ram);
        self.line = line;
    }

    /// Runs `f` with an [`Io`] (timed writes) on the core's RAM and VIC registers.
    fn io<R>(&mut self, f: impl FnOnce(&mut Io) -> R) -> R {
        let l = self.l;
        let mut io = Io::new(&mut self.music.driver.mem[..], &mut self.vic[..], l);
        f(&mut io)
    }

    /// Copies the text rows `rows` of screen and colour RAM into [`Core::shown`] (the picture rule: a row shows what
    /// was in RAM when the VIC fetched it).
    pub fn picture_rows(&mut self, rows: std::ops::Range<usize>) {
        let screen = self.l.screen as usize;
        for r in rows {
            self.shown.taken |= 1 << r;
            let s = screen + r * 40;
            self.shown.screen[r].copy_from_slice(&self.music.driver.mem[s..s + 40]);
            for c in 0..40 {
                self.shown.color[r][c] = self.color[r * 40 + c] & 15;
            }
        }
    }

    /// The raster IRQ that comes next in this frame after cycle `t` (from line `$41`): `(cycle, vector)`.
    fn next_irq(&self, main: &crate::flow::Main, t: i64) -> Option<(i64, u16)> {
        if self.vic[0x1A] & 1 == 0 || main.i {
            return None;
        }
        if self.latch {
            let v = if self.peek(1) & 2 != 0 { self.l.kernal_irq_vector } else { self.l.irq_vector };
            return Some((t, u16::from_le_bytes([self.peek(v), self.peek(v + 1)])));
        }
        let rel = (self.compare_line() as i64 + LINES as i64 - FRAME_LINE as i64) % LINES as i64;
        let at = rel * 63;
        if at < self.armed || (rel == 0 && self.armed >= 0) {
            return None;
        }
        // with the KERNAL ROM in (`$01` bit 1) the CPU's vector is the ROM's: it goes through `$0314`
        let v = if self.peek(1) & 2 != 0 { self.l.kernal_irq_vector } else { self.l.irq_vector };
        Some((at.max(t), u16::from_le_bytes([self.peek(v), self.peek(v + 1)])))
    }

    /// Keeps the character set for the picture ([`Shown::chars`]).
    fn take_chars(&mut self) {
        let c = self.l.charset as usize;
        self.shown.chars.copy_from_slice(&self.music.driver.mem[c..c + 2048]);
        self.shown.chars_taken = true;
    }

    /// The VIC registers of every raster line 0..311 of the picture (one raster frame: the lines above `$41` from the
    /// previous native frame, the rest from this one), from the frames' start registers and [`Core::vic_writes`].
    pub fn picture_regs(&self) -> Vec<[u8; 47]> {
        let rel = |line: u16| (line + LINES - FRAME_LINE) % LINES;
        let at = |start: &[u8; 47], writes: &[(u16, u8, u8)], line: u16| {
            let mut v = *start;
            for &(l, r, x) in writes {
                if rel(l) <= rel(line) {
                    v[r as usize] = x;
                }
            }
            v
        };
        (0..LINES)
            .map(|line| if line < FRAME_LINE { at(&self.prev_writes.0, &self.prev_writes.1, line) } else { at(&self.shown.vic, &self.vic_writes, line) })
            .collect()
    }

    /// Whether the raster crossed the compare line between the cycles `m` (may be from an earlier frame) and `t`.
    fn crossed(&self, m: i64, t: i64) -> bool {
        if t - m >= FRAME_CYCLES {
            return true;
        }
        let rel = (self.compare_line() as i64 + LINES as i64 - FRAME_LINE as i64) % LINES as i64 * 63;
        (m..t).contains(&rel) || (m..t).contains(&(rel - FRAME_CYCLES))
    }

    /// Cycles the VIC takes from the CPU between `t0` and `t1` (cycles from line `$41`): 43 per bad line while the
    /// screen is on.
    fn stolen(&self, t0: i64, t1: i64) -> i64 {
        if self.vic[0x11] & 0x10 == 0 || t1 <= t0 {
            return 0;
        }
        let ys = (self.vic[0x11] & 7) as i64;
        let (l0, l1) = (t0 / 63, (t1 + 62) / 63);
        (l0..l1)
            .filter(|&r| {
                let line = (r + FRAME_LINE as i64) % LINES as i64;
                (0x30..=0xF7).contains(&line) && line & 7 == ys
            })
            .count() as i64
            * 43
    }

    /// One video frame, from raster line `$41` to the next line `$41`: the raster IRQs the game has set up, each by its
    /// native handler at its line, and the main program in the time between them (the CPU cycles of the gap minus the
    /// VIC's bad lines minus the handler's estimated cycles, [`handler_cycles`]).
    pub fn run_frame(&mut self, main: &mut crate::flow::Main, orig: &mut dyn Original) {
        self.prev_writes = (Box::new(self.shown.vic), std::mem::take(&mut self.vic_writes));
        self.shown.vic.copy_from_slice(&self.vic[..47]);
        self.shown.scroll = [self.peek(self.l.fine_scroll), self.peek(self.l.fine_scroll + 1)];
        // the picture's sprite rows: lines 0..`$40` from the end of the previous native frame, then this frame's
        self.unit.rows = std::mem::take(&mut self.next_top_rows);
        let first_row = self.unit.rows.len();
        self.handled.clear();
        self.bars.clear();
        self.screen_frame = false;
        self.shown.chars_taken = false;
        self.shown.taken = 0;
        let mut t: i64 = 0;
        // the end of a handler that runs with IRQs on and was interrupted (cycles from line `$41`)
        let mut interrupted: Option<i64> = None;
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 1000, "the frame does not end");
            let next = self.next_irq(main, t);
            let until = next.map_or(FRAME_CYCLES, |n| n.0);
            if until > t && !self.starved {
                let mut budget = until - t - self.stolen(t, until);
                let total = budget;
                if main.i && self.masked_since.is_none() {
                    self.masked_since = Some(t);
                }
                if budget > 0 && main.slice(self, orig, &mut budget, t, until) {
                    // the main program changed the IRQ setup: the rest of the gap is planned again
                    // the CPU time used, scaled back to raster time (the gap also holds the VIC's cycles)
                    t += ((total - budget) * (until - t) / total.max(1)).max(1);
                    self.armed = t;
                    if main.i && self.masked_since.is_none() {
                        self.masked_since = Some(t);
                    } else if !main.i {
                        if let Some(m) = self.masked_since.take() {
                            // `CLI`: a raster flag latched while the IRQs were masked gives its IRQ now
                            if self.vic[0x1A] & 1 != 0 && self.crossed(m, t) {
                                self.latch = true;
                            }
                        }
                    }
                    continue;
                }
            }
            match next {
                None => break,
                Some((at, v)) => {
                    let line = ((at / 63) + FRAME_LINE as i64) as u16 % LINES;
                    self.raster_to(line);
                    self.handled.push((line, v));
                    self.latch = false;
                    // the line has been reached: it fires again only in the next frame (unless a handler sets another)
                    self.armed = at + 1;
                    if line >= LAST_TEXT_LINE && !self.shown.chars_taken {
                        self.take_chars();
                    }
                    self.irq(v, orig);
                    let cycles = handler_cycles(self, v);
                    t = at + cycles;
                    // the split path of the `$5A` handler (`$321F`) runs the frame logic with IRQs on: the split IRQ
                    // at Giana's `y - 4` (and the multiplexer's after it) comes in the middle of the logic, at its
                    // line, and the logic goes on after it (it is done later by the time of those handlers)
                    let rel = (self.compare_line() as i64 + LINES as i64 - FRAME_LINE as i64) % LINES as i64 * 63;
                    let vector = u16::from_le_bytes([self.peek(self.l.irq_vector), self.peek(self.l.irq_vector + 1)]);
                    let split = v == self.l.irq_5a && vector == self.l.irq_split;
                    let end = match interrupted.take() {
                        Some(end) => Some(end + cycles),
                        None if split => Some(t),
                        None => None,
                    };
                    if let Some(end) = end {
                        t = t.max(end);
                        if rel > at && rel < end {
                            t = rel;
                            interrupted = Some(end);
                        }
                    }
                    if v == self.l.irq_fb {
                        // the `$FB` handler runs its objects and music with IRQs on: the IRQ it set up comes in the
                        // middle of it when its line is that near (the title's `$31` after a late `$FB`)
                        let rel = (self.compare_line() as i64 + LINES as i64 - FRAME_LINE as i64) % LINES as i64 * 63;
                        if rel > at && rel < t {
                            t = rel;
                        }
                    }
                    if t >= FRAME_CYCLES {
                        t = FRAME_CYCLES - 1;
                    }
                }
            }
        }
        if self.vic[0x1A] & 1 == 0 {
            // the raster passed the compare line in this frame without an IRQ
            self.latch = true;
        }
        self.armed -= FRAME_CYCLES;
        if let Some(m) = self.masked_since {
            self.masked_since = Some(m - FRAME_CYCLES);
        }
        self.raster_to(FRAME_LINE - 1);
        // the rows after the raster's wrap to line 0 are the next picture's
        let wrap = self.unit.rows[first_row..].iter().position(|r| r.line < FRAME_LINE).map_or(self.unit.rows.len(), |k| first_row + k);
        self.next_top_rows = self.unit.rows.split_off(wrap);
        // the status rows are fetched at the end (lines `$33..$40` of the next raster frame); rows that no handler took
        // (screens without the play chain) as they are now
        if !self.shown.chars_taken {
            self.take_chars();
        }
        let rest = self.shown.taken;
        self.shown.top = [[self.shown.screen[0], self.shown.screen[1]], [self.shown.color[0], self.shown.color[1]]];
        self.picture_rows(0..2);
        for r in 2..25 {
            if rest & (1 << r) == 0 {
                self.picture_rows(r..r + 1);
            }
        }
        self.frames += 1;
    }

    // ---- the IRQ handlers ---------------------------------------------------------------------------------------

    /// Runs the native handler of the raster IRQ with this vector at the current line (the play chain here, the
    /// screens of the game flow in [`crate::flow`]); anything else goes to `orig`.
    pub fn irq(&mut self, vector: u16, orig: &mut dyn Original) {
        let before = self.vic;
        let line = self.line;
        let screen = self.irq_handler(vector, orig);
        if screen {
            self.screen_frame = true;
        } else {
            // the play chain's handlers write the picture's registers right at their line
            for reg in PICTURE_REGS {
                if self.vic[reg as usize] != before[reg as usize] {
                    self.vic_writes.push(((line + 1) % LINES, reg, self.vic[reg as usize]));
                }
            }
        }
    }

    /// [`Core::irq`]'s dispatch; `true` = a screen's handler ([`crate::menus`]), which logs its own writes.
    fn irq_handler(&mut self, vector: u16, orig: &mut dyn Original) -> bool {
        let l = self.l;
        match vector {
            v if v == l.irq_41 => {
                self.raster_to(FRAME_LINE);
                self.io(sprites::irq_41);
            }
            v if v == l.irq_42 => {
                self.io(|io| {
                    sprites::irq_42(io);
                });
                self.block_animations_42();
            }
            v if v == l.irq_5a => self.irq_5a(orig),
            v if v == l.irq_split => self.io(sprites::irq_split),
            v if v == l.irq_mux => self.io(sprites::irq_mux),
            v if v == l.irq_fb => self.irq_fb(orig),
            v => {
                if crate::menus::irq(self, v, orig) {
                    return true;
                }
                orig.irq(self, v);
            }
        }
        false
    }

    /// `$30D8..$30F7`: the block animations of the `$42` IRQ, unless the logic is frozen for good (`$20` negative) or a
    /// column step runs (`$14` positive).
    fn block_animations_42(&mut self) {
        let l = self.l;
        let step = self.peek(l.scroll_step);
        if self.peek(l.game_state) & 0x80 != 0 || (step != 0 && step & 0x80 == 0) {
            return;
        }
        let (ram, color, _) = self.parts();
        scroll::anim_3991(ram, color, l);
        scroll::anim_38f8(ram, color, l);
        scroll::anim_389d(ram, color, l);
        scroll::anim_3991(ram, color, l);
        ram[l.blocks_skip as usize] = 0;
        scroll::anim_3b3e(ram, l);
        scroll::anim_3b3e(ram, l);
    }

    /// `$31F4..$32B3` (line `$5A`): the playfield's fine scroll and the sprite split ([`sprites::irq_5a`]), then the frame
    /// logic, or the end of the column step that the previous frame started.
    fn irq_5a(&mut self, orig: &mut dyn Original) {
        let x = 0;
        let (split, _) = self.io(|io| sprites::irq_5a(io, x));
        let _ = split == Split::No;
        self.logic_and_step(orig);
    }

    /// `$323E..$32B3`: the frame logic, or the end of the column step that the previous frame started (the tail of the
    /// `$5A` handler, also of the title's `$3025` handler).
    pub fn logic_and_step(&mut self, orig: &mut dyn Original) {
        let l = self.l;
        // rows 2..4 were fetched before this line; the playfield rows are fetched while the logic runs: the upper ones
        // before the logic's block changes come in, the lower ones (from `LOGIC_DONE_ROW`, fetched after the logic's
        // time) with them; the column step's shift comes later still
        self.picture_rows(2..LOGIC_DONE_ROW);
        let step = self.peek(l.scroll_step);
        if step != 0 {
            // `$323E`: a column step runs (`$7F`): the step's second half, `$1AC3..$1AD7`, then `$14 = 0`
            self.logic(orig);
            self.picture_rows(LOGIC_DONE_ROW..25);
            if self.peek(l.blocks_skip) == 0 {
                let (ram, color, _) = self.parts();
                scroll::anim_389d(ram, color, l);
                scroll::anim_3991(ram, color, l);
                scroll::anim_38f8(ram, color, l);
                scroll::anim_3991(ram, color, l);
            }
            self.poke(l.scroll_step, 0);
            self.starved = false;
            return;
        }
        self.logic(orig);
        self.picture_rows(LOGIC_DONE_ROW..25);
        if self.peek(l.scroll_step) & 0x80 != 0 {
            // `$3249`: the column step: counters now, the shift (`$18C4`, IRQs on) runs into the next frame; the picture
            // of this frame shows the playfield before it
            let (ram, color, _) = self.parts();
            scroll::scroll_counters(ram, l);
            scroll::scroll_shift(ram, color, l);
            self.starved = true;
        }
    }

    /// `$30FE..$3182`: one frame of game logic.
    pub fn logic(&mut self, orig: &mut dyn Original) {
        let l = self.l;
        let _ = orig;
        let fc = self.peek(l.frame_counter).wrapping_add(1);
        self.poke(l.frame_counter, fc & 0x1F);
        self.poke(l.frame4, fc & 3);
        let saved: Vec<u8> = (0..4).map(|i| self.peek(l.scratch_ptr + i)).collect();
        self.read_input(0);
        if self.peek(l.game_state) != 0 {
            if self.peek(l.state_02e0) != 0 {
                let (ram, color, _) = self.parts();
                probe_if_on(ram, l);
                blocks::pickups_and_bumps(ram, color, l);
            }
        } else {
            let (ram, color, vic) = self.parts();
            probe_if_on(ram, l);
            let mid = ram[(l.probe_tiles + 4) as usize];
            if mid == 0xD9 {
                ram[l.stage_end as usize] = 1;
                ram[l.game_state as usize] = 1;
            } else {
                blocks::block_hits(ram, color, vic, l, mid);
                blocks::time_tick(ram, l);
                if player::hazards(ram, l) {
                    player::die(ram, l, 0xFF, 0);
                }
                player::vertical(ram, l);
                if let MoveEnd::Died { y, .. } = player::player_move(ram, l) {
                    player::die(ram, l, 0xFF, y);
                }
                scroll::scroll_apply(ram, l);
            }
        }
        self.fine_scroll_sprites();
        for (i, v) in saved.into_iter().enumerate() {
            self.poke(l.scratch_ptr + i as u16, v);
        }
    }

    /// `$8F95` with the CIA: `$DC00 = $FF`, then the joystick.
    fn read_input(&mut self, y: u8) {
        let l = self.l;
        let joy = self.joy;
        if player::read_input(self.ram(), l, joy, y).is_some() {
            self.cia_pra = 0xFF;
        }
    }

    /// `$3149..$3177`: the sprites of the objects and the bump sprites move with the playfield's fine scroll change of
    /// this frame (`$D016` as the `$5A` IRQ wrote it minus the new `$67`, `$FA`), with their x high bits in `$88`.
    fn fine_scroll_sprites(&mut self) {
        let l = self.l;
        let d = self.vic[0x16].wrapping_sub(self.peek(l.fine_scroll)) & 7;
        self.poke(l.fine_delta, d);
        let mut carries = 0u8;
        for x in (0..5u16).rev() {
            let a = l.object_sprite_x + x;
            let v = self.peek(a);
            let (r, borrow) = v.overflowing_sub(d);
            self.poke(a, r);
            carries = carries << 1 | (!borrow) as u8;
        }
        let msb = ((carries << 3) ^ 0xF8) ^ self.peek(l.sprite_x_msb);
        self.poke(l.sprite_x_msb, msb);
        let mut carry = true;
        for x in (0..3u16).rev() {
            let a = l.bump_x + x;
            let v = self.peek(a) as i16 - d as i16 - (!carry) as i16;
            self.poke(a, v as u8);
            carry = v >= 0;
        }
    }

    /// `$32DC..$345E` (line `$FB`): the status bar's registers, then with IRQs on the object frame, Giana's position,
    /// the sprites, Giana's frame, the tile shimmer, the effect request, the object pointers, the tile animation and the
    /// music.
    fn irq_fb(&mut self, orig: &mut dyn Original) {
        let l = self.l;
        self.io(sprites::irq_fb);
        self.objects(orig);
        // the sprite writes come after the object code: earlier when it is frozen
        let line = if self.peek(l.game_state) != 0 { SPRITES_LINE_FROZEN } else { SPRITES_LINE };
        self.raster_to(line);
        {
            let ram = self.ram();
            player::sprite_position(ram, l);
        }
        self.io(sprites::object_sprites);
        {
            let ram = self.ram();
            player::frame_select(ram, l);
            sprites::shimmer(ram, l);
        }
        let tail = sprites::fb_tail(self.ram(), l);
        match tail {
            sprites::FbTail::TileAnim => scroll::tile_anim(self.ram(), l),
            sprites::FbTail::Music => {}
            sprites::FbTail::Effect(e) => {
                self.music.start_effect(e);
                self.poke(l.effect, 0xFF);
                self.io(sprites::object_pointers);
                scroll::tile_anim(self.ram(), l);
            }
            sprites::FbTail::Pointers(_) => {
                self.io(sprites::object_pointers);
                scroll::tile_anim(self.ram(), l);
            }
        }
        self.music.tick();
    }

    /// `$72EA`: the object frame (in the `$FB` IRQ, with IRQs on).
    pub fn objects(&mut self, orig: &mut dyn Original) {
        let l = self.l;
        let flash = self.peek(l.power_flash);
        if flash != 0 {
            // `$72CC`: a frame of the power-up flash (package O3)
            self.call(orig, l.flash_step, Regs { a: flash, x: 0, y: flash });
            return;
        }
        if self.peek(l.scroll_step) != 0 {
            // a column step runs
            self.raster_to(COLLISION_LINE - 2);
            self.call(orig, l.step_contact, Regs { a: 0, x: 0, y: 0 });
            if self.peek(l.on_object) != 0 {
                let (ram, _, _) = self.parts();
                ram[l.update_bits as usize] = 0;
                objects::probe_clear(ram, l, 0);
                let x = ram[l.land_slot as usize];
                let (ram, _, vic) = self.parts();
                let end = classes::moving_class(ram, vic, l, x);
                self.class_continue(end, x, orig);
                let vic = self.vic;
                let x = self.peek(l.land_slot);
                let x = objects::ride(self.ram(), &vic, l, x);
                self.poke(l.ride_slot, x);
            }
            self.call(orig, l.shot_move, Regs { a: 0, x: 0, y: 0 });
            return;
        }
        if self.peek(l.layer_hires) != 0 {
            // `$733E`: Giana flashes (power-up change)
            let c = self.peek(l.flash_colours + self.peek(l.frame_counter) as u16);
            self.poke(l.sprite_color, c);
            self.vic[0x28] = c;
            return;
        }
        crate::flow::keys(self, orig);
        if self.peek(l.in_room) != 0 {
            return;
        }
        if self.peek(l.probe_skip_bottom) != 0 {
            objects::lift(self.ram(), l);
        }
        let state = self.peek(l.game_state);
        if state & 0x80 != 0 {
            return;
        }
        if state == 0 {
            let (ram, _, vic) = self.parts();
            objects::spawn(ram, vic, l);
            self.raster_to(COLLISION_LINE - 1);
            self.call(orig, l.contact, Regs { a: 0, x: 0, y: 0 });
        }
        self.update_loop(orig);
        // `$8521` (package O4) leaves `X` = 3, which `$81C6` reads
        let r = self.call(orig, l.group_place, Regs { a: 0, x: 0, y: 0 });
        let vic = self.vic;
        objects::ride(self.ram(), &vic, l, r.x);
        objects::despawn(self.ram(), l);
        self.call(orig, l.shot_fire, Regs { a: 0, x: 0, y: 0 });
        self.call(orig, l.shot_move, Regs { a: 0, x: 0, y: 0 });
    }

    /// `$7613`: the update loop over the object slots 7..4.
    fn update_loop(&mut self, orig: &mut dyn Original) {
        let l = self.l;
        let a = 0;
        let d = objects::dispatch(self.ram(), l, None, a);
        self.dispatch_loop(d, orig);
    }

    fn dispatch_loop(&mut self, mut d: Dispatch, orig: &mut dyn Original) {
        let l = self.l;
        while let Dispatch::Run { route, x, .. } = d {
            let (ram, _, vic) = self.parts();
            let end = classes::run(ram, vic, l, route, x);
            match self.class_end(end, x, orig) {
                Some(next) => d = next,
                None => return,
            }
        }
    }

    /// Where the update loop goes on after the class code of slot `x` ended with `end`; `None` = the loop is over.
    fn class_end(&mut self, end: ClassEnd, x: u8, orig: &mut dyn Original) -> Option<Dispatch> {
        let l = self.l;
        let mut x = x;
        match end {
            ClassEnd::Return => return None,
            ClassEnd::Next => {}
            ClassEnd::Probe | ClassEnd::Tail => {
                if end == ClassEnd::Probe {
                    objects::animate(self.ram(), l, x);
                }
                let (ram, _, vic) = self.parts();
                if let (TailEnd::Kill { a }, y) = objects::update_tail(ram, vic, l, x) {
                    let r = self.call(orig, l.obj_hit, Regs { a, x, y: y.unwrap_or(0) });
                    x = r.x;
                }
            }
        }
        Some(objects::dispatch(self.ram(), l, Some(x), 0))
    }

    /// The rest of the update loop after class code that was entered by `JSR` (`$7303`).
    fn class_continue(&mut self, end: ClassEnd, x: u8, orig: &mut dyn Original) {
        if let Some(d) = self.class_end(end, x, orig) {
            self.dispatch_loop(d, orig);
        }
    }

    /// A routine of the object frame by address: the native ports of packages O3 (`pickups`) and O4 (`bosses`), with
    /// the registers as the original's `RTS` leaves them for the caller; any other address goes to the fallback (see
    /// [`Original`]). The two routines whose first instruction reads `$D01E` take the sprite unit's latch and clear it,
    /// as the read does.
    pub fn call(&mut self, orig: &mut dyn Original, addr: u16, regs: Regs) -> Regs {
        let l = self.l;
        let coll = |c: &mut Core| std::mem::take(&mut c.unit.d01e);
        match addr {
            a if a == l.contact => {
                let coll = coll(self);
                let (ram, _, vic) = self.parts();
                match pickups::contact(ram, vic, l, coll) {
                    ContactEnd::Return => regs,
                    // `JMP $3F26`: the death, whose `RTS` returns to the object frame
                    ContactEnd::Die { y, .. } => player::die(ram, l, 0xFF, y),
                }
            }
            a if a == l.step_contact => {
                let coll = coll(self);
                let (ram, _, vic) = self.parts();
                pickups::step_contact(ram, vic, l, coll);
                regs
            }
            a if a == l.shot_fire => {
                let (ram, _, vic) = self.parts();
                pickups::shot_fire(ram, vic, l);
                regs
            }
            a if a == l.shot_move => {
                let (ram, _, vic) = self.parts();
                pickups::shot_move(ram, vic, l);
                regs
            }
            a if a == l.flash_step => {
                let (ram, _, vic) = self.parts();
                pickups::power_flash(ram, vic, l, regs.y);
                regs
            }
            a if a == l.obj_hit => {
                pickups::hit(self.ram(), l, regs.x);
                regs
            }
            a if a == l.group_place => {
                let r = bosses::place_groups(self.ram(), l, regs.y);
                Regs { a: r.a, x: r.x, y: r.y }
            }
            _ => orig.call(self, addr, regs),
        }
    }
}

/// `JSR $8E9F`: the tile probe, unless the title has turned it off by writing an `RTS` over its first byte.
fn probe_if_on(ram: &mut [u8], l: &C30aLayout) {
    if ram[l.probe as usize] != 0x60 {
        player::probe(ram, l);
    }
}
