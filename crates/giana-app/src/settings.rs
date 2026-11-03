//! Window, sound and network settings in `settings.conf` next to `controls.conf`.

use crate::result::Winner;
use std::path::{Path, PathBuf};

pub const MAX_VOLUME: u8 = 10;

/// How the picture fills the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// the whole C64 frame including its border, scaled by a whole number (sharp pixels, black bars)
    Whole,
    /// while playing the empty border is cut off and the playing area is scaled by a whole number (bigger, sharp pixels)
    NoBorder,
    /// the playing area without border, as big as the window allows with its proportions (not a whole number, so the
    /// pixels are smoothed a little at their edges)
    Fill,
}

impl Scaling {
    pub fn label(self) -> &'static str {
        match self {
            Scaling::Whole => "WHOLE PIXELS",
            Scaling::NoBorder => "NO BORDER",
            Scaling::Fill => "FILL SCREEN",
        }
    }

    pub fn next(self, forward: bool) -> Self {
        match (self, forward) {
            (Scaling::Whole, true) | (Scaling::Fill, false) => Scaling::NoBorder,
            (Scaling::NoBorder, true) | (Scaling::Whole, false) => Scaling::Fill,
            (Scaling::Fill, true) | (Scaling::NoBorder, false) => Scaling::Whole,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Scaling::Whole => "integer",
            Scaling::NoBorder => "no_border",
            Scaling::Fill => "fill",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "integer" => Scaling::Whole,
            "no_border" => Scaling::NoBorder,
            // "fit" is what older versions wrote for the largest size
            "fill" | "fit" => Scaling::Fill,
            _ => return None,
        })
    }
}

/// What the menus sound like (`MENU SOUNDS` on the options page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSoundMode {
    /// no sounds in the menus
    Off,
    /// short, quiet ticks made on the SID chip (default)
    Soft,
    /// the sound effects of the original game
    Classic,
}

impl MenuSoundMode {
    pub fn label(self) -> &'static str {
        match self {
            MenuSoundMode::Off => "OFF",
            MenuSoundMode::Soft => "SOFT",
            MenuSoundMode::Classic => "CLASSIC",
        }
    }

    pub fn next(self, forward: bool) -> Self {
        match (self, forward) {
            (MenuSoundMode::Off, true) | (MenuSoundMode::Classic, false) => MenuSoundMode::Soft,
            (MenuSoundMode::Soft, true) | (MenuSoundMode::Off, false) => MenuSoundMode::Classic,
            (MenuSoundMode::Classic, true) | (MenuSoundMode::Soft, false) => MenuSoundMode::Off,
        }
    }

    fn name(self) -> &'static str {
        match self {
            MenuSoundMode::Off => "off",
            MenuSoundMode::Soft => "soft",
            MenuSoundMode::Classic => "classic",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "off" => MenuSoundMode::Off,
            "soft" => MenuSoundMode::Soft,
            "classic" => MenuSoundMode::Classic,
            _ => return None,
        })
    }
}

/// Changes that are not part of the original game: `MODE: ORIGINAL / ENHANCED` on the options page. In `ORIGINAL` mode
/// everything is as it always was, whatever the single switches say.
///
/// Enhancements that only change how the game is shown or heard stay on this computer. One that changes how the game
/// plays has to be the same on both computers of a network game: the host decides and sends `rules` in the handshake (see
/// `net`); no such enhancement exists yet, so `rules` is always 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enhancements {
    /// the mode switch: false is `ORIGINAL`
    pub enhanced: bool,
    /// on displays that are not a multiple of 50 Hz the picture is shown between two game steps (render only)
    pub smooth_motion: bool,
}

impl Default for Enhancements {
    fn default() -> Self {
        Self { enhanced: false, smooth_motion: true }
    }
}

impl Enhancements {
    /// Smooth motion is on: the mode is `ENHANCED` and the switch is on.
    pub fn smooth_motion(&self) -> bool {
        self.enhanced && self.smooth_motion
    }

    /// Bits for the network handshake: the enhancements that change the game itself, as the host chose them. None exist yet.
    pub fn rules(&self) -> u8 {
        0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub fullscreen: bool,
    /// 0 to `MAX_VOLUME`
    pub volume: u8,
    pub muted: bool,
    pub scaling: Scaling,
    /// 0 to `MAX_VOLUME`: music and effects on their own (both at `MAX_VOLUME` is the original sound from one chip)
    pub music_volume: u8,
    pub effects_volume: u8,
    pub enhancements: Enhancements,
    pub menu_sounds: MenuSoundMode,
    /// the address typed last on the join page
    pub last_address: String,
    /// the rule for the winner of a game of two players (the host's choice counts in a network game)
    pub winner: Winner,
    /// fullscreen at 50 Hz (or 100, 150, 200) when the display offers it (`DISPLAY 50 HZ: AUTO`), otherwise the desktop mode stays
    pub display_50: bool,
    /// look for a new version once a day (`CHECK FOR UPDATES: AUTO / OFF`)
    pub update_check: bool,
    /// seconds since 1970 of the last check, 0 for never
    pub last_update_check: u64,
    /// the newest version that the last check found (empty: none)
    pub update_seen: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            volume: MAX_VOLUME,
            muted: false,
            scaling: Scaling::Whole,
            music_volume: MAX_VOLUME,
            effects_volume: MAX_VOLUME,
            enhancements: Enhancements::default(),
            menu_sounds: MenuSoundMode::Soft,
            last_address: String::new(),
            winner: Winner::Score,
            display_50: true,
            update_check: true,
            last_update_check: 0,
            update_seen: String::new(),
        }
    }
}

impl Settings {
    /// Factor for the samples, 256 is full volume. The curve is quadratic, so that the steps sound even.
    pub fn gain(&self) -> i32 {
        if self.muted {
            return 0;
        }
        Self::part_gain(self.volume)
    }

    /// Factor for one slider (music or effects), 256 is full volume (same curve as `gain`).
    pub fn part_gain(volume: u8) -> i32 {
        let v = volume.min(MAX_VOLUME) as i32;
        256 * v * v / (MAX_VOLUME as i32 * MAX_VOLUME as i32)
    }

    /// Music and effects run on two chips of their own when one of the sliders is not at its full value.
    pub fn split_sound(&self) -> bool {
        self.music_volume < MAX_VOLUME || self.effects_volume < MAX_VOLUME
    }

    pub fn change_music(&mut self, up: bool) {
        self.music_volume = if up { (self.music_volume + 1).min(MAX_VOLUME) } else { self.music_volume.saturating_sub(1) };
    }

    pub fn change_effects(&mut self, up: bool) {
        self.effects_volume = if up { (self.effects_volume + 1).min(MAX_VOLUME) } else { self.effects_volume.saturating_sub(1) };
    }

    pub fn change_volume(&mut self, up: bool) {
        self.muted = false;
        self.volume = if up { (self.volume + 1).min(MAX_VOLUME) } else { self.volume.saturating_sub(1) };
    }

    pub fn to_text(&self) -> String {
        format!(
            "# The Great Giana Sisters: settings.\nfullscreen = {}\nvolume = {}\nmuted = {}\nscaling = {}\nwinner = {}\ndisplay_50hz = {}\nmusic_volume = {}\neffects_volume = {}\nmode = {}\nsmooth_motion = {}\nmenu_sounds = {}\nupdate_check = {}\nlast_update_check = {}\nupdate_seen = {}\nlast_address = {}\n",
            self.fullscreen,
            self.volume,
            self.muted,
            self.scaling.name(),
            self.winner.label().to_lowercase(),
            if self.display_50 { "auto" } else { "off" },
            self.music_volume,
            self.effects_volume,
            if self.enhancements.enhanced { "enhanced" } else { "original" },
            self.enhancements.smooth_motion,
            self.menu_sounds.name(),
            if self.update_check { "auto" } else { "off" },
            self.last_update_check,
            self.update_seen,
            self.last_address
        )
    }

    /// Lines that cannot be read keep their default.
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else { continue };
            let (name, value) = (name.trim(), value.trim());
            match name {
                "fullscreen" => s.fullscreen = value.parse().unwrap_or(s.fullscreen),
                "muted" => s.muted = value.parse().unwrap_or(s.muted),
                "volume" => s.volume = value.parse::<u8>().map_or(s.volume, |v| v.min(MAX_VOLUME)),
                "scaling" => s.scaling = Scaling::from_name(value).unwrap_or(s.scaling),
                "music_volume" => s.music_volume = value.parse::<u8>().map_or(s.music_volume, |v| v.min(MAX_VOLUME)),
                "effects_volume" => s.effects_volume = value.parse::<u8>().map_or(s.effects_volume, |v| v.min(MAX_VOLUME)),
                "mode" => s.enhancements.enhanced = value == "enhanced",
                "smooth_motion" => s.enhancements.smooth_motion = value.parse().unwrap_or(s.enhancements.smooth_motion),
                "menu_sounds" => s.menu_sounds = MenuSoundMode::from_name(value).unwrap_or(s.menu_sounds),
                "update_check" => s.update_check = value != "off",
                "last_update_check" => s.last_update_check = value.parse().unwrap_or(0),
                "update_seen" => s.update_seen = value.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')).take(32).collect(),
                "display_50hz" => s.display_50 = value != "off",
                "winner" => s.winner = if value == "furthest" { Winner::Furthest } else { Winner::Score },
                "last_address" => s.last_address = value.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ':').take(21).collect(),
                _ => {}
            }
        }
        s
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map_or_else(|_| Self::default(), |t| Self::from_text(&t))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::fsutil::write_atomic(path, &self.to_text())
    }
}

pub fn settings_path() -> Option<PathBuf> {
    crate::bindings::config_path().map(|p| p.with_file_name("settings.conf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings {
            fullscreen: true,
            volume: 4,
            muted: true,
            scaling: Scaling::Fill,
            music_volume: 3,
            effects_volume: 0,
            enhancements: Enhancements { enhanced: true, smooth_motion: false },
            last_address: "192.168.1.5:47064".into(),
            winner: Winner::Furthest,
            display_50: false,
            menu_sounds: MenuSoundMode::Classic,
            update_check: false,
            last_update_check: 1_790_000_000,
            update_seen: "0.2.0".into(),
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        for menu_sounds in [MenuSoundMode::Off, MenuSoundMode::Soft, MenuSoundMode::Classic] {
            let s = Settings { menu_sounds, ..Settings::default() };
            assert_eq!(Settings::from_text(&s.to_text()).menu_sounds, menu_sounds);
        }
        // default, old files and damaged values give SOFT
        assert_eq!(Settings::default().menu_sounds, MenuSoundMode::Soft);
        assert_eq!(Settings::from_text("volume = 3\n").menu_sounds, MenuSoundMode::Soft);
        assert_eq!(Settings::from_text("menu_sounds = loud\n").menu_sounds, MenuSoundMode::Soft);
        assert_eq!(MenuSoundMode::Soft.next(true), MenuSoundMode::Classic);
        assert_eq!(MenuSoundMode::Off.next(false), MenuSoundMode::Classic);
        for scaling in [Scaling::Whole, Scaling::NoBorder, Scaling::Fill] {
            let s = Settings { scaling, ..Settings::default() };
            assert_eq!(Settings::from_text(&s.to_text()).scaling, scaling);
        }
    }

    #[test]
    fn the_original_mode_is_the_default_and_old_files_still_load() {
        let s = Settings::default();
        assert!(!s.enhancements.enhanced && !s.enhancements.smooth_motion() && !s.split_sound());
        // a file of an older version: "fit" was the largest size, no mode, no sliders
        let old = Settings::from_text("scaling = fit\nvolume = 7\n");
        assert_eq!((old.scaling, old.volume, old.enhancements, old.music_volume, old.effects_volume), (Scaling::Fill, 7, Enhancements::default(), MAX_VOLUME, MAX_VOLUME));
        // the single switch does nothing in the original mode
        let mut e = Enhancements { enhanced: false, smooth_motion: true };
        assert!(!e.smooth_motion());
        e.enhanced = true;
        assert!(e.smooth_motion());
        assert_eq!(e.rules(), 0);
    }

    #[test]
    fn the_sound_is_split_when_a_slider_is_down() {
        let mut s = Settings::default();
        s.change_effects(false);
        assert!(s.split_sound());
        s.change_effects(true);
        assert!(!s.split_sound());
        s.change_music(false);
        assert!(s.split_sound());
        assert_eq!(Settings::part_gain(MAX_VOLUME), 256);
        assert_eq!(Settings::part_gain(0), 0);
        for _ in 0..20 {
            s.change_music(false);
        }
        assert_eq!(s.music_volume, 0);
    }

    #[test]
    fn scaling_cycles_through_all_modes() {
        let mut m = Scaling::Whole;
        let mut seen = Vec::new();
        for _ in 0..3 {
            m = m.next(true);
            seen.push(m);
        }
        assert_eq!(seen, [Scaling::NoBorder, Scaling::Fill, Scaling::Whole]);
        assert_eq!(Scaling::Whole.next(false), Scaling::Fill);
    }

    #[test]
    fn damaged_lines_keep_the_default() {
        assert_eq!(Settings::from_text("fullscreen = maybe\nvolume = 99x\nnonsense\n"), Settings::default());
        assert_eq!(Settings::from_text("volume = 99").volume, MAX_VOLUME);
    }

    #[test]
    fn gain_is_full_by_default_and_zero_when_muted() {
        let mut s = Settings::default();
        assert_eq!(s.gain(), 256);
        s.change_volume(false);
        assert!(s.gain() < 256 && s.gain() > 0);
        s.muted = true;
        assert_eq!(s.gain(), 0);
        s.change_volume(true);
        assert!(!s.muted, "changing the volume ends the mute");
        s.volume = 0;
        s.change_volume(false);
        assert_eq!(s.volume, 0);
    }
}
