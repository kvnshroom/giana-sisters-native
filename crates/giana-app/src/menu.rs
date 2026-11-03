//! The main menu, the pause menu and the two controls pages. Not part of the original game.
//!
//! The menu is steered with fixed keys and buttons (see `main.rs`), so that no binding can lock anyone out of it.

use crate::bindings::{Bindings, PadInput, ACTIONS, SLOTS};
use crate::net::NetMode;
use crate::result::MatchResult;
use crate::scores::{Entry, Scores, NAME_LEN};
use crate::settings::Settings;
use sdl2::controller::{Axis, Button};
use sdl2::keyboard::Keycode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Clear,
}

/// Fixed menu keys, whatever the bindings say: the first one of each is the one the hints show.
const MENU_KEYS: [(Nav, &[Keycode]); 7] = [
    (Nav::Up, &[Keycode::Up, Keycode::W]),
    (Nav::Down, &[Keycode::Down, Keycode::S]),
    (Nav::Left, &[Keycode::Left, Keycode::A]),
    (Nav::Right, &[Keycode::Right, Keycode::D]),
    (Nav::Confirm, &[Keycode::Return, Keycode::KpEnter, Keycode::Space]),
    (Nav::Back, &[Keycode::Escape, Keycode::Backspace]),
    (Nav::Clear, &[Keycode::Delete]),
];

/// Fixed menu buttons.
const MENU_BUTTONS: [(Nav, &[Button]); 7] = [
    (Nav::Up, &[Button::DPadUp]),
    (Nav::Down, &[Button::DPadDown]),
    (Nav::Left, &[Button::DPadLeft]),
    (Nav::Right, &[Button::DPadRight]),
    (Nav::Confirm, &[Button::A]),
    (Nav::Back, &[Button::B, Button::Start]),
    (Nav::Clear, &[Button::Y]),
];

/// The keys that do `nav` in the menus.
pub fn menu_keys(nav: Nav) -> &'static [Keycode] {
    MENU_KEYS.iter().find(|(n, _)| *n == nav).map_or(&[], |(_, k)| k)
}

/// The buttons that do `nav` in the menus.
pub fn menu_buttons(nav: Nav) -> &'static [Button] {
    MENU_BUTTONS.iter().find(|(n, _)| *n == nav).map_or(&[], |(_, b)| b)
}

/// Moving repeats while a key is held, the rest does not.
pub fn nav_for_key(key: Keycode, repeat: bool) -> Option<Nav> {
    let nav = MENU_KEYS.iter().find(|(_, keys)| keys.contains(&key))?.0;
    (!repeat || matches!(nav, Nav::Up | Nav::Down | Nav::Left | Nav::Right)).then_some(nav)
}

pub fn nav_for_button(button: Button) -> Option<Nav> {
    MENU_BUTTONS.iter().find(|(_, buttons)| buttons.contains(&button)).map(|(n, _)| *n)
}

/// Fixed menu directions of the left stick.
pub fn nav_for_stick(input: PadInput) -> Option<Nav> {
    Some(match input {
        PadInput::AxisNeg(Axis::LeftY) => Nav::Up,
        PadInput::AxisPos(Axis::LeftY) => Nav::Down,
        PadInput::AxisNeg(Axis::LeftX) => Nav::Left,
        PadInput::AxisPos(Axis::LeftX) => Nav::Right,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Pad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Root,
    Controls(Device),
    /// host or join a game with another computer
    Network,
    /// a game is open, waiting for the other player
    Hosting,
    Join,
    Scores,
    /// a score made the table: the name for it
    EnterName,
    Options,
    /// `MODE: ORIGINAL / ENHANCED` with the single enhancements, reached from the options page
    Enhanced,
    /// volumes and menu sounds, reached from the options page
    Sound,
    /// the end of a game of two players: who won
    Result,
    /// asked in the pause menu before a game is given up: `QUIT GAME? YES / NO`
    ConfirmQuit,
}

/// How the search for games on the join page is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search {
    Looking,
    /// no game answered for a while
    NothingFound,
    /// searching does not work at all
    Unavailable,
}

/// A setting changed on the options page; the app applies and saves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionChange {
    Fullscreen,
    VolumeDown,
    VolumeUp,
    /// the next (true) or the previous way to fill the window
    Scaling(bool),
    /// the rule for the winner of a game of two players
    Winner,
    /// fullscreen at 50 Hz when the display offers it
    Display50,
    MusicDown,
    MusicUp,
    EffectsDown,
    EffectsUp,
    /// the next (true) or previous `MENU SOUNDS` setting
    MenuSounds(bool),
    /// `MODE: ORIGINAL / ENHANCED`
    Mode,
    SmoothMotion,
    /// `CHECK FOR UPDATES: AUTO / OFF`
    UpdateCheck,
}

/// The lines of the options page, in order
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptionItem {
    Fullscreen,
    Scaling,
    Display50,
    /// opens the sound page
    Sound,
    Winner,
    Mode,
    UpdateCheck,
    Back,
}

pub(crate) const OPTION_ITEMS: [OptionItem; 8] = [
    OptionItem::Fullscreen,
    OptionItem::Scaling,
    OptionItem::Display50,
    OptionItem::Sound,
    OptionItem::Winner,
    OptionItem::Mode,
    OptionItem::UpdateCheck,
    OptionItem::Back,
];

impl OptionItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            OptionItem::Fullscreen => "FULLSCREEN",
            OptionItem::Scaling => "SCALING",
            OptionItem::Display50 => "DISPLAY 50 HZ",
            OptionItem::Sound => "SOUND",
            OptionItem::Winner => "WINNER",
            OptionItem::Mode => "MODE",
            OptionItem::UpdateCheck => "UPDATE CHECK",
            OptionItem::Back => "BACK",
        }
    }
}

/// The lines of the sound page, reached from the options page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SoundItem {
    Volume,
    Music,
    Effects,
    MenuSounds,
    Back,
}

pub(crate) const SOUND_ITEMS: [SoundItem; 5] = [SoundItem::Volume, SoundItem::Music, SoundItem::Effects, SoundItem::MenuSounds, SoundItem::Back];

impl SoundItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SoundItem::Volume => "VOLUME",
            SoundItem::Music => "MUSIC",
            SoundItem::Effects => "EFFECTS",
            SoundItem::MenuSounds => "MENU SOUNDS",
            SoundItem::Back => "BACK",
        }
    }
}

/// The lines of the page with the enhancements. More can be added here: each is a switch in `Enhancements` that only works
/// in `ENHANCED` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnhancedItem {
    Mode,
    SmoothMotion,
    Back,
}

pub(crate) const ENHANCED_ITEMS: [EnhancedItem; 3] = [EnhancedItem::Mode, EnhancedItem::SmoothMotion, EnhancedItem::Back];

impl EnhancedItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            EnhancedItem::Mode => "MODE",
            EnhancedItem::SmoothMotion => "SMOOTH MOTION",
            EnhancedItem::Back => "BACK",
        }
    }
}

/// How long the result stays on the screen before a key can close it, in logic steps (50 per second)
pub const RESULT_HOLD: u32 = 150;

/// How a new game is played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMode {
    OnePlayer,
    /// two players on one computer, the turn changes after every lost life (the original)
    TwoPlayers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootItem {
    Start(GameMode),
    Resume,
    Network,
    HighScores,
    Keyboard,
    Controller,
    Options,
    QuitToTitle,
    Quit,
}

impl RootItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RootItem::Start(GameMode::OnePlayer) => "1 PLAYER",
            RootItem::Start(GameMode::TwoPlayers) => "2 PLAYERS",
            RootItem::Resume => "RESUME",
            RootItem::Network => "NETWORK",
            RootItem::HighScores => "HIGH SCORES",
            RootItem::Keyboard => "KEYBOARD",
            RootItem::Controller => "CONTROLLER",
            RootItem::Options => "OPTIONS",
            RootItem::QuitToTitle => "QUIT TO TITLE",
            RootItem::Quit => "QUIT",
        }
    }

}

pub(crate) const NETWORK_ITEMS: [&str; 4] = ["HOST SAME TIME", "HOST TAKE TURNS", "JOIN GAME", "BACK"];
const MAIN_ITEMS: [RootItem; 8] = [
    RootItem::Start(GameMode::OnePlayer),
    RootItem::Start(GameMode::TwoPlayers),
    RootItem::Network,
    RootItem::HighScores,
    RootItem::Keyboard,
    RootItem::Controller,
    RootItem::Options,
    RootItem::Quit,
];
const PAUSE_ITEMS: [RootItem; 5] = [RootItem::Resume, RootItem::Keyboard, RootItem::Controller, RootItem::Options, RootItem::QuitToTitle];

/// What the app has to do after a menu step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    StartGame(GameMode),
    Resume,
    QuitToTitle,
    QuitApp,
    /// a binding changed: save it
    Changed,
    Host(NetMode),
    /// start looking for games
    Search,
    Join(String),
    /// close an open game or stop looking
    StopNetwork,
    /// give up connecting to a host
    CancelJoin,
    Option(OptionChange),
    /// a name was entered
    AddScore(Entry),
    /// install the new version that the main menu offers
    Update,
}

pub(crate) const RESET_ROW: usize = ACTIONS;
pub(crate) const BACK_ROW: usize = ACTIONS + 1;

#[derive(Clone, Debug)]
pub struct Menu {
    pub page: Page,
    /// opened from a running game: the root page is the pause menu
    pub paused: bool,
    pub cursor: usize,
    pub slot: usize,
    /// waiting for the key or button of the selected slot
    pub capturing: bool,
    /// a message for the player, e.g. why a network game ended
    pub notice: Option<String>,
    /// the address typed on the join page
    pub address: String,
    /// games found in the network
    pub hosts: Vec<String>,
    /// the address of this computer
    pub my_ip: String,
    /// scores that made the table and still need a name: who played, score, stage
    pub unnamed: Vec<(String, u32, u32)>,
    /// the name being entered
    pub name: String,
    /// the place to mark in the table
    pub latest: Option<usize>,
    /// where the cursor goes when the table is closed after a game
    pub return_cursor: Option<usize>,
    /// the settings as they are now (the app keeps them up to date)
    pub settings: Settings,
    /// the sound device works
    pub audio_ok: bool,
    /// the join page is waiting for a host to answer
    pub connecting: bool,
    pub search: Search,
    /// the game this computer hosts: its port, and whether other computers can find it by searching
    pub host_port: u16,
    pub host_findable: bool,
    /// the game that is paused is played with another computer (the question says that the other player is told)
    pub in_network: bool,
    /// the result page: who won, and the steps left before a key closes it
    pub result: Option<MatchResult>,
    pub result_hold: u32,
    /// the new version notice of the main menu (see `update`)
    pub update: crate::update::View,
}

impl Menu {
    pub fn main() -> Self {
        Self {
            page: Page::Root,
            paused: false,
            cursor: 0,
            slot: 0,
            capturing: false,
            notice: None,
            address: String::new(),
            hosts: Vec::new(),
            my_ip: String::new(),
            unnamed: Vec::new(),
            name: String::new(),
            latest: None,
            return_cursor: None,
            settings: Settings::default(),
            audio_ok: true,
            connecting: false,
            search: Search::Looking,
            host_port: crate::net::PORT,
            host_findable: true,
            in_network: false,
            result: None,
            result_hold: 0,
            update: crate::update::View::None,
        }
    }

    pub fn pause() -> Self {
        Self { paused: true, ..Self::main() }
    }

    pub(crate) fn root_items(&self) -> &'static [RootItem] {
        if self.paused {
            &PAUSE_ITEMS
        } else {
            &MAIN_ITEMS
        }
    }

    pub fn step(&mut self, nav: Nav, b: &mut Bindings) -> Outcome {
        if self.capturing {
            if nav == Nav::Back {
                self.capturing = false;
            }
            return Outcome::None;
        }
        self.notice = None;
        match self.page {
            Page::Root => self.step_root(nav),
            Page::Controls(device) => self.step_controls(nav, device, b),
            Page::Network => self.step_network(nav),
            Page::Hosting => self.step_hosting(nav),
            Page::Join => self.step_join(nav),
            Page::Scores => self.step_scores(nav),
            Page::EnterName => self.step_name(nav),
            Page::Options => self.step_options(nav),
            Page::Enhanced => self.step_enhanced(nav),
            Page::Sound => self.step_sound(nav),
            Page::ConfirmQuit => self.step_confirm_quit(nav),
            Page::Result => self.step_result(nav),
        }
    }

    /// The key or button for the update was pressed: only the main menu with an offer starts it.
    pub fn request_update(&self) -> Outcome {
        if self.page == Page::Root && !self.paused && matches!(self.update, crate::update::View::Available(_)) {
            Outcome::Update
        } else {
            Outcome::None
        }
    }

    /// The game of two players is over: show who won. Call it after `finish_game`; the names for the high score table come after it.
    pub fn show_result(&mut self, result: MatchResult) {
        self.result = Some(result);
        self.result_hold = RESULT_HOLD;
        self.page = Page::Result;
    }

    /// One logic step (50 per second): the result stays for a while, whatever keys are pressed.
    pub fn tick(&mut self) {
        if self.page == Page::Result {
            self.result_hold = self.result_hold.saturating_sub(1);
        }
    }

    fn step_result(&mut self, nav: Nav) -> Outcome {
        if matches!(nav, Nav::Confirm | Nav::Back) && self.result_hold == 0 {
            self.result = None;
            self.page = Page::Root;
            self.next_name();
            if self.page == Page::Root {
                self.cursor = self.return_cursor.take().unwrap_or(0);
            }
        }
        Outcome::None
    }

    /// `YES` is the first line, `NO` the second and the one the cursor starts on: a key pressed by mistake loses nothing.
    fn step_confirm_quit(&mut self, nav: Nav) -> Outcome {
        match nav {
            Nav::Up | Nav::Down | Nav::Left | Nav::Right => self.cursor ^= 1,
            Nav::Confirm if self.cursor == 0 => return Outcome::QuitToTitle,
            Nav::Confirm | Nav::Back => {
                self.page = Page::Root;
                self.cursor = self.root_items().iter().position(|&i| i == RootItem::QuitToTitle).unwrap_or(0);
            }
            _ => {}
        }
        Outcome::None
    }

    /// After a game the cursor stands on what was played: the mode, or `NETWORK` for `None`.
    pub fn select_mode(&mut self, mode: Option<GameMode>) {
        let item = mode.map_or(RootItem::Network, RootItem::Start);
        self.cursor = self.root_items().iter().position(|&i| i == item).unwrap_or(0);
        self.return_cursor = Some(self.cursor);
    }

    /// After a game: ask for the names of the `results` (who, score, stage) that make the table.
    pub fn finish_game(&mut self, results: Vec<(String, u32, u32)>, scores: &Scores) {
        let mut trial = scores.clone();
        for (who, score, stage) in results {
            if trial.insert(Entry { name: who.clone(), score, stage }).is_some() {
                self.unnamed.push((who, score, stage));
            }
        }
        self.next_name();
    }

    fn next_name(&mut self) {
        if !self.unnamed.is_empty() {
            self.page = Page::EnterName;
            self.name.clear();
        } else if self.latest.is_some() {
            self.page = Page::Scores;
        }
    }

    /// The entry was stored at `place`.
    pub fn score_added(&mut self, place: Option<usize>) {
        self.latest = place;
        self.next_name();
    }

    fn step_scores(&mut self, nav: Nav) -> Outcome {
        if matches!(nav, Nav::Back | Nav::Confirm) {
            self.leave_scores();
        }
        Outcome::None
    }

    fn leave_scores(&mut self) {
        self.latest = None;
        self.page = Page::Root;
        let item = self.root_items().iter().position(|&i| i == RootItem::HighScores).unwrap_or(0);
        self.cursor = self.return_cursor.take().unwrap_or(item);
    }

    /// Keyboards type the name, controllers pick the letters: up and down change the last one, right adds one, left removes one.
    fn step_name(&mut self, nav: Nav) -> Outcome {
        const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ";
        match nav {
            // The first step on an empty name gives "A", after that the last letter changes
            Nav::Up | Nav::Down if self.name.is_empty() => self.name.push('A'),
            Nav::Up | Nav::Down => {
                let last = self.name.pop().unwrap_or('A');
                let n = LETTERS.len();
                let i = LETTERS.find(last).unwrap_or(0);
                let i = if nav == Nav::Up { (i + n - 1) % n } else { (i + 1) % n };
                self.name.push(LETTERS.as_bytes()[i] as char);
            }
            Nav::Right if self.name.len() < NAME_LEN => self.name.push('A'),
            Nav::Left | Nav::Clear => {
                self.name.pop();
            }
            Nav::Confirm | Nav::Back => {
                let (who, score, stage) = self.unnamed.remove(0);
                let name = self.name.trim().to_string();
                let name = if name.is_empty() { who } else { name };
                return Outcome::AddScore(Entry { name, score, stage });
            }
            _ => {}
        }
        Outcome::None
    }

    /// A letter or digit typed for the name.
    pub fn type_name(&mut self, c: char) {
        if self.page == Page::EnterName && self.name.len() < NAME_LEN && (c.is_ascii_alphanumeric() || c == ' ') {
            self.name.push(c.to_ascii_uppercase());
        }
    }

    fn back_to_network(&mut self, cursor: usize) {
        self.page = Page::Network;
        self.cursor = cursor;
    }

    fn step_network(&mut self, nav: Nav) -> Outcome {
        let n = NETWORK_ITEMS.len();
        match nav {
            Nav::Up => self.cursor = (self.cursor + n - 1) % n,
            Nav::Down => self.cursor = (self.cursor + 1) % n,
            Nav::Back => self.leave_network(),
            Nav::Confirm => match self.cursor {
                0 => return Outcome::Host(NetMode::SameTime),
                1 => return Outcome::Host(NetMode::TakeTurns),
                2 => {
                    self.page = Page::Join;
                    self.cursor = 0;
                    self.hosts.clear();
                    self.search = Search::Looking;
                    self.connecting = false;
                    return Outcome::Search;
                }
                _ => self.leave_network(),
            },
            _ => {}
        }
        Outcome::None
    }

    fn leave_network(&mut self) {
        self.page = Page::Root;
        self.cursor = self.root_items().iter().position(|&i| i == RootItem::Network).unwrap_or(0);
    }

    fn step_hosting(&mut self, nav: Nav) -> Outcome {
        match nav {
            Nav::Back => {
                self.back_to_network(0);
                return Outcome::StopNetwork;
            }
            // the host decides who wins: the other computer is told when it joins
            Nav::Left | Nav::Right | Nav::Confirm => return Outcome::Option(OptionChange::Winner),
            _ => {}
        }
        Outcome::None
    }

    /// The games to pick from: the ones found, and the address joined last if it was not found now. (address, is the last one)
    pub(crate) fn join_entries(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = self.hosts.iter().map(|h| (h.clone(), false)).collect();
        let last = &self.settings.last_address;
        if !last.is_empty() && !self.hosts.iter().any(|h| h == last) {
            out.push((last.clone(), true));
        }
        out
    }

    fn step_join(&mut self, nav: Nav) -> Outcome {
        if self.connecting {
            if nav == Nav::Back {
                self.connecting = false;
                return Outcome::CancelJoin;
            }
            return Outcome::None;
        }
        let entries = self.join_entries();
        let n = entries.len();
        match nav {
            Nav::Up if n > 0 => self.cursor = (self.cursor + n - 1) % n,
            Nav::Down if n > 0 => self.cursor = (self.cursor + 1) % n,
            Nav::Back => {
                self.back_to_network(2);
                return Outcome::StopNetwork;
            }
            Nav::Confirm if !self.address.is_empty() => return Outcome::Join(self.address.clone()),
            Nav::Confirm => {
                if let Some((addr, _)) = entries.get(self.cursor) {
                    return Outcome::Join(addr.clone());
                }
            }
            _ => {}
        }
        Outcome::None
    }

    /// A digit or dot typed on the join page.
    pub fn type_char(&mut self, c: char) {
        if self.page == Page::Join && !self.connecting && self.address.len() < 21 && (c.is_ascii_digit() || c == '.' || c == ':') {
            self.address.push(c);
        }
    }

    /// Backspace on a page with text. False when it means something else: leaving the join page once nothing is left to delete.
    /// On the name page it only ever deletes, so that it cannot save a name by accident.
    pub fn delete_char(&mut self) -> bool {
        match self.page {
            Page::Join => !self.connecting && self.address.pop().is_some(),
            Page::EnterName => {
                self.name.pop();
                true
            }
            _ => false,
        }
    }

    fn step_root(&mut self, nav: Nav) -> Outcome {
        let n = self.root_items().len();
        match nav {
            Nav::Up => self.cursor = (self.cursor + n - 1) % n,
            Nav::Down => self.cursor = (self.cursor + 1) % n,
            Nav::Back if self.paused => return Outcome::Resume,
            Nav::Confirm => match self.root_items()[self.cursor] {
                RootItem::Start(mode) => return Outcome::StartGame(mode),
                RootItem::Resume => return Outcome::Resume,
                RootItem::Network => {
                    self.page = Page::Network;
                    self.cursor = 0;
                }
                RootItem::HighScores => self.page = Page::Scores,
                RootItem::Keyboard => self.open(Device::Keyboard),
                RootItem::Controller => self.open(Device::Pad),
                RootItem::Options => {
                    self.page = Page::Options;
                    self.cursor = 0;
                }
                RootItem::QuitToTitle => {
                    self.page = Page::ConfirmQuit;
                    self.cursor = 1;
                }
                RootItem::Quit => return Outcome::QuitApp,
            },
            _ => {}
        }
        Outcome::None
    }

    fn step_options(&mut self, nav: Nav) -> Outcome {
        let n = OPTION_ITEMS.len();
        let item = OPTION_ITEMS[self.cursor.min(n - 1)];
        match (nav, item) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, OptionItem::Back) => {
                self.page = Page::Root;
                self.cursor = self.root_items().iter().position(|&i| i == RootItem::Options).unwrap_or(0);
            }
            (Nav::Left | Nav::Right | Nav::Confirm, OptionItem::Fullscreen) => return Outcome::Option(OptionChange::Fullscreen),
            (Nav::Left, OptionItem::Scaling) => return Outcome::Option(OptionChange::Scaling(false)),
            (Nav::Right | Nav::Confirm, OptionItem::Scaling) => return Outcome::Option(OptionChange::Scaling(true)),
            (Nav::Left | Nav::Right | Nav::Confirm, OptionItem::Display50) => return Outcome::Option(OptionChange::Display50),
            (Nav::Left | Nav::Right | Nav::Confirm, OptionItem::UpdateCheck) => return Outcome::Option(OptionChange::UpdateCheck),
            (Nav::Left | Nav::Right | Nav::Confirm, OptionItem::Winner) => return Outcome::Option(OptionChange::Winner),
            (Nav::Right | Nav::Confirm, OptionItem::Sound) => {
                self.page = Page::Sound;
                self.cursor = 0;
            }
            // left and right switch the mode, confirm opens the page with the single enhancements
            (Nav::Left | Nav::Right, OptionItem::Mode) => return Outcome::Option(OptionChange::Mode),
            (Nav::Confirm, OptionItem::Mode) => {
                self.page = Page::Enhanced;
                self.cursor = 0;
            }
            _ => {}
        }
        Outcome::None
    }

    fn step_sound(&mut self, nav: Nav) -> Outcome {
        let n = SOUND_ITEMS.len();
        let item = SOUND_ITEMS[self.cursor.min(n - 1)];
        match (nav, item) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, SoundItem::Back) => {
                self.page = Page::Options;
                self.cursor = OPTION_ITEMS.iter().position(|&i| i == OptionItem::Sound).unwrap_or(0);
            }
            (Nav::Left, SoundItem::Volume) => return Outcome::Option(OptionChange::VolumeDown),
            (Nav::Right | Nav::Confirm, SoundItem::Volume) => return Outcome::Option(OptionChange::VolumeUp),
            (Nav::Left, SoundItem::Music) => return Outcome::Option(OptionChange::MusicDown),
            (Nav::Right | Nav::Confirm, SoundItem::Music) => return Outcome::Option(OptionChange::MusicUp),
            (Nav::Left, SoundItem::Effects) => return Outcome::Option(OptionChange::EffectsDown),
            (Nav::Right | Nav::Confirm, SoundItem::Effects) => return Outcome::Option(OptionChange::EffectsUp),
            (Nav::Left, SoundItem::MenuSounds) => return Outcome::Option(OptionChange::MenuSounds(false)),
            (Nav::Right | Nav::Confirm, SoundItem::MenuSounds) => return Outcome::Option(OptionChange::MenuSounds(true)),
            _ => {}
        }
        Outcome::None
    }

    fn step_enhanced(&mut self, nav: Nav) -> Outcome {
        let n = ENHANCED_ITEMS.len();
        let item = ENHANCED_ITEMS[self.cursor.min(n - 1)];
        match (nav, item) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, EnhancedItem::Back) => {
                self.page = Page::Options;
                self.cursor = OPTION_ITEMS.iter().position(|&i| i == OptionItem::Mode).unwrap_or(0);
            }
            (Nav::Left | Nav::Right | Nav::Confirm, EnhancedItem::Mode) => return Outcome::Option(OptionChange::Mode),
            (Nav::Left | Nav::Right | Nav::Confirm, EnhancedItem::SmoothMotion) => return Outcome::Option(OptionChange::SmoothMotion),
            _ => {}
        }
        Outcome::None
    }

    fn open(&mut self, device: Device) {
        self.page = Page::Controls(device);
        self.cursor = 0;
        self.slot = 0;
    }

    fn close_controls(&mut self, device: Device) {
        self.page = Page::Root;
        let item = if device == Device::Keyboard { RootItem::Keyboard } else { RootItem::Controller };
        self.cursor = self.root_items().iter().position(|&i| i == item).unwrap_or(0);
    }

    fn step_controls(&mut self, nav: Nav, device: Device, b: &mut Bindings) -> Outcome {
        let rows = BACK_ROW + 1;
        match nav {
            Nav::Up => self.cursor = (self.cursor + rows - 1) % rows,
            Nav::Down => self.cursor = (self.cursor + 1) % rows,
            Nav::Left if self.cursor < ACTIONS => self.slot = (self.slot + SLOTS - 1) % SLOTS,
            Nav::Right if self.cursor < ACTIONS => self.slot = (self.slot + 1) % SLOTS,
            Nav::Back => self.close_controls(device),
            Nav::Clear if self.cursor < ACTIONS => {
                match device {
                    Device::Keyboard => b.keys[self.cursor][self.slot] = None,
                    Device::Pad => b.pad[self.cursor][self.slot] = None,
                }
                return Outcome::Changed;
            }
            Nav::Confirm => match self.cursor {
                RESET_ROW => {
                    match device {
                        Device::Keyboard => b.reset_keys(),
                        Device::Pad => b.reset_pad(),
                    }
                    return Outcome::Changed;
                }
                BACK_ROW => self.close_controls(device),
                _ => self.capturing = true,
            },
            _ => {}
        }
        Outcome::None
    }

    /// The key pressed while waiting on the keyboard page. Escape never gets here: it cancels (`Nav::Back`).
    pub fn capture_key(&mut self, key: Keycode, b: &mut Bindings) -> Outcome {
        if !self.capturing || self.page != Page::Controls(Device::Keyboard) {
            return Outcome::None;
        }
        b.set_key(self.cursor, self.slot, key);
        self.capturing = false;
        Outcome::Changed
    }

    /// The button or stick direction pressed while waiting on the controller page.
    pub fn capture_pad(&mut self, input: PadInput, b: &mut Bindings) -> Outcome {
        if !self.capturing || self.page != Page::Controls(Device::Pad) {
            return Outcome::None;
        }
        b.set_pad(self.cursor, self.slot, input);
        self.capturing = false;
        Outcome::Changed
    }

}

/// Clip a text to `width` characters.
pub(crate) fn clip(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}

/// Only what the character set can show: anything else, e.g. in the name of a controller, becomes a question mark.
pub fn safe_text(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || " -_.:,'+/*[]=;!?()>".contains(c) { c } else { '?' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Winner;

    fn nav(m: &mut Menu, b: &mut Bindings, steps: &[Nav]) -> Outcome {
        let mut last = Outcome::None;
        for &n in steps {
            last = m.step(n, b);
        }
        last
    }

    fn sample_result() -> MatchResult {
        use crate::result::{decide, Standing};
        let p = |name: &str, score, stage| Standing { name: name.into(), score, stage };
        decide(Winner::Furthest, [p("GIANA", 999_999, 33), p("MARIA", 999_998, 33)])
    }

    #[test]
    fn the_result_stays_for_a_while_then_leads_to_the_names_and_the_table() {
        let mut b = Bindings::default();
        let scores = Scores::default();
        let mut m = Menu::main();
        m.select_mode(Some(GameMode::TwoPlayers));
        m.finish_game(vec![("GIANA".into(), 500, 3)], &scores);
        m.show_result(sample_result());
        assert_eq!(m.page, Page::Result);
        // keys do nothing until the time is over
        assert_eq!(nav(&mut m, &mut b, &[Nav::Confirm, Nav::Back]), Outcome::None);
        assert_eq!(m.page, Page::Result);
        for _ in 0..RESULT_HOLD {
            m.tick();
        }
        m.step(Nav::Confirm, &mut b);
        assert_eq!(m.page, Page::EnterName, "the high score name comes after the result");
        // no new score: back on the menu, the cursor on the game that was played
        let mut m = Menu::main();
        m.select_mode(Some(GameMode::TwoPlayers));
        m.finish_game(vec![("GIANA".into(), 0, 3)], &scores);
        m.show_result(sample_result());
        m.result_hold = 0;
        m.step(Nav::Confirm, &mut b);
        assert_eq!((m.page, m.cursor), (Page::Root, 1));
    }

    #[test]
    fn main_menu_starts_the_game_and_quits() {
        let mut b = Bindings::default();
        assert_eq!(Menu::main().step(Nav::Confirm, &mut b), Outcome::StartGame(GameMode::OnePlayer));
        assert_eq!(nav(&mut Menu::main(), &mut b, &[Nav::Down, Nav::Confirm]), Outcome::StartGame(GameMode::TwoPlayers));
        assert_eq!(nav(&mut Menu::main(), &mut b, &[Nav::Up, Nav::Confirm]), Outcome::QuitApp);
    }

    #[test]
    fn pause_menu_resumes_and_leaves() {
        let mut b = Bindings::default();
        assert_eq!(Menu::pause().step(Nav::Back, &mut b), Outcome::Resume);
        // leaving a game asks first, and the answer starts on NO
        let mut m = Menu::pause();
        assert_eq!(nav(&mut m, &mut b, &[Nav::Up, Nav::Confirm]), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::ConfirmQuit, 1));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None, "NO goes back");
        assert_eq!((m.page, m.cursor), (Page::Root, 4));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Confirm, Nav::Back]), Outcome::None, "back is NO too");
        assert_eq!(m.page, Page::Root);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Confirm, Nav::Up, Nav::Confirm]), Outcome::QuitToTitle);
    }

    #[test]
    fn rebinding_a_key_stores_it_in_the_selected_slot() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Down, Nav::Down, Nav::Down, Nav::Right, Nav::Confirm]);
        assert!(m.capturing);
        assert_eq!(m.capture_key(Keycode::J, &mut b), Outcome::Changed);
        assert_eq!(b.keys[3][1], Some(Keycode::J));
        assert!(!m.capturing);
    }

    #[test]
    fn back_cancels_a_capture_and_stays_on_the_page() {
        let mut b = Bindings::default();
        let before = b.clone();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Confirm, Nav::Back]);
        assert!(!m.capturing);
        assert_eq!(m.page, Page::Controls(Device::Keyboard));
        assert_eq!(b, before);
    }

    #[test]
    fn clear_and_reset_only_touch_their_device() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm]);
        assert_eq!(m.step(Nav::Clear, &mut b), Outcome::Changed);
        assert_eq!(b.keys[0][0], None);
        let pad = b.pad;
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up]);
        assert_eq!(m.cursor, RESET_ROW);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Changed);
        assert_eq!(b, Bindings::default());
        assert_eq!(b.pad, pad);
    }

    #[test]
    fn back_returns_to_the_item_that_opened_the_page() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up, Nav::Up, Nav::Confirm, Nav::Back]);
        assert_eq!((m.page, m.cursor), (Page::Root, 5));
    }

    #[test]
    fn a_new_score_asks_for_a_name_and_ends_on_the_table() {
        let mut b = Bindings::default();
        let mut scores = Scores::default();
        let mut m = Menu::main();
        m.finish_game(vec![("GIANA".into(), 500, 3), ("MARIA".into(), 0, 1)], &scores);
        assert_eq!(m.page, Page::EnterName);
        for c in "kev1n".chars() {
            m.type_name(c);
        }
        let Outcome::AddScore(e) = m.step(Nav::Confirm, &mut b) else { panic!() };
        assert_eq!(e, Entry { name: "KEV1N".into(), score: 500, stage: 3 });
        let place = scores.insert(e);
        m.score_added(place);
        assert_eq!((m.page, m.latest), (Page::Scores, Some(0)));
        m.step(Nav::Back, &mut b);
        assert_eq!(m.page, Page::Root);
    }

    #[test]
    fn two_new_scores_ask_twice_and_an_empty_name_uses_the_player() {
        let mut b = Bindings::default();
        let mut scores = Scores::default();
        let mut m = Menu::main();
        m.finish_game(vec![("GIANA".into(), 500, 3), ("MARIA".into(), 700, 4)], &scores);
        let Outcome::AddScore(first) = m.step(Nav::Confirm, &mut b) else { panic!() };
        assert_eq!(first.name, "GIANA");
        let place = scores.insert(first);
        m.score_added(place);
        assert_eq!(m.page, Page::EnterName);
        nav(&mut m, &mut b, &[Nav::Down, Nav::Right, Nav::Up]);
        assert_eq!(m.name, "A ", "the first step gives A");
        let Outcome::AddScore(second) = m.step(Nav::Confirm, &mut b) else { panic!() };
        assert_eq!(second.score, 700);
        assert_eq!(m.page, Page::EnterName);
    }

    #[test]
    fn backspace_deletes_letters_and_never_saves_the_name() {
        let mut m = Menu::main();
        m.finish_game(vec![("GIANA".into(), 500, 3)], &Scores::default());
        m.type_name('K');
        m.type_name('X');
        assert!(m.delete_char());
        assert_eq!(m.name, "K");
        assert!(m.delete_char());
        assert!(m.delete_char(), "on an empty name too");
        assert_eq!((m.page, m.name.as_str()), (Page::EnterName, ""));
    }

    #[test]
    fn after_the_table_the_cursor_is_on_the_mode_that_was_played() {
        let mut b = Bindings::default();
        let mut scores = Scores::default();
        for (mode, cursor) in [(Some(GameMode::OnePlayer), 0), (Some(GameMode::TwoPlayers), 1), (None, 2)] {
            let mut m = Menu::main();
            m.select_mode(mode);
            m.finish_game(vec![("GIANA".into(), 500, 3)], &scores);
            let Outcome::AddScore(e) = m.step(Nav::Confirm, &mut b) else { panic!() };
            let place = scores.insert(e);
            m.score_added(place);
            m.step(Nav::Back, &mut b);
            assert_eq!((m.page, m.cursor), (Page::Root, cursor));
        }
        // opened by hand it still comes back to its own entry
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Back]);
        assert_eq!(m.cursor, 3);
    }

    #[test]
    fn a_score_that_misses_the_table_stays_quiet() {
        let mut m = Menu::main();
        m.finish_game(vec![("GIANA".into(), 0, 1)], &Scores::default());
        assert_eq!(m.page, Page::Root);
    }

    #[test]
    fn network_pages_lead_to_their_outcomes() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Confirm]);
        assert_eq!(m.page, Page::Network);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Host(NetMode::SameTime));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Host(NetMode::TakeTurns));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Search);
        assert_eq!(m.page, Page::Join);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None, "nothing found or typed");
        m.hosts = vec!["192.168.1.7".into()];
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Join("192.168.1.7".into()));
        for c in "10.0.0.5x".chars() {
            m.type_char(c);
        }
        assert_eq!(m.address, "10.0.0.5");
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Join("10.0.0.5".into()));
        assert_eq!(m.step(Nav::Back, &mut b), Outcome::StopNetwork);
        assert_eq!(m.page, Page::Network);
    }

    #[test]
    fn options_change_nothing_themselves_and_ask_the_app() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up, Nav::Confirm]);
        assert_eq!(m.page, Page::Options);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Option(OptionChange::Fullscreen));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Option(OptionChange::Scaling(true)));
        assert_eq!(m.step(Nav::Left, &mut b), Outcome::Option(OptionChange::Scaling(false)));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Option(OptionChange::Display50));
        // SOUND opens its page; left does nothing on the line itself
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Left]), Outcome::None);
        assert_eq!(m.page, Page::Options);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Sound, 0));
        assert_eq!(m.step(Nav::Left, &mut b), Outcome::Option(OptionChange::VolumeDown));
        assert_eq!(m.step(Nav::Right, &mut b), Outcome::Option(OptionChange::VolumeUp));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Left]), Outcome::Option(OptionChange::MusicDown));
        assert_eq!(m.step(Nav::Right, &mut b), Outcome::Option(OptionChange::MusicUp));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Left]), Outcome::Option(OptionChange::EffectsDown));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Option(OptionChange::EffectsUp));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Option(OptionChange::MenuSounds(true)));
        assert_eq!(m.step(Nav::Left, &mut b), Outcome::Option(OptionChange::MenuSounds(false)));
        nav(&mut m, &mut b, &[Nav::Back]);
        assert_eq!(m.page, Page::Options, "BACK returns to the options page, on the SOUND line");
        assert_eq!(OPTION_ITEMS[m.cursor], OptionItem::Sound);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Right]), Outcome::Option(OptionChange::Winner));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Right]), Outcome::Option(OptionChange::Mode));
        nav(&mut m, &mut b, &[Nav::Back]);
        assert_eq!((m.page, m.cursor), (Page::Root, 6));
    }

    #[test]
    fn the_mode_item_opens_the_page_with_the_enhancements() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        m.page = Page::Options;
        m.cursor = OPTION_ITEMS.iter().position(|&i| i == OptionItem::Mode).unwrap();
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Enhanced, 0));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Option(OptionChange::Mode));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Right]), Outcome::Option(OptionChange::SmoothMotion));
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!(m.page, Page::Options, "BACK returns to the options page, on the MODE line");
        assert_eq!(OPTION_ITEMS[m.cursor], OptionItem::Mode);
    }

    #[test]
    fn the_first_step_on_an_empty_name_gives_a() {
        let mut b = Bindings::default();
        for first in [Nav::Up, Nav::Down] {
            let mut m = Menu::main();
            m.finish_game(vec![("GIANA".into(), 500, 3)], &Scores::default());
            m.step(first, &mut b);
            assert_eq!(m.name, "A");
        }
    }

    #[test]
    fn cancelling_a_connection_and_the_last_address() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        m.page = Page::Join;
        m.settings.last_address = "10.0.0.9".into();
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Join("10.0.0.9".into()), "the last address is in the list");
        m.connecting = true;
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!(m.step(Nav::Back, &mut b), Outcome::CancelJoin);
        assert!(!m.connecting && m.page == Page::Join);
    }

    #[test]
    fn texts_the_font_cannot_show_become_question_marks() {
        assert_eq!(safe_text("Pad \u{e9}\u{4e2d}-1"), "Pad ??-1");
    }
}
