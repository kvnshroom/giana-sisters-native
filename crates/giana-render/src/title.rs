//! Pixels of the title sequence: turns the raster lines of `giana_core::title::TitleScreen` into a picture of C64 colour
//! indices (384 x 272). Text, multicolour text and standard / multicolour bitmap lines, the colour changes of `$D021` inside
//! a line at the pixel of their write cycle (the raster bars), and the "grey dot" the 6569 shows for one pixel when a colour
//! register is written.

use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use crate::palette::c64_color;
use giana_core::title::{x_of_cycle, LineState, TitleAssets, TitleScreen};
use giana_core::title_sound::{TimedWrite, TitleSound};
use std::cell::RefCell;
use std::rc::Rc;

/// `$D021` at picture pixel `x`.
fn d021_at(st: &LineState, x: i32) -> u8 {
    let mut v = st.d021_start & 0x0F;
    for &(c, val) in st.d021_events {
        if x >= x_of_cycle(c) {
            v = val;
            if x == x_of_cycle(c) {
                return 15;
            }
        }
    }
    v
}

/// Draws one raster line (`out` is 384 colour indices).
pub fn draw_line(st: &LineState, out: &mut [u8]) {
    let ecm = st.d011 & 0x40 != 0;
    let bmm = st.d011 & 0x20 != 0;
    let mcm = st.d016 & 0x10 != 0;
    let xscroll = (st.d016 & 7) as usize;
    let cols40 = st.d016 & 8 != 0;
    // colour index per pixel, 0x10 = background colour
    let mut row = [0x10u8; WIDTH];
    let in_window = !st.vborder;
    let ram = |a: usize| st.ram.get(a).copied().unwrap_or(0);
    if in_window {
        let base_x = 32 + xscroll;
        if st.fetch {
            let cb = ((st.d018 as usize >> 1) & 7) << 11;
            for i in 0..40 {
                let ch = st.matrix[i] as usize;
                let col = st.colors[i];
                let rc = st.rc as usize;
                let byte = if bmm {
                    let bm = ((st.d018 as usize >> 3) & 1) << 13;
                    ram(bm + (((st.vc as usize + i) & 0x3FF) << 3) + rc)
                } else {
                    ram(cb + ((if ecm { ch & 0x3F } else { ch }) << 3) + rc)
                };
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
                                }
                            }
                        }
                    } else {
                        for k in 0..8 {
                            let x = x0 + k;
                            if x < WIDTH {
                                row[x] = if (byte >> (7 - k)) & 1 == 1 { ch as u8 >> 4 } else { ch as u8 & 15 };
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
                            }
                        }
                    }
                } else {
                    for k in 0..8 {
                        let x = x0 + k;
                        if x < WIDTH {
                            row[x] = if (byte >> (7 - k)) & 1 == 1 {
                                col
                            } else if ecm {
                                0x10 + (ch >> 6) as u8
                            } else {
                                0x10
                            };
                        }
                    }
                }
            }
        } else {
            // idle state: the byte at $3FFF (or $39FF) as black pixels
            let byte = ram(if ecm { 0x39FF } else { 0x3FFF });
            for i in 0..40 * 8 {
                let bit = if mcm && bmm { (byte >> (6 - 2 * ((i % 8) / 2))) & 2 != 0 } else { (byte >> (7 - i % 8)) & 1 != 0 };
                let x = base_x + i;
                if x < WIDTH {
                    row[x] = if bit { 0 } else { 0x10 };
                }
            }
        }
        let (lx, rx) = if cols40 { (32, 352) } else { (39, 345) };
        for (x, p) in row.iter_mut().enumerate() {
            if x < lx || x >= rx {
                *p = 0x10;
            }
        }
    }
    let (lx, rx) = if cols40 { (32, 352) } else { (39, 345) };
    for x in 0..WIDTH {
        out[x] = match row[x] {
            0x10 => d021_at(st, x as i32),
            0x11 | 0x12 | 0x13 => 0,
            c => c,
        };
        if st.vborder || x < lx || x >= rx {
            out[x] = st.border;
        }
    }
}

/// The title sequence with its picture.
pub struct TitlePlayer {
    pub screen: TitleScreen,
    pic: Rc<RefCell<Vec<u8>>>,
    /// the music of the title picture (see `attach_music`), started when the picture appears
    music: Option<TitleSound>,
    music_started: bool,
    sid_writes: Vec<TimedWrite>,
}

impl TitlePlayer {
    pub fn new(assets: TitleAssets, koala: &[u8]) -> Self {
        let pic = Rc::new(RefCell::new(vec![0u8; WIDTH * HEIGHT]));
        let p2 = pic.clone();
        let draw: Box<dyn FnMut(usize, &LineState)> = Box::new(move |y, st| {
            let mut p = p2.borrow_mut();
            draw_line(st, &mut p[y * WIDTH..(y + 1) * WIDTH]);
        });
        TitlePlayer { screen: TitleScreen::new(assets, koala, draw), pic, music: None, music_started: false, sid_writes: Vec::new() }
    }

    pub fn run_frame(&mut self) {
        self.screen.run_frame();
        self.sid_writes.clear();
        if let Some(m) = self.music.as_mut() {
            let part = self.screen.part();
            if part == 2 {
                self.music_started = true;
            }
            if self.music_started {
                // part 0 again: the program has left the picture and stops the music
                self.sid_writes = if part == 0 || self.screen.done() { m.silence() } else { m.run_frame() };
            }
        }
    }

    /// Gives the title picture its music and sample (`setup_memory` is the title program after its set-up, see
    /// `giana_core::title_sound`). They start when the picture appears.
    pub fn attach_music(&mut self, setup_memory: &[u8]) {
        self.music = Some(TitleSound::new(setup_memory));
        self.music_started = false;
    }

    /// The SID writes of the last frame (cycle in the frame, register, value); empty before and after the music.
    pub fn take_sid_writes(&mut self) -> Vec<TimedWrite> {
        std::mem::take(&mut self.sid_writes)
    }

    /// The SID registers `$D400..$D418` the music has left.
    pub fn sid_regs(&self) -> [u8; 25] {
        self.music.as_ref().map_or([0; 25], |m| m.regs)
    }

    /// The picture of the last frame: 384 x 272 C64 colour indices.
    pub fn picture(&self) -> Vec<u8> {
        self.pic.borrow().clone()
    }

    /// Blacks the picture out (the program is loading).
    pub fn blank(&mut self) {
        self.pic.borrow_mut().fill(0);
    }

    /// The picture as ARGB pixels.
    pub fn draw_to(&self, fb: &mut Framebuffer) {
        for (d, &c) in fb.pixels.iter_mut().zip(self.pic.borrow().iter()) {
            *d = c64_color(c);
        }
    }
}
