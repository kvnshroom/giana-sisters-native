//! The original game data, loaded at run time from a data directory. The program itself contains none of it.
//!
//! The directory is made by `giana-extract` (the app does that on first start) from the user's own disk image; see
//! `tools/data/manifest.txt` for the files. [`install`] sets the process-wide copy once at start-up, everything else
//! reads it through [`get`] (and the helpers in `image`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Name of the folder inside the platform's data location.
pub const APP_DIR: &str = "giana-native";

pub struct GameData {
    /// 64K program image, file offset == C64 address
    pub ingame: Vec<u8>,
    pub collision_table: Vec<u8>,
    pub colortable: Vec<u8>,
    pub stage1_map: Vec<u8>,
    pub stage_palettes: Vec<u8>,
    /// title picture in Koala layout (10001 bytes)
    pub title_koala: Vec<u8>,
}

/// File name and required size of every file the game reads at run time.
const FILES: [(&str, usize); 6] = [
    ("ingame.bin", 65536),
    ("collision_table.bin", 256),
    ("colortable.bin", 256),
    ("stage1_map.bin", 5120),
    ("stage_palettes.bin", 132),
    ("title_koala.bin", 10001),
];

impl GameData {
    /// Reads the files of `dir` and checks their sizes. (The app additionally checks the SHA-256 before it trusts a folder.)
    pub fn load_dir(dir: &Path) -> Result<GameData, String> {
        let mut v: Vec<Vec<u8>> = Vec::new();
        for (name, size) in FILES {
            let p = dir.join(name);
            let d = std::fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            if d.len() != size {
                return Err(format!("{} has {} bytes, expected {size}", p.display(), d.len()));
            }
            v.push(d);
        }
        let mut it = v.into_iter();
        let mut next = || it.next().unwrap();
        Ok(GameData {
            ingame: next(),
            collision_table: next(),
            colortable: next(),
            stage1_map: next(),
            stage_palettes: next(),
            title_koala: next(),
        })
    }
}

/// Where the app stores the data it extracts: `$XDG_DATA_HOME/giana-native/data`, `~/.local/share/giana-native/data`,
/// or `%APPDATA%\giana-native\data` on Windows (`~/Library/Application Support/giana-native/data` on macOS).
pub fn user_data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else if let Some(x) = env("XDG_DATA_HOME").filter(|p| p.is_absolute()) {
        x
    } else {
        env("HOME")?.join(".local/share")
    };
    Some(base.join(APP_DIR).join("data"))
}

/// Folders that may hold the data, best first: the command line choice, the user data folder, `data/` next to the
/// executable, `data/` in the repository (development: the working directory, the folders above the executable such
/// as `target/release/..`, and the source tree of a debug build).
pub fn candidate_dirs(cli: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Some(c) = cli {
        v.push(c.to_path_buf());
    }
    v.extend(user_data_dir());
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..4 {
            let Some(d) = dir else { break };
            v.push(d.join("data"));
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    v.push(PathBuf::from("data"));
    #[cfg(debug_assertions)]
    v.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"));
    v.dedup();
    v
}

static DATA: OnceLock<GameData> = OnceLock::new();

/// Makes `data` the process-wide game data. Returns false when data was already installed.
pub fn install(data: GameData) -> bool {
    DATA.set(data).is_ok()
}

pub fn is_installed() -> bool {
    DATA.get().is_some()
}

/// True when game data is installed or can be found (tests use this through [`skip_without_data!`] so that they pass
/// on machines without the original game, such as CI).
pub fn available() -> bool {
    is_installed() || candidate_dirs(None).iter().any(|d| GameData::load_dir(d).is_ok())
}

/// In a test: return early (and say so) when the original game data is not available.
#[macro_export]
macro_rules! skip_without_data {
    () => {
        if !$crate::data::available() {
            eprintln!("skipped: the original game data is not available");
            return;
        }
    };
}

/// The installed game data. When nothing was installed (unit tests, tools), the first usable folder of
/// [`candidate_dirs`] is loaded; if there is none this panics with an explanation.
pub fn get() -> &'static GameData {
    DATA.get_or_init(|| {
        let dirs = candidate_dirs(None);
        for d in &dirs {
            if let Ok(g) = GameData::load_dir(d) {
                return g;
            }
        }
        panic!(
            "the original game data is not loaded: no usable data folder in {dirs:?}. Run the game once and give it your .g64, \
             or: ./tools/data/setup.sh <your .g64>"
        );
    })
}
