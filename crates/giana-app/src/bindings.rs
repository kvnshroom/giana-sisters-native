//! Keyboard and controller bindings: defaults, the rebinding rules and the settings file.
//!
//! Every action has `SLOTS` places per device. The port's `ControllerInput` is computed from the inputs held at the moment,
//! so a binding is the only thing that decides what a key or button does in the game.

use giana_core::state::ControllerInput;
use sdl2::controller::{Axis, Button};
use sdl2::keyboard::Keycode;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const SLOTS: usize = 3;
pub const ACTIONS: usize = 6;

/// Stick or trigger position that counts as pressed, and the one below which it counts as released again.
const AXIS_PRESS: i16 = 16000;
const AXIS_RELEASE: i16 = 12000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Left,
    Right,
    Duck,
    Jump,
    Fire,
    Item,
}

impl Action {
    pub const ALL: [Action; ACTIONS] = [Action::Left, Action::Right, Action::Duck, Action::Jump, Action::Fire, Action::Item];

    pub fn label(self) -> &'static str {
        match self {
            Action::Left => "LEFT",
            Action::Right => "RIGHT",
            Action::Duck => "DUCK",
            Action::Jump => "JUMP",
            Action::Fire => "FIRE",
            Action::Item => "ITEM",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Action::Left => "left",
            Action::Right => "right",
            Action::Duck => "duck",
            Action::Jump => "jump",
            Action::Fire => "fire",
            Action::Item => "item",
        }
    }
}

/// A controller input that can be bound: a button or one direction of an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadInput {
    Button(Button),
    AxisNeg(Axis),
    AxisPos(Axis),
}

impl PadInput {
    pub fn label(self) -> String {
        match self {
            PadInput::Button(b) => match b {
                Button::A => "A".into(),
                Button::B => "B".into(),
                Button::X => "X".into(),
                Button::Y => "Y".into(),
                Button::LeftShoulder => "LB".into(),
                Button::RightShoulder => "RB".into(),
                Button::LeftStick => "L STICK".into(),
                Button::RightStick => "R STICK".into(),
                Button::DPadUp => "PAD UP".into(),
                Button::DPadDown => "PAD DOWN".into(),
                Button::DPadLeft => "PAD LEFT".into(),
                Button::DPadRight => "PAD RIGHT".into(),
                Button::Back => "BACK".into(),
                Button::Start => "START".into(),
                other => other.string().to_uppercase(),
            },
            PadInput::AxisNeg(a) => axis_label(a, false),
            PadInput::AxisPos(a) => axis_label(a, true),
        }
    }

    fn to_file(self) -> String {
        match self {
            PadInput::Button(b) => format!("button:{}", b.string()),
            PadInput::AxisNeg(a) => format!("axis:{}:-", a.string()),
            PadInput::AxisPos(a) => format!("axis:{}:+", a.string()),
        }
    }

    fn from_file(s: &str) -> Option<Self> {
        let mut p = s.split(':');
        match (p.next()?, p.next()?, p.next()) {
            ("button", b, None) => Button::from_string(b).map(PadInput::Button),
            ("axis", a, Some("-")) => Axis::from_string(a).map(PadInput::AxisNeg),
            ("axis", a, Some("+")) => Axis::from_string(a).map(PadInput::AxisPos),
            _ => None,
        }
    }
}

fn axis_label(a: Axis, positive: bool) -> String {
    match (a, positive) {
        (Axis::LeftX, false) => "STICK LT".into(),
        (Axis::LeftX, true) => "STICK RT".into(),
        (Axis::LeftY, false) => "STICK UP".into(),
        (Axis::LeftY, true) => "STICK DN".into(),
        (Axis::RightX, false) => "RSTICK LT".into(),
        (Axis::RightX, true) => "RSTICK RT".into(),
        (Axis::RightY, false) => "RSTICK UP".into(),
        (Axis::RightY, true) => "RSTICK DN".into(),
        (Axis::TriggerLeft, _) => "LT".into(),
        (Axis::TriggerRight, _) => "RT".into(),
    }
}

pub fn key_label(k: Keycode) -> String {
    match k {
        Keycode::LCtrl => "L CTRL".into(),
        Keycode::RCtrl => "R CTRL".into(),
        Keycode::LShift => "L SHIFT".into(),
        Keycode::RShift => "R SHIFT".into(),
        Keycode::LAlt => "L ALT".into(),
        Keycode::RAlt => "R ALT".into(),
        Keycode::Escape => "ESC".into(),
        Keycode::Return => "ENTER".into(),
        Keycode::KpEnter => "KP ENTER".into(),
        Keycode::Backspace => "BKSP".into(),
        Keycode::Delete => "DEL".into(),
        other => other.name().to_uppercase(),
    }
}

/// What is held right now, whatever it is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Key(Keycode),
    Pad(PadInput),
}

#[derive(Clone, Debug, Default)]
pub struct Held(HashSet<Source>);

impl Held {
    pub fn press(&mut self, s: Source) {
        self.0.insert(s);
    }

    pub fn release(&mut self, s: Source) {
        self.0.remove(&s);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn clear_pad(&mut self) {
        self.0.retain(|s| matches!(s, Source::Key(_)));
    }

    pub fn is_held(&self, s: Source) -> bool {
        self.0.contains(&s)
    }

    /// Track an axis with a dead zone; returns the direction that was newly pressed.
    pub fn axis(&mut self, axis: Axis, value: i16) -> Option<PadInput> {
        let (neg, pos) = (Source::Pad(PadInput::AxisNeg(axis)), Source::Pad(PadInput::AxisPos(axis)));
        let mut newly = None;
        for (s, v, input) in [(neg, -(value as i32), PadInput::AxisNeg(axis)), (pos, value as i32, PadInput::AxisPos(axis))] {
            if v > AXIS_PRESS as i32 {
                if !self.is_held(s) {
                    newly = Some(input);
                }
                self.press(s);
            } else if v < AXIS_RELEASE as i32 {
                self.release(s);
            }
        }
        newly
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings {
    pub keys: [[Option<Keycode>; SLOTS]; ACTIONS],
    pub pad: [[Option<PadInput>; SLOTS]; ACTIONS],
}

impl Default for Bindings {
    /// Arrow keys or WASD for the left or right hand, jump and fire apart so that both can be held together.
    fn default() -> Self {
        Self { keys: default_keys(), pad: default_pad() }
    }
}

fn default_keys() -> [[Option<Keycode>; SLOTS]; ACTIONS] {
    [
        [Some(Keycode::Left), Some(Keycode::A), None],
        [Some(Keycode::Right), Some(Keycode::D), None],
        [Some(Keycode::Down), Some(Keycode::S), None],
        [Some(Keycode::Space), Some(Keycode::Up), Some(Keycode::W)],
        [Some(Keycode::X), Some(Keycode::LCtrl), Some(Keycode::Z)],
        [Some(Keycode::C), None, None],
    ]
}

fn default_pad() -> [[Option<PadInput>; SLOTS]; ACTIONS] {
    use PadInput::*;
    [
        [Some(Button(self::Button::DPadLeft)), Some(AxisNeg(Axis::LeftX)), None],
        [Some(Button(self::Button::DPadRight)), Some(AxisPos(Axis::LeftX)), None],
        [Some(Button(self::Button::DPadDown)), Some(AxisPos(Axis::LeftY)), None],
        [Some(Button(self::Button::B)), Some(Button(self::Button::LeftShoulder)), None],
        [Some(Button(self::Button::A)), Some(Button(self::Button::X)), Some(AxisPos(Axis::TriggerRight))],
        [Some(Button(self::Button::Y)), None, None],
    ]
}

impl Bindings {
    /// A key belongs to one action only: it leaves its old place when it is bound again.
    pub fn set_key(&mut self, action: usize, slot: usize, key: Keycode) {
        for row in self.keys.iter_mut() {
            for s in row.iter_mut() {
                if *s == Some(key) {
                    *s = None;
                }
            }
        }
        self.keys[action][slot] = Some(key);
    }

    pub fn set_pad(&mut self, action: usize, slot: usize, input: PadInput) {
        for row in self.pad.iter_mut() {
            for s in row.iter_mut() {
                if *s == Some(input) {
                    *s = None;
                }
            }
        }
        self.pad[action][slot] = Some(input);
    }

    pub fn reset_keys(&mut self) {
        self.keys = default_keys();
    }

    pub fn reset_pad(&mut self) {
        self.pad = default_pad();
    }

    fn action_held(&self, action: usize, held: &Held) -> bool {
        self.keys[action].iter().flatten().any(|&k| held.is_held(Source::Key(k)))
            || self.pad[action].iter().flatten().any(|&p| held.is_held(Source::Pad(p)))
    }

    /// Write the held actions into the input of the game.
    pub fn apply(&self, held: &Held, input: &mut ControllerInput) {
        input.left = self.action_held(0, held);
        input.right = self.action_held(1, held);
        input.down = self.action_held(2, held);
        input.jump = self.action_held(3, held);
        input.fire = self.action_held(4, held);
        input.item = self.action_held(5, held);
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# The Great Giana Sisters: controls. Empty value = nothing bound.\n");
        for (a, action) in Action::ALL.iter().enumerate() {
            for slot in 0..SLOTS {
                let v = self.keys[a][slot].map_or(String::new(), |k| k.name());
                out += &format!("key.{}.{} = {}\n", action.file_name(), slot, v);
            }
        }
        for (a, action) in Action::ALL.iter().enumerate() {
            for slot in 0..SLOTS {
                let v = self.pad[a][slot].map_or(String::new(), PadInput::to_file);
                out += &format!("pad.{}.{} = {}\n", action.file_name(), slot, v);
            }
        }
        out
    }

    /// Lines that cannot be read keep their default, so that a damaged file cannot take the controls away.
    pub fn from_text(text: &str) -> Self {
        let mut b = Self::default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else { continue };
            let (name, value) = (name.trim(), value.trim());
            if name.starts_with('#') {
                continue;
            }
            let mut p = name.split('.');
            let (Some(device), Some(action), Some(slot), None) = (p.next(), p.next(), p.next(), p.next()) else { continue };
            let Some(a) = Action::ALL.iter().position(|x| x.file_name() == action) else { continue };
            let Some(slot) = slot.parse::<usize>().ok().filter(|&s| s < SLOTS) else { continue };
            match device {
                "key" if value.is_empty() => b.keys[a][slot] = None,
                "key" => {
                    if let Some(k) = Keycode::from_name(value) {
                        b.keys[a][slot] = Some(k);
                    }
                }
                "pad" if value.is_empty() => b.pad[a][slot] = None,
                "pad" => {
                    if let Some(i) = PadInput::from_file(value) {
                        b.pad[a][slot] = Some(i);
                    }
                }
                _ => {}
            }
        }
        b
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map_or_else(|_| Self::default(), |t| Self::from_text(&t))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::fsutil::write_atomic(path, &self.to_text())
    }
}

/// `$XDG_CONFIG_HOME/giana-native/controls.conf`, or the same under `~/.config`, on Windows under `%APPDATA%`.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("giana-native").join("controls.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trip_keeps_every_binding() {
        let mut b = Bindings::default();
        b.set_key(3, 2, Keycode::Return);
        b.keys[0][1] = None;
        b.set_pad(4, 0, PadInput::AxisNeg(Axis::RightY));
        assert_eq!(Bindings::from_text(&b.to_text()), b);
    }

    #[test]
    fn damaged_lines_keep_the_default() {
        let b = Bindings::from_text("key.jump.0 = NoSuchKey\nnonsense\npad.fire.9 = button:a\nkey.left.5 = A\n");
        assert_eq!(b, Bindings::default());
    }

    #[test]
    fn a_key_bound_again_leaves_its_old_place() {
        let mut b = Bindings::default();
        b.set_key(4, 0, Keycode::Space); // Space was jump
        assert_eq!(b.keys[3][0], None);
        assert_eq!(b.keys[4][0], Some(Keycode::Space));
    }

    #[test]
    fn jump_and_fire_are_separate_by_default() {
        let b = Bindings::default();
        let mut held = Held::default();
        held.press(Source::Key(Keycode::X));
        let mut i = ControllerInput::new();
        b.apply(&held, &mut i);
        assert!(i.fire && !i.jump);
        held.clear();
        held.press(Source::Pad(PadInput::Button(Button::B)));
        b.apply(&held, &mut i);
        assert!(i.jump && !i.fire);
        held.clear();
        held.press(Source::Pad(PadInput::Button(Button::A)));
        b.apply(&held, &mut i);
        assert!(i.fire && !i.jump);
    }

    #[test]
    fn item_is_its_own_action_on_c_and_y() {
        let b = Bindings::default();
        let mut held = Held::default();
        held.press(Source::Key(Keycode::C));
        let mut i = ControllerInput::new();
        b.apply(&held, &mut i);
        assert!(i.item && !i.fire && !i.jump);
        held.clear();
        held.press(Source::Pad(PadInput::Button(Button::Y)));
        b.apply(&held, &mut i);
        assert!(i.item);
    }

    #[test]
    fn axis_has_a_dead_zone_and_reports_a_new_press_once() {
        let mut held = Held::default();
        assert_eq!(held.axis(Axis::LeftX, 10000), None);
        assert_eq!(held.axis(Axis::LeftX, 20000), Some(PadInput::AxisPos(Axis::LeftX)));
        assert_eq!(held.axis(Axis::LeftX, 25000), None);
        assert!(held.is_held(Source::Pad(PadInput::AxisPos(Axis::LeftX))));
        held.axis(Axis::LeftX, 14000); // between both thresholds: still held
        assert!(held.is_held(Source::Pad(PadInput::AxisPos(Axis::LeftX))));
        held.axis(Axis::LeftX, 5000);
        assert!(!held.is_held(Source::Pad(PadInput::AxisPos(Axis::LeftX))));
    }

    #[test]
    fn full_negative_axis_does_not_overflow() {
        let mut held = Held::default();
        assert_eq!(held.axis(Axis::LeftY, i16::MIN), Some(PadInput::AxisNeg(Axis::LeftY)));
    }
}
