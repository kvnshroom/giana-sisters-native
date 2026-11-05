use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use crate::palette::c64_color;

/// Bank 3 (`$C000..$FFFF`) as the port's game state holds it when a game starts: characters at `$C000`, sprite shapes
/// above (the game's own copies from `$A000..` into `$C800..` already done).
pub fn bank3() -> &'static [u8] {
    static BANK: giana_core::data::Cache<Vec<u8>> = giana_core::data::Cache::new(None);
    giana_core::data::cached(&BANK, || giana_core::state::GameState::new_port_game().sprite_mem).as_slice()
}

/// Character set `$C000..$C7FF`.
pub fn charset() -> &'static [u8] {
    &bank3()[..0x800]
}
use giana_core::collision::colortable;

/// Renders a single 8x8 character into the framebuffer at pixel position (dest_x, dest_y).
/// Automatically handles C64 standard vs. multicolor mode based on colortable attribute.
pub fn render_char(
    fb: &mut Framebuffer,
    char_idx: u8,
    dest_x: usize,
    dest_y: usize,
    bg0: u8,
    bg1: u8,
    bg2: u8,
) {
    if char_idx == 0 {
        // Space / clear sky: fill with line background color
        let color = c64_color(bg0);
        for row in 0..8 {
            let py = dest_y + row;
            if py >= HEIGHT {
                break;
            }
            for col in 0..8 {
                let px = dest_x + col;
                if px < WIDTH {
                    fb.set_pixel(px, py, color);
                }
            }
        }
        return;
    }

    let char_offset = (char_idx as usize) * 8;
    let charset = charset();
    if char_offset + 8 > charset.len() {
        return;
    }
    let char_data = &charset[char_offset..char_offset + 8];
    let color_attr = colortable()[char_idx as usize];

    if color_attr < 8 {
        // High-resolution mode (8x8, 1 bit per pixel)
        let fg_color = c64_color(color_attr);
        let bg_color = c64_color(bg0);

        for row in 0..8 {
            let py = dest_y + row;
            if py >= HEIGHT {
                break;
            }
            let byte = char_data[row];
            for col in 0..8 {
                let px = dest_x + col;
                if px >= WIDTH {
                    break;
                }
                let is_fg = (byte & (0x80 >> col)) != 0;
                fb.set_pixel(px, py, if is_fg { fg_color } else { bg_color });
            }
        }
    } else {
        // Multicolor mode (4 double-wide pixels per row)
        let c00 = c64_color(bg0);
        let c01 = c64_color(bg1);
        let c10 = c64_color(bg2);
        let c11 = c64_color(color_attr & 0x07);

        for row in 0..8 {
            let py = dest_y + row;
            if py >= HEIGHT {
                break;
            }
            let byte = char_data[row];
            for pair in 0..4 {
                let shift = 6 - (pair * 2);
                let bits = (byte >> shift) & 0x03;
                let color = match bits {
                    0 => c00,
                    1 => c01,
                    2 => c10,
                    _ => c11,
                };

                let px = dest_x + pair * 2;
                fb.set_pixel(px, py, color);
                fb.set_pixel(px + 1, py, color);
            }
        }
    }
}
