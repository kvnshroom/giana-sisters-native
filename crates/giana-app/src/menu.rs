//! The main menu, the pause menu, the options pages and the controls pages. Not part of the original game.
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
    /// the modules page: add the module to the favorites or take it out
    Favorite,
    /// the modules page: the next way to order the list
    Sort,
}

/// Fixed menu keys, whatever the bindings say: the first one of each is the one the hints show.
const MENU_KEYS: [(Nav, &[Keycode]); 9] = [
    (Nav::Up, &[Keycode::Up, Keycode::W]),
    (Nav::Down, &[Keycode::Down, Keycode::S]),
    (Nav::Left, &[Keycode::Left, Keycode::A]),
    (Nav::Right, &[Keycode::Right, Keycode::D]),
    (Nav::Confirm, &[Keycode::Return, Keycode::KpEnter, Keycode::Space]),
    (Nav::Back, &[Keycode::Escape, Keycode::Backspace]),
    (Nav::Clear, &[Keycode::Delete]),
    (Nav::Favorite, &[Keycode::F]),
    (Nav::Sort, &[Keycode::O]),
];

/// Fixed menu buttons.
const MENU_BUTTONS: [(Nav, &[Button]); 8] = [
    (Nav::Up, &[Button::DPadUp]),
    (Nav::Down, &[Button::DPadDown]),
    (Nav::Left, &[Button::DPadLeft]),
    (Nav::Right, &[Button::DPadRight]),
    (Nav::Confirm, &[Button::A]),
    (Nav::Back, &[Button::B, Button::Start]),
    // Y is `Clear` everywhere, and `Favorite` on the modules page (see `step_modules`)
    (Nav::Clear, &[Button::Y]),
    (Nav::Sort, &[Button::X]),
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
    /// `CONTROLS`: picks the device whose keys or buttons are rebound
    ControlsPick,
    Controls(Device),
    /// host or join a game with another computer
    Network,
    /// a game is open, waiting for the other player
    Hosting,
    Join,
    Scores,
    /// a score made the table: the name for it
    EnterName,
    /// `OPTIONS`: the hub with the pages VIDEO, SOUND and GENERAL
    Options,
    /// picture and screen settings, reached from the options page
    Video,
    /// volumes and menu sounds, reached from the options page
    Sound,
    /// winner rule, style and update check, reached from the options page
    General,
    /// the end of a game of two players: who won
    Result,
    /// asked in the pause menu before a game is given up: `QUIT GAME? YES / NO`
    ConfirmQuit,
    /// `ROM MODULES`: the original game and the installed modules
    Modules,
    /// one entry of the modules page: its texts, and what can be done with it
    ModuleDetail,
    /// asked before a module is deleted: `REMOVE THIS MODULE? YES / NO`
    ConfirmRemove,
}

/// What the menu knows about a ROM module (the app fills the list from the modules folder). The texts are ready to be
/// drawn: capitals, only characters the font has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    /// the module changes code that the port runs itself (so it may not play as intended)
    pub changes_code: bool,
}

/// Said on the detail page of a module that changes game code, on two lines.
pub const CODE_NOTICE: [&str; 2] = ["CHANGES GAME CODE -", "MAY NOT PLAY AS INTENDED"];

fn shown(s: &str) -> String {
    safe_text(&s.to_ascii_uppercase())
}

impl ModuleInfo {
    pub fn new(id: &str, name: &str, author: &str, version: &str, description: &str, changes_code: bool) -> ModuleInfo {
        ModuleInfo { id: id.to_string(), name: shown(name), author: shown(author), version: shown(version), description: shown(description), changes_code }
    }

    /// `NAME  V1.0`, cut to `width` characters (the name gives way, the version stays).
    pub fn list_label(&self, width: usize) -> String {
        if self.version.is_empty() {
            return clip(&self.name, width);
        }
        let tail = format!("  V{}", self.version);
        let room = width.saturating_sub(tail.chars().count());
        if room < 4 {
            return clip(&self.name, width);
        }
        format!("{}{tail}", clip(&self.name, room))
    }
}

/// Characters of the list label of a module (name and version, see `ModuleInfo::list_label`) on the modules page when
/// the row has nothing else on it. The right end of a row holds the favorite star in the last column and, before it,
/// the stage reached (`label_room` takes that room from the label).
pub const LIST_LABEL_MAX: usize = 30;

/// Characters left for the list label of a module that has reached `stage`: a row with a stage on it is cut shorter.
pub fn label_room(stage: Option<u32>) -> usize {
    match progress_text(stage).chars().count() {
        0 => LIST_LABEL_MAX,
        n => LIST_LABEL_MAX - n - 2,
    }
}

/// The stage that was reached, for a row of the modules page (dim, right aligned): `STAGE 12`; empty when never played.
pub fn progress_text(stage: Option<u32>) -> String {
    stage.map(|s| format!("STAGE {}", s.min(99))).unwrap_or_default()
}

/// The lines of the detail page of a module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetailItem {
    Play,
    /// `FAVORITE: ON / OFF` (modules only, not the original game)
    Favorite,
    Remove,
    Back,
}

impl DetailItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            DetailItem::Play => "PLAY THIS",
            DetailItem::Favorite => "FAVORITE",
            DetailItem::Remove => "REMOVE",
            DetailItem::Back => "BACK",
        }
    }
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

/// Whether `PRESENTATION: AUTHENTIC` can be used (the app sets it, see `authentic_run`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthenticAvail {
    Ready,
    /// the data folder was made before AUTHENTIC existed: the disk image has to be read once more
    NeedsData,
    /// a ROM module is played: AUTHENTIC runs the original game only
    ModuleActive,
}

/// A setting changed on the options page; the app applies and saves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionChange {
    Fullscreen,
    VolumeDown,
    VolumeUp,
    /// the next (true) or the previous way to fill the window
    Scaling(bool),
    /// `SCREEN FILTER: SHARP / CRT TV`
    Crt,
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
    /// `SMOOTH MOTION: ON / OFF` (see `Enhancements::toggle_smooth_motion`)
    SmoothMotion,
    /// `STYLE: MODERN / AUTHENTIC`
    Presentation,
    /// `UPDATE CHECK: DAILY / OFF`
    UpdateCheck,
}

/// The lines of the options page, the hub: each opens a page with settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptionItem {
    Video,
    Sound,
    General,
    Back,
}

pub(crate) const OPTION_ITEMS: [OptionItem; 4] = [OptionItem::Video, OptionItem::Sound, OptionItem::General, OptionItem::Back];

impl OptionItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            OptionItem::Video => "VIDEO",
            OptionItem::Sound => "SOUND",
            OptionItem::General => "GENERAL",
            OptionItem::Back => "BACK",
        }
    }
}

/// The lines of the video page, reached from the options page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VideoItem {
    Fullscreen,
    Scaling,
    Crt,
    Display50,
    SmoothMotion,
    Back,
}

pub(crate) const VIDEO_ITEMS: [VideoItem; 6] = [VideoItem::Fullscreen, VideoItem::Scaling, VideoItem::Crt, VideoItem::Display50, VideoItem::SmoothMotion, VideoItem::Back];

impl VideoItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            VideoItem::Fullscreen => "FULLSCREEN",
            VideoItem::Scaling => "PICTURE SIZE",
            VideoItem::Crt => "SCREEN FILTER",
            VideoItem::Display50 => "50 HZ MODE",
            VideoItem::SmoothMotion => "SMOOTH MOTION",
            VideoItem::Back => "BACK",
        }
    }
}

/// The lines of the general page, reached from the options page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneralItem {
    Winner,
    Presentation,
    UpdateCheck,
    Back,
}

pub(crate) const GENERAL_ITEMS: [GeneralItem; 4] = [GeneralItem::Winner, GeneralItem::Presentation, GeneralItem::UpdateCheck, GeneralItem::Back];

impl GeneralItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            GeneralItem::Winner => "WINNER RULE",
            GeneralItem::Presentation => "STYLE",
            GeneralItem::UpdateCheck => "UPDATE CHECK",
            GeneralItem::Back => "BACK",
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
    /// the main menu, when a suspended game of 1 or 2 players waits (see `suspend`)
    Continue(u8),
    Start(GameMode),
    Resume,
    Network,
    HighScores,
    Modules,
    Controls,
    Options,
    QuitToTitle,
    Quit,
}

impl RootItem {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RootItem::Continue(2) => "CONTINUE 2 PLAYERS",
            RootItem::Continue(_) => "CONTINUE 1 PLAYER",
            RootItem::Start(GameMode::OnePlayer) => "1 PLAYER",
            RootItem::Start(GameMode::TwoPlayers) => "2 PLAYERS",
            RootItem::Resume => "RESUME",
            RootItem::Network => "NETWORK",
            RootItem::HighScores => "HIGH SCORES",
            RootItem::Modules => "ROM MODULES",
            RootItem::Controls => "CONTROLS",
            RootItem::Options => "OPTIONS",
            RootItem::QuitToTitle => "QUIT TO TITLE",
            RootItem::Quit => "QUIT",
        }
    }

}

pub(crate) const NETWORK_ITEMS: [&str; 4] = ["HOST SAME TIME", "HOST TAKE TURNS", "JOIN GAME", "BACK"];
/// The device pages behind `CONTROLS`.
pub(crate) const CONTROLS_ITEMS: [&str; 3] = ["KEYBOARD", "CONTROLLER", "BACK"];
const MAIN_ITEMS: [RootItem; 8] = [
    RootItem::Start(GameMode::OnePlayer),
    RootItem::Start(GameMode::TwoPlayers),
    RootItem::Network,
    RootItem::Modules,
    RootItem::HighScores,
    RootItem::Controls,
    RootItem::Options,
    RootItem::Quit,
];
const PAUSE_ITEMS: [RootItem; 4] = [RootItem::Resume, RootItem::Controls, RootItem::Options, RootItem::QuitToTitle];

/// What the app has to do after a menu step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    StartGame(GameMode),
    /// `CONTINUE 1 PLAYER` / `CONTINUE 2 PLAYERS`: take the suspended game of that many players
    Continue(u8),
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
    /// `ADD ROM MODULE...`: the app asks for a file
    ModuleAdd,
    /// the options page asks for the disk image again: AUTHENTIC needs a file that older data folders lack
    AuthenticData,
    /// play this module (its id, empty for the original game): the app saves the choice and switches the game in place
    ModulePlay(String),
    /// delete this module
    ModuleRemove(String),
    /// add the module to the favorites or take it out (its id)
    ModuleFavorite(String),
    /// the next way to order the modules
    ModuleSort,
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
    /// the installed ROM modules (the app keeps the list up to date)
    pub modules: Vec<ModuleInfo>,
    /// id of the module that is played now (empty: the original game)
    pub active_module: String,
    /// why the last module could not be added, in plain words
    pub module_error: Option<String>,
    /// the entry of the detail page: an index into `modules`, `None` for the original game
    pub module_sel: Option<usize>,
    /// can `PRESENTATION: AUTHENTIC` be used? (the app keeps it up to date)
    pub authentic: AuthenticAvail,
    /// the games of this module that wait to be continued, one player first (the app keeps it up to date, see `set_suspend`)
    pub suspended: Vec<crate::suspend::Info>,
    /// the game that is paused is saved when the program is closed (a local game: the pause menu says so)
    pub can_suspend: bool,
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
            modules: Vec::new(),
            active_module: String::new(),
            module_error: None,
            module_sel: None,
            authentic: AuthenticAvail::Ready,
            suspended: Vec::new(),
            can_suspend: false,
        }
    }

    pub fn pause() -> Self {
        Self { paused: true, ..Self::main() }
    }

    /// The lines of the root page. The main menu starts with a CONTINUE line for every suspended game.
    pub(crate) fn root_items(&self) -> Vec<RootItem> {
        if self.paused {
            PAUSE_ITEMS.to_vec()
        } else {
            self.suspended.iter().map(|i| RootItem::Continue(i.players)).chain(MAIN_ITEMS).collect()
        }
    }

    /// The suspended game of `players` players, if there is one.
    pub fn suspended_game(&self, players: u8) -> Option<&crate::suspend::Info> {
        self.suspended.iter().find(|i| i.players == players)
    }

    /// Tells the main menu about the suspended games (none, one or both). CONTINUE lines come and go, so the cursor follows
    /// the line it was on.
    pub fn set_suspend(&mut self, suspended: Vec<crate::suspend::Info>) {
        let same_lines = self.suspended.iter().map(|i| i.players).eq(suspended.iter().map(|i| i.players));
        if same_lines {
            self.suspended = suspended;
            return;
        }
        let on_root = self.page == Page::Root && !self.paused;
        let was = on_root.then(|| self.root_items().get(self.cursor).copied()).flatten();
        let had_none = self.suspended.is_empty();
        self.suspended = suspended;
        if let Some(item) = was {
            // A menu that has not been touched yet (the cursor on its first line) offers CONTINUE first; otherwise the cursor
            // stays on the line it was on. When its line goes, it goes to the first line.
            let untouched = had_none && self.cursor == 0 && !self.suspended.is_empty();
            self.cursor = if untouched { 0 } else { self.root_items().iter().position(|&i| i == item).unwrap_or(0) };
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
        self.module_error = None;
        match self.page {
            Page::Root => self.step_root(nav),
            Page::ControlsPick => self.step_controls_pick(nav),
            Page::Controls(device) => self.step_controls(nav, device, b),
            Page::Network => self.step_network(nav),
            Page::Hosting => self.step_hosting(nav),
            Page::Join => self.step_join(nav),
            Page::Scores => self.step_scores(nav),
            Page::EnterName => self.step_name(nav),
            Page::Options => self.step_options(nav),
            Page::Video => self.step_video(nav),
            Page::General => self.step_general(nav),
            Page::Sound => self.step_sound(nav),
            Page::ConfirmQuit => self.step_confirm_quit(nav),
            Page::Modules => self.step_modules(nav),
            Page::ModuleDetail => self.step_module_detail(nav),
            Page::ConfirmRemove => self.step_confirm_remove(nav),
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

    /// Rows of the modules page: `ORIGINAL`, the modules, `ADD ROM MODULE...`, `BACK`.
    pub(crate) fn module_rows(&self) -> usize {
        self.modules.len() + 3
    }

    fn leave_modules(&mut self) {
        self.page = Page::Root;
        self.module_error = None;
        self.cursor = self.root_items().iter().position(|&i| i == RootItem::Modules).unwrap_or(0);
    }

    fn step_modules(&mut self, nav: Nav) -> Outcome {
        let n = self.module_rows();
        let m = self.modules.len();
        match nav {
            Nav::Up => self.cursor = (self.cursor + n - 1) % n,
            Nav::Down => self.cursor = (self.cursor + 1) % n,
            Nav::Back => self.leave_modules(),
            Nav::Sort => return Outcome::ModuleSort,
            // Y on the controller is `Clear`: on this page it marks a favorite
            Nav::Favorite | Nav::Clear if (1..=m).contains(&self.cursor) => return Outcome::ModuleFavorite(self.modules[self.cursor - 1].id.clone()),
            Nav::Confirm if self.cursor == m + 1 => return Outcome::ModuleAdd,
            Nav::Confirm if self.cursor == m + 2 => self.leave_modules(),
            Nav::Confirm => {
                self.module_sel = self.cursor.checked_sub(1);
                self.page = Page::ModuleDetail;
                self.cursor = 0;
            }
            _ => {}
        }
        Outcome::None
    }

    /// The module of the detail page, `None` for the original game.
    pub fn selected_module(&self) -> Option<&ModuleInfo> {
        self.module_sel.and_then(|i| self.modules.get(i))
    }

    /// The module of the detail page is the one that is played now.
    pub fn selected_is_active(&self) -> bool {
        self.selected_module().map_or("", |m| m.id.as_str()) == self.active_module
    }

    /// The lines of the detail page: the original game and the module that is played cannot be removed.
    pub(crate) fn detail_items(&self) -> Vec<DetailItem> {
        let removable = self.selected_module().is_some() && !self.selected_is_active();
        let mut v = vec![DetailItem::Play];
        if self.selected_module().is_some() {
            v.push(DetailItem::Favorite);
        }
        if removable {
            v.push(DetailItem::Remove);
        }
        v.push(DetailItem::Back);
        v
    }

    fn back_to_module_list(&mut self) {
        self.page = Page::Modules;
        self.cursor = self.module_sel.map_or(0, |i| i + 1).min(self.modules.len());
    }

    fn step_module_detail(&mut self, nav: Nav) -> Outcome {
        let items = self.detail_items();
        let n = items.len();
        match (nav, items[self.cursor.min(n - 1)]) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, DetailItem::Back) => self.back_to_module_list(),
            (Nav::Confirm, DetailItem::Play) => {
                if self.selected_is_active() {
                    // it is the one that is played: nothing to switch
                    self.back_to_module_list();
                } else {
                    return Outcome::ModulePlay(self.selected_module().map_or(String::new(), |m| m.id.clone()));
                }
            }
            (Nav::Confirm, DetailItem::Favorite) | (Nav::Favorite | Nav::Clear, _) if self.selected_module().is_some() => {
                return Outcome::ModuleFavorite(self.selected_module().map_or(String::new(), |m| m.id.clone()));
            }
            (Nav::Confirm, DetailItem::Remove) => {
                self.page = Page::ConfirmRemove;
                self.cursor = 1;
            }
            _ => {}
        }
        Outcome::None
    }

    /// `YES` is the first line, `NO` the second and the one the cursor starts on.
    fn step_confirm_remove(&mut self, nav: Nav) -> Outcome {
        match nav {
            Nav::Up | Nav::Down | Nav::Left | Nav::Right => self.cursor ^= 1,
            Nav::Confirm if self.cursor == 0 => {
                if let Some(id) = self.selected_module().map(|m| m.id.clone()).filter(|id| *id != self.active_module) {
                    self.module_sel = None;
                    self.page = Page::Modules;
                    self.cursor = 0;
                    return Outcome::ModuleRemove(id);
                }
                self.back_to_module_list();
            }
            Nav::Confirm | Nav::Back => {
                self.page = Page::ModuleDetail;
                self.cursor = self.detail_items().iter().position(|&i| i == DetailItem::Remove).unwrap_or(0);
            }
            _ => {}
        }
        Outcome::None
    }

    /// The app tells which modules are installed and which one is played (called every frame: nothing changes when
    /// nothing differs). The cursor stays on the page.
    pub fn set_modules(&mut self, modules: &[ModuleInfo], active: &str) {
        if self.active_module != active {
            self.active_module = active.to_string();
        }
        if self.modules == modules {
            return;
        }
        // a new order (favorites, sorting) keeps the cursor and the open page on the same module
        let on_list = |this: &Self| (this.page == Page::Modules && this.cursor >= 1).then(|| this.modules.get(this.cursor - 1).map(|m| m.id.clone())).flatten();
        let list_id = on_list(self);
        let sel_id = self.selected_module().map(|m| m.id.clone());
        self.modules = modules.to_vec();
        let find = |this: &Self, id: &Option<String>| id.as_ref().and_then(|id| this.modules.iter().position(|m| m.id == *id));
        self.module_sel = match (&sel_id, find(self, &sel_id)) {
            (Some(_), found) => found,
            (None, _) => None,
        };
        if self.page == Page::Modules {
            if let Some(i) = find(self, &list_id) {
                self.cursor = i + 1;
            }
            self.cursor = self.cursor.min(self.module_rows() - 1);
        }
    }

    /// A module was added: the cursor stands on it. Or it could not be added, and the error says why.
    pub fn module_added(&mut self, id: Option<&str>, error: Option<String>) {
        self.module_error = error;
        if let Some(i) = id.and_then(|id| self.modules.iter().position(|m| m.id == id)) {
            self.page = Page::Modules;
            self.cursor = i + 1;
        }
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
                RootItem::Continue(n) => return Outcome::Continue(n),
                RootItem::Start(mode) => return Outcome::StartGame(mode),
                RootItem::Resume => return Outcome::Resume,
                RootItem::Network => {
                    self.page = Page::Network;
                    self.cursor = 0;
                }
                RootItem::HighScores => self.page = Page::Scores,
                RootItem::Modules => {
                    self.page = Page::Modules;
                    // the module that is played starts selected
                    self.cursor = self.modules.iter().position(|m| m.id == self.active_module).map_or(0, |i| i + 1);
                }
                RootItem::Controls => {
                    self.page = Page::ControlsPick;
                    self.cursor = 0;
                }
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
            (Nav::Right | Nav::Confirm, OptionItem::Video | OptionItem::Sound | OptionItem::General) => {
                self.page = match item {
                    OptionItem::Video => Page::Video,
                    OptionItem::Sound => Page::Sound,
                    _ => Page::General,
                };
                self.cursor = 0;
            }
            _ => {}
        }
        Outcome::None
    }

    /// BACK from a page of the options hub: the hub, on the line that opened the page.
    fn close_options_page(&mut self, item: OptionItem) {
        self.page = Page::Options;
        self.cursor = OPTION_ITEMS.iter().position(|&i| i == item).unwrap_or(0);
    }

    fn step_video(&mut self, nav: Nav) -> Outcome {
        let n = VIDEO_ITEMS.len();
        let item = VIDEO_ITEMS[self.cursor.min(n - 1)];
        match (nav, item) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, VideoItem::Back) => self.close_options_page(OptionItem::Video),
            (Nav::Left | Nav::Right | Nav::Confirm, VideoItem::Fullscreen) => return Outcome::Option(OptionChange::Fullscreen),
            (Nav::Left, VideoItem::Scaling) => return Outcome::Option(OptionChange::Scaling(false)),
            (Nav::Right | Nav::Confirm, VideoItem::Scaling) => return Outcome::Option(OptionChange::Scaling(true)),
            (Nav::Left | Nav::Right | Nav::Confirm, VideoItem::Crt) => return Outcome::Option(OptionChange::Crt),
            (Nav::Left | Nav::Right | Nav::Confirm, VideoItem::Display50) => return Outcome::Option(OptionChange::Display50),
            (Nav::Left | Nav::Right | Nav::Confirm, VideoItem::SmoothMotion) => return Outcome::Option(OptionChange::SmoothMotion),
            _ => {}
        }
        Outcome::None
    }

    fn step_general(&mut self, nav: Nav) -> Outcome {
        let n = GENERAL_ITEMS.len();
        let item = GENERAL_ITEMS[self.cursor.min(n - 1)];
        match (nav, item) {
            (Nav::Up, _) => self.cursor = (self.cursor + n - 1) % n,
            (Nav::Down, _) => self.cursor = (self.cursor + 1) % n,
            (Nav::Back, _) | (Nav::Confirm, GeneralItem::Back) => self.close_options_page(OptionItem::General),
            (Nav::Left | Nav::Right | Nav::Confirm, GeneralItem::UpdateCheck) => return Outcome::Option(OptionChange::UpdateCheck),
            (Nav::Left | Nav::Right | Nav::Confirm, GeneralItem::Winner) => return Outcome::Option(OptionChange::Winner),
            // the way out of AUTHENTIC is always open; the way in needs the data
            (Nav::Left | Nav::Right | Nav::Confirm, GeneralItem::Presentation) => match self.authentic {
                _ if self.settings.presentation == crate::settings::Presentation::Authentic => return Outcome::Option(OptionChange::Presentation),
                AuthenticAvail::Ready => return Outcome::Option(OptionChange::Presentation),
                AuthenticAvail::NeedsData if nav == Nav::Confirm => return Outcome::AuthenticData,
                _ => {}
            },
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
            (Nav::Back, _) | (Nav::Confirm, SoundItem::Back) => self.close_options_page(OptionItem::Sound),
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

    fn open(&mut self, device: Device) {
        self.page = Page::Controls(device);
        self.cursor = 0;
        self.slot = 0;
    }

    fn close_controls(&mut self, device: Device) {
        self.page = Page::ControlsPick;
        self.cursor = if device == Device::Keyboard { 0 } else { 1 };
    }

    fn step_controls_pick(&mut self, nav: Nav) -> Outcome {
        let n = CONTROLS_ITEMS.len();
        match nav {
            Nav::Up => self.cursor = (self.cursor + n - 1) % n,
            Nav::Down => self.cursor = (self.cursor + 1) % n,
            Nav::Back => self.leave_controls_pick(),
            Nav::Confirm => match self.cursor {
                0 => self.open(Device::Keyboard),
                1 => self.open(Device::Pad),
                _ => self.leave_controls_pick(),
            },
            _ => {}
        }
        Outcome::None
    }

    fn leave_controls_pick(&mut self) {
        self.page = Page::Root;
        self.cursor = self.root_items().iter().position(|&i| i == RootItem::Controls).unwrap_or(0);
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
        assert_eq!((m.page, m.cursor), (Page::Root, 3));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Confirm, Nav::Back]), Outcome::None, "back is NO too");
        assert_eq!(m.page, Page::Root);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Confirm, Nav::Up, Nav::Confirm]), Outcome::QuitToTitle);
    }

    fn suspended(players: u8) -> crate::suspend::Info {
        crate::suspend::Info { players, stage: 7, score: 12350, player: 0, other_score: 0 }
    }

    fn labels(m: &Menu) -> Vec<&'static str> {
        m.root_items().iter().map(|i| i.label()).collect()
    }

    #[test]
    fn a_continue_line_for_every_suspended_game_comes_first() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        assert_eq!(m.root_items().len(), 8);
        assert!(!labels(&m).iter().any(|l| l.starts_with("CONTINUE")));
        // one suspended game: one line, and a menu that was not touched yet offers it
        m.set_suspend(vec![suspended(2)]);
        assert_eq!(labels(&m)[..2], ["CONTINUE 2 PLAYERS", "1 PLAYER"]);
        assert_eq!((m.root_items().len(), m.cursor), (9, 0));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Continue(2));
        // both: one player first, each line its own outcome
        let mut m = Menu::main();
        m.set_suspend(vec![suspended(1), suspended(2)]);
        assert_eq!(labels(&m)[..3], ["CONTINUE 1 PLAYER", "CONTINUE 2 PLAYERS", "1 PLAYER"]);
        assert_eq!((m.root_items().len(), m.cursor), (10, 0));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Continue(1));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Continue(2));
        // 1 PLAYER and 2 PLAYERS follow
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::StartGame(GameMode::OnePlayer));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::StartGame(GameMode::TwoPlayers));
        assert_eq!(m.suspended_game(2).map(|i| i.players), Some(2));
        assert!(Menu::main().suspended_game(1).is_none());
        // a pause menu never has them
        let mut p = Menu::pause();
        p.set_suspend(vec![suspended(1), suspended(2)]);
        assert_eq!(p.root_items().len(), 4);
    }

    #[test]
    fn the_cursor_stays_on_its_line_when_continue_lines_come_and_go() {
        let mut m = Menu::main();
        m.select_mode(Some(GameMode::TwoPlayers));
        assert_eq!(m.root_items()[m.cursor].label(), "2 PLAYERS");
        m.set_suspend(vec![suspended(1)]);
        assert_eq!(m.root_items()[m.cursor].label(), "2 PLAYERS");
        m.set_suspend(vec![suspended(1), suspended(2)]);
        assert_eq!(m.root_items()[m.cursor].label(), "2 PLAYERS");
        // the same lines again (other values) change nothing
        m.set_suspend(vec![suspended(1), crate::suspend::Info { score: 5, ..suspended(2) }]);
        assert_eq!(m.root_items()[m.cursor].label(), "2 PLAYERS");
        m.set_suspend(Vec::new());
        assert_eq!(m.root_items()[m.cursor].label(), "2 PLAYERS");
        // the line that went away: the first one
        m.set_suspend(vec![suspended(1), suspended(2)]);
        m.cursor = 1;
        m.set_suspend(vec![suspended(1)]);
        assert_eq!(m.root_items()[m.cursor].label(), "CONTINUE 1 PLAYER");
        m.cursor = 0;
        m.set_suspend(Vec::new());
        assert_eq!(m.root_items()[m.cursor].label(), "1 PLAYER");
        // a cursor that is on the first line of a fresh menu goes to the first CONTINUE line
        let mut m = Menu::main();
        m.set_suspend(vec![suspended(1), suspended(2)]);
        assert_eq!(m.cursor, 0);
    }

    #[test]
    fn rebinding_a_key_stores_it_in_the_selected_slot() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Confirm, Nav::Down, Nav::Down, Nav::Down, Nav::Right, Nav::Confirm]);
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
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Confirm, Nav::Confirm, Nav::Back]);
        assert!(!m.capturing);
        assert_eq!(m.page, Page::Controls(Device::Keyboard));
        assert_eq!(b, before);
    }

    #[test]
    fn clear_and_reset_only_touch_their_device() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Confirm]);
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
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up, Nav::Up, Nav::Confirm]);
        assert_eq!(m.page, Page::ControlsPick);
        // CONTROLS picks the device, and BACK from a device page returns to that line
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!(m.page, Page::Controls(Device::Pad));
        nav(&mut m, &mut b, &[Nav::Back]);
        assert_eq!((m.page, m.cursor), (Page::ControlsPick, 1));
        nav(&mut m, &mut b, &[Nav::Back]);
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
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Down, Nav::Confirm, Nav::Back]);
        assert_eq!(m.cursor, 4);
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
        // OPTIONS is the third entry from the bottom (before QUIT)
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up, Nav::Confirm]);
        assert_eq!(m.page, Page::Options, "the hub");
        assert_eq!(m.step(Nav::Left, &mut b), Outcome::None, "nothing is changed on the hub");
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Video, 0));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Option(OptionChange::Fullscreen));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Option(OptionChange::Scaling(true)));
        assert_eq!(m.step(Nav::Left, &mut b), Outcome::Option(OptionChange::Scaling(false)));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Right]), Outcome::Option(OptionChange::Crt));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::Option(OptionChange::Display50));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Left]), Outcome::Option(OptionChange::SmoothMotion));
        nav(&mut m, &mut b, &[Nav::Back]);
        assert_eq!((m.page, OPTION_ITEMS[m.cursor]), (Page::Options, OptionItem::Video), "BACK returns to the VIDEO line");
        // SOUND
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::None);
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
        assert_eq!((m.page, OPTION_ITEMS[m.cursor]), (Page::Options, OptionItem::Sound), "BACK returns to the SOUND line");
        // GENERAL
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!((m.page, m.cursor), (Page::General, 0));
        assert_eq!(m.step(Nav::Right, &mut b), Outcome::Option(OptionChange::Winner));
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Right]), Outcome::Option(OptionChange::UpdateCheck));
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!((m.page, OPTION_ITEMS[m.cursor]), (Page::Options, OptionItem::General), "the BACK line returns too");
        nav(&mut m, &mut b, &[Nav::Back]);
        assert_eq!((m.page, m.cursor), (Page::Root, 6));
    }

    #[test]
    fn the_style_item_needs_the_data_to_switch_to_authentic_but_can_always_switch_back() {
        use crate::settings::Presentation;
        let mut b = Bindings::default();
        let mut m = Menu::main();
        m.page = Page::General;
        m.cursor = GENERAL_ITEMS.iter().position(|&i| i == GeneralItem::Presentation).unwrap();
        // data there: left, right and confirm switch
        m.authentic = AuthenticAvail::Ready;
        for n in [Nav::Left, Nav::Right, Nav::Confirm] {
            assert_eq!(m.step(n, &mut b), Outcome::Option(OptionChange::Presentation));
        }
        // an older data folder: nothing switches, confirm asks for the disk image
        m.authentic = AuthenticAvail::NeedsData;
        assert_eq!(m.step(Nav::Right, &mut b), Outcome::None);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::AuthenticData);
        // a ROM module: nothing
        m.authentic = AuthenticAvail::ModuleActive;
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        // in AUTHENTIC the way out is always open
        m.settings.presentation = Presentation::Authentic;
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::Option(OptionChange::Presentation));
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

    fn two_modules() -> Vec<ModuleInfo> {
        vec![ModuleInfo::new("aaaaaaaaaaaaaaaa", "More levels", "me", "1.0", "33 new stages", false), ModuleInfo::new("bbbbbbbbbbbbbbbb", "Hard mode", "you", "", "", true)]
    }

    fn modules_page() -> (Menu, Bindings) {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        m.set_modules(&two_modules(), "");
        // ROM MODULES is the fourth item
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down]);
        assert_eq!(m.root_items()[m.cursor].label(), "ROM MODULES");
        m.step(Nav::Confirm, &mut b);
        assert_eq!((m.page, m.cursor), (Page::Modules, 0));
        (m, b)
    }

    #[test]
    fn the_pause_menu_has_no_rom_modules() {
        assert!(!Menu::pause().root_items().contains(&RootItem::Modules));
    }

    #[test]
    fn the_modules_page_lists_original_modules_add_and_back() {
        let (mut m, mut b) = modules_page();
        assert_eq!(m.module_rows(), 5);
        // ORIGINAL, the two modules, ADD ROM MODULE..., BACK
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Confirm]), Outcome::ModuleAdd);
        assert_eq!(m.page, Page::Modules);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Root, 3), "back to the item that opened the page");
        // back with the back key, and up wraps to BACK
        nav(&mut m, &mut b, &[Nav::Confirm, Nav::Back]);
        assert_eq!(m.page, Page::Root);
        nav(&mut m, &mut b, &[Nav::Confirm, Nav::Up, Nav::Confirm]);
        assert_eq!(m.page, Page::Root);
    }

    #[test]
    fn a_module_opens_its_page_and_play_asks_the_app_to_switch() {
        let (mut m, mut b) = modules_page();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!((m.page, m.selected_module().map(|x| x.name.as_str())), (Page::ModuleDetail, Some("MORE LEVELS")));
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::ModulePlay("aaaaaaaaaaaaaaaa".into()));
        // the original game: ModulePlay with an empty id
        m.active_module = "aaaaaaaaaaaaaaaa".into();
        m.page = Page::Modules;
        m.cursor = 0;
        nav(&mut m, &mut b, &[Nav::Confirm]);
        assert!(m.selected_module().is_none());
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::ModulePlay(String::new()));
    }

    #[test]
    fn playing_the_module_that_is_played_only_goes_back() {
        let (mut m, mut b) = modules_page();
        m.active_module = "bbbbbbbbbbbbbbbb".into();
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Confirm]);
        assert!(m.selected_is_active());
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Modules, 2));
        // and the original game, when no module is played
        m.active_module.clear();
        nav(&mut m, &mut b, &[Nav::Up, Nav::Up, Nav::Confirm]);
        assert!(m.selected_is_active());
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None);
        assert_eq!((m.page, m.cursor), (Page::Modules, 0));
    }

    #[test]
    fn remove_asks_first_and_is_not_offered_for_the_original_or_the_active_module() {
        let (mut m, mut b) = modules_page();
        m.active_module = "bbbbbbbbbbbbbbbb".into();
        // the original game: PLAY THIS, BACK
        m.step(Nav::Confirm, &mut b);
        assert_eq!(m.detail_items(), vec![DetailItem::Play, DetailItem::Back]);
        m.step(Nav::Back, &mut b);
        // the module that is played: no REMOVE either
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Confirm]);
        assert_eq!(m.detail_items(), vec![DetailItem::Play, DetailItem::Favorite, DetailItem::Back]);
        m.step(Nav::Back, &mut b);
        // any other: PLAY THIS, FAVORITE, REMOVE, BACK; the question starts on NO
        nav(&mut m, &mut b, &[Nav::Up, Nav::Confirm, Nav::Down, Nav::Down, Nav::Confirm]);
        assert_eq!(m.page, Page::ConfirmRemove);
        assert_eq!(m.cursor, 1);
        assert_eq!(m.step(Nav::Confirm, &mut b), Outcome::None, "NO");
        assert_eq!((m.page, m.cursor), (Page::ModuleDetail, 2));
        m.step(Nav::Confirm, &mut b);
        assert_eq!(m.step(Nav::Back, &mut b), Outcome::None, "back is NO too");
        assert_eq!(m.page, Page::ModuleDetail);
        m.step(Nav::Confirm, &mut b);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Up, Nav::Confirm]), Outcome::ModuleRemove("aaaaaaaaaaaaaaaa".into()));
        assert_eq!(m.page, Page::Modules);
        // the app reads the folder again
        m.set_modules(&two_modules()[1..], "bbbbbbbbbbbbbbbb");
        assert_eq!(m.module_rows(), 4);
        assert!(m.cursor < m.module_rows());
    }

    const ID_A: &str = "aaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbb";

    #[test]
    fn f_and_y_mark_a_favorite_on_a_module_row_but_not_on_the_original_game() {
        let (mut m, mut b) = modules_page();
        assert_eq!(m.step(Nav::Favorite, &mut b), Outcome::None, "the original game is no favorite");
        m.step(Nav::Down, &mut b);
        assert_eq!(m.step(Nav::Favorite, &mut b), Outcome::ModuleFavorite(ID_A.into()));
        // Y on the controller is `Clear`
        m.step(Nav::Down, &mut b);
        assert_eq!(m.step(Nav::Clear, &mut b), Outcome::ModuleFavorite(ID_B.into()));
        // ADD and BACK rows do nothing
        nav(&mut m, &mut b, &[Nav::Down]);
        assert_eq!(m.step(Nav::Favorite, &mut b), Outcome::None);
        assert_eq!(m.step(Nav::Sort, &mut b), Outcome::ModuleSort, "sorting works from every row");
        assert_eq!(nav_for_key(Keycode::F, false), Some(Nav::Favorite));
        assert_eq!(nav_for_key(Keycode::O, false), Some(Nav::Sort));
        assert_eq!(nav_for_button(Button::X), Some(Nav::Sort));
        assert_eq!(nav_for_button(Button::Y), Some(Nav::Clear));
        // S moves down, so sorting is not on S
        assert_eq!(nav_for_key(Keycode::S, false), Some(Nav::Down));
    }

    #[test]
    fn the_detail_page_has_a_favorite_item_for_modules_only() {
        let (mut m, mut b) = modules_page();
        m.step(Nav::Confirm, &mut b);
        assert!(!m.detail_items().contains(&DetailItem::Favorite), "not for the original game");
        assert_eq!(m.step(Nav::Favorite, &mut b), Outcome::None);
        m.step(Nav::Back, &mut b);
        nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]);
        assert_eq!(m.detail_items(), vec![DetailItem::Play, DetailItem::Favorite, DetailItem::Remove, DetailItem::Back]);
        assert_eq!(nav(&mut m, &mut b, &[Nav::Down, Nav::Confirm]), Outcome::ModuleFavorite(ID_A.into()));
        assert_eq!((m.page, m.cursor), (Page::ModuleDetail, 1), "the cursor stays on the item");
        assert_eq!(m.step(Nav::Favorite, &mut b), Outcome::ModuleFavorite(ID_A.into()), "F works on every item");
    }

    #[test]
    fn a_new_order_keeps_the_cursor_and_the_detail_page_on_the_same_module() {
        let (mut m, mut b) = modules_page();
        nav(&mut m, &mut b, &[Nav::Down]);
        assert_eq!(m.cursor, 1);
        // the app puts the other module first (it became a favorite)
        let mut flipped = two_modules();
        flipped.reverse();
        m.set_modules(&flipped, "");
        assert_eq!((m.cursor, m.modules[m.cursor - 1].id.as_str()), (2, ID_A), "the cursor follows the module");
        // on the detail page the selected module stays selected
        m.step(Nav::Confirm, &mut b);
        assert_eq!(m.selected_module().map(|x| x.id.as_str()), Some(ID_A));
        m.set_modules(&two_modules(), "");
        assert_eq!(m.selected_module().map(|x| x.id.as_str()), Some(ID_A));
        // the original game stays selected as well
        m.step(Nav::Back, &mut b);
        m.cursor = 0;
        m.step(Nav::Confirm, &mut b);
        m.set_modules(&flipped, "");
        assert!(m.selected_module().is_none() && m.page == Page::ModuleDetail);
    }

    #[test]
    fn a_row_with_star_and_progress_fits_the_panel() {
        // the columns of `draw_modules`: label, a gap, the stage, a gap, the star in the last column
        let panel_w = 38;
        let star = panel_w - 3;
        for stage in [None, Some(1), Some(12), Some(99)] {
            let label_end = crate::ui::Panel::LABEL_COL + label_room(stage);
            let progress = progress_text(stage).chars().count();
            let progress_start = panel_w - 4 - progress;
            if progress > 0 {
                assert!(label_end < progress_start, "label of {stage:?} reaches the stage");
                assert_eq!(progress_start - label_end, 2, "two gaps between label and stage");
            } else {
                assert!(label_end < star, "label reaches the star");
            }
        }
        assert_eq!(label_room(None), LIST_LABEL_MAX);
        assert!(progress_text(Some(12)).starts_with("STAGE 12") && progress_text(None).is_empty());
        assert_eq!(progress_text(Some(250)), "STAGE 99", "never wider than two digits");
        // long names are cut, the version stays
        let long = ModuleInfo::new("0", &"W".repeat(24), "", "1.0", "", false);
        for stage in [None, Some(7), Some(99)] {
            let room = label_room(stage);
            let label = long.list_label(room);
            assert!(label.chars().count() <= room && label.ends_with("  V1.0"), "{label:?}");
            let fav = ModuleInfo::new("0", "Frankie's Horror Trip 2", "", "12.34", "", false);
            assert!(fav.list_label(room).chars().count() <= room);
        }
        // a module that was not played keeps all of its name
        let trip = ModuleInfo::new("0", "Frankie's Horror Trip 2", "", "", "", false);
        assert_eq!(trip.list_label(label_room(None)), "FRANKIE'S HORROR TRIP 2");
        // the star is no character a name can bring along
        assert_eq!(safe_text("\u{2605}"), "?");
    }

    #[test]
    fn the_page_of_the_module_that_is_played_starts_on_it_and_errors_show_until_a_key() {
        let mut b = Bindings::default();
        let mut m = Menu::main();
        m.set_modules(&two_modules(), "bbbbbbbbbbbbbbbb");
        nav(&mut m, &mut b, &[Nav::Down, Nav::Down, Nav::Down, Nav::Confirm]);
        assert_eq!(m.cursor, 2);
        m.module_added(None, Some("this is not a valid ROM module".into()));
        assert!(m.module_error.is_some());
        m.step(Nav::Up, &mut b);
        assert!(m.module_error.is_none(), "the next key clears it");
        // a module that was added gets the cursor
        m.module_added(Some("aaaaaaaaaaaaaaaa"), None);
        assert_eq!((m.page, m.cursor), (Page::Modules, 1));
        // a list that shrinks keeps the cursor on the page
        m.cursor = 4;
        m.set_modules(&[], "");
        assert_eq!(m.cursor, 2);
    }

    #[test]
    fn module_texts_are_capitals_the_font_has_and_labels_are_cut() {
        let m = ModuleInfo::new("0", "caf\u{e9} n\u{f6}", "\u{4e2d}", "1.0-b", "l\u{e4}ng", true);
        assert_eq!((m.name.as_str(), m.author.as_str(), m.description.as_str()), ("CAF? N?", "?", "L?NG"));
        let long = ModuleInfo::new("0", &"W".repeat(24), "", "1.0", "", false);
        let label = long.list_label(20);
        assert_eq!(label.chars().count(), 20);
        assert!(label.ends_with("  V1.0"));
        assert_eq!(ModuleInfo::new("0", "A", "", "", "", false).list_label(20), "A");
        assert_eq!(CODE_NOTICE.join(" "), "CHANGES GAME CODE - MAY NOT PLAY AS INTENDED");
    }
}
