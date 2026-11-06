//! Menu look ("clean panel"): a framed blue panel with a title bar, rows with a yellow selection
//! bar, `< VALUE >` rows, volume bars and a footer with keycap-style key hints.
//!
//! Everything sits on the 40 x 25 character grid (8 x 8 pixel cells) of the 384 x 272 frame. The
//! footer is the one exception: it lives in the bottom border, below the grid. Nothing here owns a
//! background: pages draw the title picture, a black screen ([`backdrop_black`]) or the dimmed
//! game ([`dim`]) first and then the panel on top.

use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::palette::{c64_color, C64_PALETTE};
use crate::padicons::{Icon, ICON_H};
use giana_render::text::{cell_px, draw_text_px, fill_rect_px};
use giana_render::vic::{ORIGIN_X, ORIGIN_Y};

/// Character grid size.
pub const COLS: usize = 40;
pub const ROWS: usize = 25;

/// C64 palette indices of the style.
pub mod color {
    pub const BLACK: u8 = 0;
    pub const WHITE: u8 = 1;
    pub const CYAN: u8 = 3;
    pub const BLUE: u8 = 6;
    pub const YELLOW: u8 = 7;
    pub const DARK_GREY: u8 = 11;
    pub const GREY: u8 = 12;
    pub const LIGHT_BLUE: u8 = 14;
    pub const LIGHT_GREY: u8 = 15;

    /// Panel background.
    pub const FILL: u8 = BLUE;
    /// Frame lines, title bar, arrows of an unselected value.
    pub const FRAME: u8 = LIGHT_BLUE;
    /// Normal text.
    pub const TEXT: u8 = WHITE;
    /// Values (unselected row).
    pub const VALUE: u8 = CYAN;
    /// Disabled rows, notes, empty volume cells.
    pub const DIM: u8 = GREY;
    /// Selection bar. Text on it is black: about 17:1 contrast (the spike's blue on yellow was
    /// dark and muddy, white on light blue would only be 2.5:1).
    pub const SELECT_BG: u8 = YELLOW;
    pub const SELECT_FG: u8 = BLACK;
    /// Empty volume cells on the selection bar.
    pub const SELECT_DIM: u8 = DARK_GREY;
    /// Footer bar and key caps.
    pub const FOOTER_BG: u8 = BLACK;
    pub const KEYCAP_BG: u8 = LIGHT_GREY;
    pub const KEYCAP_FG: u8 = BLACK;
}
use color as c;

fn px(index: u8) -> u32 {
    c64_color(index)
}

/// Distance between item rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pitch {
    /// 16 px, two character rows.
    Roomy,
    /// 12 px, one and a half character rows (long lists).
    Tight,
}

impl Pitch {
    pub fn px(self) -> usize {
        match self {
            Pitch::Roomy => 16,
            Pitch::Tight => 12,
        }
    }
}

/// How a row is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    Normal,
    Selected,
    Disabled,
}

/// Pixels the shadow sticks out right and below the panel.
pub const SHADOW: usize = 4;

/// A framed panel on the character grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub col: usize,
    pub row: usize,
    pub w: usize,
    pub h: usize,
    pub titled: bool,
    pub pitch: Pitch,
}

impl Panel {
    /// Column in the panel where labels start; the marker sits two columns left of it.
    pub const LABEL_COL: usize = 4;

    /// Rows above the first item: frame, title bar, one empty row (without a title: frame, empty).
    pub fn head_rows(titled: bool) -> usize {
        if titled {
            3
        } else {
            2
        }
    }

    /// Panel at a fixed place.
    pub fn new(col: usize, row: usize, w: usize, h: usize, titled: bool) -> Panel {
        Panel { col, row, w, h, titled, pitch: Pitch::Roomy }
    }

    /// Panel of size `w` x `h` centred on the grid.
    pub fn centered(w: usize, h: usize, titled: bool) -> Panel {
        Panel::new((COLS - w) / 2, (ROWS - h) / 2, w, h, titled)
    }

    /// Height in rows of a panel with `items` evenly spaced rows plus `extra_rows` character rows
    /// below them (notes, separators).
    pub fn height_for(items: usize, pitch: Pitch, titled: bool, extra_rows: usize) -> usize {
        let list_px = if items == 0 { 0 } else { (items - 1) * pitch.px() + 8 };
        // one empty row and the bottom frame line after the content
        Panel::head_rows(titled) + list_px.div_ceil(8) + extra_rows + 2
    }

    /// Panel for a list of `items` rows, centred on the grid.
    pub fn for_list(items: usize, w: usize, pitch: Pitch, titled: bool, extra_rows: usize) -> Panel {
        let h = Panel::height_for(items, pitch, titled, extra_rows);
        Panel { pitch, ..Panel::centered(w, h, titled) }
    }

    /// The same panel centred vertically in the part of `view` above its footer (on the whole frame: unchanged).
    pub fn in_view(self, view: View) -> Panel {
        if view.is_crop() {
            Panel { row: view.grid_rows().saturating_sub(self.h) / 2, ..self }
        } else {
            self
        }
    }

    /// Same panel, with its top edge on `row`.
    pub fn at_row(self, row: usize) -> Panel {
        Panel { row, ..self }
    }

    /// First character row (absolute) below the head.
    pub fn content_row(&self) -> usize {
        self.row + Panel::head_rows(self.titled)
    }

    /// Frame-buffer y of item `i`.
    pub fn item_y(&self, i: usize) -> isize {
        cell_px(0, self.content_row()).1 + (i * self.pitch.px()) as isize
    }

    /// Frame-buffer x of text of `chars` characters centred in the panel, to the pixel (an odd difference of
    /// columns gives a 4 pixel offset, not a whole column to one side).
    pub fn centered_x(&self, chars: usize) -> isize {
        let (x, _) = cell_px(self.col, 0);
        x + ((self.w * 8) as isize - (chars * 8) as isize) / 2
    }

    /// Frame-buffer y of a character row counted from the top of the panel.
    pub fn row_y(&self, row_in_panel: usize) -> isize {
        cell_px(0, self.row + row_in_panel).1
    }

    /// Frame-buffer x of a character column counted from the left edge of the panel.
    pub fn col_x(&self, col_in_panel: usize) -> isize {
        cell_px(self.col + col_in_panel, 0).0
    }

    /// Pixel rectangle `(x, y, w, h)` of the panel including its shadow.
    pub fn pixel_rect(&self) -> (isize, isize, usize, usize) {
        let (x, y) = cell_px(self.col, self.row);
        (x, y, self.w * 8 + SHADOW, self.h * 8 + SHADOW)
    }

    /// Longest label that fits in a plain item row.
    pub fn max_label_len(&self) -> usize {
        self.w.saturating_sub(Panel::LABEL_COL + 2)
    }

    /// Widest value column a value row can have next to a label of `label_len` characters, or
    /// None if the label alone does not fit.
    pub fn max_value_len(&self, label_len: usize) -> Option<usize> {
        // label, one empty cell, `<` value `>`, then two cells to the right edge
        self.w.checked_sub(Panel::LABEL_COL + label_len + 1 + 2 + 2)
    }

    /// Smallest panel width for labels up to `label_len` and value columns up to `value_len`.
    pub fn min_width(label_len: usize, value_len: usize) -> usize {
        Panel::LABEL_COL + label_len + 1 + value_len + 2 + 2
    }

    /// Draw shadow, fill, frame and the title (centred in the title bar).
    pub fn draw(&self, fb: &mut Framebuffer, title: Option<&str>) {
        let (x, y) = cell_px(self.col, self.row);
        let (w, h) = (self.w * 8, self.h * 8);
        // hard shadow to lift the panel off a busy picture
        fill_rect_px(fb, x + SHADOW as isize, y + SHADOW as isize, w, h, px(c::BLACK));
        fill_rect_px(fb, x, y, w, h, px(c::FILL));
        for i in 0..self.w {
            let (t, b) = match i {
                0 => ('┌', '└'),
                i if i == self.w - 1 => ('┐', '┘'),
                _ => ('─', '─'),
            };
            glyph(fb, self.col + i, self.row, t, c::FRAME);
            glyph(fb, self.col + i, self.row + self.h - 1, b, c::FRAME);
        }
        for j in 1..self.h - 1 {
            glyph(fb, self.col, self.row + j, '│', c::FRAME);
            glyph(fb, self.col + self.w - 1, self.row + j, '│', c::FRAME);
        }
        if let (true, Some(t)) = (self.titled, title) {
            fill_rect_px(fb, x + 8, y + 8, w - 16, 8, px(c::FRAME));
            draw_text_px(fb, self.centered_x(t.chars().count()), y + 8, t, px(c::BLACK));
        }
    }

    /// Horizontal line across the panel, joined to the frame, at a row counted from the panel top.
    pub fn separator(&self, fb: &mut Framebuffer, row_in_panel: usize) {
        let row = self.row + row_in_panel;
        for i in 0..self.w {
            let g = match i {
                0 => '├',
                i if i == self.w - 1 => '┤',
                _ => '─',
            };
            glyph(fb, self.col + i, row, g, c::FRAME);
        }
    }

    /// Rows of `items` evenly spaced items, in character rows.
    pub fn list_rows(items: usize, pitch: Pitch) -> usize {
        if items == 0 {
            0
        } else {
            ((items - 1) * pitch.px() + 8).div_ceil(8)
        }
    }

    /// The yellow bar and ▶ marker across the whole panel; returns the text colour for the row.
    pub fn row_colors(&self, fb: &mut Framebuffer, y: isize, state: ItemState, nudge: usize) -> u8 {
        self.row_colors_w(fb, y, state, nudge, self.w - 2)
    }

    /// Same with a bar of only `bar_cols` character columns (from the first inside the frame).
    pub fn row_colors_w(&self, fb: &mut Framebuffer, y: isize, state: ItemState, nudge: usize, bar_cols: usize) -> u8 {
        match state {
            ItemState::Normal => c::TEXT,
            ItemState::Selected => {
                // two pixels taller than the text so the letters do not touch the bar
                fill_rect_px(fb, self.col_x(1), y - 1, bar_cols * 8, 10, px(c::SELECT_BG));
                draw_text_px(fb, self.col_x(2) + (nudge & 1) as isize, y, "▶", px(c::SELECT_FG));
                c::SELECT_FG
            }
            ItemState::Disabled => c::DIM,
        }
    }

    /// Plain item row. `nudge` (0 or 1, e.g. a blink phase) moves the marker one pixel.
    pub fn item(&self, fb: &mut Framebuffer, y: isize, label: &str, state: ItemState, nudge: usize) {
        debug_assert!(label.chars().count() <= self.max_label_len(), "label {label:?} too long for {self:?}");
        let fg = self.row_colors(fb, y, state, nudge);
        draw_text_px(fb, self.col_x(Panel::LABEL_COL), y, label, px(fg));
    }

    /// Item centred in the panel (a lone "BACK" below a separator).
    pub fn item_centered(&self, fb: &mut Framebuffer, y: isize, label: &str, state: ItemState, nudge: usize) {
        debug_assert!(label.chars().count() <= self.max_label_len(), "label {label:?} too long for {self:?}");
        let fg = self.row_colors(fb, y, state, nudge);
        draw_text_px(fb, self.centered_x(label.chars().count()), y, label, px(fg));
    }

    /// Panel columns of the `<` and `>` glyphs of a value column `value_w` characters wide.
    pub fn value_arrows(&self, value_w: usize) -> (usize, usize) {
        let right = self.w - 3;
        (right - value_w - 1, right)
    }

    /// Label on the left, `< VALUE >` at the right with the value centred in a column of
    /// `value_w` characters.
    #[allow(clippy::too_many_arguments)]
    pub fn value_row(&self, fb: &mut Framebuffer, y: isize, label: &str, value: &str, value_w: usize, state: ItemState, nudge: usize) {
        debug_assert!(self.max_value_len(label.chars().count()).is_some_and(|m| m >= value_w && value.chars().count() <= value_w), "{label:?} {value:?} do not fit {self:?}");
        let fg = self.row_colors(fb, y, state, nudge);
        draw_text_px(fb, self.col_x(Panel::LABEL_COL), y, label, px(fg));
        let (left, right) = self.value_arrows(value_w);
        let pad = value_w.saturating_sub(value.chars().count()) / 2;
        let val = if state == ItemState::Normal { c::VALUE } else { fg };
        draw_text_px(fb, self.col_x(left + 1 + pad), y, value, px(val));
        self.arrows(fb, y, left, right, state, nudge);
    }

    /// Volume row: label and `< ■■■■■■□□□□ >` with `level` of `max` cells filled.
    #[allow(clippy::too_many_arguments)]
    pub fn volume_row(&self, fb: &mut Framebuffer, y: isize, label: &str, level: usize, max: usize, state: ItemState, nudge: usize) {
        self.volume_row_in(fb, y, label, level, max, max, state, nudge);
    }

    /// Same with the bar centred in a value column `col_w` characters wide, so that the arrows
    /// stand where those of the value rows do.
    #[allow(clippy::too_many_arguments)]
    pub fn volume_row_in(&self, fb: &mut Framebuffer, y: isize, label: &str, level: usize, max: usize, col_w: usize, state: ItemState, nudge: usize) {
        let fg = self.row_colors(fb, y, state, nudge);
        draw_text_px(fb, self.col_x(Panel::LABEL_COL), y, label, px(fg));
        let (left, right) = self.value_arrows(col_w);
        let first = left + 1 + col_w.saturating_sub(max) / 2;
        let (on, off) = match state {
            ItemState::Selected => (c::SELECT_FG, c::SELECT_DIM),
            ItemState::Normal => (c::YELLOW, c::DIM),
            ItemState::Disabled => (c::DIM, c::DARK_GREY),
        };
        for k in 0..max {
            let (g, col) = if k < level { ("■", on) } else { ("□", off) };
            draw_text_px(fb, self.col_x(first + k), y, g, px(col));
        }
        self.arrows(fb, y, left, right, state, nudge);
    }

    fn arrows(&self, fb: &mut Framebuffer, y: isize, left: usize, right: usize, state: ItemState, nudge: usize) {
        let (color, lean) = match state {
            ItemState::Normal => (c::FRAME, 0),
            ItemState::Selected => (c::SELECT_FG, (nudge & 1) as isize),
            ItemState::Disabled => (c::DIM, 0),
        };
        // on the selected row the arrows lean outwards with the nudge
        draw_text_px(fb, self.col_x(left) - lean, y, "◀", px(color));
        draw_text_px(fb, self.col_x(right) + lean, y, "▶", px(color));
    }

    /// Centred message line inside the panel (a question, a status), row counted from the top.
    pub fn message(&self, fb: &mut Framebuffer, row_in_panel: usize, text: &str) {
        self.message_colored(fb, row_in_panel, text, c::TEXT);
    }

    pub fn message_colored(&self, fb: &mut Framebuffer, row_in_panel: usize, text: &str, color: u8) {
        debug_assert!(text.chars().count() <= self.w - 2, "message {text:?} too wide for {self:?}");
        draw_text_px(fb, self.centered_x(text.chars().count()), self.row_y(row_in_panel), text, px(color));
    }

    /// Small dim note inside the panel at a character row counted from the top (left aligned
    /// under the labels' margin).
    pub fn note(&self, fb: &mut Framebuffer, row_in_panel: usize, text: &str) {
        debug_assert!(text.chars().count() <= self.w - 4, "note {text:?} too wide for {self:?}");
        draw_text_px(fb, self.col_x(2), self.row_y(row_in_panel), text, px(c::DIM));
    }

    /// Like [`Panel::note`] in another colour.
    pub fn note_colored(&self, fb: &mut Framebuffer, row_in_panel: usize, text: &str, color: u8) {
        debug_assert!(text.chars().count() <= self.w - 4, "note {text:?} too wide for {self:?}");
        draw_text_px(fb, self.col_x(2), self.row_y(row_in_panel), text, px(color));
    }

    /// Text at a column and character row counted from the panel's top left corner.
    pub fn text_at(&self, fb: &mut Framebuffer, col_in_panel: usize, row_in_panel: usize, text: &str, color: u8) {
        debug_assert!(col_in_panel >= 1 && col_in_panel + text.chars().count() < self.w, "text {text:?} at {col_in_panel} outside {self:?}");
        draw_text_px(fb, self.col_x(col_in_panel), self.row_y(row_in_panel), text, px(color));
    }

    /// Text right aligned so that it ends at the column `end_col` (exclusive) of the panel.
    pub fn text_right(&self, fb: &mut Framebuffer, end_col: usize, row_in_panel: usize, text: &str, color: u8) {
        let n = text.chars().count();
        self.text_at(fb, end_col - n, row_in_panel, text, color);
    }

    /// An input field: a black box `w_chars` characters wide with a light blue line, text inside
    /// (with a `_` after it while `cursor` is true). `y` is the pixel row of the text.
    pub fn field(&self, fb: &mut Framebuffer, y: isize, col_in_panel: usize, w_chars: usize, text: &str, cursor: bool) {
        let x = self.col_x(col_in_panel);
        let w = w_chars * 8;
        fill_rect_px(fb, x - 3, y - 3, w + 6, 14, px(c::FRAME));
        fill_rect_px(fb, x - 2, y - 2, w + 4, 12, px(c::BLACK));
        let shown = if cursor { format!("{text}_") } else { text.to_string() };
        debug_assert!(shown.chars().count() <= w_chars, "field text {shown:?} too long");
        draw_text_px(fb, x, y, &shown, px(c::YELLOW));
    }
}

fn glyph(fb: &mut Framebuffer, col: usize, row: usize, ch: char, color: u8) {
    let (x, y) = cell_px(col, row);
    draw_text_px(fb, x, y, &ch.to_string(), px(color));
}

// ---------- backgrounds ----------

/// Plain black screen (options, scores, network).
pub fn backdrop_black(fb: &mut Framebuffer) {
    fb.clear(px(c::BLACK));
}

/// Darken the whole frame, for panels over the running game.
pub fn dim(fb: &mut Framebuffer) {
    use giana_render::palette::DARKER;
    for p in fb.pixels.iter_mut() {
        if let Some(i) = C64_PALETTE.iter().position(|&k| k == *p) {
            *p = px(DARKER[i]);
        }
    }
}

// ---------- footer ----------

/// One cap of a hint: a key name on a key cap, or a controller glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cap {
    Text(String),
    Icon(Icon),
}

impl Cap {
    fn width(&self) -> usize {
        match self {
            Cap::Text(t) => 8 + 8 * t.chars().count(),
            Cap::Icon(i) => i.width(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Cap::Text(t) => t.clone(),
            Cap::Icon(i) => i.name(),
        }
    }
}

/// One hint: the key caps or glyphs and what they do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub caps: Vec<Cap>,
    pub action: String,
}

impl Hint {
    pub fn new(key: impl Into<String>, action: impl Into<String>) -> Hint {
        Hint { caps: vec![Cap::Text(key.into())], action: action.into() }
    }

    pub fn with_caps(caps: Vec<Cap>, action: impl Into<String>) -> Hint {
        Hint { caps, action: action.into() }
    }

    pub fn icons(icons: Vec<Icon>, action: impl Into<String>) -> Hint {
        Hint { caps: icons.into_iter().map(Cap::Icon).collect(), action: action.into() }
    }

    /// Hint for a bound keyboard key, labelled like the key bindings page.
    pub fn bound(key: sdl2::keyboard::Keycode, action: impl Into<String>) -> Hint {
        Hint::new(crate::bindings::key_label(key), action)
    }

    /// Plain words of the caps, for tests.
    pub fn key_text(&self) -> String {
        self.caps.iter().map(Cap::label).collect::<Vec<_>>().join("+")
    }

    fn caps_width(&self) -> usize {
        self.caps.iter().map(Cap::width).sum::<usize>() + CAP_GAP * self.caps.len().saturating_sub(1)
    }

    /// Width in pixels: the caps (a key cap has 4 pixels of end on each side), gap, action text.
    fn width(&self) -> usize {
        if self.action.is_empty() {
            self.caps_width()
        } else {
            self.caps_width() + KEY_GAP + 8 * self.action.chars().count()
        }
    }
}

const KEY_GAP: usize = 6;
const CAP_GAP: usize = 3;
const HINT_GAP: usize = 16;
/// Footer content starts this far from the frame edge (and ends as far from the other).
pub const FOOTER_MARGIN: usize = 16;
/// Top pixel row and height of the footer bar: the bottom border, below the grid (ends at y 236).
pub const FOOTER_Y: usize = 240;
pub const FOOTER_H: usize = 18;
/// Widest footer content.
pub const FOOTER_MAX: usize = WIDTH - 2 * FOOTER_MARGIN;
/// Top of the strip with the game controls on the main and the pause menu, and of a notice.
pub const STRIP_Y: usize = 220;
pub const STRIP_H: usize = 16;

/// The part of the 384 x 272 frame that the window shows. Menus over a running game have to stay inside it: with the
/// scaling modes that cut the border (`present`), only the playing area is visible, and the footer, the notices and the
/// panels then sit inside it, with the footer at its bottom edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl View {
    /// The whole frame, border included (menus without a game behind them).
    pub const FULL: View = View { x: 0, y: 0, w: WIDTH, h: HEIGHT };
    /// The playing area: the 38 columns x 25 rows of the screen (same part as `present::PLAY`).
    pub const CROP: View = View { x: ORIGIN_X + 8, y: ORIGIN_Y, w: 304, h: 200 };

    /// `crop`: the cut-down picture, otherwise the whole frame.
    pub fn new(crop: bool) -> View {
        if crop {
            View::CROP
        } else {
            View::FULL
        }
    }

    pub fn is_crop(&self) -> bool {
        *self != View::FULL
    }

    /// Top row, height and text row of the footer bar. On the whole frame it stands in the bottom border; in the cut-down
    /// picture it is the last 16 rows of it.
    pub fn footer_y(&self) -> usize {
        if self.is_crop() {
            self.y + self.h - self.footer_h()
        } else {
            FOOTER_Y
        }
    }

    pub fn footer_h(&self) -> usize {
        if self.is_crop() {
            STRIP_H
        } else {
            FOOTER_H
        }
    }

    fn footer_text_dy(&self) -> usize {
        if self.is_crop() {
            4
        } else {
            5
        }
    }

    /// Distance of the footer content from the edge, and between two hints.
    pub fn footer_margin(&self) -> usize {
        if self.is_crop() {
            4
        } else {
            FOOTER_MARGIN
        }
    }

    pub fn hint_gap(&self) -> usize {
        if self.is_crop() {
            8
        } else {
            HINT_GAP
        }
    }

    /// Widest footer content.
    pub fn footer_max(&self) -> usize {
        self.w - 2 * self.footer_margin()
    }

    /// Top of a pill that stands where a notice goes: above the footer on the whole frame; in the cut-down picture the
    /// notice takes the place of the hints inside the footer bar.
    pub fn notice_y(&self) -> isize {
        if self.is_crop() {
            self.footer_y() as isize + 2
        } else {
            NOTICE_Y
        }
    }

    /// Lowest pixel row (exclusive) the frame of a panel may reach: the footer starts there.
    pub fn panel_bottom(&self) -> usize {
        if self.is_crop() {
            self.footer_y()
        } else {
            NOTICE_Y as usize
        }
    }

    /// Character rows of the grid that panels may use: all of them on the whole frame, in the cut-down picture the ones
    /// above the footer.
    pub fn grid_rows(&self) -> usize {
        if self.is_crop() {
            (self.panel_bottom() - self.y) / 8
        } else {
            ROWS
        }
    }

    /// Columns of the wide panels (notes, value rows): in the cut-down picture they keep 8 pixels from its edges.
    pub fn wide(&self) -> usize {
        if self.is_crop() {
            self.w / 8 - 2
        } else {
            38
        }
    }

    /// Left end of something `w` pixels wide that is centred in the view.
    fn centered_x0(&self, w: usize) -> isize {
        (self.x + (self.w - w.min(self.w)) / 2) as isize
    }
}

/// Width in pixels all hints need, including the gaps between them.
pub fn hints_width(hints: &[Hint]) -> usize {
    hints_width_gap(hints, HINT_GAP)
}

/// Like [`hints_width`] with the gap of `view`.
pub fn hints_width_in(hints: &[Hint], view: View) -> usize {
    hints_width_gap(hints, view.hint_gap())
}

fn hints_width_gap(hints: &[Hint], gap: usize) -> usize {
    hints.iter().map(Hint::width).sum::<usize>() + gap * hints.len().saturating_sub(1)
}

/// Whether the note still fits to the right of the centred hints (otherwise [`footer`] leaves it out).
pub fn note_fits(hints: &[Hint], note: &str) -> bool {
    note_fits_in(View::FULL, hints, note)
}

fn note_fits_in(view: View, hints: &[Hint], note: &str) -> bool {
    let w = hints_width_in(hints, view);
    let end = view.centered_x0(w) + w as isize;
    end + view.hint_gap() as isize <= (view.x + view.w - view.footer_margin()) as isize - 8 * note.chars().count() as isize
}

/// Draw the footer bar with hints. `note` (the version, say) goes to the right end in dim text, so
/// it can never touch a panel.
pub fn footer(fb: &mut Framebuffer, hints: &[Hint], note: Option<&str>) {
    footer_in(fb, View::FULL, hints, note)
}

/// [`footer`] inside `view`: the bar covers the width of the view and stands at its bottom edge when it is cut down.
pub fn footer_in(fb: &mut Framebuffer, view: View, hints: &[Hint], note: Option<&str>) {
    let (y, h) = (view.footer_y() as isize, view.footer_h());
    fill_rect_px(fb, view.x as isize, y, view.w, h, px(c::FOOTER_BG));
    fill_rect_px(fb, view.x as isize, y, view.w, 1, px(c::DARK_GREY));
    // centred in the view, like the strip of controls above it; the note keeps to the right end
    let ty = y + view.footer_text_dy() as isize;
    hint_line_at(fb, view.centered_x0(hints_width_in(hints, view)), ty, hints, view.hint_gap());
    if let Some(n) = note.filter(|n| note_fits_in(view, hints, n)) {
        let nx = (view.x + view.w - view.footer_margin()) as isize - 8 * n.chars().count() as isize;
        draw_text_px(fb, nx, ty, n, px(c::DIM));
    }
}

/// A black strip above the footer with hints, centred (the game controls under a menu).
pub fn strip(fb: &mut Framebuffer, hints: &[Hint]) {
    strip_in(fb, View::FULL, hints)
}

/// [`strip`] inside `view`: directly above its footer.
pub fn strip_in(fb: &mut Framebuffer, view: View, hints: &[Hint]) {
    let y = if view.is_crop() { view.footer_y() - STRIP_H } else { STRIP_Y };
    fill_rect_px(fb, view.x as isize, y as isize, view.w, STRIP_H, px(c::FOOTER_BG));
    hint_line_at(fb, view.centered_x0(hints_width_in(hints, view)), (y + 4) as isize, hints, view.hint_gap());
}

/// Hints in a row, the text baseline row of the letters at `ty`, starting at `x0`.
fn hint_line_at(fb: &mut Framebuffer, x0: isize, ty: isize, hints: &[Hint], gap: usize) {
    let mut x = x0;
    for h in hints {
        for (k, cap) in h.caps.iter().enumerate() {
            if k > 0 {
                x += CAP_GAP as isize;
            }
            match cap {
                Cap::Text(t) => {
                    let kw = 8 * t.chars().count();
                    keycap(fb, x, ty - 1, kw);
                    draw_text_px(fb, x + 4, ty, t, px(c::KEYCAP_FG));
                }
                Cap::Icon(i) => i.draw(fb, x, ty - 1 + (10 - ICON_H as isize) / 2),
            }
            x += cap.width() as isize;
        }
        if !h.action.is_empty() {
            draw_text_px(fb, x + KEY_GAP as isize, ty, &h.action, px(c::TEXT));
            x += (KEY_GAP + 8 * h.action.chars().count()) as isize;
        }
        x += gap as isize;
    }
}

/// Key cap of 10 pixels height: a body `key_w` pixels wide at x + 4 with rounded ends 4 pixels
/// wide on both sides (so it occupies `key_w + 8` pixels from `x`).
fn keycap(fb: &mut Framebuffer, x: isize, y: isize, key_w: usize) {
    // (distance from the body edge, height)
    const END: [(isize, usize); 4] = [(0, 10), (1, 8), (2, 6), (3, 4)];
    fill_rect_px(fb, x + 4, y, key_w, 10, px(c::KEYCAP_BG));
    for (d, h) in END {
        let top = y + (10 - h as isize) / 2;
        fill_rect_px(fb, x + 3 - d, top, 1, h, px(c::KEYCAP_BG));
        fill_rect_px(fb, x + 4 + key_w as isize + d, top, 1, h, px(c::KEYCAP_BG));
    }
}

/// A key cap with `text` on it, the text row of the letters at `ty`; returns its width.
pub fn cap_text(fb: &mut Framebuffer, x: isize, ty: isize, text: &str) -> usize {
    let kw = 8 * text.chars().count();
    keycap(fb, x, ty - 1, kw);
    draw_text_px(fb, x + 4, ty, text, px(c::KEYCAP_FG));
    kw + 8
}

/// Width of a key cap with `n` characters.
pub fn cap_width(n: usize) -> usize {
    8 * n + 8
}

// ---------- toasts ----------

/// Colour of a pill's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillTone {
    /// white: information
    Info,
    /// yellow: something went wrong, something to read
    Notice,
}

/// Width in pixels of a pill with `text`.
pub fn pill_width(text: &str) -> usize {
    8 * text.chars().count() + 16
}

/// A small framed message, centred on the frame with its top edge at `y`: 12 pixels high, blue with
/// a light blue line. Used for notices in the menus and for messages over the game.
pub fn pill(fb: &mut Framebuffer, y: isize, text: &str, tone: PillTone) {
    pill_in(fb, View::FULL, y, text, tone)
}

/// [`pill`] centred in `view` (the cut-down picture is not centred on the frame).
pub fn pill_in(fb: &mut Framebuffer, view: View, y: isize, text: &str, tone: PillTone) {
    let w = pill_width(text).min(view.w);
    pill_at_in(fb, view, view.centered_x0(w) + (w / 2) as isize, y, text, tone)
}

/// A pill like [`pill`], centred on the pixel column `cx` instead of the frame.
pub fn pill_at(fb: &mut Framebuffer, cx: isize, y: isize, text: &str, tone: PillTone) {
    pill_at_in(fb, View::FULL, cx, y, text, tone)
}

/// [`pill_at`] kept inside `view`.
pub fn pill_at_in(fb: &mut Framebuffer, view: View, cx: isize, y: isize, text: &str, tone: PillTone) {
    let w = pill_width(text).min(view.w) as isize;
    let x = (cx - w / 2).clamp(view.x as isize, (view.x + view.w) as isize - w);
    fill_rect_px(fb, x + 1, y, w as usize - 2, 12, px(c::FRAME));
    fill_rect_px(fb, x, y + 1, w as usize, 10, px(c::FRAME));
    fill_rect_px(fb, x + 2, y + 1, w as usize - 4, 10, px(c::FILL));
    fill_rect_px(fb, x + 1, y + 2, w as usize - 2, 8, px(c::FILL));
    let color = match tone {
        PillTone::Info => c::TEXT,
        PillTone::Notice => c::YELLOW,
    };
    draw_text_px(fb, x + 8, y + 2, text, px(color));
}

/// Horizontal centre of what is drawn in the rows `y0..y1` (pixels that differ from the colour at
/// the left edge of each row), or `None` when the rows are empty. Used to stand a hint under the
/// banner text of the game, which the original does not centre exactly.
pub fn content_center_x(fb: &Framebuffer, y0: usize, y1: usize) -> Option<isize> {
    let (mut lo, mut hi) = (usize::MAX, 0usize);
    for y in y0..y1.min(HEIGHT) {
        let row = &fb.pixels[y * WIDTH..(y + 1) * WIDTH];
        let bg = row[0];
        for (x, &p) in row.iter().enumerate() {
            if p != bg {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
    }
    (lo <= hi).then(|| (lo + hi).div_ceil(2) as isize)
}

/// Top of a pill that stands where a notice goes: just above the footer.
pub const NOTICE_Y: isize = 224;

/// Small dim text anywhere on the grid (the caller keeps it off the panels, see [`footer`]'s
/// `note` for the usual place of the version).
pub fn dim_text(fb: &mut Framebuffer, col: usize, row: usize, text: &str) {
    let (x, y) = cell_px(col, row);
    draw_text_px(fb, x, y, text, px(c::DIM));
}

#[cfg(test)]
mod tests {
    use super::*;
    use giana_render::framebuffer::HEIGHT;

    /// Frame area of the grid in pixels: x 32..352, y 36..236.
    fn grid_rect() -> (isize, isize, isize, isize) {
        let (x0, y0) = cell_px(0, 0);
        let (x1, y1) = cell_px(COLS, ROWS);
        (x0, y0, x1, y1)
    }

    fn inside_grid(p: &Panel) -> bool {
        let (gx0, gy0, gx1, gy1) = grid_rect();
        let (x, y) = cell_px(p.col, p.row);
        let (x1, y1) = cell_px(p.col + p.w, p.row + p.h);
        // the shadow may reach into the border, but not out of the frame
        x >= gx0 && y >= gy0 && x1 <= gx1 && y1 <= gy1 && (x1 as usize + SHADOW) <= WIDTH && (y1 as usize + SHADOW) <= HEIGHT
    }

    #[test]
    fn lists_are_centred_and_fit_the_grid() {
        for pitch in [Pitch::Roomy, Pitch::Tight] {
            for titled in [true, false] {
                for items in 0..=9 {
                    let p = Panel::for_list(items, 24, pitch, titled, 0);
                    assert!(inside_grid(&p), "{items} items {pitch:?} {titled}: {p:?}");
                    assert_eq!(p.col * 2 + p.w, COLS, "horizontally centred");
                    assert!(p.row.abs_diff(ROWS - p.row - p.h) <= 1, "vertically centred");
                }
            }
        }
    }

    #[test]
    fn items_stay_inside_the_panel_and_do_not_overlap() {
        for pitch in [Pitch::Roomy, Pitch::Tight] {
            for titled in [true, false] {
                let p = Panel::for_list(8, 20, pitch, titled, 2);
                let (px0, py0, _, _) = p.pixel_rect();
                let frame_bottom = py0 + ((p.h - 1) * 8) as isize;
                let head_bottom = py0 + (Panel::head_rows(titled) * 8) as isize;
                for i in 0..8 {
                    let (top, bottom) = (p.item_y(i) - 1, p.item_y(i) + 9); // selection bar extent
                    assert!(top >= head_bottom - 1 && bottom <= frame_bottom, "{pitch:?} item {i}");
                    if i > 0 {
                        assert!(top >= p.item_y(i - 1) + 9, "bars of rows {} and {i} overlap", i - 1);
                    }
                    assert!(p.col_x(1) > px0);
                }
                // two extra rows below the last item still fit above the frame
                assert!(p.item_y(7) + 8 + 2 * 8 <= frame_bottom);
            }
        }
    }

    #[test]
    fn longest_menu_labels_fit() {
        // the longest labels of the pages (main, pause, options, network)
        let labels = ["HIGH SCORES", "CONTROLLER", "QUIT TO TITLE", "RESUME GAME", "RETURN TO TITLE"];
        let p = Panel::for_list(8, 22, Pitch::Roomy, true, 0);
        for l in labels {
            assert!(l.len() <= p.max_label_len(), "{l}");
        }
    }

    #[test]
    fn value_rows_keep_label_and_value_apart() {
        let rows = [("FULLSCREEN", "OFF"), ("PICTURE SIZE", "WITH BORDER"), ("50 HZ MODE", "AUTO"), ("WINNER", "FURTHEST"), ("VOLUME", "")];
        let label_max = rows.iter().map(|r| r.0.len()).max().unwrap();
        let value_w = 12;
        let w = Panel::min_width(label_max, value_w);
        assert!(w <= 36, "options panel needs {w} columns");
        let p = Panel::for_list(7, w, Pitch::Roomy, true, 2);
        assert!(p.max_value_len(label_max).unwrap() >= value_w);
        let (left, right) = p.value_arrows(value_w);
        assert!(Panel::LABEL_COL + label_max < left, "label reaches the arrows");
        assert!(right < p.w - 1, "arrow touches the frame");
        // the volume bar (10 cells) fits the same column
        assert!(10 <= value_w || p.value_arrows(10).0 > Panel::LABEL_COL + 6);
        // a too narrow panel is detected
        assert_eq!(Panel::new(0, 0, 10, 10, true).max_value_len(10), None);
    }

    #[test]
    fn drawing_stays_inside_the_panel_and_its_shadow() {
        giana_core::skip_without_data!();
        let mut fb = Framebuffer::new();
        fb.clear(px(c::CYAN));
        let p = Panel::for_list(6, 30, Pitch::Roomy, true, 2);
        p.draw(&mut fb, Some("OPTIONS"));
        p.separator(&mut fb, p.h - 4);
        p.item(&mut fb, p.item_y(0), "FIRST", ItemState::Selected, 1);
        p.value_row(&mut fb, p.item_y(1), "SIZE", "WITH BORDER", 12, ItemState::Normal, 0);
        p.volume_row(&mut fb, p.item_y(2), "VOLUME", 6, 10, ItemState::Selected, 1);
        p.value_row(&mut fb, p.item_y(3), "OFF", "X", 12, ItemState::Disabled, 0);
        p.item_centered(&mut fb, p.item_y(4), "BACK", ItemState::Normal, 0);
        p.note(&mut fb, p.h - 2, "A NOTE");
        let (x, y, w, h) = p.pixel_rect();
        for py in 0..HEIGHT {
            for pxl in 0..WIDTH {
                let inside = (pxl as isize) >= x && (pxl as isize) < x + w as isize && (py as isize) >= y && (py as isize) < y + h as isize;
                if !inside {
                    assert_eq!(fb.get_pixel(pxl, py), px(c::CYAN), "stray pixel at {pxl},{py}");
                }
            }
        }
    }

    #[test]
    fn selected_text_has_high_contrast() {
        fn lum(i: u8) -> f64 {
            let v = px(i);
            let ch = |s: u32| {
                let x = ((v >> s) & 0xFF) as f64 / 255.0;
                if x <= 0.03928 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
        }
        let ratio = |a: u8, b: u8| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        assert!(ratio(c::SELECT_FG, c::SELECT_BG) >= 7.0);
        assert!(ratio(c::TEXT, c::FILL) >= 7.0);
        assert!(ratio(c::DIM, c::FILL) >= 2.0);
        assert!(ratio(c::DIM, c::FOOTER_BG) >= 4.5);
        assert!(ratio(c::KEYCAP_FG, c::KEYCAP_BG) >= 7.0);
        assert!(ratio(c::SELECT_DIM, c::SELECT_BG) >= 4.5);
    }

    #[test]
    fn footer_hints_fit_the_frame() {
        let sets = [
            vec![Hint::new("UP/DN", "MOVE"), Hint::new("SPACE", "SELECT")],
            vec![Hint::new("UP/DN", "MOVE"), Hint::new("L/R", "CHANGE"), Hint::new("ESC", "BACK")],
            // widest realistic: three hints with the longest key names, note on the right
            vec![Hint::new("UP/DN", "MOVE"), Hint::new("RETURN", "SELECT"), Hint::new("ESC", "RESUME")],
        ];
        for s in &sets {
            assert!(hints_width(s) <= FOOTER_MAX, "{s:?} = {}", hints_width(s));
        }
        // the hints are centred: the version note (main menu, two hints) fits right of them, three hints push it out
        assert!(note_fits(&sets[0], "V0.1.0"));
        assert!(!note_fits(&sets[1], "V0.1.0") && !note_fits(&sets[2], "V0.1.0"));
        assert!(FOOTER_Y >= grid_rect().3 as usize, "footer must not overlap the grid");
        const _: () = assert!(FOOTER_Y + FOOTER_H <= HEIGHT);
        // bound keys: the longest key label still fits
        let h = Hint::new("R SHIFT", "FIRE");
        assert!(hints_width(&[h]) <= FOOTER_MAX);
    }

    #[test]
    fn footer_draws_only_in_its_bar() {
        giana_core::skip_without_data!();
        let mut fb = Framebuffer::new();
        fb.clear(px(c::CYAN));
        footer(&mut fb, &[Hint::new("UP/DN", "MOVE"), Hint::new("ESC", "BACK")], Some("V0.1.0"));
        for y in 0..HEIGHT {
            if (FOOTER_Y..FOOTER_Y + FOOTER_H).contains(&y) {
                continue;
            }
            for x in 0..WIDTH {
                assert_eq!(fb.get_pixel(x, y), px(c::CYAN));
            }
        }
    }

    #[test]
    fn dim_makes_everything_darker_and_keeps_black() {
        let mut fb = Framebuffer::new();
        fb.clear(px(c::LIGHT_BLUE));
        dim(&mut fb);
        assert_eq!(fb.get_pixel(5, 5), px(c::BLUE));
        fb.clear(px(c::BLACK));
        dim(&mut fb);
        assert_eq!(fb.get_pixel(5, 5), px(c::BLACK));
    }
}
