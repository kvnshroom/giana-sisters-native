//! Text on the 40 x 25 character grid, drawn with the letters of the game's own character set. Before the original
//! data is there (the first start), letters and digits come from our own font in `fallback_font`.

use crate::fallback_font;
use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_core::data;
use giana_core::image::img;
use std::sync::OnceLock;

const CHARSET: usize = 0xC000;
/// Screen position of character cell (0, 0) in the frame buffer.
const ORIGIN_X: usize = 32;
const ORIGIN_Y: usize = 36;

/// The game has letters and digits only.
fn extra_glyph(c: char) -> Option<[u8; 8]> {
    Some(match c {
        '-' => [0, 0, 0, 0x7E, 0, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0, 0x7E],
        ':' => [0, 0x18, 0x18, 0, 0, 0x18, 0x18, 0],
        '>' => [0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0],
        '/' => [0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0],
        '.' => [0, 0, 0, 0, 0, 0x18, 0x18, 0],
        ',' => [0, 0, 0, 0, 0, 0x18, 0x18, 0x30],
        '\'' => [0x18, 0x18, 0x30, 0, 0, 0, 0, 0],
        '+' => [0, 0x18, 0x18, 0x7E, 0x18, 0x18, 0, 0],
        '=' => [0, 0, 0x7E, 0, 0x7E, 0, 0, 0],
        '*' => [0, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0, 0],
        '[' => [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0],
        ']' => [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0],
        '!' => [0x18, 0x18, 0x18, 0x18, 0x18, 0, 0x18, 0],
        '?' => [0x3C, 0x66, 0x06, 0x0C, 0x18, 0, 0x18, 0],
        '(' => [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0],
        ')' => [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0],
        ';' => [0, 0x18, 0x18, 0, 0, 0x18, 0x18, 0x30],
        '<' => [0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0],
        '#' => [0x24, 0x7E, 0x24, 0x24, 0x7E, 0x24, 0, 0],
        // Box lines, 2 pixels wide, meeting in the middle of the cell (rows 3 and 4)
        '─' => [0, 0, 0, 0xFF, 0xFF, 0, 0, 0],
        '│' => [0x18; 8],
        '┌' => box_line(0, 0x1F, 0x18),
        '┐' => box_line(0, 0xF8, 0x18),
        '└' => box_line(0x18, 0x1F, 0),
        '┘' => box_line(0x18, 0xF8, 0),
        '├' => box_line(0x18, 0x1F, 0x18),
        '┤' => box_line(0x18, 0xF8, 0x18),
        // Arrows and value markers
        '▶' => [0x10, 0x18, 0x1C, 0x1E, 0x1C, 0x18, 0x10, 0],
        '◀' => [0x08, 0x18, 0x38, 0x78, 0x38, 0x18, 0x08, 0],
        // Volume bar cells: filled and empty
        '■' => [0, 0x7E, 0x7E, 0x7E, 0x7E, 0x7E, 0x7E, 0],
        '□' => [0, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x7E, 0],
        '█' => [0xFF; 8],
        // A small five-pointed star: the favorite mark of the ROM modules page
        '★' => [0x18, 0x18, 0xFF, 0x7E, 0x3C, 0x7E, 0x66, 0x42],
        _ => return None,
    })
}

/// A box glyph: the same row pattern for the top 3, middle 2 and bottom 3 pixel rows.
const fn box_line(top: u8, mid: u8, bottom: u8) -> [u8; 8] {
    [top, top, top, mid, mid, bottom, bottom, bottom]
}

/// True when the game's character set can be used: the data is installed, or (tests, tools) can be found.
fn original_charset() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    data::is_installed() || *FOUND.get_or_init(data::available)
}

fn glyph(c: char) -> [u8; 8] {
    if !original_charset() {
        if let Some(g) = fallback_font::glyph(c.to_ascii_uppercase()) {
            return g;
        }
    }
    let code = match c.to_ascii_uppercase() {
        l @ 'A'..='Z' => Some(l as usize - 'A' as usize + 1),
        d @ '0'..='9' => Some(d as usize - '0' as usize + 0x30),
        _ => None,
    };
    match code {
        Some(code) => std::array::from_fn(|i| img(CHARSET + code * 8 + i)),
        None => extra_glyph(c).unwrap_or([0; 8]),
    }
}

/// Fill whole character cells, columns `col..col + w` of rows `row..row + h`.
pub fn fill_cells(fb: &mut Framebuffer, col: usize, row: usize, w: usize, h: usize, color: u32) {
    for y in 0..h * 8 {
        for x in 0..w * 8 {
            fb.set_pixel(ORIGIN_X + col * 8 + x, ORIGIN_Y + row * 8 + y, color);
        }
    }
}

/// Fill a rectangle given in frame-buffer pixels (clipped to the frame).
pub fn fill_rect_px(fb: &mut Framebuffer, x: isize, y: isize, w: usize, h: usize, color: u32) {
    for j in 0..h as isize {
        for i in 0..w as isize {
            put_px(fb, x + i, y + j, color);
        }
    }
}

fn put_px(fb: &mut Framebuffer, x: isize, y: isize, color: u32) {
    if x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT {
        fb.set_pixel(x as usize, y as usize, color);
    }
}

/// Draw text with its top left corner at a frame-buffer pixel (not tied to the character grid).
pub fn draw_text_px(fb: &mut Framebuffer, x: isize, y: isize, text: &str, color: u32) {
    for (i, c) in text.chars().enumerate() {
        let g = glyph(c);
        for (gy, bits) in g.iter().enumerate() {
            for gx in 0..8 {
                if bits >> (7 - gx) & 1 != 0 {
                    put_px(fb, x + (i * 8 + gx) as isize, y + gy as isize, color);
                }
            }
        }
    }
}

/// Top left pixel of character cell (col, row).
pub fn cell_px(col: usize, row: usize) -> (isize, isize) {
    ((ORIGIN_X + col * 8) as isize, (ORIGIN_Y + row * 8) as isize)
}

pub fn draw_text(fb: &mut Framebuffer, col: usize, row: usize, text: &str, color: u32) {
    let (x, y) = cell_px(col, row);
    draw_text_px(fb, x, y, text, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_star_is_a_glyph_of_its_own_and_symmetric() {
        let star = extra_glyph('\u{2605}').expect("star glyph");
        assert_ne!(star, extra_glyph('*').unwrap());
        assert!(star.iter().any(|&row| row != 0));
        // the arms are mirrored left and right
        for row in star {
            assert_eq!(row.reverse_bits(), row, "row {row:#04x} is not symmetric");
        }
    }
}
