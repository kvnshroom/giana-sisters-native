//! `PRESENTATION: AUTHENTIC`: the original's presentation, from its title program to the high score table, shown exactly as the
//! C64 showed it and run natively (no emulation): the title sequence (`giana_core::title`), the black loading interval, the
//! game program's own title and the game itself (`GameState` with `render_vic_authentic`, the same physics and timing as the
//! modern presentation), and the high score screen (`giana_core::highscore`). This module is the glue to the app: keys and
//! joystick, the file for the high scores, the picture, and the small settings overlay. The cycle exact C64 slice
//! (`giana-authentic`) is only the reference of the tests. See `docs/authentic-mode.md`.

use crate::bindings::{config_path, Bindings, Held, Source};
use crate::menu::{AuthenticAvail, Menu};
use giana_core::highscore::Input as HsInput;
use giana_core::highscore_game::{plausible_scores, GameOver, Tables};
use giana_core::ingame_title::{TitleEvent, TitleKeys};
use giana_core::state::{ControllerInput, GameState};
use giana_render::framebuffer::Framebuffer;
use giana_render::highscore::HighScorePlayer;
use giana_render::palette::{c64_color, C64_PALETTE};
use giana_render::title::TitlePlayer;
use giana_render::vic::{render_vic_authentic, VicView};
use sdl2::keyboard::Keycode;
use std::path::{Path, PathBuf};

/// The file of the high score table the original saved to disk (the program's `SAVE` of `ATG`), in the settings folder.
/// It has the format of the disk file: a `.prg` with the load address `$36F7` and 120 bytes. The table of the modern
/// presentation (`scores.txt`) is separate and not touched.
pub fn scores_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("authentic_scores.prg"))
}

/// Can AUTHENTIC be started with the data in `data_dir`?
pub fn availability(data_dir: &Path, module_active: bool) -> AuthenticAvail {
    if module_active {
        AuthenticAvail::ModuleActive
    } else if giana_extract::missing_optional(data_dir).is_empty() {
        AuthenticAvail::Ready
    } else {
        AuthenticAvail::NeedsData
    }
}

/// A key of the C64 keyboard matrix: column (port A line) and row (port B line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub col: u8,
    pub row: u8,
}

impl Key {
    pub const fn new(col: u8, row: u8) -> Self {
        Key { col, row }
    }
    pub const RETURN: Key = Key::new(0, 1);
    pub const DELETE: Key = Key::new(0, 0);
    pub const F1: Key = Key::new(0, 4);
    pub const SPACE: Key = Key::new(7, 4);
}

/// The matrix position of a letter or digit, for typing a name.
fn key_for_char(c: char) -> Option<Key> {
    Some(match c.to_ascii_lowercase() {
        'a' => Key::new(1, 2),
        'b' => Key::new(3, 4),
        'c' => Key::new(2, 4),
        'd' => Key::new(2, 2),
        'e' => Key::new(1, 6),
        'f' => Key::new(2, 5),
        'g' => Key::new(3, 2),
        'h' => Key::new(3, 5),
        'i' => Key::new(4, 1),
        'j' => Key::new(4, 2),
        'k' => Key::new(4, 5),
        'l' => Key::new(5, 2),
        'm' => Key::new(4, 4),
        'n' => Key::new(4, 7),
        'o' => Key::new(4, 6),
        'p' => Key::new(5, 1),
        'q' => Key::new(7, 6),
        'r' => Key::new(2, 1),
        's' => Key::new(1, 5),
        't' => Key::new(2, 6),
        'u' => Key::new(3, 6),
        'v' => Key::new(3, 7),
        'w' => Key::new(1, 1),
        'x' => Key::new(2, 7),
        'y' => Key::new(3, 1),
        'z' => Key::new(1, 4),
        '0' => Key::new(4, 3),
        '1' => Key::new(7, 0),
        '2' => Key::new(7, 3),
        '3' => Key::new(1, 0),
        '4' => Key::new(1, 3),
        '5' => Key::new(2, 0),
        '6' => Key::new(2, 3),
        '7' => Key::new(3, 0),
        '8' => Key::new(3, 3),
        '9' => Key::new(4, 0),
        _ => return None,
    })
}

fn matrix(keys: &[Key]) -> [u8; 8] {
    let mut m = [0u8; 8];
    for k in keys {
        m[k.col as usize & 7] |= 1 << (k.row & 7);
    }
    m
}

/// The C64 keys that the PC keyboard types: letters, digits, space, return and delete (for the name entry).
fn typing_keys() -> Vec<(Keycode, Key)> {
    let mut v = Vec::new();
    for c in ('a'..='z').chain('0'..='9') {
        if let (Some(code), Some(key)) = (Keycode::from_name(&c.to_string()), key_for_char(c)) {
            v.push((code, key));
        }
    }
    v.push((Keycode::Return, Key::RETURN));
    v.push((Keycode::KpEnter, Key::RETURN));
    v.push((Keycode::Backspace, Key::DELETE));
    v.push((Keycode::Space, Key::SPACE));
    v
}

/// Frames of black screen while the original would load the game (the real loading takes about 40 seconds).
pub const LOADING_FRAMES: u32 = 50;

/// What the presentation reads from the data folder besides the title: the game program's image holds the two high score
/// tables, `atg.prg` is the disk file of the "all time" table.
struct Data {
    ingame: Vec<u8>,
    atg: Vec<u8>,
}

impl Data {
    fn load(dir: &Path) -> Result<Data, String> {
        let rd = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("cannot read {n}: {e}"));
        let ingame = rd("ingame.bin")?;
        if ingame.len() < 65536 {
            return Err("ingame.bin is not a program image".to_string());
        }
        Ok(Data { ingame, atg: rd("atg.prg")? })
    }

    /// The tables the game has at power-on: the image's, with the disk file (the player's own save when there is one).
    fn tables(&self, saved: Option<&[u8]>) -> Tables {
        let t = Tables::from_image(&self.ingame).with_disk_file(&self.atg);
        match saved {
            Some(prg) => t.with_disk_file(prg),
            None => t,
        }
    }
}

/// The game program: its title, and the game that the title starts.
struct Play {
    game: GameState,
    /// the title (attract level, 1/2 player choice) is still showing
    in_title: bool,
}

/// The high score screen of a player who is out, and the game it came from.
struct HighScore {
    screen: HighScorePlayer,
    game: GameState,
}

/// Where the presentation is.
enum Part {
    /// the title program (native)
    Title(Box<TitlePlayer>),
    /// the black loading interval
    Loading(u32),
    /// the game program's title and the game
    Play(Box<Play>),
    /// the high score table, name entry and save
    HighScore(Box<HighScore>),
}

pub struct AuthenticRun {
    part: Part,
    data_dir: PathBuf,
    data: Data,
    /// the high score tables; they live as long as the presentation runs ("today's" fills up over the games)
    tables: Tables,
    scores: Option<PathBuf>,
    keys: Vec<(Keycode, Key)>,
    /// frames since the program last waited at its "PRESS F1 TO SAVE HIGHSCORES" prompt (F1 is the C64 key there)
    since_prompt: u32,
    /// the settings overlay, over the paused picture
    pub overlay: Option<Menu>,
    /// something the player should know, e.g. that the high scores could not be saved
    pub notice: Option<String>,
}

fn read_saved(path: &Option<PathBuf>) -> Option<Vec<u8>> {
    path.as_ref().and_then(|p| std::fs::read(p).ok()).filter(|d| plausible_scores(d))
}

impl AuthenticRun {
    /// Starts the presentation from the title program on. `scores` is the file for the high score table.
    pub fn new(data_dir: &Path, scores: Option<PathBuf>) -> Result<Self, String> {
        // check the game program's files now, not when the loading interval is over
        let data = Data::load(data_dir)?;
        let title = Self::title_player(data_dir)?;
        let tables = data.tables(read_saved(&scores).as_deref());
        Ok(Self { part: Part::Title(Box::new(title)), data_dir: data_dir.to_path_buf(), data, tables, scores, keys: typing_keys(), since_prompt: u32::MAX, overlay: None, notice: None })
    }

    fn title_player(data_dir: &Path) -> Result<TitlePlayer, String> {
        let rd = |n: &str| std::fs::read(data_dir.join(n)).map_err(|e| format!("cannot read {n}: {e}"));
        let setup = giana_extract::derive::title_setup_memory(&rd("f_2.prg")?)?;
        let mut player = TitlePlayer::new(giana_core::title::TitleAssets::from_setup_memory(&setup), &rd("title_koala.bin")?);
        // the title picture's music and sample (SID writes with their cycle in the frame, see `take_sid_writes`)
        player.attach_music(&setup);
        Ok(player)
    }

    /// The game program at its title (`L084F`), as the original reaches it after loading.
    fn new_play() -> Play {
        let mut game = GameState::new_ingame_title();
        game.hold_for_high_score = true;
        Play { game, in_title: true }
    }

    /// F1 goes to the program: it is waiting at the question whether to save the high scores. Everywhere else F1 opens the
    /// overlay.
    pub fn f1_is_for_the_program(&self) -> bool {
        self.since_prompt < 25
    }

    /// The current tables of the high scores (for tests).
    pub fn tables(&self) -> &Tables {
        &self.tables
    }

    /// The game of the part that plays (for tests: cheats and inspection).
    pub fn game_mut(&mut self) -> Option<&mut GameState> {
        match &mut self.part {
            Part::Play(p) => Some(&mut p.game),
            Part::HighScore(h) => Some(&mut h.game),
            _ => None,
        }
    }

    /// Which part shows now: "title", "loading", "game title", "game" or "high score" (for tests and logs).
    pub fn part_name(&self) -> &'static str {
        match &self.part {
            Part::Title(_) => "title",
            Part::Loading(_) => "loading",
            Part::Play(p) if p.in_title => "game title",
            Part::Play(_) => "game",
            Part::HighScore(_) => "high score",
        }
    }

    /// Runs one frame (20 ms) with the keys and buttons that are held now.
    pub fn frame(&mut self, held: &Held, bindings: &Bindings) -> Result<(), String> {
        let mut input = ControllerInput::new();
        bindings.apply(held, &mut input);
        let mut joy = 0u8;
        for (on, bit) in [(input.jump, 0x01), (input.down, 0x02), (input.left, 0x04), (input.right, 0x08), (input.fire, 0x10)] {
            if on {
                joy |= bit;
            }
        }
        let is_held = |k: Keycode| held.is_held(Source::Key(k));
        match &mut self.part {
            Part::Title(t) => {
                t.screen.set_fire(joy & 0x10 != 0);
                t.run_frame();
                if t.screen.done() {
                    self.part = Part::Loading(0);
                }
            }
            Part::Loading(n) => {
                *n += 1;
                if *n >= LOADING_FRAMES {
                    self.part = Part::Play(Box::new(Self::new_play()));
                }
            }
            Part::Play(p) => {
                if p.in_title {
                    let keys = TitleKeys { key_1: is_held(Keycode::Num1) || is_held(Keycode::Kp1), key_2: is_held(Keycode::Num2) || is_held(Keycode::Kp2) };
                    if let TitleEvent::Started { .. } = p.game.title_frame(&input, keys) {
                        p.in_title = false;
                    }
                } else {
                    p.game.update(&input);
                    if p.game.game_over || p.game.high_score_due {
                        self.begin_high_score();
                    }
                }
            }
            Part::HighScore(_) => self.high_score_frame(held, joy, &input),
        }
        Ok(())
    }

    /// `$0C4E`: a player is out. The game hands its score, screen and sound driver to the high score screen.
    fn begin_high_score(&mut self) {
        let Part::Play(p) = std::mem::replace(&mut self.part, Part::Loading(0)) else { return };
        let game = p.game;
        let screen = HighScorePlayer::begin(&GameOver::from_game(&game), &self.tables);
        self.since_prompt = u32::MAX;
        self.part = Part::HighScore(Box::new(HighScore { screen, game }));
    }

    fn high_score_frame(&mut self, held: &Held, joy: u8, input: &ControllerInput) {
        let Part::HighScore(h) = &mut self.part else { return };
        let mut keys: Vec<Key> = self.keys.iter().filter(|(k, _)| held.is_held(Source::Key(*k))).map(|(_, c)| *c).collect();
        if input.item {
            keys.push(Key::SPACE);
        }
        if self.since_prompt < 25 && held.is_held(Source::Key(Keycode::F1)) {
            keys.push(Key::F1);
        }
        let running = h.screen.run_frame(HsInput { joy, keys: matrix(&keys) });
        if let Some(e) = h.screen.hs.error() {
            // the native code met something it does not know: leave the screen, keep the game going
            eprintln!("AUTHENTIC high score screen stopped: {e}");
        }
        if h.screen.hs.waiting_for_save_key() {
            self.since_prompt = 0;
        } else {
            self.since_prompt = self.since_prompt.saturating_add(1);
        }
        for prg in h.screen.hs.take_saves() {
            if let Some(path) = &self.scores {
                if let Err(e) = crate::fsutil::write_atomic_bytes(path, &prg) {
                    eprintln!("Could not save the high scores to {}: {e}", path.display());
                    self.notice = Some("COULD NOT SAVE THE HIGH SCORES".to_string());
                }
            }
        }
        if running && !h.screen.hs.finished() && h.screen.hs.error().is_none() {
            return;
        }
        // back to the game over code: the tables and the sound driver go back, then the other player's turn or the title
        self.since_prompt = u32::MAX;
        let Part::HighScore(h) = std::mem::replace(&mut self.part, Part::Loading(0)) else { return };
        let HighScore { screen, mut game } = *h;
        self.tables = screen.hs.tables();
        screen.hs.give_back_driver(&mut game.sound);
        if game.game_over {
            game.after_game_over();
            game.hold_for_high_score = true;
            self.part = Part::Play(Box::new(Play { game, in_title: true }));
        } else {
            game.resume_after_high_score();
            self.part = Part::Play(Box::new(Play { game, in_title: false }));
        }
    }

    /// Back to the title program, as if the computer had been switched off and on (the tables are the saved ones again).
    pub fn restart(&mut self) -> Result<(), String> {
        self.since_prompt = u32::MAX;
        self.part = Part::Title(Box::new(Self::title_player(&self.data_dir)?));
        self.tables = self.data.tables(read_saved(&self.scores).as_deref());
        Ok(())
    }

    /// The picture of the last frame as the C64 showed it (384 x 272).
    pub fn draw(&self, fb: &mut Framebuffer) {
        match &self.part {
            Part::Title(t) => paint(fb, &t.picture()),
            Part::Loading(_) => paint(fb, &[]),
            Part::Play(p) => render_vic_authentic(fb, &VicView::from_game(&p.game)),
            Part::HighScore(h) => h.screen.draw_to(fb),
        }
    }

    /// The picture of the last frame: 384 x 272 C64 colour indices.
    pub fn picture(&self) -> Vec<u8> {
        match &self.part {
            Part::Title(t) => t.picture(),
            Part::HighScore(h) => h.screen.picture(),
            _ => {
                let mut fb = Framebuffer::new();
                self.draw(&mut fb);
                fb.pixels.iter().map(|p| C64_PALETTE.iter().position(|c| c == p).unwrap_or(0) as u8).collect()
            }
        }
    }

    /// The timed SID writes of the last frame (the title picture's music and its sample); empty in the game.
    pub fn take_sid_writes(&mut self) -> Vec<(u32, u8, u8)> {
        match &mut self.part {
            Part::Title(t) => t.take_sid_writes(),
            _ => Vec::new(),
        }
    }

    /// The SID registers `$D400..$D418` after the last frame.
    pub fn sid(&self) -> [u8; 25] {
        match &self.part {
            Part::Title(t) => t.sid_regs(),
            Part::Play(p) => p.game.sound.sid[..25].try_into().unwrap_or([0; 25]),
            Part::HighScore(h) => h.screen.hs.sid_regs(),
            Part::Loading(_) => [0; 25],
        }
    }
}

/// A picture of colour indices into the framebuffer (an empty one is black).
fn paint(fb: &mut Framebuffer, pic: &[u8]) {
    for (i, d) in fb.pixels.iter_mut().enumerate() {
        *d = c64_color(pic.get(i).copied().unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> Option<PathBuf> {
        giana_core::data::candidate_dirs(None).into_iter().find(|d| d.join("atg.prg").is_file() && d.join("f_2.prg").is_file() && d.join("ingame.bin").is_file())
    }

    #[test]
    fn availability_follows_the_files() {
        let dir = std::env::temp_dir().join(format!("giana-authentic-avail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(availability(&dir, false), AuthenticAvail::NeedsData, "an older data folder has no atg.prg");
        assert_eq!(availability(&dir, true), AuthenticAvail::ModuleActive);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn typing_covers_letters_digits_return_and_delete() {
        let k = typing_keys();
        assert_eq!(k.len(), 26 + 10 + 4);
        assert!(k.iter().any(|(c, key)| *c == Keycode::Return && *key == Key::RETURN));
        assert_eq!(matrix(&[Key::F1, Key::SPACE]), [0x10, 0, 0, 0, 0, 0, 0, 0x10]);
    }
}
