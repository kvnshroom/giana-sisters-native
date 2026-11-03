use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use crate::palette::c64_color;

/// Bank 3 (`$C000..$FFFF`): sprite shapes at `pointer * 64`.
fn sprites() -> &'static [u8] {
    crate::tiles::bank3()
}

/// Renders a multicolor sprite frame at virtual screen coordinate (x, y).
///
/// * `pointer`: Sprite frame index (0..127)
/// * `x`, `y`: Virtual screen coordinates (matching C64 VIC-II coordinate space)
/// * `main_color`: Individual sprite color (0..15)
/// * `mc0`: Shared sprite multicolor 0 (default: 10 = skin tone)
/// * `mc1`: Shared sprite multicolor 1 (default: 2 = red)
pub fn render_sprite_multicolor(
    fb: &mut Framebuffer,
    pointer: usize,
    x: isize,
    y: isize,
    main_color: u8,
    mc0: u8,
    mc1: u8,
) {
    let offset = pointer * 64;
    let sprites = sprites();
    if offset + 63 >= sprites.len() {
        return;
    }
    let sprite_data = &sprites[offset..offset + 64];

    let c_mc0 = c64_color(mc0);
    let c_main = c64_color(main_color);
    let c_mc1 = c64_color(mc1);

    for row in 0..21 {
        let dest_y = y + row as isize;
        if dest_y < 0 || dest_y >= HEIGHT as isize {
            continue;
        }

        let b0 = sprite_data[row * 3];
        let b1 = sprite_data[row * 3 + 1];
        let b2 = sprite_data[row * 3 + 2];

        // 3 bytes = 12 double-wide pixels
        let bytes = [b0, b1, b2];
        for (b_idx, &byte) in bytes.iter().enumerate() {
            for pair in 0..4 {
                let shift = 6 - (pair * 2);
                let bits = (byte >> shift) & 0x03;
                if bits == 0 {
                    continue; // Transparent
                }

                let color = match bits {
                    1 => c_mc0,
                    2 => c_main,
                    _ => c_mc1,
                };

                let px = x + (b_idx * 8 + pair * 2) as isize;
                if px >= 0 && px + 1 < WIDTH as isize {
                    fb.set_pixel(px as usize, dest_y as usize, color);
                    fb.set_pixel((px + 1) as usize, dest_y as usize, color);
                }
            }
        }
    }
}

/// Renders a single-color high-resolution sprite (1 bit per pixel, 24x21 resolution).
/// Used by C64 VIC-II Sprite 2 for Giana's crisp blue eyes and shoes.
pub fn render_sprite_hires(
    fb: &mut Framebuffer,
    pointer: usize,
    x: isize,
    y: isize,
    color_idx: u8,
) {
    let offset = pointer * 64;
    let sprites = sprites();
    if offset + 63 >= sprites.len() {
        return;
    }
    let sprite_data = &sprites[offset..offset + 64];
    let color = c64_color(color_idx);

    for row in 0..21 {
        let dest_y = y + row as isize;
        if dest_y < 0 || dest_y >= HEIGHT as isize {
            continue;
        }

        let b0 = sprite_data[row * 3];
        let b1 = sprite_data[row * 3 + 1];
        let b2 = sprite_data[row * 3 + 2];

        let bytes = [b0, b1, b2];
        for (b_idx, &byte) in bytes.iter().enumerate() {
            for bit in 0..8 {
                if (byte & (0x80 >> bit)) != 0 {
                    let px = x + (b_idx * 8 + bit) as isize;
                    if px >= 0 && px < WIDTH as isize {
                        fb.set_pixel(px as usize, dest_y as usize, color);
                    }
                }
            }
        }
    }
}
