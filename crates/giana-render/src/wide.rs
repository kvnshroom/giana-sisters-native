//! The WIDESCREEN view: the C64 picture with extra columns of the level to the left and right.
//!
//! The picture of the C64 is 304 x 200 pixels (the 38 column window of the 40 column screen) inside a border. The widescreen
//! view keeps that picture exactly as it is and puts [`EXTRA`] pixels of the level next to each of its sides, between the
//! picture and the border: the frame goes from 384 x 272 to [`WIDE_WIDTH`] x 272 (480 x 272, 1.76:1, close to 16:9), and
//! without the border (the playing area alone) from 304 x 200 to 352 x 200 with [`EXTRA_CROP`] on each side (1.76:1 as well).
//!
//! The extra columns are drawn from the level map, not from screen RAM (which only holds the 40 columns of the picture): see
//! [`WideCols`]. They are not part of what the C64 shows, so they are shaded (smoothly darker towards the outer edge, so the
//! old edge reads as the edge of the game, the status bar lines excepted), and the sprites in them fade out with the distance from the old edge: the objects
//! of the game still come and go at the old edges, the fade hides that (see `vic::draw_strips`).
//!
//! The strips are separate from the 384 x 272 frame buffer on purpose: the frame buffer, with everything that is drawn on it
//! (pause menu, messages), stays the C64 frame, and [`compose`] puts the strips in at the last step.

use crate::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use crate::palette::{c64_color, DARKER};
use crate::vic::{ORIGIN_X, ORIGIN_Y};

/// Pixels of extra picture on each side of the 304 pixel playing area (a multiple of 8: six character columns).
pub const EXTRA: usize = 48;
/// The same without the border (`PICTURE SIZE: NO BORDER / FILL SCREEN`): 304 + 2 x 24 = 352 pixels for 200 lines.
pub const EXTRA_CROP: usize = 24;
/// Width of the frame with the border: 40 border + 48 extra + 304 + 48 + 40 border.
pub const WIDE_WIDTH: usize = WIDTH + 2 * EXTRA;
/// Playing area: first frame column (of the plain frame) and size in pixels.
pub const PLAY_X: usize = ORIGIN_X + 8;
pub const PLAY_W: usize = 304;
pub const PLAY_H: usize = 200;

/// Character columns of tiles kept on each side (6 are visible, one more for the fine scroll).
pub const WIDE_COLS: usize = 7;

/// Brightness (of 256) of the extra picture at the outer edge; at the old edge it is 256 (no step), and it goes down smoothly.
const SHADE_OUTER: u32 = 110;
/// Sprites fade out over this many pixels beyond the old edge.
pub const FADE: usize = 28;

/// Characters and colours of the level columns next to the picture: `WIDE_COLS` columns left of screen column 0 and right of
/// screen column 39, for the 20 rows of the playfield.
#[derive(Clone, Debug)]
pub struct WideCols {
    /// bit 15: there is a map column here; low byte: character, bits 8..11: colour. 0: outside of the map (shown as border)
    tiles: Vec<u16>,
    /// the map could be lined up with the screen; otherwise (title picture, ending, menus of a hack) the extra columns are
    /// empty background
    aligned: bool,
}

const MAP_W: usize = 256;
const MAP_ROWS: usize = 20;

impl Default for WideCols {
    fn default() -> Self {
        Self { tiles: vec![0; 2 * MAP_ROWS * WIDE_COLS], aligned: false }
    }
}

impl WideCols {
    /// The columns next to the picture of `game`. Which map column is on screen column 39 is not just `map_column`: the
    /// scroll counts the column when the copy is started, the copy itself is done in the next frame. The right alignment is
    /// the one under which the screen agrees with the map (a few cells differ where the game changed the screen: broken
    /// blocks, debris, items); when none does, the screen is not a scrolling level and the extra columns stay empty.
    pub fn from_game(game: &giana_core::state::GameState) -> Self {
        Self::from_game_with(game, &WideHistory::default())
    }

    /// [`WideCols::from_game`], where the columns that scrolled out of the picture come from `history` (and the others from the
    /// map).
    pub fn from_game_with(game: &giana_core::state::GameState, history: &WideHistory) -> Self {
        let mut me = Self::default();
        let map = &game.current_map;
        if map.len() < MAP_W * MAP_ROWS {
            return me;
        }
        let mc = game.map_column as i32;
        let expect = if game.column_scroll_pending { mc - 1 } else { mc };
        let score = |last: i32| -> (usize, usize) {
            let (mut hit, mut total) = (0, 0);
            for r in 0..MAP_ROWS {
                for c in 0..40usize {
                    let m = last - 39 + c as i32;
                    if !(0..MAP_W as i32).contains(&m) {
                        continue;
                    }
                    let (on_screen, in_map) = (game.screen_ram[(r + 5) * 40 + c], map[r * MAP_W + m as usize]);
                    // empty cells agree with anything (a banner on a blank screen would line up with every map)
                    if on_screen == 0 && in_map == 0 {
                        continue;
                    }
                    total += 1;
                    if on_screen == in_map {
                        hit += 1;
                    }
                }
            }
            (hit, total)
        };
        let mut best: Option<(i32, usize, usize)> = None;
        for last in [expect, expect - 1, expect + 1] {
            let (hit, total) = score(last);
            if total > 0 && best.is_none_or(|(_, h, t)| hit * t > h * total) {
                best = Some((last, hit, total));
            }
        }
        let Some((last, hit, total)) = best else { return me };
        // no scrolling level on screen (an empty or mostly empty map agrees with anything: look at what is there)
        if hit * 10 < total * 6 {
            return me;
        }
        me.aligned = true;
        for side in 0..2usize {
            for r in 0..MAP_ROWS {
                for k in 0..WIDE_COLS {
                    // screen column of this slot
                    let col = if side == 0 { k as i32 - WIDE_COLS as i32 } else { 40 + k as i32 };
                    let m = last - 39 + col;
                    let kept = if side == 0 { history.left.get(WIDE_COLS - 1 - k) } else { history.right.get(k) };
                    if let Some(t) = kept {
                        me.tiles[(side * MAP_ROWS + r) * WIDE_COLS + k] = 0x8000 | t[r];
                    } else {
                        // before the first column of the level (stage start) the edge column goes on, as the objects
                        // see it (`GameState::wide_tile`), instead of a black hole in the picture
                        let m = m.clamp(0, MAP_W as i32 - 1);
                        let ch = map[r * MAP_W + m as usize];
                        me.tiles[(side * MAP_ROWS + r) * WIDE_COLS + k] = 0x8000 | (game.tile_colour(ch) as u16 & 0x0F) << 8 | ch as u16;
                    }
                }
            }
        }
        me
    }

    /// Character and colour of the tile in playfield row `row` (0..20) at screen column `col`, which is left of 0 or right of
    /// 39. `None`: outside of the map (the border colour).
    pub fn tile(&self, row: usize, col: i32) -> Option<(u8, u8)> {
        if !self.aligned {
            return Some((0, 0));
        }
        let (side, k) = if (-(WIDE_COLS as i32)..0).contains(&col) {
            (0, (col + WIDE_COLS as i32) as usize)
        } else if (40..40 + WIDE_COLS as i32).contains(&col) {
            (1, (col - 40) as usize)
        } else {
            return None;
        };
        let t = *self.tiles.get((side * MAP_ROWS + row) * WIDE_COLS + k)?;
        (t & 0x8000 != 0).then_some((t as u8, (t >> 8) as u8 & 0x0F))
    }

    /// The map column on screen column `col` is within the map and drawn (test helper).
    pub fn is_aligned(&self) -> bool {
        self.aligned
    }
}

/// The columns that have scrolled out of the picture, newest first: the extra area shows them as they left (with the blocks
/// that Giana hit, the diamonds she took), not as the map has them. Fed once per game step with [`WideHistory::observe`]; it
/// finds the scroll by comparing the screen with the one of the step before, so it needs nothing from the game.
#[derive(Clone, Default)]
pub struct WideHistory {
    /// playfield rows of the screen of the last step: character and colour, row major (20 x 40)
    prev: Vec<u16>,
    /// columns left of the picture (nearest first) and right of it
    left: std::collections::VecDeque<[u16; MAP_ROWS]>,
    right: std::collections::VecDeque<[u16; MAP_ROWS]>,
}

impl WideHistory {
    pub fn observe(&mut self, game: &giana_core::state::GameState) {
        let now: Vec<u16> = (0..MAP_ROWS * 40).map(|i| game.screen_ram[200 + i] as u16 | (game.color_ram[200 + i] as u16 & 0x0F) << 8).collect();
        if self.prev.len() == now.len() {
            let ch = |v: &[u16], r: usize, c: usize| v[r * 40 + c] & 0xFF;
            let rows = |f: &dyn Fn(usize, usize) -> bool| (0..MAP_ROWS).filter(|&r| (0..39).all(|c| f(r, c))).count();
            let same = rows(&|r, c| ch(&now, r, c) == ch(&self.prev, r, c));
            let left = rows(&|r, c| ch(&now, r, c) == ch(&self.prev, r, c + 1));
            let right = rows(&|r, c| ch(&now, r, c + 1) == ch(&self.prev, r, c));
            let column = |v: &[u16], c: usize| -> [u16; MAP_ROWS] { std::array::from_fn(|r| v[r * 40 + c]) };
            if same >= 14 {
                // no scroll: a few cells may have changed
            } else if left >= 14 {
                self.left.push_front(column(&self.prev, 0));
                self.right.pop_front();
                self.left.truncate(WIDE_COLS);
            } else if right >= 14 {
                self.right.push_front(column(&self.prev, 39));
                self.left.pop_front();
                self.right.truncate(WIDE_COLS);
            } else {
                // another picture (a new stage, a banner): nothing of the old columns fits any more
                self.left.clear();
                self.right.clear();
            }
        }
        self.prev = now;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The two strips of extra picture, `EXTRA` x 200 pixels each, as they are drawn (shaded, with the sprites in them).
/// `left` ends at the old left edge of the picture, `right` begins at the old right edge.
#[derive(Clone)]
pub struct Strips {
    pub left: Vec<u32>,
    pub right: Vec<u32>,
    /// a menu stands over the game (it is dimmed): the strips take the same step darker
    pub menu_dim: bool,
    /// hide that the game works from the old edges: shade the extra picture towards the outer edge and fade the sprites out
    /// (`Widescreen::masks_edge`; one switch for both)
    pub mask: bool,
    /// the groups of cells of the status bar (first and last column, both included): [`compose`] spreads them over the wide
    /// picture. Empty: no status bar on screen, nothing is moved.
    pub hud: Vec<(usize, usize)>,
    /// the background colour of the status bar lines
    pub hud_bg: u32,
    /// the picture is shown without the border (`NO BORDER`, `FILL SCREEN`): only `EXTRA_CROP` pixels of each strip are
    /// visible, and the status bar is spread over that width
    pub crop: bool,
}

impl Default for Strips {
    fn default() -> Self {
        Self::new()
    }
}

impl Strips {
    pub fn new() -> Self {
        Self { left: vec![0xFF000000; EXTRA * PLAY_H], right: vec![0xFF000000; EXTRA * PLAY_H], menu_dim: false, mask: true, hud: Vec::new(), hud_bg: 0xFF000000, crop: false }
    }

    /// All black (a page over the game that has no picture behind it).
    pub fn blank(&mut self) {
        self.left.fill(0xFF000000);
        self.right.fill(0xFF000000);
    }

    pub(crate) fn side_mut(&mut self, side: usize) -> &mut Vec<u32> {
        if side == 0 {
            &mut self.left
        } else {
            &mut self.right
        }
    }
}

/// How bright the extra picture is at `dist` pixels from the old edge (1 is the pixel next to the edge), of 256: 256 at the
/// edge, then a smooth fall (smoothstep, so there is no visible step or kink at the edge) to `SHADE_OUTER` at the outer end.
/// `dist` 0 is not shaded at all (the status bar lines).
pub fn brightness(dist: usize) -> u32 {
    if dist == 0 {
        return 256;
    }
    let t = (dist.min(EXTRA) as u32 - 1) * 1000 / EXTRA as u32; // 0..1000
    let ease = t * t * (3000 - 2 * t) / 1_000_000; // smoothstep, 0..1000
    256 - (256 - SHADE_OUTER) * ease / 1000
}

/// The colour `index` as the extra picture shows it at `dist` pixels from the old edge. `mask` is the switch of the edge
/// masking (`Strips::mask`): without it the extra picture is as bright as the rest.
pub fn shade(index: u8, dist: usize, menu_dim: bool, mask: bool) -> u32 {
    let index = if menu_dim { DARKER[(index & 15) as usize] } else { index };
    let c = c64_color(index);
    let f = if mask { brightness(dist) } else { 256 };
    let ch = |shift: u32| ((c >> shift & 0xFF) * f / 256) << shift;
    0xFF000000 | ch(16) | ch(8) | ch(0)
}

/// Mixes `top` over `base` with `alpha` of 256.
pub fn mix(base: u32, top: u32, alpha: u32) -> u32 {
    let ch = |shift: u32| (((base >> shift & 0xFF) * (256 - alpha) + (top >> shift & 0xFF) * alpha) / 256) << shift;
    0xFF000000 | ch(16) | ch(8) | ch(0)
}

/// Alpha (of 256) of a sprite pixel `dist` pixels beyond the old edge (1 is next to it): opaque at the edge, gone after `FADE`.
pub fn sprite_alpha(dist: usize, mask: bool) -> u32 {
    if !mask {
        256
    } else if dist > FADE {
        0
    } else {
        256 * (FADE + 1 - dist) as u32 / FADE as u32
    }
}

/// Space between the left edge of the wide playing area and the first group of the status bar, and between the last one and
/// the right edge.
pub const HUD_MARGIN: usize = 8;
/// Status bar lines of the playing area (two character rows).
pub const HUD_LINES: usize = 16;

/// The groups of the status bar in the two top rows of `screen` (GIANA / the score, BONUS, LIVES, STAGE, TIME, as a hack
/// has them): runs of used columns, a gap of two or more empty columns separates two. First and last column of each. Empty
/// when the rows do not look like a status bar (3 to 8 groups, none wider than 12 columns, none in the hidden columns 0 and
/// 39), so a picture that uses these rows otherwise is left alone.
pub fn hud_groups(screen: &[u8]) -> Vec<(usize, usize)> {
    if screen.len() < 80 {
        return Vec::new();
    }
    let used = |c: usize| [screen[c], screen[40 + c]].iter().any(|&b| b != 0 && b != 0x20);
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for c in 0..40 {
        if !used(c) {
            continue;
        }
        match groups.last_mut() {
            Some(g) if c - g.1 <= 2 => g.1 = c,
            _ => groups.push((c, c)),
        }
    }
    let ok = (3..=8).contains(&groups.len()) && groups.iter().all(|&(a, b)| a >= 1 && b <= 38 && b - a < 12);
    if ok {
        groups
    } else {
        Vec::new()
    }
}

/// The part of the wide playing area (`2 x EXTRA + PLAY_W` pixels) that is shown: first pixel and width.
pub fn hud_visible(crop: bool) -> (usize, usize) {
    let e = if crop { EXTRA_CROP } else { EXTRA };
    (EXTRA - e, PLAY_W + 2 * e)
}

/// Where the groups of the status bar go in the visible part of the wide playing area (see `hud_visible`): the first at the
/// left edge, the last at the right edge (each `HUD_MARGIN` inside), the others with the same gaps between all of them. Left
/// pixel of each group, counted in the wide playing area.
pub fn hud_slots(groups: &[(usize, usize)], crop: bool) -> Vec<usize> {
    let (start, width) = hud_visible(crop);
    let widths: Vec<usize> = groups.iter().map(|&(a, b)| (b - a + 1) * 8).collect();
    let span = width - 2 * HUD_MARGIN;
    let free = span.saturating_sub(widths.iter().sum::<usize>());
    let n = groups.len();
    let mut x = start + HUD_MARGIN;
    let mut out = Vec::new();
    for (i, w) in widths.iter().enumerate() {
        out.push(x + if n > 1 { (i * free + (n - 1) / 2) / (n - 1) } else { 0 });
        x += w;
    }
    out
}

/// Pixel x of the left edge of a group within the wide playing area before it is moved (the picture starts at screen pixel 8).
pub fn hud_home(group: (usize, usize)) -> usize {
    EXTRA + group.0 * 8 - 8
}

/// Puts the plain frame and the strips together: `out` is `WIDE_WIDTH` x `HEIGHT`. In the lines of the playing area the strips
/// go between the border and the picture; the lines above and below are border, the frame goes in the middle of them.
pub fn compose(fb: &Framebuffer, strips: &Strips, out: &mut [u32]) {
    let black = c64_color(0);
    for y in 0..HEIGHT {
        let src = &fb.pixels[y * WIDTH..(y + 1) * WIDTH];
        let dst = &mut out[y * WIDE_WIDTH..(y + 1) * WIDE_WIDTH];
        if (ORIGIN_Y..ORIGIN_Y + PLAY_H).contains(&y) {
            let sy = y - ORIGIN_Y;
            dst[..PLAY_X].copy_from_slice(&src[..PLAY_X]);
            dst[PLAY_X..PLAY_X + EXTRA].copy_from_slice(&strips.left[sy * EXTRA..(sy + 1) * EXTRA]);
            dst[PLAY_X + EXTRA..PLAY_X + EXTRA + PLAY_W].copy_from_slice(&src[PLAY_X..PLAY_X + PLAY_W]);
            dst[PLAY_X + EXTRA + PLAY_W..PLAY_X + 2 * EXTRA + PLAY_W].copy_from_slice(&strips.right[sy * EXTRA..(sy + 1) * EXTRA]);
            dst[PLAY_X + 2 * EXTRA + PLAY_W..].copy_from_slice(&src[PLAY_X + PLAY_W..]);
            if sy + 1 == HUD_LINES && !strips.hud.is_empty() {
                spread_hud(out, strips);
            }
        } else {
            dst[..EXTRA].fill(black);
            dst[EXTRA..EXTRA + WIDTH].copy_from_slice(src);
            dst[EXTRA + WIDTH..].fill(black);
        }
    }
}

/// The frame with the strips as a new buffer (for screenshots and tests).
pub fn compose_frame(fb: &Framebuffer, strips: &Strips) -> Vec<u32> {
    let mut out = vec![0xFF000000; WIDE_WIDTH * HEIGHT];
    compose(fb, strips, &mut out);
    out
}

/// The status bar lines of the wide playing area: every group of cells is taken out of its place, the place is made background,
/// and the groups are put in at their slots (see `hud_slots`). Nothing but the position of the cells changes.
fn spread_hud(out: &mut [u32], strips: &Strips) {
    let width = 2 * EXTRA + PLAY_W;
    let slots = hud_slots(&strips.hud, strips.crop);
    let at = |sy: usize| (ORIGIN_Y + sy) * WIDE_WIDTH + PLAY_X;
    let blocks: Vec<Vec<u32>> = strips
        .hud
        .iter()
        .map(|&g| {
            let w = (g.1 - g.0 + 1) * 8;
            (0..HUD_LINES).flat_map(|sy| out[at(sy) + hud_home(g)..at(sy) + hud_home(g) + w].to_vec()).collect()
        })
        .collect();
    for sy in 0..HUD_LINES {
        out[at(sy)..at(sy) + width].fill(strips.hud_bg);
    }
    for ((&g, &x), block) in strips.hud.iter().zip(&slots).zip(&blocks) {
        let w = (g.1 - g.0 + 1) * 8;
        for sy in 0..HUD_LINES {
            out[at(sy) + x..at(sy) + x + w].copy_from_slice(&block[sy * w..(sy + 1) * w]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shading_starts_at_full_brightness_and_falls_smoothly() {
        assert_eq!(brightness(0), 256, "status bar lines are not shaded");
        assert_eq!(brightness(1), 256, "no step at the old edge");
        let b: Vec<u32> = (1..=EXTRA).map(brightness).collect();
        assert!(b.windows(2).all(|w| w[0] >= w[1]), "never gets brighter outwards");
        assert!(b.windows(2).all(|w| w[0] - w[1] <= 12), "no step: {b:?}");
        assert!(b[0] - b[1] <= 1, "starts flat: {b:?}");
        assert!(*b.last().unwrap() <= SHADE_OUTER + 4);
        // without the mask the extra picture is as bright as the rest, and sprites do not fade
        assert_eq!(shade(7, EXTRA, false, false), c64_color(7));
        assert_eq!(sprite_alpha(EXTRA, false), 256);
        assert!(shade(7, EXTRA, false, true) != c64_color(7));
        assert_eq!((sprite_alpha(1, true), sprite_alpha(FADE + 1, true)), (256, 0));
    }
}
