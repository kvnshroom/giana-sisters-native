//! Pixels of the high score screen: `giana_core::highscore::Hs` runs the original routines beam by beam and hands over the
//! raster lines; `title::draw_line` turns them into colour indices (384 x 272), the colour bars included.

use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use crate::palette::c64_color;
use crate::title::draw_line;
use giana_core::highscore::{Hs, Input, VicState};
use giana_core::highscore_game::{GameOver, Tables};
use giana_core::title::LineState;
use std::cell::RefCell;
use std::rc::Rc;

pub struct HighScorePlayer {
    pub hs: Hs,
    pic: Rc<RefCell<Vec<u8>>>,
}

impl HighScorePlayer {
    /// Starts the screen. `backdrop` is the picture that is on screen until the first raster lines are drawn (the lines
    /// of the frame the game over code was in): 384 x 272 colour indices, or `None` for black.
    pub fn enter(ram: Vec<u8>, color: Vec<u8>, vic: VicState, cia2_0: u8, backdrop: Option<&[u8]>) -> Self {
        let pic = Rc::new(RefCell::new(match backdrop {
            Some(b) if b.len() == WIDTH * HEIGHT => b.to_vec(),
            _ => vec![0u8; WIDTH * HEIGHT],
        }));
        let p2 = pic.clone();
        let draw: Box<dyn FnMut(usize, &LineState)> = Box::new(move |y, st| {
            let mut p = p2.borrow_mut();
            draw_line(st, &mut p[y * WIDTH..(y + 1) * WIDTH]);
        });
        HighScorePlayer { hs: Hs::enter(ram, color, vic, cia2_0, draw), pic }
    }

    /// Game over in the port: the screen from the state of the game (see `giana_core::highscore_game`).
    pub fn begin(over: &GameOver, tables: &Tables) -> Self {
        let pic = Rc::new(RefCell::new(vec![0u8; WIDTH * HEIGHT]));
        let p2 = pic.clone();
        let draw: Box<dyn FnMut(usize, &LineState)> = Box::new(move |y, st| {
            let mut p = p2.borrow_mut();
            draw_line(st, &mut p[y * WIDTH..(y + 1) * WIDTH]);
        });
        HighScorePlayer { hs: Hs::begin(over, tables, draw), pic }
    }

    /// Runs one frame with the keys and the joystick of this frame. Returns false when the screen is over (or stopped).
    pub fn run_frame(&mut self, input: Input) -> bool {
        self.hs.set_input(input);
        self.hs.run_frame()
    }

    /// The picture of the last frame: 384 x 272 C64 colour indices.
    pub fn picture(&self) -> Vec<u8> {
        self.pic.borrow().clone()
    }

    pub fn draw_to(&self, fb: &mut Framebuffer) {
        for (d, &c) in fb.pixels.iter_mut().zip(self.pic.borrow().iter()) {
            *d = c64_color(c);
        }
    }
}
