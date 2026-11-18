//! The 30th Anniversary preview ("Giana Sisters 30th Anniversary Extended Preview", demo 2.0, engine family C) in the
//! app: the check of the player's `.d64`, the high-score file, the input of a frame and the run of the native game
//! (`giana_engine_c::game::C30Game`). **Local only**: the disk stays in the module store of this computer and is never
//! offered for sharing or sent over the network (see `giana_engine_c`). See `docs/rom-hack-family-c.md` 7.13.

use crate::bindings::{config_path, Bindings, Held, Source};
use crate::menu::Menu;
use giana_core::state::ControllerInput;
use giana_engine_c::game::{C30Game, Input};
use giana_engine_c::hiscore::ScoreStore;
use giana_engine_c::Image;
use giana_engine_c::game::Shot;
use giana_render::framebuffer::Framebuffer;
use sdl2::keyboard::Keycode;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// SHA-256 of the unpacked program of the known release (CSDb 168405, "Giana 30A Demo 2.0", 174,848 byte `.d64`). The
/// check is on the program, not the disk, so a copy with another directory or error bytes is taken as well.
pub const DEMO_2_0: &str = "f811b12dc5b393943c0ffb7416bc5485de9b73f970a978df234a83e78b30917e";

/// The store id of the preview: the first 16 hex digits of [`DEMO_2_0`].
pub fn store_id() -> &'static str {
    &DEMO_2_0[..16]
}

/// Name, author and version of the store entry.
pub const NAME: &str = "30TH ANNIVERSARY";
pub const AUTHOR: &str = "MERLIN, MONTY MOLE";
pub const VERSION: &str = "2.0";
pub const DESCRIPTION: &str = "THE FAN SEQUEL OF 2018 BY THE SEVEN-A-THREE, 16 STAGES";

/// Hex of a SHA-256.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads and checks the preview's disk: it unpacks, and its program is the known release.
pub fn check_disk(d64: &[u8]) -> Result<Image, String> {
    let image = Image::from_d64(d64).map_err(|_| "this is not the 30th Anniversary disk image".to_string())?;
    if sha256_hex(&image.ram) != DEMO_2_0 {
        return Err("this version of the preview is not known. Use the demo 2.0 disk image".to_string());
    }
    Ok(image)
}

/// The file of the preview's high scores (the 150 bytes the original saves to its disk as `hid2!g30a`), next to the
/// settings.
pub fn scores_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("c30a_scores.bin"))
}

/// The high scores in a file of the settings folder.
pub struct FileScores {
    pub path: Option<PathBuf>,
    /// the last save could not be written
    pub failed: bool,
}

impl ScoreStore for FileScores {
    fn load(&mut self) -> Option<Vec<u8>> {
        self.path.as_ref().and_then(|p| std::fs::read(p).ok()).filter(|b| b.len() == giana_engine_c::hiscore::FILE_LEN)
    }

    fn save(&mut self, bytes: &[u8]) {
        let Some(path) = &self.path else { return };
        if let Err(e) = crate::fsutil::write_atomic_bytes(path, bytes) {
            eprintln!("giana-app: could not save the 30th Anniversary high scores to {}: {e}", path.display());
            self.failed = true;
        }
    }
}

/// C64 keys the preview reads (matrix column, row), from the PC keys that stand for them. `P` opens the preview's own
/// pause menu (save and quit, suicide); Escape stays the app's overlay, so RUN/STOP is not mapped.
const KEYS: [(Keycode, (u8, u8)); 7] = [
    (Keycode::Num1, (7, 0)),
    (Keycode::Kp1, (7, 0)),
    (Keycode::Num2, (7, 3)),
    (Keycode::Kp2, (7, 3)),
    (Keycode::H, (3, 5)),
    (Keycode::Space, (7, 4)),
    (Keycode::P, (5, 1)),
];

/// The PETSCII character a key types in the name entry (letters, digits, space, DEL, RETURN).
pub fn petscii_for(key: Keycode) -> Option<u8> {
    let name = key.name();
    let c = match key {
        Keycode::Backspace | Keycode::Delete => return Some(0x14),
        Keycode::Return | Keycode::KpEnter => return Some(0x0D),
        Keycode::Space => return Some(0x20),
        _ if name.chars().count() == 1 => name.chars().next()?,
        _ => name.strip_prefix("Keypad ").filter(|n| n.len() == 1)?.chars().next()?,
    };
    (c.is_ascii_alphanumeric()).then(|| c.to_ascii_uppercase() as u8)
}

/// The input of a frame: joystick port 2 from the bindings (jump is up, as on the C64), the keys the preview reads,
/// the item action as the space bar, and what was typed.
pub fn input_for(held: &Held, bindings: &Bindings, typed: Vec<u8>) -> Input {
    let mut c = ControllerInput::new();
    bindings.apply(held, &mut c);
    let mut joy = 0u8;
    for (on, bit) in [(c.jump, 0x01), (c.down, 0x02), (c.left, 0x04), (c.right, 0x08), (c.fire, 0x10)] {
        if on {
            joy |= bit;
        }
    }
    let mut keys: Vec<(u8, u8)> = Vec::new();
    for (k, pos) in KEYS {
        if held.is_held(Source::Key(k)) && !keys.contains(&pos) {
            keys.push(pos);
        }
    }
    if c.item && !keys.contains(&(7, 4)) {
        keys.push((7, 4));
    }
    Input { joy, keys, typed }
}

/// A running preview: the game, its high scores and the overlay of the app.
pub struct C30Run {
    pub game: C30Game,
    pub scores: FileScores,
    /// the settings overlay (the pause menu of the app), over the paused picture
    pub overlay: Option<Menu>,
    /// the picture of the last frame
    pub fb: Framebuffer,
    /// the SID registers of the last frame
    pub sid: [u8; 25],
    /// frames run
    pub frames: u64,
    /// the pictures of the last two frames (older first), for SMOOTH MOTION ([`C30Run::render_between`])
    pub shots: [Option<Shot>; 2],
}

/// Why the preview stopped by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// the game reached a part that has no native port yet (its address)
    NotPorted(u16),
    /// the native game stopped on an error (the message)
    Failed(String),
}

impl C30Run {
    /// SMOOTH MOTION: the picture `t` (0..1) of the way from the frame before the last to the last one into `fb`
    /// (the playfield's scroll and the sprites in between, `Shot::render_between`); `false` when there are not two
    /// frames yet (then `fb` is unchanged).
    pub fn render_between(&self, t: f64, fb: &mut Framebuffer) -> bool {
        match &self.shots {
            [Some(prev), Some(curr)] => {
                Shot::render_between(prev, curr, t, fb);
                true
            }
            _ => false,
        }
    }

    /// Power-on of the preview from its disk, with the high scores of `scores` (`None`: no file, the disk's tables).
    pub fn new(d64: &[u8], scores: Option<PathBuf>) -> Result<C30Run, String> {
        let image = check_disk(d64)?;
        let mut store = FileScores { path: scores, failed: false };
        // the frame driver's fallback for routines without a native port: none (`C30Game::new`)
        let game = C30Game::new(&image, store.load());
        Ok(C30Run { game, scores: store, overlay: None, fb: Framebuffer::new(), sid: [0; 25], frames: 0, shots: [None, None] })
    }

    /// Power-on from the disk in the module store.
    pub fn from_file(disk: &Path, scores: Option<PathBuf>) -> Result<C30Run, String> {
        let d64 = std::fs::read(disk).map_err(|e| format!("cannot read {}: {e}", disk.display()))?;
        C30Run::new(&d64, scores)
    }

    /// Runs one video frame (50 Hz): the picture into [`C30Run::fb`], the SID registers into [`C30Run::sid`], a save of
    /// the high scores into the file.
    pub fn frame(&mut self, input: &Input) -> Result<(), Stop> {
        let (game, fb) = (&mut self.game, &mut self.fb);
        let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let out = game.step(input);
            out.render(fb);
            (out.sid, out.shot())
        }));
        match step {
            Ok((sid, shot)) => {
                self.sid = sid;
                self.shots[0] = self.shots[1].take();
                self.shots[1] = Some(shot);
            }
            Err(e) => {
                let why = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                return Err(Stop::Failed(why));
            }
        }
        self.frames += 1;
        if let Some(bytes) = self.game.take_save() {
            self.scores.save(&bytes);
        }
        if let giana_engine_c::flow::Pc::Unported(addr) = self.game.main.pc {
            return Err(Stop::NotPorted(addr));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_gives_petscii() {
        assert_eq!(petscii_for(Keycode::A), Some(b'A'));
        assert_eq!(petscii_for(Keycode::Num7), Some(b'7'));
        assert_eq!(petscii_for(Keycode::Kp3), Some(b'3'));
        assert_eq!(petscii_for(Keycode::Backspace), Some(0x14));
        assert_eq!(petscii_for(Keycode::Return), Some(0x0D));
        assert_eq!(petscii_for(Keycode::F1), None);
        assert_eq!(petscii_for(Keycode::Escape), None);
    }

    #[test]
    fn joystick_and_keys_from_the_bindings() {
        let b = Bindings::default();
        let mut held = Held::default();
        let jump = b.keys[3][0].unwrap();
        let fire = b.keys[4][0].unwrap();
        held.press(Source::Key(jump));
        held.press(Source::Key(fire));
        held.press(Source::Key(Keycode::Num2));
        let i = input_for(&held, &b, vec![b'A']);
        assert_eq!(i.joy, 0x11);
        assert!(i.keys.contains(&(7, 3)));
        assert_eq!(i.typed, vec![b'A']);
    }

    #[test]
    fn a_disk_that_is_not_the_preview_is_refused() {
        assert!(check_disk(&[0u8; 1000]).is_err());
    }

    #[test]
    fn the_preview_disk_is_known() {
        let Ok(path) = std::env::var("GIANA_C30_DISK") else {
            eprintln!("skipped: GIANA_C30_DISK is not set");
            return;
        };
        let d64 = std::fs::read(path).unwrap();
        let image = Image::from_d64(&d64).unwrap();
        assert_eq!(sha256_hex(&image.ram), DEMO_2_0);
        assert!(check_disk(&d64).is_ok());
    }
}
