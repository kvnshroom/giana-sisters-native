//! Software rasterizer and C64 framebuffer for The Great Giana Sisters.

pub mod framebuffer;
pub mod palette;
pub mod tiles;
pub mod parallax;
pub mod sprites;
pub mod text;
pub mod title;
mod fallback_font;
pub mod vic;

#[cfg(test)]
mod tests {
    use super::framebuffer::{Framebuffer, BORDER_X, BORDER_Y, HEIGHT, WIDTH};
    use super::palette::c64_color;
    use super::parallax::render_stage;
    use super::sprites::{render_sprite_hires, render_sprite_multicolor};
    use std::fs::File;
    use std::io::Write;

    fn save_ppm(fb: &Framebuffer, path: &str) {
        let mut f = File::create(path).unwrap();
        writeln!(f, "P6\n{} {}\n255", WIDTH, HEIGHT).unwrap();
        for pixel in &fb.pixels {
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            f.write_all(&[r, g, b]).unwrap();
        }
    }

    #[test]
    fn test_render_title() {
        giana_core::skip_without_data!();
        let mut fb = Framebuffer::new();
        fb.draw_title_screen();
        save_ppm(&fb, "/tmp/native_title.ppm");
    }

    #[test]
    fn test_render_stage1() {
        giana_core::skip_without_data!();
        let mut fb = Framebuffer::new();
        fb.clear(c64_color(14)); // Sky blue border & bg

        render_stage(&mut fb, crate::parallax::stage1_map(), 0, 0, 0, 0, 300, 1, 14, 8, 9, false);

        let playfield_y = (BORDER_Y + 5 * 8) as isize;

        // Render Giana standing on stage 1 at (20, 115)
        let gx = (BORDER_X + 20) as isize;
        let gy = playfield_y + 115;
        // Head/Body 0x4E, Dress 0x4F, Eye/Shoes 0x50 (authentic HIRES layer)
        render_sprite_multicolor(&mut fb, 0x4E, gx, gy, 7, 10, 2);
        render_sprite_multicolor(&mut fb, 0x4F, gx, gy, 6, 10, 2);
        render_sprite_hires(&mut fb, 0x50, gx, gy, 1);

        // Render Stage 1 Monster at (120, 115)
        let mx = (BORDER_X + 120) as isize;
        let my = playfield_y + 115;
        render_sprite_multicolor(&mut fb, 0x92, mx, my, 7, 10, 2);

        // Render Stage 1 Rolling Ball at (200, 68) on platform ($B9..$BF)
        let rx = (BORDER_X + 200) as isize;
        let ry = playfield_y + 68;
        render_sprite_multicolor(&mut fb, 0xB9, rx, ry, 7, 10, 2);

        // Black outer border (38-column mode: X=40..343, Y=40..239)
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if x < 40 || x > 343 || y < 40 || y >= 240 {
                    fb.set_pixel(x, y, c64_color(0));
                }
            }
        }

        save_ppm(&fb, "/tmp/stage1_verification.ppm");
    }
}
pub mod highscore;
