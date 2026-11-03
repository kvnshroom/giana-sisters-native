//! Reading the captures of `tools/c30/` and comparing frames with VICE screenshots (tooling for tests and examples, not
//! used by the game).

use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::palette::C64_PALETTE;

/// VIC registers and colour RAM out of the text the VICE monitor prints for `m d000 d02e` and `m d800 dbff` (the format
/// `tools/c30/frame_capture.py` and `tools/c30/snapshot.py` store).
pub fn parse_monitor_dump(text: &str) -> Option<([u8; 47], Vec<u8>)> {
    let mut vic = [0u8; 47];
    let mut color = vec![0u8; 1024];
    let (mut seen_vic, mut seen_col) = (false, false);
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(">C:") else { continue };
        let (addr, bytes) = rest.split_at(4);
        let a = u16::from_str_radix(addr, 16).ok()? as usize;
        // up to 16 hex bytes separated by blanks; the ASCII rendering after them is not two-digit hex
        for (i, tok) in bytes.split_whitespace().take(16).enumerate() {
            if tok.len() != 2 {
                break;
            }
            let Ok(b) = u8::from_str_radix(tok, 16) else { break };
            let at = a + i;
            if (0xD000..0xD02F).contains(&at) {
                vic[at - 0xD000] = b;
                seen_vic = true;
            } else if (0xD800..0xDC00).contains(&at) {
                color[at - 0xD800] = b & 15;
                seen_col = true;
            }
        }
    }
    (seen_vic && seen_col).then_some((vic, color))
}

/// The palette of the VICE x64sc screenshots the references come from, by C64 colour index (read off screenshots of the
/// preview; indices that never showed up are `u32::MAX`, a pixel of such a colour counts as a difference).
pub const VICE_PALETTE: [u32; 16] = [
    0x000000,
    0xffffff,
    0xaf3c58,
    0x7ef3d6,
    u32::MAX,
    0x62d532,
    0x2c3dec,
    0xffff46,
    0xb7631e,
    0x775300,
    0xee7b95,
    0x626262,
    0x949494,
    0xb7ff86,
    0x7385ff,
    0xcdcdcd,
];

/// Writes 384x272 pixels (0xRRGGBB) as a binary PPM.
pub fn write_ppm(path: &str, px: &[u32]) {
    let mut v = format!("P6\n{} {}\n255\n", WIDTH, HEIGHT).into_bytes();
    for p in px {
        v.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, v).unwrap();
}

/// Result of [`compare_with_vice`].
pub struct Comparison {
    pub total: usize,
    pub differing: usize,
    /// first differing pixel (x, y)
    pub first: Option<(usize, usize)>,
    /// (VICE colour, our palette index, count) for every kind of difference, most frequent first
    pub pairs: Vec<(u32, Option<usize>, usize)>,
    /// our frame with the differing pixels in magenta
    pub diff_image: Vec<u32>,
}

impl Comparison {
    /// Number of differing pixels inside a rectangle (x0..x1, y0..y1) of the frame.
    pub fn differing_outside(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> usize {
        (0..WIDTH * HEIGHT)
            .filter(|&i| self.diff_image[i] == 0xFF00FF && !((x0..x1).contains(&(i % WIDTH)) && (y0..y1).contains(&(i / WIDTH))))
            .count()
    }
}

/// Compares a frame with a VICE screenshot (`rgb` = 384x272x3 bytes), colour index by colour index.
pub fn compare_with_vice(fb: &Framebuffer, rgb: &[u8]) -> Comparison {
    assert_eq!(rgb.len(), WIDTH * HEIGHT * 3, "screenshot must be 384x272");
    let vice_index = |c: u32| VICE_PALETTE.iter().position(|&p| p == c);
    let our_index = |c: u32| C64_PALETTE.iter().position(|&p| p & 0xFFFFFF == c & 0xFFFFFF);
    let mut cmp = Comparison { total: WIDTH * HEIGHT, differing: 0, first: None, pairs: Vec::new(), diff_image: fb.pixels.clone() };
    let mut kinds: std::collections::BTreeMap<(u32, Option<usize>), usize> = Default::default();
    for i in 0..WIDTH * HEIGHT {
        let v = (rgb[3 * i] as u32) << 16 | (rgb[3 * i + 1] as u32) << 8 | rgb[3 * i + 2] as u32;
        let (want, mine) = (vice_index(v), our_index(fb.pixels[i]));
        if want.is_none() || want != mine {
            cmp.differing += 1;
            cmp.first.get_or_insert((i % WIDTH, i / WIDTH));
            cmp.diff_image[i] = 0xFF00FF;
            *kinds.entry((v, mine)).or_default() += 1;
        }
    }
    cmp.pairs = kinds.into_iter().map(|((v, m), n)| (v, m, n)).collect();
    cmp.pairs.sort_by_key(|p| std::cmp::Reverse(p.2));
    cmp
}

/// Loads a capture of `tools/c30/frame_capture.py`: the RAM at the start of the frame, with screen RAM and colour RAM
/// replaced by their state at the end of the frame when `<prefix>.late.*` exist. The game's logic (IRQ at line `$5A`) rewrites
/// screen rows and colours *below* the beam before it gets there, so the picture of a frame shows the end-of-frame state of
/// both (a stage-15 frame differs in 2,629 pixels with the start-of-frame state and in none with the end-of-frame state);
/// sprite positions and fine scroll values are the ones of the frame start.
pub fn load_frame(prefix: &str) -> Option<(Vec<u8>, [u8; 47], Vec<u8>)> {
    let mut ram = std::fs::read(format!("{prefix}.ram")).ok()?;
    if ram.len() != 0x1_0002 {
        return None;
    }
    ram.drain(..2);
    let (vic, mut color) = parse_monitor_dump(&std::fs::read_to_string(format!("{prefix}.io")).ok()?)?;
    if let (Ok(late), Ok(late_io)) = (std::fs::read(format!("{prefix}.late.ram")), std::fs::read_to_string(format!("{prefix}.late.io"))) {
        // `.late.ram` holds $C000..$CFFF with a two byte header
        ram[0xC800..0xCBE8].copy_from_slice(&late[2 + 0x800..2 + 0xBE8]);
        color = parse_monitor_dump(&format!("{late_io}\n>C:d000 {}", "00 ".repeat(16)))?.1;
    }
    Some((ram, vic, color))
}
