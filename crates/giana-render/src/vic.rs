//! Renders what the C64 video chip draws from the state of the port: multicolour text mode (bank 3: characters at
//! `$C000`, screen `$C800`), the fine scroll `$D016` of the raster IRQs and the eight hardware sprites.
//!
//! `render_vic` draws one frame of the game exactly. `render_vic_between` draws a picture between two frames (the
//! `SMOOTH MOTION` enhancement): the background is shifted by whole pixels and the sprites are placed at the interpolated
//! positions. It only reads what the chip would draw; the game state is never touched.

use crate::framebuffer::{Framebuffer, WIDTH};
use crate::palette::c64_color;

/// Everything the video chip reads for one frame.
pub struct VicView<'a> {
    /// bank 3 memory, `$C000..$FFFF` (characters, sprite shapes)
    pub mem: &'a [u8],
    pub screen: &'a [u8],
    /// character set `$C000..$C7FF` as the chip drew it
    pub chars: &'a [u8],
    /// cloud rows (screen cells 80..200) as the chip drew them
    pub clouds: &'a [u8],
    pub color: &'a [u8],
    /// `$D021`, `$D022`, `$D023`
    pub bg: [u8; 3],
    /// fine scroll `$D016` below the status bar
    pub xscroll: u8,
    /// fine scroll `$D016` of the cloud band (`$0405`, written at raster line `$41`)
    pub sky_xscroll: u8,
    /// sprite enable `$D015`, multicolour `$D01C`, behind characters `$D01B`
    pub me: u8,
    pub mmc: u8,
    pub mdp: u8,
    /// `$D000..$D010`: x/y pairs and the x high bits
    pub xy: [u8; 17],
    /// sprite pointers (shape at `ptr * 64` in the bank)
    pub ptr: [u8; 8],
    /// `$D027..$D02E`
    pub spr_col: [u8; 8],
    /// `$D025`, `$D026`
    pub spr_mc: [u8; 2],
}

impl<'a> VicView<'a> {
    pub fn from_game(game: &'a giana_core::state::GameState) -> Self {
        let stage_idx = (game.stage.clamp(1, 33) - 1) as usize;
        let img = giana_core::image::img;
        Self {
            mem: &game.sprite_mem,
            screen: &game.screen_ram,
            clouds: &game.en.vic_frame.draw_clouds,
            chars: &game.en.vic_frame.draw_chars,
            color: &game.color_ram,
            // engine B: the third background colour comes from the stage header (`vic_bg2`), not from the original's table
            bg: [game.en.vic_bg0, game.en.vic_bg1, if game.is_engine_b() || game.tnw.is_some() || game.title_active { game.vic_bg2 } else { img(0x609C + stage_idx) }],
            xscroll: game.en.vic_frame.draw_xscroll,
            sky_xscroll: game.en.vic_frame.draw_sky_xscroll,
            me: game.en.vic_frame.draw_me,
            mmc: game.en.vic_frame.draw_mmc,
            mdp: game.en.vic_frame.draw_mdp,
            xy: game.en.vic_frame.draw_xy,
            ptr: game.en.vic_frame.draw_ptr,
            spr_col: game.en.vic_frame.draw_col,
            spr_mc: if game.is_engine_b() { game.vic_mc } else { [10, 2] },
        }
    }
}

/// A copy of a `VicView` that owns its data, to keep the frame of the last game step for `render_vic_between`.
#[derive(Clone, Default)]
pub struct VicSnapshot {
    mem: Vec<u8>,
    screen: Vec<u8>,
    chars: Vec<u8>,
    clouds: Vec<u8>,
    color: Vec<u8>,
    bg: [u8; 3],
    xscroll: u8,
    sky_xscroll: u8,
    me: u8,
    mmc: u8,
    mdp: u8,
    xy: [u8; 17],
    ptr: [u8; 8],
    spr_col: [u8; 8],
    spr_mc: [u8; 2],
}

impl VicSnapshot {
    pub fn from_view(v: &VicView) -> Self {
        Self {
            mem: v.mem.to_vec(),
            screen: v.screen.to_vec(),
            chars: v.chars.to_vec(),
            clouds: v.clouds.to_vec(),
            color: v.color.to_vec(),
            bg: v.bg,
            xscroll: v.xscroll,
            sky_xscroll: v.sky_xscroll,
            me: v.me,
            mmc: v.mmc,
            mdp: v.mdp,
            xy: v.xy,
            ptr: v.ptr,
            spr_col: v.spr_col,
            spr_mc: v.spr_mc,
        }
    }

    /// Copies a new frame into this snapshot (no new allocations once it has the size).
    pub fn set(&mut self, v: &VicView) {
        self.mem.clear();
        self.mem.extend_from_slice(v.mem);
        self.screen.clear();
        self.screen.extend_from_slice(v.screen);
        self.chars.clear();
        self.chars.extend_from_slice(v.chars);
        self.clouds.clear();
        self.clouds.extend_from_slice(v.clouds);
        self.color.clear();
        self.color.extend_from_slice(v.color);
        self.bg = v.bg;
        self.xscroll = v.xscroll;
        self.sky_xscroll = v.sky_xscroll;
        self.me = v.me;
        self.mmc = v.mmc;
        self.mdp = v.mdp;
        self.xy = v.xy;
        self.ptr = v.ptr;
        self.spr_col = v.spr_col;
        self.spr_mc = v.spr_mc;
    }

    pub fn view(&self) -> VicView<'_> {
        VicView {
            mem: &self.mem,
            screen: &self.screen,
            chars: &self.chars,
            clouds: &self.clouds,
            color: &self.color,
            bg: self.bg,
            xscroll: self.xscroll,
            sky_xscroll: self.sky_xscroll,
            me: self.me,
            mmc: self.mmc,
            mdp: self.mdp,
            xy: self.xy,
            ptr: self.ptr,
            spr_col: self.spr_col,
            spr_mc: self.spr_mc,
        }
    }
}

/// `$D011 = $1B`: 25 rows, text starts at raster line 51. `$D016` is written about 1.8 raster lines after the interrupts at
/// lines `$41` and `$59`, which lands on the character row boundaries.
/// Rows 0 and 1 (status bar) keep scroll 0, rows 2..4 use the cloud scroll, the playfield starts at row 5.
const STATUS_LINES: usize = 16;
const SKY_LINES: usize = 40;
/// Position of screen pixel (0, 0) in the 384x272 frame (video chip: x 24 -> 32, raster line 51 -> 36).
pub const ORIGIN_X: usize = 32;
pub const ORIGIN_Y: usize = 36;

/// 38 columns x 25 rows window of the border (`$D016` bit 3 clear, `$D011` bit 3 set).
fn visible(x: isize, y: isize, g: &Geometry) -> bool {
    (g.left..g.right).contains(&x) && (0..200).contains(&y)
}

/// Where the lines and columns of the picture are: the oracle recordings (`render_vic`) and the raster beam of the AUTHENTIC
/// slice (`render_vic_authentic`) differ by a line at the top, and the slice latches `$D016` for a whole line.
#[derive(Clone, Copy)]
pub struct Geometry {
    /// picture line of text line 0
    pub oy: usize,
    /// first text line of the cloud band and of the playfield
    pub sky: usize,
    pub play: usize,
    /// first and one past the last visible column (the 38 column window), in text columns
    pub left: isize,
    pub right: isize,
}

/// Geometry of the 30A preview as VICE x64sc shows it (measured against screenshots, `giana-engine-c`): text line 0 at
/// picture line 35, the cloud band from text line 16, the playfield from 39, the plain 38 column window 7..311.
pub const GEOMETRY_30A: Geometry = Geometry { oy: ORIGIN_Y - 1, sky: STATUS_LINES, play: SKY_LINES - 1, left: 7, right: 311 };

const ORACLE: Geometry = Geometry { oy: ORIGIN_Y, sky: STATUS_LINES, play: SKY_LINES, left: 8, right: 312 };
const SLICE: Geometry = Geometry { oy: ORIGIN_Y - 1, sky: STATUS_LINES - 1, play: SKY_LINES - 1, left: 7, right: 313 };

/// One sprite as it is drawn: its shape and colours come from one frame, its position can be anywhere in between.
struct SpriteDraw<'a> {
    mem: &'a [u8],
    ptr: u8,
    x: isize,
    y: isize,
    multi: bool,
    behind: bool,
    color: u8,
    mc: [u8; 2],
}

/// The background of a band of lines: which frame it is read from and its fine scroll (a whole number of pixels; between two
/// frames it can leave the range 0..7, up to -8..8, because the 38-column window hides 8 pixels on each side).
#[derive(Clone, Copy)]
struct Band<'a> {
    view: &'a VicView<'a>,
    xscroll: i32,
    /// text mode without multicolour (`$D016` bit 4 clear): every cell is drawn hi-res in its full colour nibble
    hires: bool,
}

pub fn render_vic(fb: &mut Framebuffer, v: &VicView) {
    let bands = [
        Band { view: v, xscroll: 0, hires: false },
        Band { view: v, xscroll: (v.sky_xscroll & 7) as i32, hires: false },
        Band { view: v, xscroll: (v.xscroll & 7) as i32, hires: false },
    ];
    let sprites: Vec<SpriteDraw> = (0..8usize).filter(|&s| v.me >> s & 1 != 0).map(|s| sprite_of(v, s, sprite_x(v, s), v.xy[2 * s + 1] as isize)).collect();
    draw(fb, &bands, &sprites, ORACLE);
}

/// The same picture in the geometry of the AUTHENTIC presentation (the raster beam of `giana_authentic`: text row 0 starts at
/// picture line 35, one line above the oracle recordings that `render_vic` is calibrated to).
pub fn render_vic_authentic(fb: &mut Framebuffer, v: &VicView) {
    let bands = [
        Band { view: v, xscroll: 0, hires: false },
        Band { view: v, xscroll: (v.sky_xscroll & 7) as i32, hires: false },
        Band { view: v, xscroll: (v.xscroll & 7) as i32, hires: false },
    ];
    let sprites: Vec<SpriteDraw> = (0..8usize).filter(|&s| v.me >> s & 1 != 0).map(|s| sprite_of(v, s, sprite_x(v, s), v.xy[2 * s + 1] as isize)).collect();
    draw(fb, &bands, &sprites, SLICE);
}

/// What differs between the three bands of a frame when the raster IRQs rewrite the video chip between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandSpec {
    /// fine scroll (`$D016` bits 0..2)
    pub xscroll: u8,
    /// `$D021`, `$D022`, `$D023` while the band is drawn
    pub bg: [u8; 3],
    /// `$D016` bit 4 clear: hi-res text, a cell's colour is its whole colour nibble and there is no multicolour cell
    pub hires: bool,
}

/// Draws a frame whose three bands (status bar, cloud band, playfield) each have their own fine scroll and background colours,
/// in the given geometry (`SLICE` like `render_vic_authentic` uses, or [`GEOMETRY_30A`]). The original keeps the status bar
/// at scroll 0 and one set of colours; the 30A preview's raster chain (family C, `giana-engine-c`) writes `$D016 = $17`
/// (scroll 7) and `$D022/$D023 = $0A/$02` there and the stage's colours from the cloud band on. Characters, colours and
/// sprites come from `v`; its `xscroll`/`sky_xscroll`/`bg` are not used.
pub fn render_vic_bands(fb: &mut Framebuffer, v: &VicView, specs: [BandSpec; 3], geo: Geometry) {
    let views = [0, 1, 2].map(|i| VicView { bg: specs[i].bg, ..*v });
    let bands = [0, 1, 2].map(|i| Band { view: &views[i], xscroll: (specs[i].xscroll & 7) as i32, hires: specs[i].hires });
    let sprites: Vec<SpriteDraw> = (0..8usize).filter(|&s| v.me >> s & 1 != 0).map(|s| sprite_of(v, s, sprite_x(v, s), v.xy[2 * s + 1] as isize)).collect();
    draw(fb, &bands, &sprites, geo);
}

fn sprite_x(v: &VicView, s: usize) -> isize {
    v.xy[2 * s] as isize | ((v.xy[16] >> s & 1) as isize) << 8
}

fn sprite_of<'a>(v: &'a VicView, s: usize, x: isize, y: isize) -> SpriteDraw<'a> {
    SpriteDraw {
        mem: v.mem,
        ptr: v.ptr[s],
        x,
        y,
        multi: v.mmc >> s & 1 != 0,
        behind: v.mdp >> s & 1 != 0,
        color: v.spr_col[s],
        mc: v.spr_mc,
    }
}

/// How far the picture moved to the left (in pixels) between two fine scroll values: the shift is under 4 pixels per step.
fn fine_delta(a: u8, b: u8) -> i32 {
    (a as i32 - b as i32 + 4).rem_euclid(8) - 4
}

/// `delta * t` rounded to a whole number. A tie goes to the earlier frame, so that the same distance gives the same
/// rounding in both directions: a sprite that stands in the world moves with the background to the pixel.
fn lerp_round(delta: i32, t: f64) -> i32 {
    let n = ((delta.abs() as f64 * t - 0.5).ceil() as i32).clamp(0, delta.abs());
    n * delta.signum()
}

/// The band between two frames at time `t` (0 is `a`, 1 is `b`): the frame that is nearer in time, with the scroll that puts
/// its picture where the other frames' motion has brought it. When the scroll would leave the range that the 38-column window
/// can fill, the other frame is used.
fn band_between<'a>(a: &'a VicView<'a>, b: &'a VicView<'a>, xa: u8, xb: u8, t: f64) -> Band<'a> {
    let d = fine_delta(xa & 7, xb & 7);
    let o = lerp_round(d, t);
    let from_a = Band { view: a, xscroll: (xa & 7) as i32 - o, hires: false };
    let from_b = Band { view: b, xscroll: (xb & 7) as i32 + (d - o), hires: false };
    let ok = |band: &Band| (-8..=8).contains(&band.xscroll);
    let (first, second) = if t < 0.5 { (from_a, from_b) } else { (from_b, from_a) };
    if ok(&first) || !ok(&second) {
        first
    } else {
        second
    }
}

/// Moves further than this between two frames are not motion but a change of the scene (another stage, a new sprite in the
/// slot): such a sprite is not interpolated.
const MAX_SPRITE_STEP: isize = 40;

/// Draws a picture between two frames: `t` is how far it is from `a` (0.0) to `b` (1.0). The background is shifted by whole
/// pixels, the sprites are at the interpolated position, rounded to a pixel. `t` = 0 gives `a` and `t` = 1 gives `b` exactly.
pub fn render_vic_between(fb: &mut Framebuffer, a: &VicView, b: &VicView, t: f64) {
    let t = t.clamp(0.0, 1.0);
    let bands = [
        Band { view: if t < 0.5 { a } else { b }, xscroll: 0, hires: false },
        band_between(a, b, a.sky_xscroll, b.sky_xscroll, t),
        band_between(a, b, a.xscroll, b.xscroll, t),
    ];
    let mut sprites = Vec::with_capacity(8);
    for s in 0..8usize {
        let (in_a, in_b) = (a.me >> s & 1 != 0, b.me >> s & 1 != 0);
        let nearer = if t < 0.5 { a } else { b };
        match (in_a, in_b) {
            (false, false) => {}
            (true, false) => {
                if t < 0.5 {
                    sprites.push(sprite_of(a, s, sprite_x(a, s), a.xy[2 * s + 1] as isize));
                }
            }
            (false, true) => {
                if t >= 0.5 {
                    sprites.push(sprite_of(b, s, sprite_x(b, s), b.xy[2 * s + 1] as isize));
                }
            }
            (true, true) => {
                let (xa, xb) = (sprite_x(a, s), sprite_x(b, s));
                let (ya, yb) = (a.xy[2 * s + 1] as isize, b.xy[2 * s + 1] as isize);
                let (x, y) = if (xb - xa).abs() <= MAX_SPRITE_STEP && (yb - ya).abs() <= MAX_SPRITE_STEP {
                    (xa + lerp_round((xb - xa) as i32, t) as isize, ya + lerp_round((yb - ya) as i32, t) as isize)
                } else if t < 0.5 {
                    (xa, ya)
                } else {
                    (xb, yb)
                };
                sprites.push(sprite_of(nearer, s, x, y));
            }
        }
    }
    draw(fb, &bands, &sprites, ORACLE);
}

/// `bands`: status bar, cloud band, playfield. Sprites in the order of their numbers; number 7 is drawn first so that number 0
/// ends up on top.
fn draw(fb: &mut Framebuffer, bands: &[Band; 3], sprites: &[SpriteDraw], geo: Geometry) {
    let oy = geo.oy;
    fb.clear(c64_color(0));
    let mut fg = vec![false; 320 * 200];
    for y in 0..200usize {
        let band = if y < geo.sky {
            bands[0]
        } else if y < geo.play {
            bands[1]
        } else {
            bands[2]
        };
        let (v, xs) = (band.view, band.xscroll as isize);
        let row = y / 8;
        let line = y % 8;
        for col in 0..40usize {
            let i = row * 40 + col;
            let ch = if (80..200).contains(&i) { v.clouds.get(i - 80) } else { v.screen.get(i) }.copied().unwrap_or(0) as usize;
            let cc = v.color.get(i).copied().unwrap_or(0) & 0x0F;
            let bits = v.chars.get(ch * 8 + line).copied().unwrap_or(0);
            if !band.hires && cc & 8 != 0 {
                for p in 0..4 {
                    let pair = (bits >> (6 - 2 * p)) & 3;
                    let colr = match pair {
                        0 => v.bg[0],
                        1 => v.bg[1],
                        2 => v.bg[2],
                        _ => cc & 7,
                    };
                    for k in 0..2 {
                        let x = (col * 8 + p * 2 + k) as isize + xs;
                        if (0..320).contains(&x) {
                            fb.pixels[(oy + y) * WIDTH + ORIGIN_X + x as usize] = c64_color(colr);
                            fg[y * 320 + x as usize] = pair >= 2;
                        }
                    }
                }
            } else {
                for p in 0..8 {
                    let on = bits >> (7 - p) & 1 != 0;
                    let x = (col * 8 + p) as isize + xs;
                    if (0..320).contains(&x) {
                        fb.pixels[(oy + y) * WIDTH + ORIGIN_X + x as usize] = c64_color(if on { if band.hires { cc } else { cc & 7 } } else { v.bg[0] });
                        fg[y * 320 + x as usize] = on;
                    }
                }
            }
        }
        for x in 0..320usize {
            if !visible(x as isize, y as isize, &geo) {
                fb.pixels[(oy + y) * WIDTH + ORIGIN_X + x] = c64_color(0);
            }
        }
    }
    for sp in sprites.iter().rev() {
        let base = sp.ptr as usize * 64;
        for r in 0..21usize {
            for b in 0..3usize {
                let byte = sp.mem.get(base + r * 3 + b).copied().unwrap_or(0);
                let mut put = |px: isize, w: isize, colr: Option<u8>| {
                    let Some(c) = colr else { return };
                    for k in 0..w {
                        let dx = sp.x - 24 + px + k;
                        let dy = sp.y - 50 + r as isize; // the first sprite line is raster line Y + 1, text starts at line 51
                        if !visible(dx, dy, &geo) {
                            continue;
                        }
                        if sp.behind && fg[dy as usize * 320 + dx as usize] {
                            continue;
                        }
                        fb.pixels[(oy + dy as usize) * WIDTH + ORIGIN_X + dx as usize] = c64_color(c);
                    }
                };
                if sp.multi {
                    for p in 0..4isize {
                        let pair = (byte >> (6 - 2 * p)) & 3;
                        let colr = match pair {
                            0 => None,
                            1 => Some(sp.mc[0]),
                            2 => Some(sp.color),
                            _ => Some(sp.mc[1]),
                        };
                        put(b as isize * 8 + p * 2, 2, colr);
                    }
                } else {
                    for p in 0..8isize {
                        let on = byte >> (7 - p) & 1 != 0;
                        put(b as isize * 8 + p, 1, on.then_some(sp.color));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_scroll_distance_goes_the_short_way_round() {
        assert_eq!(fine_delta(0, 6), 2); // scroll 0 -> 6 after a coarse step: the picture moved 2 pixels left
        assert_eq!(fine_delta(5, 3), 2);
        assert_eq!(fine_delta(3, 5), -2);
        assert_eq!(fine_delta(6, 0), -2);
        assert_eq!(fine_delta(4, 4), 0);
    }

    #[test]
    fn rounding_is_the_same_in_both_directions_and_ties_go_to_the_earlier_frame() {
        for d in [-3, -2, -1, 1, 2, 3] {
            for t in [0.0, 0.1, 0.25, 0.333, 0.5, 0.75, 0.9, 1.0] {
                assert_eq!(lerp_round(d, t), -lerp_round(-d, t), "d {d} t {t}");
            }
        }
        assert_eq!(lerp_round(1, 0.5), 0);
        assert_eq!(lerp_round(2, 0.25), 0);
        assert_eq!(lerp_round(2, 0.75), 1);
        assert_eq!(lerp_round(2, 0.8), 2);
        assert_eq!((lerp_round(2, 0.0), lerp_round(2, 1.0), lerp_round(-2, 1.0)), (0, 2, -2));
    }

    #[test]
    fn a_band_between_two_frames_ends_where_the_frames_are() {
        let (a, b) = (snapshot(0), snapshot(6));
        let (va, vb) = (a.view(), b.view());
        let at0 = band_between(&va, &vb, 0, 6, 0.0);
        let at1 = band_between(&va, &vb, 0, 6, 1.0);
        assert_eq!(at0.xscroll, 0);
        assert_eq!(at1.xscroll, 6);
        // half way: the picture of a frame is shifted by one pixel towards the other
        let mid_a = band_between(&va, &vb, 0, 6, 0.4);
        assert!((-1..=0).contains(&mid_a.xscroll) && std::ptr::eq(mid_a.view, &va));
        let mid_b = band_between(&va, &vb, 0, 6, 0.6);
        assert!(std::ptr::eq(mid_b.view, &vb) && mid_b.xscroll == 7);
        // never outside of what the 38 columns can fill
        for xa in 0..8u8 {
            for step in [-3i32, -2, -1, 1, 2, 3] {
                let xb = (xa as i32 + step).rem_euclid(8) as u8;
                for i in 0..=20 {
                    let band = band_between(&va, &vb, xa, xb, i as f64 / 20.0);
                    assert!((-8..=8).contains(&band.xscroll), "{xa}->{xb} at {i}: {}", band.xscroll);
                }
            }
        }
    }

    fn snapshot(xscroll: u8) -> VicSnapshot {
        let mut s = VicSnapshot { mem: vec![0; 0x4000], screen: vec![0; 1000], chars: vec![0; 2048], clouds: vec![0; 120], color: vec![0; 1000], ..Default::default() };
        s.xscroll = xscroll;
        s
    }

    /// A frame of the scrolling playfield: a bright column of characters in the screen, moved `shift` pixels to the left
    fn scrolled(shift: usize) -> VicSnapshot {
        let mut s = snapshot(((8 - shift % 8) % 8) as u8);
        s.chars[8..16].fill(0xFF); // character 1: a solid block
        s.color.fill(1);
        s.bg = [6, 0, 0];
        let col = 20 - shift.div_ceil(8);
        for row in 5..25 {
            s.screen[row * 40 + col] = 1;
        }
        s
    }

    #[test]
    fn the_end_points_are_the_frames_themselves() {
        let (a, b) = (scrolled(0), scrolled(2));
        let (mut fa, mut fb_, mut f0, mut f1) = (Framebuffer::new(), Framebuffer::new(), Framebuffer::new(), Framebuffer::new());
        render_vic(&mut fa, &a.view());
        render_vic(&mut fb_, &b.view());
        render_vic_between(&mut f0, &a.view(), &b.view(), 0.0);
        render_vic_between(&mut f1, &a.view(), &b.view(), 1.0);
        assert!(fa.pixels == f0.pixels, "t = 0 is the first frame");
        assert!(fb_.pixels == f1.pixels, "t = 1 is the second frame");
    }

    /// The x of the first pixel of the block in a playfield line
    fn block_x(fb: &Framebuffer) -> usize {
        let y = ORIGIN_Y + 100;
        (ORIGIN_X + 8..ORIGIN_X + 312).find(|&x| fb.pixels[y * WIDTH + x] == c64_color(1)).unwrap()
    }

    #[test]
    fn the_background_moves_a_pixel_at_a_time_between_two_frames() {
        // the picture scrolls 2 pixels to the left per step; the second pair has a coarse step in it (screen columns move)
        for (from, to) in [(0usize, 2usize), (6, 8), (14, 16)] {
            let (a, b) = (scrolled(from), scrolled(to));
            let mut xs = Vec::new();
            for i in 0..=10 {
                let mut fb = Framebuffer::new();
                render_vic_between(&mut fb, &a.view(), &b.view(), i as f64 / 10.0);
                xs.push(block_x(&fb));
            }
            let (first, last) = (xs[0], *xs.last().unwrap());
            assert_eq!(first - last, 2, "{from}->{to}: {xs:?}");
            assert!(xs.windows(2).all(|w| w[1] <= w[0]), "monotonic: {xs:?}");
            assert!(xs.iter().any(|&x| x != first && x != last), "the middle position is shown: {xs:?}");
        }
    }

    #[test]
    fn a_sprite_that_jumps_is_not_interpolated_but_a_moving_one_is() {
        let mut a = scrolled(0);
        a.me = 1;
        a.mmc = 0;
        a.mem[64] = 0xFF;
        a.ptr[0] = 1;
        a.spr_col[0] = 5;
        a.xy[0] = 100;
        a.xy[1] = 100;
        let mut b = a.clone();
        b.xy[0] = 104;
        let sprite_x = |t: f64, a: &VicSnapshot, b: &VicSnapshot| {
            let mut fb = Framebuffer::new();
            render_vic_between(&mut fb, &a.view(), &b.view(), t);
            let y = ORIGIN_Y + 50;
            (ORIGIN_X + 8..ORIGIN_X + 312).find(|&x| fb.pixels[y * WIDTH + x] == c64_color(5)).map(|x| x - ORIGIN_X + 24)
        };
        assert_eq!(sprite_x(0.0, &a, &b), Some(100));
        assert_eq!(sprite_x(0.5, &a, &b), Some(102));
        assert_eq!(sprite_x(1.0, &a, &b), Some(104));
        // a jump of more than 40 pixels (a new object in the slot) is shown where it is, not slid across
        b.xy[0] = 200;
        assert_eq!(sprite_x(0.3, &a, &b), Some(100));
        assert_eq!(sprite_x(0.7, &a, &b), Some(200));
    }
}
