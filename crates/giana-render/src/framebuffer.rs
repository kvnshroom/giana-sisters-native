use crate::palette::c64_color;

pub const WIDTH: usize = 384;
pub const HEIGHT: usize = 272;

pub const BORDER_X: usize = 40;
pub const BORDER_Y: usize = 40;
pub const ACTIVE_WIDTH: usize = 320;
pub const VISIBLE_WIDTH: usize = 304;
pub const ACTIVE_HEIGHT: usize = 200;

/// The title picture of the original: a multicolour bitmap in Koala layout (8000 bytes bitmap, 1000 screen RAM,
/// 1000 colour RAM, background colour), cut out of the decrunched title program by `giana-extract`.
pub fn title_koala() -> &'static [u8] {
    &giana_core::data::get().title_koala
}

/// C64 colour indices (320x200) of the title picture, as the original shows it: the picture is one raster line higher
/// than the bitmap rows, the last line is black.
fn title_indices() -> &'static [u8] {
    static PIXELS: giana_core::data::Cache<Vec<u8>> = giana_core::data::Cache::new(None);
    giana_core::data::cached(&PIXELS, || {
        let (bitmap, rest) = title_koala().split_at(8000);
        let (screen, rest) = rest.split_at(1000);
        let (color, rest) = rest.split_at(1000);
        let background = rest[0] & 15;
        let mut out = vec![0u8; ACTIVE_WIDTH * ACTIVE_HEIGHT];
        for y in 1..ACTIVE_HEIGHT {
            for x in 0..ACTIVE_WIDTH {
                let cell = (y / 8) * 40 + x / 8;
                let bits = (bitmap[cell * 8 + y % 8] >> (6 - 2 * ((x % 8) / 2))) & 3;
                out[(y - 1) * ACTIVE_WIDTH + x] = match bits {
                    0 => background,
                    1 => screen[cell] >> 4,
                    2 => screen[cell] & 15,
                    _ => color[cell] & 15,
                };
            }
        }
        out
    }).as_slice()
}

/// Linear 32-bit ARGB/XRGB virtual framebuffer.
#[derive(Clone)]
pub struct Framebuffer {
    pub pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0xFF000000; WIDTH * HEIGHT],
        }
    }

    #[inline(always)]
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < WIDTH && y < HEIGHT {
            self.pixels[y * WIDTH + x] = color;
        }
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < WIDTH && y < HEIGHT {
            self.pixels[y * WIDTH + x]
        } else {
            0xFF000000
        }
    }

    /// Draws the authentic 320x200 Title Screen centered within the 384x272 C64 PAL frame.
    pub fn draw_title_screen(&mut self) {
        // Border color is black (0) for title screen
        self.clear(c64_color(0));

        let src_w = ACTIVE_WIDTH;
        let src_h = ACTIVE_HEIGHT;

        for y in 0..src_h {
            let dy = crate::vic::ORIGIN_Y + y;
            let src_row = y * src_w;
            for x in 0..src_w {
                let dx = crate::vic::ORIGIN_X + x;
                let color_idx = title_indices()[src_row + x];
                self.set_pixel(dx, dy, c64_color(color_idx));
            }
        }
    }

    /// The intro screen of the old recordings is gone (it was the cracker's rasterbar logo, not part of the game and
    /// never reached by the app); `GameScreen::Intro` shows the title picture.
    pub fn draw_intro_screen(&mut self) {
        self.draw_title_screen();
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}
