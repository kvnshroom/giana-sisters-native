//! The native 30A preview as one object for the app (package I, `docs/rom-hack-family-c.md` 7.9): power-on from the
//! player's disk image, then one [`C30Game::step`] per video frame (50 Hz) with the input, giving the picture, the SID
//! registers and the driver calls of the frame.
//!
//! ```ignore
//! let image = Image::from_d64(&d64)?;
//! let mut game = C30Game::new(&image, store.load());
//! loop {
//!     let out = game.step(&Input { joy: pressed_bits, ..Input::default() });
//!     out.render(&mut framebuffer);          // 384 x 272, giana-render
//!     audio.live_regs(out.sid);              // the 25 SID registers of the frame
//!     if let Some(bytes) = game.take_save() { store.save(&bytes) }
//! }
//! ```
//!
//! The routines of packages O3 and O4 (contacts, pickups, the shot, bosses) are native ([`crate::frame::Core::call`]):
//! [`C30Game::new`] needs no fallback for play; [`C30Game::with_original`] takes one for the tests (every call of it
//! is counted there; [`NoOriginal`] panics when it is called).

use crate::frame::{Core, NoOriginal, Original};
use crate::layout::{C30aLayout, PREVIEW};
use crate::music::DriverCall;
use crate::render::FrameState;
use crate::sprites::{SpriteDma, SpriteRow};
use crate::{flow, Image};
use giana_render::framebuffer::Framebuffer;

/// The input of a frame.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// joystick port 2, pressed bits: 1 up, 2 down, 4 left, 8 right, `$10` fire
    pub joy: u8,
    /// keys held, as C64 key matrix positions `(column, row)` (`1` = (7, 0), `2` = (7, 3), `H` = (3, 5), `P` = (5, 1),
    /// RUN/STOP = (7, 7), space = (7, 4))
    pub keys: Vec<(u8, u8)>,
    /// PETSCII characters typed this frame (the high-score name entry: letters, digits, DEL `$14`, RETURN `$0D`)
    pub typed: Vec<u8>,
}

/// The output of a frame.
pub struct Frame<'a> {
    /// the picture's memory: RAM with the text rows as the VIC fetched them, colour RAM, VIC registers of the frame start
    pub ram: Vec<u8>,
    pub color: Vec<u8>,
    pub vic: [u8; 47],
    /// sprite rows each raster line showed (empty: draw the sprites from `vic`)
    pub sprite_rows: &'a [SpriteRow],
    /// raster bars: `$D021` per raster line `(line, colour)` (menus, title, high-score screen)
    pub bars: &'a [(u16, u8)],
    /// the VIC registers of each raster line 0..311 of the picture (for [`FrameState::render_screen`]; empty when
    /// `screen` is not set)
    pub line_regs: Vec<[u8; 47]>,
    /// a screen of the game flow (menus, text pages, title, high-score screen; no play chain): drawn line by line
    /// ([`FrameState::render_screen`])
    pub screen: bool,
    /// the SID registers `$D400..$D418` at the end of the frame
    pub sid: [u8; 25],
    /// the music driver's calls of the frame (song and effect starts, ticks), when [`C30Game::record_calls`] is on
    pub calls: Vec<DriverCall>,
}

impl<'a> Frame<'a> {
    /// The frame's picture as an owned [`Shot`].
    pub fn shot(&self) -> Shot {
        Shot {
            ram: self.ram.clone(),
            color: self.color.clone(),
            vic: self.vic,
            sprite_rows: self.sprite_rows.to_vec(),
            line_regs: self.line_regs.clone(),
            screen: self.screen,
        }
    }

    /// The picture (and sound) of the frame `core` has just run ([`Core::run_frame`]): the text rows as fetched (the
    /// status rows 0-1 before the frame's start, [`crate::frame::Shown::top`]), the
    /// character set of the last text line, the fine scrolls and VIC registers of the frame start, the sprite rows; a
    /// screen of the game flow gets its registers line by line.
    pub fn of(c: &'a mut Core) -> Frame<'a> {
        let calls = c.music.driver.record.as_mut().map(std::mem::take).unwrap_or_default();
        let mut ram = c.music.driver.mem.clone();
        let mut color = c.color.clone();
        let screen = c.l.screen as usize;
        for r in 0..25 {
            let (s, k) = if r < 2 { (&c.shown.top[0][r], &c.shown.top[1][r]) } else { (&c.shown.screen[r], &c.shown.color[r]) };
            ram[screen + r * 40..screen + r * 40 + 40].copy_from_slice(s);
            color[r * 40..r * 40 + 40].copy_from_slice(k);
        }
        let cs = c.l.charset as usize;
        ram[cs..cs + 2048].copy_from_slice(&c.shown.chars[..]);
        let fs = c.l.fine_scroll as usize;
        ram[fs..fs + 2].copy_from_slice(&c.shown.scroll);
        let mut sid = [0u8; 25];
        sid.copy_from_slice(&c.music.driver.sid[..25]);
        let l = c.l;
        let play = c.handled.iter().any(|&(_, v)| [l.irq_41, l.irq_42, l.irq_5a, l.irq_fb].contains(&v));
        let screen = c.screen_frame || !play;
        let line_regs = if screen { c.picture_regs() } else { Vec::new() };
        Frame { ram, color, vic: c.shown.vic, sprite_rows: &c.unit.rows, bars: &c.bars, line_regs, screen, sid, calls }
    }

    /// The frame for the renderer of [`crate::render`].
    pub fn state(&self) -> FrameState<'_> {
        FrameState { ram: &self.ram, vic: self.vic, color: &self.color }
    }

    /// Draws the frame (384 x 272 with border).
    pub fn render(&self, fb: &mut Framebuffer) {
        let st = self.state();
        if self.screen {
            st.render_screen(fb, &self.line_regs, self.sprite_rows);
        } else if self.sprite_rows.is_empty() {
            st.render(fb);
        } else {
            st.render_rows(fb, self.sprite_rows);
        }
    }
}

/// A frame's picture that the caller keeps (owned): for the picture between two frames on a display that is not a
/// multiple of 50 Hz ([`Shot::render_between`]).
#[derive(Clone)]
pub struct Shot {
    pub ram: Vec<u8>,
    pub color: Vec<u8>,
    pub vic: [u8; 47],
    pub sprite_rows: Vec<SpriteRow>,
    pub line_regs: Vec<[u8; 47]>,
    pub screen: bool,
}

/// The largest move (pixels) of the playfield or a sprite between two frames that is drawn in between; anything
/// further (a jump, a sprite taken for another object, a new screen) is drawn as it is.
pub const MAX_STEP: isize = 8;

impl Shot {
    fn state(&self) -> FrameState<'_> {
        FrameState { ram: &self.ram, vic: self.vic, color: &self.color }
    }

    /// The same picture as [`Frame::render`].
    pub fn render(&self, fb: &mut Framebuffer) {
        let st = self.state();
        if self.screen {
            st.render_screen(fb, &self.line_regs, &self.sprite_rows);
        } else if self.sprite_rows.is_empty() {
            st.render(fb);
        } else {
            st.render_rows(fb, &self.sprite_rows);
        }
    }

    /// The picture at `t` (0..1) of the way from `prev` to `curr` (the frame after it): the playfield and the cloud
    /// band move by their share of the way (fine scroll and column step together; the status bar stays), each sprite
    /// by its share (when it moved at most [`MAX_STEP`] pixels). `t >= 1`, a screen of the game flow, or a step that
    /// cannot be told (a new screen, a scroll of more than [`MAX_STEP`]) give [`Shot::render`] of `curr`.
    pub fn render_between(prev: &Shot, curr: &Shot, t: f64, fb: &mut Framebuffer) {
        let still = t >= 1.0 || prev.screen || curr.screen;
        let play = (!still).then(|| band_move(prev, curr, 5..25, 0x67)).flatten();
        let sky = (!still).then(|| band_move(prev, curr, 2..5, 0x68)).flatten().unwrap_or(0);
        let Some(play) = play else {
            return curr.render(fb);
        };
        let back = |d: isize| (-(1.0 - t) * d as f64).round() as isize;
        let (play_off, sky_off) = (back(play), back(sky));
        let mut sprite_shift = [(0isize, 0isize); 8];
        for (s, sh) in sprite_shift.iter_mut().enumerate() {
            if let (Some(a), Some(b)) = (sprite_at(prev, s), sprite_at(curr, s)) {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                if dx.abs() <= MAX_STEP && dy.abs() <= MAX_STEP {
                    *sh = (back(dx), back(dy));
                }
            }
        }
        if play_off == 0 && sky_off == 0 && sprite_shift.iter().all(|&s| s == (0, 0)) {
            return curr.render(fb);
        }
        let st = curr.state();
        let regs = st.play_regs();
        let rows;
        let rows = if curr.sprite_rows.is_empty() {
            rows = st.sprite_rows();
            &rows[..]
        } else {
            &curr.sprite_rows[..]
        };
        let shift = |raster: u16| match raster as isize - 51 {
            y if y < 16 => 0,
            y if y < 39 => sky_off,
            _ => play_off,
        };
        st.render_lines(fb, &regs, rows, &shift, &sprite_shift);
    }
}

/// How far (pixels, positive = right) the text rows `rows` moved from `prev` to `curr`: the fine scroll at `scroll`
/// (`$67`, or `$68` whose bits 1..3 are the cloud band's) and a column step found in the rows (the cells of `curr`
/// equal those of `prev` one column on: -8); `None` when the rows match no step.
fn band_move(prev: &Shot, curr: &Shot, rows: std::ops::Range<usize>, scroll: usize) -> Option<isize> {
    let fine = |s: &Shot| if scroll == 0x68 { ((s.ram[scroll] >> 1) & 7) as isize } else { (s.ram[scroll] & 7) as isize };
    let cell = |s: &Shot, r: usize, c: usize| (s.ram[0xC800 + r * 40 + c], s.color[r * 40 + c]);
    let mut best = (0usize, 0isize, 0usize);
    for k in [0isize, 1, -1] {
        let (mut same, mut total) = (0, 0);
        for r in rows.clone() {
            for c in 1..39usize {
                let pc = (c as isize + k) as usize;
                // cells that differ from their neighbour tell a step from none
                if cell(prev, r, pc) != cell(prev, r, pc - 1) || cell(prev, r, pc) != cell(prev, r, pc + 1) {
                    total += 1;
                    same += (cell(curr, r, c) == cell(prev, r, pc)) as usize;
                }
            }
        }
        if same > best.0 {
            best = (same, k, total);
        }
    }
    let (same, k, total) = best;
    if total > 0 && same * 2 < total {
        return None;
    }
    let d = fine(curr) - fine(prev) - 8 * k;
    (d.abs() <= MAX_STEP).then_some(d)
}

/// Where sprite `s` is in the picture (x of its first row, its first raster line), if it is shown.
fn sprite_at(shot: &Shot, s: usize) -> Option<(isize, isize)> {
    let rows;
    let rows = if shot.sprite_rows.is_empty() {
        rows = shot.state().sprite_rows();
        &rows[..]
    } else {
        &shot.sprite_rows[..]
    };
    rows.iter().filter(|r| r.sprite as usize == s).min_by_key(|r| r.line).map(|r| (r.x as isize, r.line as isize))
}

/// The native game.
pub struct C30Game {
    pub core: Core,
    pub main: flow::Main,
    orig: Box<dyn Original>,
}

impl C30Game {
    /// Power-on: the game's start (`$1D10`: the intro, then the title), with the high-score file the app kept
    /// (`None`: the tables of the disk image).
    pub fn new(image: &Image, scores: Option<Vec<u8>>) -> C30Game {
        C30Game::with_original(image, scores, Box::new(NoOriginal))
    }

    /// [`C30Game::new`] with a fallback for routines that have no native port yet.
    pub fn with_original(image: &Image, scores: Option<Vec<u8>>, orig: Box<dyn Original>) -> C30Game {
        let l: &'static C30aLayout = &PREVIEW;
        let mut core = Core::new(l, image.ram.clone(), &[0; 1024], &[0; 47], &[0; 25], [SpriteDma::default(); 8], 0x41);
        core.unit.record = true;
        let mut main = flow::Main::power_on(scores);
        main.pc = flow::start_pc(&core);
        C30Game { core, main, orig }
    }

    /// Keeps the music driver's calls for [`Frame::calls`] (for an audio layer that replays them on its own SID).
    pub fn record_calls(&mut self, on: bool) {
        self.core.music.driver.record = on.then(Vec::new);
    }

    /// One video frame.
    pub fn step(&mut self, input: &Input) -> Frame<'_> {
        let c = &mut self.core;
        c.joy = !input.joy | 0xE0;
        c.keys = [0xFF; 8];
        for &(col, row) in &input.keys {
            c.keys[col as usize & 7] &= !(1 << (row & 7));
        }
        c.typed.extend(input.typed.iter().copied());
        c.held_key = match input.keys.first() {
            Some(&(7, 4)) => 0x3C,
            Some(&(0, 4)) => 0x04,
            Some(_) => 0x3F,
            None => 0x40,
        };
        c.run_frame(&mut self.main, self.orig.as_mut());
        Frame::of(c)
    }

    /// The high-score file to keep, when the game has just saved it (the high-score screen's up, the pause menu's save).
    pub fn take_save(&mut self) -> Option<Vec<u8>> {
        self.main.save.take()
    }
}
