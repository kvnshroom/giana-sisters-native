use crate::framebuffer::{Framebuffer, BORDER_X, BORDER_Y};
use crate::palette::c64_color;
use crate::tiles::{charset, render_char};

pub fn stage1_map() -> &'static [u8] {
    &giana_core::data::get().stage1_map
}

/// Renders the stage tilemap, clouds, and status bar into the framebuffer.
///
/// * `scroll_x`: Horizontal pixel scroll of the main playfield (0..2048)
/// * `cloud_scroll_x`: Horizontal pixel scroll of the parallax cloud layer
pub fn render_stage(
    fb: &mut Framebuffer,
    stage_map: &[u8],
    scroll_x: usize,
    cloud_scroll_x: usize,
    score: u32,
    diamonds: u32,
    time: u32,
    stage_num: u32,
    bg0: u8,
    bg1: u8,
    bg2: u8,
    is_underground: bool,
) {
    // 1. Draw top HUD (Rows 0 & 1 of text)
    // Row 0: "  GIANA    BONUS    STAGE 01"
    // Row 1: "  000000    $00      300    "
    let hud_text_r0 = format!("  GIANA     BONUS     STAGE {:02} ", stage_num);
    let hud_text_r1 = format!("  {:06}     ${:02}      {:03}    ", score, diamonds, time);

    let hud_bg = if is_underground { 0 } else { bg0 };

    for (c, ch) in hud_text_r0.bytes().enumerate() {
        if c >= 40 {
            break;
        }
        let char_code = ascii_to_c64(ch);
        render_hud_char(fb, char_code, BORDER_X + c * 8, BORDER_Y, 7, hud_bg); // Yellow text on Sky/Black
    }

    for (c, ch) in hud_text_r1.bytes().enumerate() {
        if c >= 40 {
            break;
        }
        let (char_code, fg_color) = if ch == b'$' {
            (0xF7, 2) // Red diamond icon
        } else if ch >= b'0' && ch <= b'9' {
            (ascii_to_c64(ch), 1) // White numbers
        } else {
            (ascii_to_c64(ch), 7) // Yellow text
        };
        render_hud_char(fb, char_code, BORDER_X + c * 8, BORDER_Y + 8, fg_color, hud_bg);
    }

    // 2. Parallax Clouds (Rows 2..4, Scanlines 16..40) - Only in above-ground stages!
    if !is_underground {
        // Clear sky background in cloud rows
        for r in 2..5 {
            for c in 0..40 {
                render_char(fb, 0x00, BORDER_X + c * 8, BORDER_Y + r * 8, bg0, bg1, bg2);
            }
        }

        // Draw clouds at periodic intervals using cloud_scroll_x
        let cloud_w = 4; // 4 characters wide
        let cloud_chars_r2 = [0xDA, 0xDC, 0xDD, 0xDE];
        let cloud_chars_r3 = [0xDF, 0xE1, 0xE2, 0xE3];

        let cloud_period = 160;
        let base_x = cloud_scroll_x % cloud_period;

        for i in -1..4 {
            let cx = (i * cloud_period as isize) - (base_x as isize);
            for idx in 0..cloud_w {
                let px = cx + (idx as isize) * 8;
                if px >= -8 && px < 320 {
                    let dest_x = (BORDER_X as isize + px) as usize;
                    render_char(fb, cloud_chars_r2[idx], dest_x, BORDER_Y + 16, bg0, 1, 3);
                    render_char(fb, cloud_chars_r3[idx], dest_x, BORDER_Y + 24, bg0, 1, 3);
                }
            }
        }
    } else {
        // Clear top rows to black in underground dungeons
        for r in 2..5 {
            for c in 0..40 {
                render_char(fb, 0x00, BORDER_X + c * 8, BORDER_Y + r * 8, 0, bg1, bg2);
            }
        }
    }

    // 3. Playfield Tilemap (Rows 5..24 = 20 rows of 16x16 / 8x8 tiles)
    let char_scroll = scroll_x / 8;
    let fine_scroll = scroll_x % 8;

    for row in 0..20 {
        let screen_row = 5 + row;
        let dest_y = BORDER_Y + screen_row * 8;
        let map_row_start = row * 256;

        // Stage sky/dungeon background color across playfield rows; open pits drop down into bottom border
        let line_bg = if screen_row < 24 {
            bg0
        } else {
            0 // Bottom Pit Black
        };

        for screen_col in 0..41 {
            let map_col = char_scroll + screen_col;
            if map_col >= 256 {
                continue;
            }

            let char_idx = stage_map[map_row_start + map_col];
            let dest_x = (BORDER_X + screen_col * 8) as isize - fine_scroll as isize;

            if dest_x >= (BORDER_X as isize - 8) && dest_x < (BORDER_X + 320) as isize {
                render_char(fb, char_idx, dest_x as usize, dest_y, line_bg, bg1, bg2);
            }
        }
    }
}

/// Helper for rendering crisp monochrome HUD characters
fn render_hud_char(fb: &mut Framebuffer, char_code: u8, x: usize, y: usize, fg: u8, bg: u8) {
    if char_code == 0 {
        let color = c64_color(bg);
        for r in 0..8 {
            for c in 0..8 {
                fb.set_pixel(x + c, y + r, color);
            }
        }
        return;
    }

    let offset = (char_code as usize) * 8;
    let charset = charset();
    if offset + 8 > charset.len() {
        return;
    }
    let data = &charset[offset..offset + 8];
    let fg_color = c64_color(fg);
    let bg_color = c64_color(bg);

    for r in 0..8 {
        let b = data[r];
        for c in 0..8 {
            let is_fg = (b & (0x80 >> c)) != 0;
            fb.set_pixel(x + c, y + r, if is_fg { fg_color } else { bg_color });
        }
    }
}

/// Helper mapping standard ASCII characters to the C64 font codes
fn ascii_to_c64(c: u8) -> u8 {
    match c {
        b'@' => 0x00,
        b'A'..=b'Z' => c - b'A' + 1,
        b'0'..=b'9' => c, // In C64 font, numbers are at 0x30..0x39
        b' ' => 0x00,
        b':' => 0x3A,
        b'-' => 0x2D,
        b'.' => 0x2E,
        _ => 0x00,
    }
}
