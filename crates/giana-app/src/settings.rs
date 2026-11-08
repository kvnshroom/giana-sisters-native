//! Window, sound and network settings in `settings.conf` next to `controls.conf`.

use crate::result::Winner;
use std::collections::BTreeMap;
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
            Scaling::Whole => "WITH BORDER",
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

/// `WIDESCREEN` on the video page: how much of the level is shown to the left and right of the C64 picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Widescreen {
    /// a wider view, and the game works from the new edges: enemies, shots and items are created and removed at the edges of
    /// the wide picture (see `Widescreen::rules`)
    On,
    /// a wider view, the game plays exactly like on the C64: enemies still come and go at the old edges
    ViewOnly,
    /// the 4:3 picture of the C64
    #[default]
    Off,
}

impl Widescreen {
    pub fn label(self) -> &'static str {
        match self {
            Widescreen::On => "ON",
            Widescreen::ViewOnly => "VIEW ONLY",
            Widescreen::Off => "OFF",
        }
    }

    pub fn next(self, forward: bool) -> Self {
        match (self, forward) {
            (Widescreen::On, true) | (Widescreen::Off, false) => Widescreen::ViewOnly,
            (Widescreen::ViewOnly, true) | (Widescreen::On, false) => Widescreen::Off,
            (Widescreen::Off, true) | (Widescreen::ViewOnly, false) => Widescreen::On,
        }
    }

    /// The picture is wider than the C64's.
    pub fn is_wide(self) -> bool {
        self != Widescreen::Off
    }

    /// Shade the extra picture and fade the sprites there, to hide that enemies still come and go at the old edges. Only the
    /// view that plays by the C64's rules needs it: `On` works from the new edges. (A game that was started with other rules
    /// than the setting says now keeps its own: see `WideRules::of_game`.)
    pub fn masks_edge(self) -> bool {
        self == Widescreen::ViewOnly
    }

    /// Which rules the game logic follows: `On` makes the objects work from the edges of the wide picture, the others play
    /// like the C64.
    pub fn rules(self) -> WideRules {
        match self {
            Widescreen::On => WideRules::NewEdges(giana_core::wide::WIDE_K),
            _ => WideRules::C64,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Widescreen::On => "on",
            Widescreen::ViewOnly => "view",
            Widescreen::Off => "off",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "on" => Widescreen::On,
            "view" => Widescreen::ViewOnly,
            "off" => Widescreen::Off,
            _ => return None,
        })
    }
}

/// Where the game logic puts the edges of the world (the hook for phase 2 of the widescreen work).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WideRules {
    /// the edges of the C64 picture: enemies come and go there
    C64,
    /// the edges of the wide picture, `k` columns (8 pixels each) beyond the C64's on each side (`GameState::set_wide`)
    NewEdges(u8),
}

impl WideRules {
    /// The columns of extra width the game logic uses (0 = the C64's rules).
    pub fn k(self) -> u8 {
        match self {
            WideRules::C64 => 0,
            WideRules::NewEdges(k) => k,
        }
    }

    /// The part of the rules byte of the network handshake: bit 7 = the objects work from the wide edges, bits 4..6 = `k`.
    pub fn byte(self) -> u8 {
        match self {
            WideRules::C64 => 0,
            WideRules::NewEdges(k) => 0x80 | (k.min(7) << 4),
        }
    }

    /// The rules of a rules byte (the host's choice).
    pub fn from_byte(b: u8) -> Self {
        if b & 0x80 != 0 {
            WideRules::NewEdges((b >> 4) & 7)
        } else {
            WideRules::C64
        }
    }

    /// What the game that runs plays by, whatever the setting is now (the rules are fixed when a game starts).
    pub fn of_game(game: &giana_core::state::GameState) -> Self {
        match game.wide_k {
            0 => WideRules::C64,
            k => WideRules::NewEdges(k),
        }
    }
}

/// What the player sees: the new design with menus (`MODERN`) or exactly what the C64 showed (`AUTHENTIC`): the original
/// title sequence, high score table and name entry, no menus, no hints, and the original program running as it was.
/// Independent of `MODE`. See `docs/authentic-mode.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Presentation {
    #[default]
    Modern,
    Authentic,
}

impl Presentation {
    pub fn label(self) -> &'static str {
        match self {
            Presentation::Modern => "MODERN",
            Presentation::Authentic => "AUTHENTIC",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Presentation::Modern => Presentation::Authentic,
            Presentation::Authentic => Presentation::Modern,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Presentation::Modern => "modern",
            Presentation::Authentic => "authentic",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "modern" => Presentation::Modern,
            "authentic" => Presentation::Authentic,
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

/// How the ROM modules are ordered below the favorites (`SORT` on the modules page).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ModuleSort {
    /// by how often the modules were downloaded, unknown ones after the known ones by name
    #[default]
    Popular,
    Name,
    /// the module played last first, modules never played after the others
    LastPlayed,
}

impl ModuleSort {
    pub fn label(self) -> &'static str {
        match self {
            ModuleSort::Popular => "POPULAR",
            ModuleSort::Name => "NAME",
            ModuleSort::LastPlayed => "LAST PLAYED",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ModuleSort::Popular => ModuleSort::Name,
            ModuleSort::Name => ModuleSort::LastPlayed,
            ModuleSort::LastPlayed => ModuleSort::Popular,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ModuleSort::Popular => "popular",
            ModuleSort::Name => "name",
            ModuleSort::LastPlayed => "last_played",
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "popular" => ModuleSort::Popular,
            "name" => ModuleSort::Name,
            "last_played" => ModuleSort::LastPlayed,
            _ => return None,
        })
    }
}

/// The key of the original game in `Settings::progress` (it has no module id).
pub const ORIGINAL_KEY: &str = "original";

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
    /// The `SMOOTH MOTION` switch of the video page. It shows `smooth_motion()`: turning it on also sets the master
    /// `mode` to enhanced (both keys stay in `settings.conf`); turning it off leaves `mode` as it is.
    pub fn toggle_smooth_motion(&mut self) {
        if self.smooth_motion() {
            self.smooth_motion = false;
        } else {
            self.enhanced = true;
            self.smooth_motion = true;
        }
    }

    /// Smooth motion is on: the mode is `ENHANCED` and the switch is on.
    pub fn smooth_motion(&self) -> bool {
        self.enhanced && self.smooth_motion
    }

    /// Bits for the network handshake: the enhancements that change the game itself, as the host chose them. None exist yet.
    pub fn rules(&self) -> u8 {
        0
    }
}

impl Settings {
    /// The rules byte for the network handshake (the host's choice): the enhancements and the widescreen rules.
    pub fn net_rules(&self) -> u8 {
        self.enhancements.rules() | self.widescreen.rules().byte()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub fullscreen: bool,
    /// 0 to `MAX_VOLUME`
    pub volume: u8,
    pub muted: bool,
    pub scaling: Scaling,
    pub widescreen: Widescreen,
    /// `SCREEN: CRT`: the picture looks like on a tube television (scanlines, glow); off is sharp pixels
    pub crt: bool,
    /// 0 to `MAX_VOLUME`: music and effects on their own (both at `MAX_VOLUME` is the original sound from one chip)
    pub music_volume: u8,
    pub effects_volume: u8,
    pub enhancements: Enhancements,
    /// `PRESENTATION: MODERN / AUTHENTIC`
    pub presentation: Presentation,
    pub menu_sounds: MenuSoundMode,
    /// the address typed last on the join page
    pub last_address: String,
    /// the rule for the winner of a game of two players (the host's choice counts in a network game)
    pub winner: Winner,
    /// fullscreen at 50 Hz (or 100, 150, 200) when the display offers it (`DISPLAY 50 HZ: AUTO`), otherwise the desktop mode stays
    pub display_50: bool,
    /// look for a new version at every start (`UPDATE CHECK: ON START / OFF`)
    pub update_check: bool,
    /// seconds since 1970 of the last check, 0 for never
    pub last_update_check: u64,
    /// the newest version that the last check found (empty: none)
    pub update_seen: String,
    /// id of the chosen ROM module (empty: the original game)
    pub module: String,
    /// ids of the favorite ROM modules (shown first on the modules page)
    pub favorites: Vec<String>,
    /// `SORT` on the modules page
    pub module_sort: ModuleSort,
    /// seconds since 1970 when a module was started, by module id
    pub last_played: BTreeMap<String, u64>,
    /// the highest stage reached with a module, by module id (`ORIGINAL_KEY` for the original game)
    pub progress: BTreeMap<String, u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            volume: MAX_VOLUME,
            muted: false,
            scaling: Scaling::NoBorder,
            widescreen: Widescreen::Off,
            crt: false,
            music_volume: MAX_VOLUME,
            effects_volume: MAX_VOLUME,
            enhancements: Enhancements::default(),
            presentation: Presentation::Modern,
            menu_sounds: MenuSoundMode::Soft,
            last_address: String::new(),
            winner: Winner::Score,
            display_50: true,
            update_check: true,
            last_update_check: 0,
            update_seen: String::new(),
            module: String::new(),
            favorites: Vec::new(),
            module_sort: ModuleSort::Popular,
            last_played: BTreeMap::new(),
            progress: BTreeMap::new(),
        }
    }
}

/// A module id as the files write it: 16 lowercase hex digits (or `ORIGINAL_KEY`), anything else is dropped.
fn clean_key(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    (s == ORIGINAL_KEY || (s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()))).then_some(s)
}

/// `id:number,id:number`
fn map_text<V: std::fmt::Display>(map: &BTreeMap<String, V>) -> String {
    map.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(",")
}

fn parse_map<V: std::str::FromStr>(value: &str) -> BTreeMap<String, V> {
    value.split(',').filter_map(|pair| pair.split_once(':')).filter_map(|(k, v)| Some((clean_key(k)?, v.trim().parse().ok()?))).collect()
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

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|f| f == id)
    }

    /// Adds the module to the favorites or takes it out; the original game is no favorite (it is always first).
    pub fn toggle_favorite(&mut self, id: &str) {
        if id == ORIGINAL_KEY || clean_key(id).as_deref() != Some(id) {
            return;
        }
        if self.is_favorite(id) {
            self.favorites.retain(|f| f != id);
        } else {
            self.favorites.push(id.to_string());
        }
    }

    /// The stage that was reached most far with a module (empty id: the original game).
    pub fn stage_reached(&self, id: &str) -> Option<u32> {
        self.progress.get(if id.is_empty() { ORIGINAL_KEY } else { id }).copied()
    }

    /// Notes a stage reached while playing. Returns true when it is a new best (the settings then want saving).
    pub fn note_stage(&mut self, id: &str, stage: u32) -> bool {
        let key = if id.is_empty() { ORIGINAL_KEY } else { id };
        if stage == 0 || self.progress.get(key).is_some_and(|&s| s >= stage) {
            return false;
        }
        self.progress.insert(key.to_string(), stage);
        true
    }

    /// Notes that a module was started now (seconds since 1970). The original game is always first, no time is kept for it.
    pub fn note_played(&mut self, id: &str, now: u64) {
        if !id.is_empty() {
            self.last_played.insert(id.to_string(), now);
        }
    }

    pub fn to_text(&self) -> String {
        format!(
            "# The Great Giana Sisters: settings.\nfullscreen = {}\nvolume = {}\nmuted = {}\nscaling = {}\nwidescreen = {}\ncrt = {}\nwinner = {}\ndisplay_50hz = {}\nmusic_volume = {}\neffects_volume = {}\nmode = {}\nsmooth_motion = {}\npresentation = {}\nmenu_sounds = {}\nupdate_check = {}\nlast_update_check = {}\nupdate_seen = {}\nmodule = {}\nfavorites = {}\nmodule_sort = {}\nlast_played = {}\nprogress = {}\nlast_address = {}\n",
            self.fullscreen,
            self.volume,
            self.muted,
            self.scaling.name(),
            self.widescreen.name(),
            if self.crt { "on" } else { "off" },
            self.winner.label().to_lowercase(),
            if self.display_50 { "auto" } else { "off" },
            self.music_volume,
            self.effects_volume,
            if self.enhancements.enhanced { "enhanced" } else { "original" },
            self.enhancements.smooth_motion,
            self.presentation.name(),
            self.menu_sounds.name(),
            if self.update_check { "auto" } else { "off" },
            self.last_update_check,
            self.update_seen,
            self.module,
            self.favorites.join(","),
            self.module_sort.name(),
            map_text(&self.last_played),
            map_text(&self.progress),
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
                "widescreen" => s.widescreen = Widescreen::from_name(value).unwrap_or(s.widescreen),
                "crt" => s.crt = value == "on",
                "music_volume" => s.music_volume = value.parse::<u8>().map_or(s.music_volume, |v| v.min(MAX_VOLUME)),
                "effects_volume" => s.effects_volume = value.parse::<u8>().map_or(s.effects_volume, |v| v.min(MAX_VOLUME)),
                "mode" => s.enhancements.enhanced = value == "enhanced",
                "smooth_motion" => s.enhancements.smooth_motion = value.parse().unwrap_or(s.enhancements.smooth_motion),
                "presentation" => s.presentation = Presentation::from_name(value).unwrap_or(s.presentation),
                "menu_sounds" => s.menu_sounds = MenuSoundMode::from_name(value).unwrap_or(s.menu_sounds),
                "update_check" => s.update_check = !matches!(value, "off" | "false" | "no" | "0"),
                "last_update_check" => s.last_update_check = value.parse().unwrap_or(0),
                "update_seen" => s.update_seen = value.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')).take(32).collect(),
                "module" => s.module = value.chars().filter(|c| c.is_ascii_hexdigit()).take(16).collect(),
                "favorites" => {
                    s.favorites.clear();
                    for id in value.split(',').filter_map(clean_key).filter(|k| k != ORIGINAL_KEY) {
                        if !s.favorites.contains(&id) {
                            s.favorites.push(id);
                        }
                    }
                }
                "module_sort" => s.module_sort = ModuleSort::from_name(value).unwrap_or(s.module_sort),
                "last_played" => s.last_played = parse_map(value),
                "progress" => s.progress = parse_map(value),
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
            widescreen: Widescreen::ViewOnly,
            crt: true,
            music_volume: 3,
            effects_volume: 0,
            enhancements: Enhancements { enhanced: true, smooth_motion: false },
            presentation: Presentation::Authentic,
            last_address: "192.168.1.5:47064".into(),
            winner: Winner::Furthest,
            display_50: false,
            menu_sounds: MenuSoundMode::Classic,
            update_check: false,
            last_update_check: 1_790_000_000,
            update_seen: "0.2.0".into(),
            module: "0123456789abcdef".into(),
            favorites: vec!["0123456789abcdef".into(), "fedcba9876543210".into()],
            module_sort: ModuleSort::LastPlayed,
            last_played: BTreeMap::from([("0123456789abcdef".to_string(), 1_790_000_000), ("fedcba9876543210".to_string(), 5)]),
            progress: BTreeMap::from([("original".to_string(), 12), ("0123456789abcdef".to_string(), 3)]),
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
        // WIDESCREEN: round trip, and the default for old files and damaged values
        for widescreen in [Widescreen::On, Widescreen::ViewOnly, Widescreen::Off] {
            let s = Settings { widescreen, ..Settings::default() };
            assert_eq!(Settings::from_text(&s.to_text()).widescreen, widescreen);
        }
        assert_eq!(Settings::default().widescreen, Widescreen::Off);
        assert_eq!(Settings::from_text("volume = 3\nscaling = fill\n").widescreen, Widescreen::Off);
        assert_eq!(Settings::from_text("widescreen = sideways\n").widescreen, Widescreen::Off);
        assert_eq!(Settings::from_text("widescreen = view\n").widescreen, Widescreen::ViewOnly);
        assert_eq!(Widescreen::On.next(true), Widescreen::ViewOnly);
        assert_eq!(Widescreen::ViewOnly.next(true), Widescreen::Off);
        assert_eq!(Widescreen::Off.next(true), Widescreen::On);
        assert_eq!(Widescreen::On.next(false), Widescreen::Off);
        assert!(Widescreen::On.is_wide() && Widescreen::ViewOnly.is_wide() && !Widescreen::Off.is_wide());
    }

    #[test]
    fn bad_favorites_sorting_and_progress_values_are_dropped() {
        let d = Settings::default();
        assert_eq!((d.module_sort, d.favorites.len(), d.last_played.len(), d.progress.len()), (ModuleSort::Popular, 0, 0, 0));
        let s = Settings::from_text("favorites = 0123456789abcdef,nonsense,0123456789ABCDEF,original\nmodule_sort = sideways\nlast_played = 0123456789abcdef:x,fedcba9876543210:7\nprogress = original:4,short:3\n");
        assert_eq!(s.favorites, vec!["0123456789abcdef".to_string()]);
        assert_eq!(s.module_sort, ModuleSort::Popular);
        assert_eq!(s.last_played, BTreeMap::from([("fedcba9876543210".to_string(), 7)]));
        assert_eq!(s.progress, BTreeMap::from([("original".to_string(), 4)]));
        for sort in [ModuleSort::Popular, ModuleSort::Name, ModuleSort::LastPlayed] {
            assert_eq!(Settings::from_text(&Settings { module_sort: sort, ..Settings::default() }.to_text()).module_sort, sort);
            assert_eq!(sort.next().next().next(), sort);
        }
    }

    #[test]
    fn toggling_a_favorite_and_noting_progress() {
        let mut s = Settings::default();
        s.toggle_favorite("0123456789abcdef");
        assert!(s.is_favorite("0123456789abcdef"));
        s.toggle_favorite("0123456789abcdef");
        assert!(!s.is_favorite("0123456789abcdef"));
        // neither the original game nor a bad id is a favorite
        s.toggle_favorite("original");
        s.toggle_favorite("");
        s.toggle_favorite("zz");
        assert!(s.favorites.is_empty());
        // the best stage only goes up
        assert!(s.note_stage("", 5));
        assert!(!s.note_stage("", 4) && !s.note_stage("", 5) && !s.note_stage("", 0));
        assert!(s.note_stage("", 6));
        assert_eq!(s.stage_reached(""), Some(6));
        assert_eq!(s.stage_reached("0123456789abcdef"), None);
        s.note_played("0123456789abcdef", 99);
        s.note_played("", 100);
        assert_eq!(s.last_played.len(), 1);
    }

    #[test]
    fn the_update_check_is_off_for_every_usual_way_to_say_no() {
        for no in ["off", "false", "no", "0"] {
            assert!(!Settings::from_text(&format!("update_check = {no}\n")).update_check, "{no}");
        }
        assert!(Settings::from_text("update_check = auto\n").update_check);
        assert!(Settings::default().update_check);
    }

    #[test]
    fn presentation_is_modern_by_default_and_survives_a_round_trip() {
        assert_eq!(Settings::default().presentation, Presentation::Modern);
        // old files and damaged values give MODERN
        assert_eq!(Settings::from_text("volume = 3\n").presentation, Presentation::Modern);
        assert_eq!(Settings::from_text("presentation = retro\n").presentation, Presentation::Modern);
        for presentation in [Presentation::Modern, Presentation::Authentic] {
            let s = Settings { presentation, ..Settings::default() };
            assert_eq!(Settings::from_text(&s.to_text()).presentation, presentation);
        }
        assert_eq!(Presentation::Modern.next().next(), Presentation::Modern);
        // independent of the mode
        let s = Settings::from_text("mode = enhanced\npresentation = authentic\n");
        assert!(s.enhancements.enhanced && s.presentation == Presentation::Authentic);
    }

    /// The one SMOOTH MOTION switch shows the same state as the old MODE + SMOOTH MOTION pair for every old file, and
    /// writes both keys back.
    #[test]
    fn the_smooth_motion_switch_reads_and_writes_mode_and_smooth_motion() {
        for (text, shown) in [
            ("mode = original\nsmooth_motion = true\n", false),
            ("mode = original\nsmooth_motion = false\n", false),
            ("mode = enhanced\nsmooth_motion = true\n", true),
            ("mode = enhanced\nsmooth_motion = false\n", false),
            ("volume = 3\n", false),
        ] {
            let mut s = Settings::from_text(text);
            assert_eq!(s.enhancements.smooth_motion(), shown, "{text:?}");
            // switching flips what is shown, and the file keeps saying so
            s.enhancements.toggle_smooth_motion();
            assert_eq!(s.enhancements.smooth_motion(), !shown, "{text:?}");
            let saved = s.to_text();
            let back = Settings::from_text(&saved);
            assert_eq!(back.enhancements.smooth_motion(), !shown, "{text:?}");
            if !shown {
                assert!(saved.contains("mode = enhanced\n") && saved.contains("smooth_motion = true\n"), "{saved}");
            }
        }
        // switched off, the master mode stays as it was
        let mut s = Settings::from_text("mode = enhanced\nsmooth_motion = true\n");
        s.enhancements.toggle_smooth_motion();
        assert!(s.to_text().contains("mode = enhanced\n") && s.to_text().contains("smooth_motion = false\n"));
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
