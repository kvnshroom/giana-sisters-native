//! The key and button hints under the menus: what is shown follows the input device that was used
//! last (keyboard or controller), the keys and buttons the menu really reacts to (`menu::menu_keys`,
//! `menu::menu_buttons`) and the current bindings of the game.

use crate::bindings::{key_label, Action, Bindings, PadInput};
use crate::menu::{menu_buttons, menu_keys, Nav};
use crate::padicons::{icon_for, merge_pair, Icon, PadKind};
use crate::ui::{hints_width_in, Cap, Hint, View};
use sdl2::controller::Button;
use sdl2::keyboard::Keycode;

/// Where the player's hands are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputDevice {
    Keyboard,
    Pad,
}

/// The device that was used last, and the glyph set of the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputState {
    pub device: InputDevice,
    pub pad: PadKind,
    /// a controller is connected
    pub pad_present: bool,
}

impl Default for InputState {
    fn default() -> Self {
        InputState { device: InputDevice::Keyboard, pad: PadKind::Generic, pad_present: false }
    }
}

impl InputState {
    /// Hints for a controller of this kind (for pictures and tests).
    pub fn pad(kind: PadKind) -> InputState {
        InputState { device: InputDevice::Pad, pad: kind, pad_present: true }
    }

    pub fn keyboard() -> InputState {
        InputState::default()
    }

    /// A key was pressed.
    pub fn key_used(&mut self) {
        self.device = InputDevice::Keyboard;
    }

    /// A button was pressed or a stick moved past its dead zone. Without a controller there is
    /// nothing to switch to.
    pub fn pad_used(&mut self) {
        if self.pad_present {
            self.device = InputDevice::Pad;
        }
    }

    /// A controller was connected; the hints stay on the keyboard until it is used.
    pub fn pad_connected(&mut self, kind: PadKind) {
        self.pad = kind;
        self.pad_present = true;
    }

    /// The controller was unplugged.
    pub fn pad_removed(&mut self) {
        self.pad_present = false;
        self.device = InputDevice::Keyboard;
    }

    fn on_pad(&self) -> bool {
        self.device == InputDevice::Pad
    }

    /// The glyph of an input on the controller that is connected.
    pub fn icon(&self, input: PadInput) -> Icon {
        icon_for(self.pad, input)
    }

    /// Caps for one or two menu moves (`[Up, Down]` makes `UP/DN` or one D-pad glyph).
    pub fn nav_caps(&self, navs: &[Nav]) -> Vec<Cap> {
        if self.on_pad() {
            let icons: Vec<Icon> = navs.iter().filter_map(|&n| menu_buttons(n).first().map(|&b| self.icon(PadInput::Button(b)))).collect();
            return match icons.as_slice() {
                [a, b] => merge_pair(a.clone(), b.clone()).into_iter().map(Cap::Icon).collect(),
                _ => icons.into_iter().map(Cap::Icon).collect(),
            };
        }
        let keys: Vec<Keycode> = navs.iter().filter_map(|&n| menu_keys(n).first().copied()).collect();
        match keys.as_slice() {
            [a, b] => vec![Cap::Text(pair_label(*a, *b))],
            _ => keys.into_iter().map(|k| Cap::Text(key_label(k))).collect(),
        }
    }

    fn nav_hint(&self, navs: &[Nav], action: &str) -> Hint {
        Hint::with_caps(self.nav_caps(navs), action)
    }

    pub fn move_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Up, Nav::Down], action)
    }

    pub fn change_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Left, Nav::Right], action)
    }

    pub fn select_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Confirm], action)
    }

    pub fn back_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Back], action)
    }

    pub fn clear_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Clear], action)
    }

    /// Favorite on the modules page: `F` on the keyboard, the button of `Clear` (Y) on the controller.
    pub fn favorite_hint(&self, action: &str) -> Hint {
        if self.on_pad() {
            self.nav_hint(&[Nav::Clear], action)
        } else {
            self.nav_hint(&[Nav::Favorite], action)
        }
    }

    /// Sort on the modules page: `O` on the keyboard, X on the controller.
    pub fn sort_hint(&self, action: &str) -> Hint {
        self.nav_hint(&[Nav::Sort], action)
    }

    /// One direction of the menu moves alone (the name page uses Right and Left apart).
    pub fn dir_hint(&self, nav: Nav, action: &str) -> Hint {
        self.nav_hint(&[nav], action)
    }

    /// How a capture is cancelled: Escape, or Start on the controller (both work everywhere).
    pub fn cancel_hint(&self, action: &str) -> Hint {
        if self.on_pad() {
            Hint::icons(vec![self.icon(PadInput::Button(Button::Start))], action)
        } else {
            Hint::with_caps(vec![Cap::Text(key_label(Keycode::Escape))], action)
        }
    }

    /// The game controls with the first binding of each action: move, jump, fire, item. What does
    /// not fit in the footer is left out from the end.
    pub fn play_hints(&self, b: &Bindings) -> Vec<Hint> {
        self.play_hints_in(b, View::FULL)
    }

    /// [`play_hints`](Self::play_hints) cut to the width of the footer of `view`.
    pub fn play_hints_in(&self, b: &Bindings, view: View) -> Vec<Hint> {
        let idx = |a: Action| Action::ALL.iter().position(|&x| x == a).unwrap_or(0);
        let mut out = Vec::new();
        if self.on_pad() {
            let first = |a: Action| b.pad[idx(a)].iter().flatten().next().map(|&p| self.icon(p));
            let mv = match (first(Action::Left), first(Action::Right)) {
                (Some(l), Some(r)) => merge_pair(l, r),
                (Some(x), None) | (None, Some(x)) => vec![x],
                (None, None) => Vec::new(),
            };
            if !mv.is_empty() {
                out.push(Hint::icons(mv, "MOVE"));
            }
            for (a, name) in [(Action::Jump, "JUMP"), (Action::Fire, "FIRE"), (Action::Item, "ITEM")] {
                if let Some(i) = first(a) {
                    out.push(Hint::icons(vec![i], name));
                }
            }
        } else {
            let first = |a: Action| b.keys[idx(a)].iter().flatten().next().copied();
            match (first(Action::Left), first(Action::Right)) {
                (Some(l), Some(r)) => out.push(Hint::with_caps(vec![Cap::Text(pair_label(l, r))], "MOVE")),
                (Some(k), None) | (None, Some(k)) => out.push(Hint::new(key_label(k), "MOVE")),
                (None, None) => {}
            }
            for (a, name) in [(Action::Jump, "JUMP"), (Action::Fire, "FIRE"), (Action::Item, "ITEM")] {
                if let Some(k) = first(a) {
                    out.push(Hint::new(key_label(k), name));
                }
            }
        }
        fit_end(out, view)
    }
}

/// `UP/DN`, `L/R`, `A/D`: two keys on one cap.
pub fn pair_label(a: Keycode, b: Keycode) -> String {
    let short = |k: Keycode| match k {
        Keycode::Up => "UP".to_string(),
        Keycode::Down => "DN".to_string(),
        Keycode::Left => "L".to_string(),
        Keycode::Right => "R".to_string(),
        other => key_label(other),
    };
    format!("{}/{}", short(a), short(b))
}

/// Drop hints from the end until the rest fits the footer.
pub fn fit(hints: Vec<Hint>) -> Vec<Hint> {
    fit_end(hints, View::FULL)
}

/// [`fit`] for the footer of `view`.
fn fit_end(mut hints: Vec<Hint>, view: View) -> Vec<Hint> {
    while hints.len() > 1 && hints_width_in(&hints, view) > view.footer_max() {
        hints.pop();
    }
    hints
}

/// Like [`fit`] for the footer of `view`. The cut-down picture has less room: there the hint before the last one goes
/// first, so the way back (RESUME, BACK) is kept.
pub fn fit_in(mut hints: Vec<Hint>, view: View) -> Vec<Hint> {
    if !view.is_crop() {
        return fit(hints);
    }
    while hints.len() > 1 && hints_width_in(&hints, view) > view.footer_max() {
        let at = hints.len().saturating_sub(2);
        hints.remove(at);
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::nav_for_button;
    use crate::ui::{hints_width, FOOTER_MAX};
    use crate::menu::nav_for_key;
    use sdl2::controller::Axis;

    fn texts(hints: &[Hint]) -> Vec<(String, String)> {
        hints.iter().map(|h| (h.key_text(), h.action.clone())).collect()
    }

    #[test]
    fn keyboard_menu_hints_name_the_keys_the_menu_reacts_to() {
        let s = InputState::keyboard();
        assert_eq!(texts(&[s.move_hint("MOVE"), s.change_hint("CHANGE"), s.select_hint("SELECT"), s.back_hint("BACK"), s.clear_hint("CLEAR")]),
            [("UP/DN", "MOVE"), ("L/R", "CHANGE"), ("ENTER", "SELECT"), ("ESC", "BACK"), ("DEL", "CLEAR")].map(|(a, b)| (a.to_string(), b.to_string())));
        // every key shown really does what the hint says
        for (nav, _) in [(Nav::Up, 0), (Nav::Down, 0), (Nav::Left, 0), (Nav::Right, 0), (Nav::Confirm, 0), (Nav::Back, 0), (Nav::Clear, 0)] {
            for &k in menu_keys(nav) {
                assert_eq!(nav_for_key(k, false), Some(nav), "{k:?}");
            }
            for &b in menu_buttons(nav) {
                assert_eq!(nav_for_button(b), Some(nav), "{b:?}");
            }
        }
    }

    #[test]
    fn controller_menu_hints_use_the_glyphs_of_the_controller() {
        let names = |s: &InputState| texts(&[s.move_hint("M"), s.select_hint("S"), s.back_hint("B"), s.clear_hint("C")]).into_iter().map(|t| t.0).collect::<Vec<_>>();
        assert_eq!(names(&InputState::pad(PadKind::Xbox)), ["DPADUD", "A", "B", "Y"]);
        assert_eq!(names(&InputState::pad(PadKind::PlayStation)), ["DPADUD", "CROSS", "CIRCLE", "TRIANGLE"]);
        assert_eq!(names(&InputState::pad(PadKind::Switch)), ["DPADUD", "A", "B", "Y"]);
        assert_eq!(names(&InputState::pad(PadKind::Generic)), ["DPADUD", "SOUTH", "EAST", "NORTH"]);
        assert_eq!(InputState::pad(PadKind::Xbox).change_hint("X").key_text(), "DPADLR");
        // cancelling a capture is Start on the pad, Escape on the keyboard
        assert_eq!(InputState::pad(PadKind::Switch).cancel_hint("CANCEL").key_text(), "+");
        assert_eq!(InputState::pad(PadKind::PlayStation).cancel_hint("CANCEL").key_text(), "OPTIONS");
        assert_eq!(InputState::keyboard().cancel_hint("CANCEL").key_text(), "ESC");
    }

    #[test]
    fn game_hints_follow_the_bindings() {
        let mut b = Bindings::default();
        let kb = InputState::keyboard();
        assert_eq!(texts(&kb.play_hints(&b)), [("L/R", "MOVE"), ("SPACE", "JUMP"), ("X", "FIRE"), ("C", "ITEM")].map(|(a, c)| (a.to_string(), c.to_string())));
        // rebind: the new key shows
        b.set_key(3, 0, Keycode::J);
        b.set_key(4, 0, Keycode::K);
        b.set_key(0, 0, Keycode::Q);
        b.set_key(1, 0, Keycode::E);
        assert_eq!(texts(&kb.play_hints(&b)), [("Q/E", "MOVE"), ("J", "JUMP"), ("K", "FIRE"), ("C", "ITEM")].map(|(a, c)| (a.to_string(), c.to_string())));
        // a cleared first slot: the next one shows
        b.keys[3][0] = None;
        assert_eq!(kb.play_hints(&b)[1].key_text(), "UP");

        let pad = InputState::pad(PadKind::PlayStation);
        let mut b = Bindings::default();
        assert_eq!(texts(&pad.play_hints(&b)), [("DPADLR", "MOVE"), ("CIRCLE", "JUMP"), ("CROSS", "FIRE"), ("TRIANGLE", "ITEM")].map(|(a, c)| (a.to_string(), c.to_string())));
        // custom: jump on the left bumper, fire on the right trigger, move with the right stick
        b.set_pad(3, 0, PadInput::Button(Button::LeftShoulder));
        b.set_pad(4, 0, PadInput::AxisPos(Axis::TriggerRight));
        b.set_pad(0, 0, PadInput::AxisNeg(Axis::RightX));
        b.set_pad(1, 0, PadInput::AxisPos(Axis::RightX));
        let h = pad.play_hints(&b);
        assert_eq!(texts(&h), [("RSTICKLR", "MOVE"), ("L1", "JUMP"), ("R2", "FIRE"), ("TRIANGLE", "ITEM")].map(|(a, c)| (a.to_string(), c.to_string())));
        let sw = InputState::pad(PadKind::Switch).play_hints(&b);
        assert_eq!(sw[1].key_text(), "L");
        assert_eq!(sw[2].key_text(), "ZR");
        // an action without a binding has no hint
        let mut b = Bindings::default();
        b.pad[5] = [None; 3];
        assert_eq!(pad.play_hints(&b).len(), 3);
    }

    #[test]
    fn long_binding_names_drop_the_last_hints_but_always_fit() {
        let mut b = Bindings::default();
        for (a, k) in [(0, Keycode::KpMultiply), (1, Keycode::KpDivide), (3, Keycode::KpPeriod), (4, Keycode::RShift), (5, Keycode::Application)] {
            b.set_key(a, 0, k);
        }
        let h = InputState::keyboard().play_hints(&b);
        assert!(!h.is_empty() && hints_width(&h) <= FOOTER_MAX);
    }

    #[test]
    fn the_hints_switch_with_the_last_used_device() {
        let mut s = InputState::default();
        assert_eq!(s.device, InputDevice::Keyboard);
        // a pad press without a connected pad changes nothing
        s.pad_used();
        assert_eq!(s.device, InputDevice::Keyboard);
        s.pad_connected(PadKind::Xbox);
        assert_eq!(s.device, InputDevice::Keyboard, "connecting alone does not switch");
        assert_eq!(s.select_hint("S").key_text(), "ENTER");
        s.pad_used();
        assert_eq!(s.device, InputDevice::Pad);
        assert_eq!(s.select_hint("S").key_text(), "A");
        s.key_used();
        assert_eq!(s.select_hint("S").key_text(), "ENTER");
        s.pad_used();
        s.pad_removed();
        assert_eq!((s.device, s.pad_present), (InputDevice::Keyboard, false));
        // another controller: another set
        s.pad_connected(PadKind::PlayStation);
        s.pad_used();
        assert_eq!(s.select_hint("S").key_text(), "CROSS");
    }
}
