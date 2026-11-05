//! The suspend point: a local game that was running when the program was closed, kept in a file to be continued later.
//!
//! Two slots per ROM module (the original game counts as one): one for a game of one player, one for two players taking
//! turns, so that a game with company is not lost by playing alone in between. They live next to the module's high scores. A slot is a suspend
//! point, not a save game: it is deleted when it is continued, and when a new game is started. A file that does not fit
//! the program (another saved layout), the data (another module or other game data) or that is damaged is ignored and deleted,
//! with a short notice. The state itself is `giana_core::snapshot`; this file adds the header, the compression and the rules.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use giana_core::snapshot;
use giana_core::state::{GameScreen, GameState};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// The version of the program that wrote a file (for information: a slot survives updates as long as the saved layout,
/// `snapshot::SNAPSHOT_FORMAT`, is the same).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a slot says about its game, for the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// 1 or 2 players (two take turns)
    pub players: u8,
    pub stage: u32,
    /// the score of the player whose turn it is
    pub score: u32,
    /// whose turn it is: 0 Giana, 1 Maria
    pub player: u8,
    /// two players: the score of the one whose turn it is not
    pub other_score: u32,
}

impl Info {
    /// The note line of the menu: the stage and the score, or both scores (Giana, Maria) of two players.
    pub fn note(&self) -> String {
        if self.players == 2 {
            let (giana, maria) = if self.player == 0 { (self.score, self.other_score) } else { (self.other_score, self.score) };
            format!("STAGE {:02}  {giana:06} / {maria:06}", self.stage)
        } else {
            format!("STAGE {:02}  SCORE {:06}", self.stage, self.score)
        }
    }
}

/// Which game a slot has to belong to: the module and the game data that is installed now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// id of the ROM module (empty: the original game)
    pub module: String,
    /// `snapshot::data_fingerprint`
    pub data: String,
}

impl Identity {
    /// For the module that is installed now (its data must be installed).
    pub fn current(module: &str) -> Self {
        Identity { module: module.to_string(), data: snapshot::data_fingerprint() }
    }
}

/// Why a file was not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// written with another saved layout (`SNAPSHOT_FORMAT`)
    Version,
    /// another module or other game data
    Data,
    /// cannot be read
    Damaged,
}

impl Reject {
    /// The notice for the menu.
    pub fn notice(self) -> &'static str {
        match self {
            Reject::Version => "SAVED GAME DISCARDED: OLD FORMAT",
            Reject::Data => "SAVED GAME DISCARDED: DATA CHANGED",
            Reject::Damaged => "SAVED GAME DISCARDED: DAMAGED",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Header {
    app: String,
    /// `snapshot::SNAPSHOT_FORMAT`
    format: u32,
    module: String,
    data: String,
    players: u8,
    stage: u32,
    score: u32,
    player: u8,
    #[serde(default)]
    other_score: u32,
}

/// The id of a module as it is used in file names: only hex digits (as for the scores).
fn file_id(module: &str) -> String {
    module.chars().filter(char::is_ascii_hexdigit).collect::<String>().to_ascii_lowercase()
}

/// `suspend-1p.bin` / `suspend-2p.bin` for the original game, `suspend-<id>-1p.bin` / `-2p.bin` for the ROM module with
/// that id.
pub fn file_name(module: &str, players: u8) -> String {
    let id = file_id(module);
    let sep = if id.is_empty() { "" } else { "-" };
    format!("suspend{sep}{id}-{}p.bin", if players == 2 { 2 } else { 1 })
}

/// The slot of `module` for a game of `players` players, in the settings folder next to its scores.
pub fn path(module: &str, players: u8) -> Option<PathBuf> {
    crate::bindings::config_path().map(|p| p.with_file_name(file_name(module, players)))
}

/// The single slot of earlier versions: `suspend.bin` / `suspend-<id>.bin`.
fn old_file_name(module: &str) -> String {
    let id = file_id(module);
    if id.is_empty() {
        "suspend.bin".to_string()
    } else {
        format!("suspend-{id}.bin")
    }
}

/// Moves a slot of the earlier single-slot version to the slot of its player count (the header says which). A file that
/// cannot be read, or whose slot is taken, is deleted.
pub fn migrate(dir: &Path, module: &str) {
    let old = dir.join(old_file_name(module));
    if !old.is_file() {
        return;
    }
    let players = read_header(&old).map(|h| h.players);
    match players {
        Some(n @ (1 | 2)) if !dir.join(file_name(module, n)).exists() => {
            let _ = std::fs::rename(&old, dir.join(file_name(module, n)));
        }
        _ => discard(&old),
    }
}

fn read_header(path: &Path) -> Option<Header> {
    let mut rest = BufReader::new(GzDecoder::new(std::fs::File::open(path).ok()?));
    let mut line = String::new();
    rest.by_ref().take(4096).read_line(&mut line).ok()?;
    serde_json::from_str(line.trim_end()).ok()
}

/// Both slots of `module`, looked at (and migrated first). The slots that fit, one player first; the reasons for the files that
/// did not and were deleted.
pub fn look(module: &str, id: &Identity) -> (Vec<Info>, Vec<Reject>) {
    match path(module, 1).and_then(|p| p.parent().map(Path::to_path_buf)) {
        Some(dir) => look_in(&dir, module, id),
        None => (Vec::new(), Vec::new()),
    }
}

fn look_in(dir: &Path, module: &str, id: &Identity) -> (Vec<Info>, Vec<Reject>) {
    let (mut found, mut rejected) = (Vec::new(), Vec::new());
    migrate(dir, module);
    for players in [1, 2] {
        let path = dir.join(file_name(module, players));
        match peek(&path, id) {
            Peek::Valid(info) => found.push(info),
            Peek::Empty => {}
            Peek::Rejected(why) => rejected.push(why),
        }
    }
    (found, rejected)
}

/// Deletes both slots of `module` (it was removed).
pub fn discard_all(module: &str) {
    for players in [1, 2] {
        if let Some(path) = path(module, players) {
            discard(&path);
        }
    }
}

/// May the game that is shown now be suspended? A local game that runs on the ported engine: not a network game (the
/// other computer cannot wait), not a game started at a chosen stage (a test), and not one that is over. AUTHENTIC
/// never gets here: it runs the original program, not the game of the port.
pub fn can_suspend(game: &GameState, in_network: bool, fixed_stage: bool) -> bool {
    game.screen == GameScreen::InGame && !game.game_over && !game.legacy_model && !in_network && !fixed_stage
}

/// 2 for a game of two players taking turns, else 1: the slot a game belongs to.
pub fn players_of(game: &GameState) -> u8 {
    if game.two_players {
        2
    } else {
        1
    }
}

fn info_of(game: &GameState) -> Info {
    // the player whose turn it is has the live values, the other one the saved ones (`player_save`)
    let other = (game.player ^ 1) as usize;
    let s = &game.player_save;
    Info {
        players: players_of(game),
        stage: game.stage,
        score: crate::scores::score_value(game.score_bcd),
        player: game.player,
        other_score: if game.two_players { crate::scores::score_value([s[other], s[2 + other], s[4 + other]]) } else { 0 },
    }
}

/// Writes the slot of the game's player count in the settings folder; returns the file.
pub fn write(module: &str, id: &Identity, game: &GameState) -> std::io::Result<PathBuf> {
    let path = path(module, players_of(game)).ok_or_else(|| std::io::Error::other("there is no settings folder"))?;
    write_to(&path, id, game)?;
    Ok(path)
}

/// Writes the slot at `path` for `game`.
pub fn write_to(path: &Path, id: &Identity, game: &GameState) -> std::io::Result<()> {
    let info = info_of(game);
    write_raw(path, id, &info, &snapshot::save(game))
}

/// The file with an already serialised state (the tests of the rules use bytes of their own).
fn write_raw(path: &Path, id: &Identity, info: &Info, state: &[u8]) -> std::io::Result<()> {
    let header = Header {
        app: APP_VERSION.to_string(),
        format: snapshot::SNAPSHOT_FORMAT,
        module: id.module.clone(),
        data: id.data.clone(),
        players: info.players,
        stage: info.stage,
        score: info.score,
        player: info.player,
        other_score: info.other_score,
    };
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    serde_json::to_writer(&mut gz, &header).map_err(std::io::Error::other)?;
    gz.write_all(b"\n")?;
    gz.write_all(state)?;
    crate::fsutil::write_atomic_bytes(path, &gz.finish()?)
}

/// Reads the header and checks it against `id`; `rest` is positioned at the state.
fn open(path: &Path, id: &Identity) -> Result<(Info, BufReader<GzDecoder<std::fs::File>>), Reject> {
    let file = std::fs::File::open(path).map_err(|_| Reject::Damaged)?;
    let mut rest = BufReader::new(GzDecoder::new(file));
    let mut line = String::new();
    // a header is a few dozen bytes: a file that has none is not ours
    rest.by_ref().take(4096).read_line(&mut line).map_err(|_| Reject::Damaged)?;
    let h: Header = serde_json::from_str(line.trim_end()).map_err(|_| Reject::Damaged)?;
    if h.players != 1 && h.players != 2 {
        return Err(Reject::Damaged);
    }
    if h.format != snapshot::SNAPSHOT_FORMAT {
        return Err(Reject::Version);
    }
    if h.module != id.module || h.data != id.data {
        return Err(Reject::Data);
    }
    Ok((Info { players: h.players, stage: h.stage, score: h.score, player: h.player, other_score: h.other_score }, rest))
}

/// Deletes the slot (nothing happens when there is none).
pub fn discard(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// What is in a slot.
#[derive(Debug, PartialEq, Eq)]
pub enum Peek {
    Empty,
    Valid(Info),
    /// the file did not fit and was deleted
    Rejected(Reject),
}

/// Looks at a slot without taking it. A file that does not fit is deleted.
pub fn peek(path: &Path, id: &Identity) -> Peek {
    if !path.is_file() {
        return Peek::Empty;
    }
    match open(path, id) {
        Ok((info, _)) => Peek::Valid(info),
        Err(r) => {
            discard(path);
            Peek::Rejected(r)
        }
    }
}

/// The game of a slot, and the slot is gone: it is a suspend point, not a save game. The game data of the module must be
/// installed (the state is tied to it).
pub fn take(path: &Path, id: &Identity) -> Result<(GameState, Info), Reject> {
    let result = take_state(path, id).and_then(|(info, state)| snapshot::restore(&state).map(|g| (g, info)).map_err(|_| Reject::Damaged));
    discard(path);
    result
}

fn take_state(path: &Path, id: &Identity) -> Result<(Info, Vec<u8>), Reject> {
    let (info, mut rest) = open(path, id)?;
    let mut state = Vec::new();
    rest.read_to_end(&mut state).map_err(|_| Reject::Damaged)?;
    Ok((info, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("giana-suspend-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn id(module: &str, data: &str) -> Identity {
        Identity { module: module.into(), data: data.into() }
    }

    fn info() -> Info {
        Info { players: 2, stage: 7, score: 12350, player: 1, other_score: 400 }
    }

    #[test]
    fn the_note_names_stage_and_score_or_both_scores() {
        // Maria's turn: Giana's score is the other one
        assert_eq!(info().note(), "STAGE 07  000400 / 012350");
        assert_eq!(Info { player: 0, ..info() }.note(), "STAGE 07  012350 / 000400");
        let one = Info { players: 1, stage: 12, score: 5, player: 0, other_score: 0 };
        assert_eq!(one.note(), "STAGE 12  SCORE 000005");
        assert!(info().note().len() <= 26 && Info { stage: 33, score: 999_999, other_score: 999_999, ..info() }.note().len() <= 26);
    }

    #[test]
    fn a_slot_belongs_to_its_module_and_its_player_count() {
        assert_eq!(file_name("", 1), "suspend-1p.bin");
        assert_eq!(file_name("", 2), "suspend-2p.bin");
        assert_eq!(file_name("AB12cd", 2), "suspend-ab12cd-2p.bin");
        // an id names a file and cannot walk out of the folder
        assert_eq!(file_name("../../xe", 1), "suspend-e-1p.bin");
        assert_ne!(file_name("", 1), file_name("ab12", 1));
        assert_ne!(file_name("ab12", 1), file_name("ab12", 2));
    }

    #[test]
    fn a_slot_that_fits_is_found_and_continued_once() {
        let d = dir("fits");
        let f = d.join("a.bin");
        let me = id("ab12", "dead");
        write_raw(&f, &me, &info(), b"state").unwrap();
        assert_eq!(peek(&f, &me), Peek::Valid(info()));
        // looking does not take it
        assert!(f.is_file());
        let (i, state) = take_state(&f, &me).unwrap();
        assert_eq!((i, state.as_slice()), (info(), &b"state"[..]));
    }

    #[test]
    fn another_version_or_other_data_is_ignored_and_deleted() {
        let d = dir("mismatch");
        let me = id("ab12", "dead");
        for (other, why) in [(id("cd34", "dead"), Reject::Data), (id("ab12", "beef"), Reject::Data), (id("", "dead"), Reject::Data)] {
            let f = d.join("a.bin");
            write_raw(&f, &other, &info(), b"state").unwrap();
            assert_eq!(peek(&f, &me), Peek::Rejected(why));
            assert!(!f.exists(), "the file stays after {why:?}");
            assert_eq!(peek(&f, &me), Peek::Empty);
        }
        // a file of another version of the program still fits as long as the layout is the same
        let f = d.join("v.bin");
        let header = Header { app: "0.0.0-other".into(), format: snapshot::SNAPSHOT_FORMAT, module: "ab12".into(), data: "dead".into(), players: 1, stage: 1, score: 0, player: 0, other_score: 0 };
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        writeln!(gz, "{}", serde_json::to_string(&header).unwrap()).unwrap();
        gz.write_all(b"state").unwrap();
        std::fs::write(&f, gz.finish().unwrap()).unwrap();
        assert_eq!(peek(&f, &me), Peek::Valid(Info { players: 1, stage: 1, score: 0, player: 0, other_score: 0 }));
        assert!(f.exists());
        // another format of the state is not used
        let header = Header { app: APP_VERSION.into(), format: snapshot::SNAPSHOT_FORMAT + 1, ..header };
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        writeln!(gz, "{}", serde_json::to_string(&header).unwrap()).unwrap();
        std::fs::write(&f, gz.finish().unwrap()).unwrap();
        assert_eq!(peek(&f, &me), Peek::Rejected(Reject::Version));
    }

    #[test]
    fn a_damaged_file_is_ignored_and_deleted() {
        let d = dir("damaged");
        let me = id("", "dead");
        for bytes in [&b""[..], b"not a slot at all", &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0][..]] {
            let f = d.join("a.bin");
            std::fs::write(&f, bytes).unwrap();
            assert_eq!(peek(&f, &me), Peek::Rejected(Reject::Damaged), "{bytes:?}");
            assert!(!f.exists());
        }
    }

    #[test]
    fn a_slot_that_is_continued_is_gone_even_when_its_state_is_bad() {
        let d = dir("take");
        let f = d.join("a.bin");
        let me = id("", "dead");
        write_raw(&f, &me, &info(), b"this is no game").unwrap();
        // the state does not parse: an error, and the file is deleted all the same
        assert_eq!(take(&f, &me).err(), Some(Reject::Damaged));
        assert!(!f.exists());
        // a slot of another module is rejected by `take` too, and deleted
        write_raw(&f, &id("ab", "dead"), &info(), b"x").unwrap();
        assert_eq!(take(&f, &me).err(), Some(Reject::Data));
        assert!(!f.exists());
    }

    #[test]
    fn slots_of_different_modules_do_not_touch_each_other() {
        let d = dir("separate");
        let (a, b) = (d.join(file_name("", 1)), d.join(file_name("ab12", 1)));
        write_raw(&a, &id("", "x"), &Info { players: 1, ..info() }, b"1").unwrap();
        write_raw(&b, &id("ab12", "y"), &Info { players: 1, stage: 3, ..info() }, b"2").unwrap();
        assert_eq!(peek(&b, &id("ab12", "y")), Peek::Valid(Info { players: 1, stage: 3, ..info() }));
        assert_eq!(peek(&a, &id("", "x")), Peek::Valid(Info { players: 1, ..info() }));
        discard(&a);
        assert!(b.is_file());
        assert!(!a.exists());
    }

    fn one() -> Info {
        Info { players: 1, stage: 5, score: 900, player: 0, other_score: 0 }
    }

    fn put(d: &Path, module: &str, info: &Info) -> PathBuf {
        let f = d.join(file_name(module, info.players));
        write_raw(&f, &id(module, "dead"), info, b"state").unwrap();
        f
    }

    #[test]
    fn the_slots_for_one_and_two_players_are_independent() {
        let d = dir("players");
        let me = id("ab12", "dead");
        let (f1, f2) = (put(&d, "ab12", &one()), put(&d, "ab12", &info()));
        assert_ne!(f1, f2);
        // both are found, one player first
        let (found, rejected) = look_in(&d, "ab12", &me);
        assert_eq!(found, vec![one(), info()]);
        assert!(rejected.is_empty());
        // continuing the one player game leaves the two player game
        let (_, state) = take_state(&f1, &me).unwrap();
        discard(&f1);
        assert_eq!(state, b"state");
        assert_eq!(look_in(&d, "ab12", &me).0, vec![info()]);
        // a new one player game (the app deletes the one player slot) leaves it too
        put(&d, "ab12", &one());
        discard(&f1);
        assert_eq!(look_in(&d, "ab12", &me).0, vec![info()]);
        // writing the one player slot again does not touch the other one
        put(&d, "ab12", &Info { stage: 9, ..one() });
        assert_eq!(look_in(&d, "ab12", &me).0, vec![Info { stage: 9, ..one() }, info()]);
        assert!(f2.is_file());
        // a bad file in one slot goes alone
        std::fs::write(&f2, b"junk").unwrap();
        let (found, rejected) = look_in(&d, "ab12", &me);
        assert_eq!((found.len(), rejected), (1, vec![Reject::Damaged]));
        assert!(f1.is_file() && !f2.exists());
        // the slots of another module are not looked at
        assert!(look_in(&d, "cd34", &id("cd34", "dead")).0.is_empty());
    }

    #[test]
    fn the_slot_of_the_earlier_version_moves_to_its_player_count() {
        let d = dir("migrate");
        let me = id("ab12", "dead");
        // two players in the old file: it becomes the two player slot
        let old = d.join("suspend-ab12.bin");
        write_raw(&old, &me, &info(), b"state").unwrap();
        assert_eq!(look_in(&d, "ab12", &me).0, vec![info()]);
        assert!(!old.exists() && d.join(file_name("ab12", 2)).is_file());
        // one player in the old file of the original game, whose one player slot is free
        let old = d.join("suspend.bin");
        write_raw(&old, &id("", "dead"), &one(), b"state").unwrap();
        assert_eq!(look_in(&d, "", &id("", "dead")).0, vec![one()]);
        assert!(!old.exists());
        // an old file whose slot is taken, and a damaged one, are deleted; the slot stays
        put(&d, "ab12", &one());
        write_raw(&d.join("suspend-ab12.bin"), &me, &Info { stage: 20, ..one() }, b"x").unwrap();
        assert_eq!(look_in(&d, "ab12", &me).0, vec![one(), info()]);
        std::fs::write(d.join("suspend-ab12.bin"), b"junk").unwrap();
        migrate(&d, "ab12");
        assert!(!d.join("suspend-ab12.bin").exists());
        assert_eq!(look_in(&d, "ab12", &me).0.len(), 2);
    }

    #[test]
    fn only_a_local_running_game_can_be_suspended() {
        giana_core::skip_without_data!();
        let g = GameState::new_port_game();
        assert!(can_suspend(&g, false, false));
        // a network game, a game at a chosen stage
        assert!(!can_suspend(&g, true, false));
        assert!(!can_suspend(&g, false, true));
        // not at the title, not when the game is over
        let mut title = g.clone();
        title.screen = GameScreen::Title;
        assert!(!can_suspend(&title, false, false));
        let mut over = g.clone();
        over.game_over = true;
        assert!(!can_suspend(&over, false, false));
        let mut legacy = g;
        legacy.legacy_model = true;
        assert!(!can_suspend(&legacy, false, false));
    }

    #[test]
    fn a_real_game_goes_into_a_slot_and_comes_back() {
        giana_core::skip_without_data!();
        let d = dir("game");
        let f = d.join("a.bin");
        let mut g = GameState::new_port_game_two();
        for _ in 0..200 {
            g.update(&giana_core::state::ControllerInput::new());
        }
        let me = Identity::current("");
        write_to(&f, &me, &g).unwrap();
        let size = std::fs::metadata(&f).unwrap().len();
        eprintln!("a slot is {size} bytes");
        assert!(size < 150_000, "a slot of {size} bytes is too big");
        assert_eq!(peek(&f, &me), Peek::Valid(Info { players: 2, stage: g.stage, score: 0, player: 0, other_score: 0 }));
        let (back, info) = take(&f, &me).unwrap();
        assert_eq!(info.players, 2);
        assert_eq!(snapshot::save(&back), snapshot::save(&g));
        assert!(!f.exists());
    }

    #[test]
    fn a_two_player_game_is_still_there_and_exact_after_a_one_player_game_was_suspended() {
        giana_core::skip_without_data!();
        let d = dir("real-both");
        let me = Identity::current("");
        let play = |mut g: GameState, frames: usize| {
            for i in 0..frames {
                let mut c = giana_core::state::ControllerInput::new();
                c.fire = i < 40;
                c.right = i >= 40;
                g.update(&c);
            }
            g
        };
        let two = play(GameState::new_port_game_two(), 300);
        let f2 = d.join(file_name("", 2));
        write_to(&f2, &me, &two).unwrap();
        // a new one player game is played and suspended
        let one = play(GameState::new_port_game(), 200);
        let f1 = d.join(file_name("", 1));
        write_to(&f1, &me, &one).unwrap();
        let (found, rejected) = look_in(&d, "", &me);
        assert_eq!((found.iter().map(|i| i.players).collect::<Vec<_>>(), rejected.len()), (vec![1, 2], 0));
        // continuing the two player game: exactly what was saved, and the one player slot stays
        let (back, _) = take(&f2, &me).unwrap();
        assert_eq!(snapshot::save(&back), snapshot::save(&two));
        assert!(f1.is_file() && !f2.exists());
        let (back, _) = take(&f1, &me).unwrap();
        assert_eq!(snapshot::save(&back), snapshot::save(&one));
    }
}
