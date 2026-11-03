//! Controller button glyphs in C64 pixel style, drawn in code (no image files), and the choice of
//! the glyph set from the controller type.
//!
//! A glyph is 9 pixels high (the height of the letters plus one) and as wide as it needs to be. The
//! catalogue follows the usual button sets (Xbox, PlayStation, Nintendo Switch, plus a neutral set
//! for unknown controllers that only shows the position of a face button).
//!
//! Naming of the inputs is the one of SDL's game controller API: `A B X Y` are the face buttons as
//! SDL reports them. On a Nintendo controller SDL reports them by label (the default of
//! `SDL_GAMECONTROLLER_USE_BUTTON_LABELS`), so `Button::A` is the button that says "A" there too.

use crate::bindings::PadInput;
use giana_render::framebuffer::Framebuffer;
use giana_render::palette::c64_color;
use giana_render::text::fill_rect_px;
use sdl2::controller::{Axis, Button};

/// Height of every glyph in pixels.
pub const ICON_H: usize = 9;

/// Which set of glyphs a controller gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadKind {
    /// Xbox 360 / One / Series, and pads that follow its layout and labels (Luna, Stadia, Shield)
    Xbox,
    /// PS3 / PS4 / PS5
    PlayStation,
    /// Switch Pro Controller and Joy-Cons
    Switch,
    /// anything SDL cannot name: neutral glyphs
    Generic,
}

impl PadKind {
    pub fn name(self) -> &'static str {
        match self {
            PadKind::Xbox => "XBOX",
            PadKind::PlayStation => "PLAYSTATION",
            PadKind::Switch => "SWITCH",
            PadKind::Generic => "GENERIC",
        }
    }

    /// The set for a value of SDL's `SDL_GameControllerType` (`SDL_CONTROLLER_TYPE_*`).
    pub fn from_sdl_type(t: i32) -> PadKind {
        match t {
            1 | 2 | 8 | 9 | 10 => PadKind::Xbox,        // Xbox 360, One, Luna, Stadia, Shield
            3 | 4 | 7 => PadKind::PlayStation,          // PS3, PS4, PS5
            5 | 11 | 12 | 13 => PadKind::Switch,        // Pro Controller, Joy-Cons
            _ => PadKind::Generic,                      // unknown (0), virtual (6), later types
        }
    }

    /// A guess from the name, for controllers that SDL does not know the type of.
    pub fn from_name(name: &str) -> PadKind {
        let n = name.to_ascii_lowercase();
        if n.contains("xbox") || n.contains("x-box") || n.contains("xinput") {
            PadKind::Xbox
        } else if n.contains("dualshock") || n.contains("dualsense") || n.contains("playstation") || n.contains("ps3") || n.contains("ps4") || n.contains("ps5") {
            PadKind::PlayStation
        } else if n.contains("nintendo") || n.contains("switch") || n.contains("joy-con") || n.contains("pro controller") {
            PadKind::Switch
        } else {
            PadKind::Generic
        }
    }

    /// The set for a controller: SDL's type first, the name when SDL says "unknown".
    pub fn detect(sdl_type: i32, name: &str) -> PadKind {
        match PadKind::from_sdl_type(sdl_type) {
            PadKind::Generic => PadKind::from_name(name),
            known => known,
        }
    }
}

/// The type SDL reports for the controller at a device index (`SDL_GameControllerTypeForIndex`).
pub fn sdl_type_for_index(index: u32) -> i32 {
    // SDL 2.0.12 or newer; older versions do not have the function at all, then the name decides
    unsafe { sdl2::sys::SDL_GameControllerTypeForIndex(index as std::os::raw::c_int) as i32 }
}

/// The face buttons as SDL names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceBtn {
    A,
    B,
    X,
    Y,
}

/// Direction bits of the D-pad and stick glyphs.
pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

/// One glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Icon {
    /// round face button
    Face(PadKind, FaceBtn),
    /// cross with the arms of `dirs` lit
    DPad(u8),
    /// a stick; with `dirs` an arrow for each direction, with 0 the stick is pressed (L3 / R3)
    Stick { right: bool, dirs: u8 },
    /// bumper: LB / L1 / L
    Shoulder(PadKind, bool),
    /// trigger: LT / L2 / ZL
    Trigger(PadKind, bool),
    Start(PadKind),
    Back(PadKind),
    Guide,
    /// anything else: a pill with a few letters
    Label(String),
}

/// The glyph for a bindable input on a controller of the given kind.
pub fn icon_for(kind: PadKind, input: PadInput) -> Icon {
    match input {
        PadInput::Button(b) => match b {
            Button::A => Icon::Face(kind, FaceBtn::A),
            Button::B => Icon::Face(kind, FaceBtn::B),
            Button::X => Icon::Face(kind, FaceBtn::X),
            Button::Y => Icon::Face(kind, FaceBtn::Y),
            Button::DPadUp => Icon::DPad(UP),
            Button::DPadDown => Icon::DPad(DOWN),
            Button::DPadLeft => Icon::DPad(LEFT),
            Button::DPadRight => Icon::DPad(RIGHT),
            Button::LeftShoulder => Icon::Shoulder(kind, false),
            Button::RightShoulder => Icon::Shoulder(kind, true),
            Button::LeftStick => Icon::Stick { right: false, dirs: 0 },
            Button::RightStick => Icon::Stick { right: true, dirs: 0 },
            Button::Start => Icon::Start(kind),
            Button::Back => Icon::Back(kind),
            Button::Guide => Icon::Guide,
            Button::Misc1 => Icon::Label(match kind {
                PadKind::PlayStation => "MIC",
                PadKind::Xbox => "SHR",
                _ => "M1",
            }
            .into()),
            Button::Touchpad => Icon::Label("PAD".into()),
            Button::Paddle1 => Icon::Label("P1".into()),
            Button::Paddle2 => Icon::Label("P2".into()),
            Button::Paddle3 => Icon::Label("P3".into()),
            Button::Paddle4 => Icon::Label("P4".into()),
        },
        PadInput::AxisNeg(a) | PadInput::AxisPos(a) => {
            let positive = matches!(input, PadInput::AxisPos(_));
            match a {
                Axis::LeftX => Icon::Stick { right: false, dirs: if positive { RIGHT } else { LEFT } },
                Axis::LeftY => Icon::Stick { right: false, dirs: if positive { DOWN } else { UP } },
                Axis::RightX => Icon::Stick { right: true, dirs: if positive { RIGHT } else { LEFT } },
                Axis::RightY => Icon::Stick { right: true, dirs: if positive { DOWN } else { UP } },
                Axis::TriggerLeft => Icon::Trigger(kind, false),
                Axis::TriggerRight => Icon::Trigger(kind, true),
            }
        }
    }
}

/// Merge the glyphs of two inputs that belong together (left and right) into one where that reads
/// better: two arms of the D-pad, two directions of the same stick.
pub fn merge_pair(a: Icon, b: Icon) -> Vec<Icon> {
    match (&a, &b) {
        (Icon::DPad(x), Icon::DPad(y)) => vec![Icon::DPad(x | y)],
        (Icon::Stick { right: r1, dirs: d1 }, Icon::Stick { right: r2, dirs: d2 }) if r1 == r2 && *d1 != 0 && *d2 != 0 => {
            vec![Icon::Stick { right: *r1, dirs: d1 | d2 }]
        }
        _ => vec![a, b],
    }
}

impl Icon {
    /// What the glyph stands for in plain letters (for tests and logs).
    pub fn name(&self) -> String {
        match self {
            Icon::Face(PadKind::PlayStation, b) => match b {
                FaceBtn::A => "CROSS",
                FaceBtn::B => "CIRCLE",
                FaceBtn::X => "SQUARE",
                FaceBtn::Y => "TRIANGLE",
            }
            .into(),
            Icon::Face(PadKind::Generic, b) => match b {
                FaceBtn::A => "SOUTH",
                FaceBtn::B => "EAST",
                FaceBtn::X => "WEST",
                FaceBtn::Y => "NORTH",
            }
            .into(),
            Icon::Face(_, b) => format!("{b:?}"),
            Icon::DPad(d) => format!("DPAD{}", dir_letters(*d)),
            Icon::Stick { right, dirs } => format!("{}STICK{}", if *right { "R" } else { "L" }, dir_letters(*dirs)),
            Icon::Shoulder(k, r) => shoulder_label(*k, *r).into(),
            Icon::Trigger(k, r) => trigger_label(*k, *r).into(),
            Icon::Start(k) => start_label(*k).into(),
            Icon::Back(k) => back_label(*k).into(),
            Icon::Guide => "GUIDE".into(),
            Icon::Label(t) => t.clone(),
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        match self {
            Icon::Face(..) | Icon::DPad(_) | Icon::Guide => 9,
            Icon::Stick { .. } => 13,
            Icon::Shoulder(k, r) => text_pill_width(shoulder_label(*k, *r)),
            Icon::Trigger(k, r) => text_pill_width(trigger_label(*k, *r)),
            Icon::Start(k) => match k {
                PadKind::PlayStation => text_pill_width("OPT"),
                _ => 11,
            },
            Icon::Back(k) => match k {
                PadKind::PlayStation => text_pill_width("SEL"),
                _ => 11,
            },
            Icon::Label(t) => text_pill_width(t),
        }
    }

    /// Draw with the top left corner at (x, y).
    pub fn draw(&self, fb: &mut Framebuffer, x: isize, y: isize) {
        match self {
            Icon::Face(kind, b) => draw_face(fb, x, y, *kind, *b),
            Icon::DPad(d) => draw_dpad(fb, x, y, *d),
            Icon::Stick { right, dirs } => draw_stick(fb, x, y, *right, *dirs),
            Icon::Shoulder(k, r) => draw_text_pill(fb, x, y, shoulder_label(*k, *r), 15, 0),
            Icon::Trigger(k, r) => draw_text_pill(fb, x, y, trigger_label(*k, *r), 12, 1),
            Icon::Start(k) => draw_start(fb, x, y, *k),
            Icon::Back(k) => draw_back(fb, x, y, *k),
            Icon::Guide => {
                draw_disc(fb, x, y, 15);
                dot(fb, x + 3, y + 3, 3, 3, 0);
            }
            Icon::Label(t) => draw_text_pill(fb, x, y, t, 15, 0),
        }
    }
}

fn dir_letters(d: u8) -> String {
    [(UP, 'U'), (DOWN, 'D'), (LEFT, 'L'), (RIGHT, 'R')].iter().filter(|(b, _)| d & b != 0).map(|(_, c)| *c).collect()
}

pub fn shoulder_label(kind: PadKind, right: bool) -> &'static str {
    match (kind, right) {
        (PadKind::PlayStation, false) => "L1",
        (PadKind::PlayStation, true) => "R1",
        (PadKind::Switch, false) => "L",
        (PadKind::Switch, true) => "R",
        (_, false) => "LB",
        (_, true) => "RB",
    }
}

pub fn trigger_label(kind: PadKind, right: bool) -> &'static str {
    match (kind, right) {
        (PadKind::PlayStation, false) => "L2",
        (PadKind::PlayStation, true) => "R2",
        (PadKind::Switch, false) => "ZL",
        (PadKind::Switch, true) => "ZR",
        (_, false) => "LT",
        (_, true) => "RT",
    }
}

pub fn start_label(kind: PadKind) -> &'static str {
    match kind {
        PadKind::Xbox => "MENU",
        PadKind::PlayStation => "OPTIONS",
        PadKind::Switch => "+",
        PadKind::Generic => "START",
    }
}

pub fn back_label(kind: PadKind) -> &'static str {
    match kind {
        PadKind::Xbox => "VIEW",
        PadKind::PlayStation => "SELECT",
        PadKind::Switch => "-",
        PadKind::Generic => "BACK",
    }
}

// ---------- drawing ----------

fn col(i: u8) -> u32 {
    c64_color(i)
}

fn dot(fb: &mut Framebuffer, x: isize, y: isize, w: usize, h: usize, color: u8) {
    fill_rect_px(fb, x, y, w, h, col(color));
}

/// Left inset of each of the 9 rows of a round glyph.
const DISC: [isize; 9] = [2, 1, 0, 0, 0, 0, 0, 1, 2];

fn draw_disc(fb: &mut Framebuffer, x: isize, y: isize, color: u8) {
    for (j, inset) in DISC.iter().enumerate() {
        dot(fb, x + inset, y + j as isize, (9 - 2 * inset) as usize, 1, color);
    }
}

/// A pill `w` wide and 9 high with rounded ends.
fn draw_pill(fb: &mut Framebuffer, x: isize, y: isize, w: usize, color: u8) {
    for (j, inset) in DISC.iter().enumerate() {
        dot(fb, x + inset, y + j as isize, w - 2 * *inset as usize, 1, color);
    }
}

fn text_pill_width(text: &str) -> usize {
    micro_width(text) + 6
}

fn draw_text_pill(fb: &mut Framebuffer, x: isize, y: isize, text: &str, fill: u8, shade: u8) {
    let w = text_pill_width(text);
    draw_pill(fb, x, y, w, fill);
    // a trigger has a darker belly line under the letters to tell it from a bumper
    if shade == 1 {
        dot(fb, x + 3, y + 7, w - 6, 1, 11);
    }
    let ink = if fill == 15 { 0 } else { 1 };
    micro_text(fb, x + 3, y + 2, text, ink);
}

fn draw_face(fb: &mut Framebuffer, x: isize, y: isize, kind: PadKind, b: FaceBtn) {
    match kind {
        PadKind::Xbox => {
            let fill = match b {
                FaceBtn::A => 13, // light green
                FaceBtn::B => 10, // light red
                FaceBtn::X => 14, // light blue
                FaceBtn::Y => 7,  // yellow
            };
            draw_disc(fb, x, y, fill);
            micro_text(fb, x + 3, y + 2, face_letter(b), 0);
        }
        PadKind::Switch => {
            draw_disc(fb, x, y, 15);
            micro_text(fb, x + 3, y + 2, face_letter(b), 0);
        }
        PadKind::PlayStation => {
            draw_disc(fb, x, y, 15);
            // coloured symbols on the light disc
            let (rows, color): ([u8; 5], u8) = match b {
                FaceBtn::A => ([0b10001, 0b01010, 0b00100, 0b01010, 0b10001], 6), // cross
                FaceBtn::B => ([0b01110, 0b10001, 0b10001, 0b10001, 0b01110], 2), // circle
                FaceBtn::X => ([0b11111, 0b10001, 0b10001, 0b10001, 0b11111], 4),  // square
                FaceBtn::Y => ([0b00100, 0b01010, 0b01010, 0b10001, 0b11111], 5), // triangle
            };
            bits5(fb, x + 2, y + 2, rows, color);
        }
        PadKind::Generic => {
            draw_disc(fb, x, y, 11);
            // the four positions; the one that is meant is lit
            let (lit_x, lit_y) = match b {
                FaceBtn::A => (4, 7), // south
                FaceBtn::B => (7, 4), // east
                FaceBtn::X => (1, 4), // west
                FaceBtn::Y => (4, 1), // north
            };
            for (px, py) in [(4, 1), (4, 7), (1, 4), (7, 4)] {
                if (px, py) == (lit_x, lit_y) {
                    dot(fb, x + px - 1, y + py - 1, 3, 3, 7);
                } else {
                    dot(fb, x + px, y + py, 1, 1, 12);
                }
            }
        }
    }
}

fn face_letter(b: FaceBtn) -> &'static str {
    match b {
        FaceBtn::A => "A",
        FaceBtn::B => "B",
        FaceBtn::X => "X",
        FaceBtn::Y => "Y",
    }
}

fn bits5(fb: &mut Framebuffer, x: isize, y: isize, rows: [u8; 5], color: u8) {
    for (j, r) in rows.iter().enumerate() {
        for i in 0..5 {
            if r >> (4 - i) & 1 != 0 {
                dot(fb, x + i, y + j as isize, 1, 1, color);
            }
        }
    }
}

fn draw_dpad(fb: &mut Framebuffer, x: isize, y: isize, dirs: u8) {
    // the cross in grey, the lit arms in white
    dot(fb, x + 3, y, 3, 9, 11);
    dot(fb, x, y + 3, 9, 3, 11);
    let arms = [(UP, 3, 0, 3, 3), (DOWN, 3, 6, 3, 3), (LEFT, 0, 3, 3, 3), (RIGHT, 6, 3, 3, 3)];
    for (bit, ax, ay, w, h) in arms {
        if dirs & bit != 0 {
            dot(fb, x + ax, y + ay, w, h, 1);
        }
    }
}

fn draw_stick(fb: &mut Framebuffer, x: isize, y: isize, right: bool, dirs: u8) {
    micro_text(fb, x, y + 2, if right { "R" } else { "L" }, 15);
    let rx = x + 4;
    draw_disc(fb, rx, y, 15);
    // the inside
    for (j, inset) in DISC.iter().enumerate().skip(1).take(7) {
        let inset = (*inset).max(1);
        dot(fb, rx + inset, y + j as isize, (9 - 2 * inset) as usize, 1, 11);
    }
    if dirs == 0 {
        dot(fb, rx + 3, y + 3, 3, 3, 1);
        return;
    }
    let arrows: [(u8, [(isize, isize); 4]); 4] = [
        (UP, [(4, 2), (3, 3), (4, 3), (5, 3)]),
        (DOWN, [(3, 5), (4, 5), (5, 5), (4, 6)]),
        (LEFT, [(2, 4), (3, 3), (3, 4), (3, 5)]),
        (RIGHT, [(6, 4), (5, 3), (5, 4), (5, 5)]),
    ];
    for (bit, px) in arrows {
        if dirs & bit != 0 {
            for (ax, ay) in px {
                dot(fb, rx + ax, y + ay, 1, 1, 1);
            }
        }
    }
}

fn draw_start(fb: &mut Framebuffer, x: isize, y: isize, kind: PadKind) {
    match kind {
        PadKind::PlayStation => draw_text_pill(fb, x, y, "OPT", 15, 0),
        PadKind::Switch => {
            // a small round button with a plus
            draw_disc(fb, x + 1, y, 15);
            dot(fb, x + 5, y + 2, 1, 5, 0);
            dot(fb, x + 3, y + 4, 5, 1, 0);
        }
        PadKind::Xbox => {
            draw_pill(fb, x, y, 11, 15);
            // three lines: the menu button
            for k in 0..3 {
                dot(fb, x + 3, y + 2 + 2 * k, 5, 1, 0);
            }
        }
        PadKind::Generic => {
            draw_pill(fb, x, y, 11, 15);
            // the triangle of "start"
            dot(fb, x + 4, y + 2, 1, 5, 0);
            dot(fb, x + 5, y + 3, 1, 3, 0);
            dot(fb, x + 6, y + 4, 1, 1, 0);
        }
    }
}

fn draw_back(fb: &mut Framebuffer, x: isize, y: isize, kind: PadKind) {
    match kind {
        PadKind::PlayStation => draw_text_pill(fb, x, y, "SEL", 15, 0),
        PadKind::Switch => {
            draw_disc(fb, x + 1, y, 15);
            dot(fb, x + 3, y + 4, 5, 1, 0);
        }
        PadKind::Xbox | PadKind::Generic => {
            draw_pill(fb, x, y, 11, 15);
            // two overlapping squares: the view button
            for (ox, oy) in [(2, 2), (4, 3)] {
                dot(fb, x + ox + 1, y + oy, 4, 1, 0);
                dot(fb, x + ox + 1, y + oy + 3, 4, 1, 0);
                dot(fb, x + ox + 1, y + oy, 1, 4, 0);
                dot(fb, x + ox + 4, y + oy, 1, 4, 0);
            }
        }
    }
}

// ---------- micro font: 3 x 5 pixels ----------

fn micro_rows(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b110],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0; 5],
    }
}

fn micro_width(text: &str) -> usize {
    let n = text.chars().count();
    if n == 0 {
        0
    } else {
        4 * n - 1
    }
}

fn micro_text(fb: &mut Framebuffer, x: isize, y: isize, text: &str, color: u8) {
    for (k, c) in text.chars().enumerate() {
        for (j, r) in micro_rows(c).iter().enumerate() {
            for i in 0..3 {
                if r >> (2 - i) & 1 != 0 {
                    dot(fb, x + (k * 4) as isize + i, y + j as isize, 1, 1, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use giana_render::framebuffer::{HEIGHT, WIDTH};

    #[test]
    fn controller_types_pick_the_glyph_set() {
        // SDL_CONTROLLER_TYPE_*: 0 unknown, 1 Xbox 360, 2 Xbox One, 3 PS3, 4 PS4, 5 Switch Pro, 6 virtual, 7 PS5,
        // 8 Luna, 9 Stadia, 10 Shield, 11-13 Joy-Cons
        let expect = [
            (0, PadKind::Generic),
            (1, PadKind::Xbox),
            (2, PadKind::Xbox),
            (3, PadKind::PlayStation),
            (4, PadKind::PlayStation),
            (5, PadKind::Switch),
            (6, PadKind::Generic),
            (7, PadKind::PlayStation),
            (8, PadKind::Xbox),
            (9, PadKind::Xbox),
            (10, PadKind::Xbox),
            (11, PadKind::Switch),
            (12, PadKind::Switch),
            (13, PadKind::Switch),
            (99, PadKind::Generic),
        ];
        for (t, kind) in expect {
            assert_eq!(PadKind::from_sdl_type(t), kind, "type {t}");
        }
    }

    #[test]
    fn the_name_decides_only_when_sdl_does_not_know() {
        assert_eq!(PadKind::detect(0, "Wireless Controller"), PadKind::Generic);
        assert_eq!(PadKind::detect(0, "PS4 Controller"), PadKind::PlayStation);
        assert_eq!(PadKind::detect(0, "Nintendo Switch Pro Controller"), PadKind::Switch);
        assert_eq!(PadKind::detect(0, "Microsoft X-Box 360 pad"), PadKind::Xbox);
        assert_eq!(PadKind::detect(0, "Generic USB Joystick"), PadKind::Generic);
        // a type that SDL knows wins over a misleading name
        assert_eq!(PadKind::detect(1, "Switch lookalike"), PadKind::Xbox);
    }

    #[test]
    fn face_buttons_per_set() {
        use PadKind::*;
        let b = |x| PadInput::Button(x);
        for (kind, a, bb, x, y) in [
            (Xbox, "A", "B", "X", "Y"),
            (Switch, "A", "B", "X", "Y"),
            (PlayStation, "CROSS", "CIRCLE", "SQUARE", "TRIANGLE"),
            (Generic, "SOUTH", "EAST", "WEST", "NORTH"),
        ] {
            let names: Vec<String> = [Button::A, Button::B, Button::X, Button::Y].iter().map(|&k| icon_for(kind, b(k)).name()).collect();
            assert_eq!(names, [a, bb, x, y], "{kind:?}");
        }
    }

    #[test]
    fn other_inputs_get_their_glyphs() {
        let b = PadInput::Button;
        assert_eq!(icon_for(PadKind::Xbox, b(Button::LeftShoulder)).name(), "LB");
        assert_eq!(icon_for(PadKind::PlayStation, b(Button::RightShoulder)).name(), "R1");
        assert_eq!(icon_for(PadKind::Switch, b(Button::LeftShoulder)).name(), "L");
        assert_eq!(icon_for(PadKind::Generic, b(Button::RightShoulder)).name(), "RB");
        assert_eq!(icon_for(PadKind::Xbox, PadInput::AxisPos(Axis::TriggerRight)).name(), "RT");
        assert_eq!(icon_for(PadKind::PlayStation, PadInput::AxisPos(Axis::TriggerLeft)).name(), "L2");
        assert_eq!(icon_for(PadKind::Switch, PadInput::AxisPos(Axis::TriggerRight)).name(), "ZR");
        assert_eq!(icon_for(PadKind::Switch, b(Button::Start)).name(), "+");
        assert_eq!(icon_for(PadKind::Switch, b(Button::Back)).name(), "-");
        assert_eq!(icon_for(PadKind::PlayStation, b(Button::Start)).name(), "OPTIONS");
        assert_eq!(icon_for(PadKind::PlayStation, b(Button::Back)).name(), "SELECT");
        assert_eq!(icon_for(PadKind::Xbox, b(Button::Start)).name(), "MENU");
        assert_eq!(icon_for(PadKind::Xbox, b(Button::DPadLeft)).name(), "DPADL");
        assert_eq!(icon_for(PadKind::Xbox, PadInput::AxisNeg(Axis::LeftY)).name(), "LSTICKU");
        assert_eq!(icon_for(PadKind::Xbox, PadInput::AxisPos(Axis::RightX)).name(), "RSTICKR");
        assert_eq!(icon_for(PadKind::Xbox, b(Button::LeftStick)).name(), "LSTICK");
        assert_eq!(icon_for(PadKind::Xbox, b(Button::Paddle2)), Icon::Label("P2".into()));
    }

    #[test]
    fn left_and_right_merge_into_one_glyph() {
        let l = icon_for(PadKind::Xbox, PadInput::Button(Button::DPadLeft));
        let r = icon_for(PadKind::Xbox, PadInput::Button(Button::DPadRight));
        assert_eq!(merge_pair(l, r), vec![Icon::DPad(LEFT | RIGHT)]);
        let l = icon_for(PadKind::Xbox, PadInput::AxisNeg(Axis::LeftX));
        let r = icon_for(PadKind::Xbox, PadInput::AxisPos(Axis::LeftX));
        assert_eq!(merge_pair(l, r), vec![Icon::Stick { right: false, dirs: LEFT | RIGHT }]);
        // different things stay apart
        let l = icon_for(PadKind::Xbox, PadInput::Button(Button::DPadLeft));
        let r = icon_for(PadKind::Xbox, PadInput::AxisPos(Axis::LeftX));
        assert_eq!(merge_pair(l, r).len(), 2);
    }

    #[test]
    fn every_glyph_draws_inside_its_box() {
        use PadKind::*;
        let mut icons = vec![Icon::Guide, Icon::Label("MIC".into()), Icon::DPad(UP | DOWN | LEFT | RIGHT), Icon::Stick { right: true, dirs: UP | LEFT }, Icon::Stick { right: false, dirs: 0 }];
        for kind in [Xbox, PlayStation, Switch, Generic] {
            for b in [FaceBtn::A, FaceBtn::B, FaceBtn::X, FaceBtn::Y] {
                icons.push(Icon::Face(kind, b));
            }
            for right in [false, true] {
                icons.push(Icon::Shoulder(kind, right));
                icons.push(Icon::Trigger(kind, right));
            }
            icons.push(Icon::Start(kind));
            icons.push(Icon::Back(kind));
        }
        for icon in icons {
            let mut fb = Framebuffer::new();
            fb.clear(col(0));
            let (x0, y0) = (100isize, 100isize);
            icon.draw(&mut fb, x0, y0);
            let mut lit = 0;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if fb.get_pixel(x, y) != col(0) {
                        lit += 1;
                        let (dx, dy) = (x as isize - x0, y as isize - y0);
                        assert!(dx >= 0 && dx < icon.width() as isize && dy >= 0 && dy < ICON_H as isize, "{icon:?} draws outside at {dx},{dy}");
                    }
                }
            }
            assert!(lit >= 8, "{icon:?} is almost empty");
        }
    }
}
