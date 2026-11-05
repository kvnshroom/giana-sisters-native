//! Applying a module to the player's base data.

use crate::bps;
use crate::code_ranges::code_changes;
use crate::container::{ModFile, Module};
use crate::family::{covered_ranges, detect_family, subtract_covered, Family};
use giana_extract::sha256::sha256_hex;

/// Result of applying a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// Patched `ingame.bin` (65536 bytes).
    pub ingame: Vec<u8>,
    /// Patched title program `f_2.prg`.
    pub f2: Vec<u8>,
    /// First 16 hex digits of SHA-256 over `ingame ‖ f2` (for a module with disk files see [`module_id_with_files`]).
    pub id: String,
    /// Changed ranges `(start, length)` of `ingame` that overlap code the port implements itself.
    pub code_changes: Vec<(usize, usize)>,
    /// Family of fan hacks the patched image belongs to (the app switches the matching features of the engine on).
    pub family: Family,
    /// Disk files the hack loads while playing (format 2), sorted by name. Empty for the original engine.
    pub files: Vec<ModFile>,
    /// Engine tag of the module (`None` = original engine, `Some("b")` = 7A3 construction-set engine).
    pub engine: Option<String>,
}

impl Applied {
    /// [`Applied::code_changes`] without the changes the port reproduces for the module's [`Family`] (those play as intended).
    pub fn uncovered_code_changes(&self) -> Vec<(usize, usize)> {
        subtract_covered(&self.code_changes, covered_ranges(self.family))
    }
}

/// Module id of a patched pair of files.
pub fn module_id(ingame: &[u8], f2: &[u8]) -> String {
    let mut all = ingame.to_vec();
    all.extend_from_slice(f2);
    sha256_hex(&all)[..16].to_string()
}

/// Module id when the module carries disk files: [`module_id`] over `ingame ‖ f2 ‖ files`, where every file (sorted by
/// name) adds `"\0FILE"`, the name length (1 byte), the name, the data length (u32 LE) and the data. The engine tag is
/// appended last (`"\0ENGINE"` and the tag) when there is one. Without files and engine this is exactly [`module_id`].
pub fn module_id_with_files(ingame: &[u8], f2: &[u8], files: &[ModFile], engine: Option<&str>) -> String {
    if files.is_empty() && engine.is_none() {
        return module_id(ingame, f2);
    }
    let mut all = ingame.to_vec();
    all.extend_from_slice(f2);
    let mut sorted: Vec<&ModFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for f in sorted {
        all.extend_from_slice(b"\0FILE");
        all.push(f.name.len() as u8);
        all.extend_from_slice(&f.name);
        all.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
        all.extend_from_slice(&f.data);
    }
    if let Some(e) = engine {
        all.extend_from_slice(b"\0ENGINE");
        all.extend_from_slice(e.as_bytes());
    }
    sha256_hex(&all)[..16].to_string()
}

/// Checks `f_2.prg`: load address `$0801`, fits into 64K.
pub fn check_f2(f2: &[u8]) -> Result<(), String> {
    if f2.len() < 3 || f2[0] != 0x01 || f2[1] != 0x08 {
        return Err("the title program does not start with the load address $0801".into());
    }
    if 0x0801 + f2.len() - 2 > 0x1_0000 {
        return Err("the title program does not fit into 64K".into());
    }
    Ok(())
}

/// First and last byte of the disk file `GAME PART 2` in memory: the sound driver (jump table `$41C0`, code `$4346..$4A79`,
/// tables) and the music data it reads (`$4AE8..` and on, up to `$5FFF`).
pub const SOUND_DRIVER: (usize, usize) = (0x41C0, 0x5FFF);

/// A hack made with the 7A3 construction set (the example game of the C-Set V3.12) is stored as a memory snapshot taken when the
/// game program starts, before its disk file `GAME PART 2` is loaded: the whole driver code area (`$4346..$4A79`) is empty. That
/// file carries the original's driver and music data (checked against the file on the C-Set disk: it equals the original's
/// `$41C0..$5FFF` except for the driver's run-time variables), so the area gets the base image's bytes back. A module whose driver
/// code area holds something is left alone. Returns whether the area was completed.
pub fn complete_sound_driver(base: &[u8], ingame: &mut [u8]) -> bool {
    if ingame[0x4346..=0x4A79].iter().any(|&b| b != 0) {
        return false;
    }
    ingame[SOUND_DRIVER.0..=SOUND_DRIVER.1].copy_from_slice(&base[SOUND_DRIVER.0..=SOUND_DRIVER.1]);
    true
}

/// Applies the module's patches to the base files and checks the results.
pub fn apply(module: &Module, base_ingame: &[u8], base_f2: &[u8]) -> Result<Applied, String> {
    if module.meta.base != sha256_hex(base_ingame) {
        return Err("this module was made for a different version of the game data".into());
    }
    let ingame = match &module.ingame_patch {
        Some(p) => bps::decode(base_ingame, p).map_err(|e| format!("ingame patch: {e}"))?,
        None => base_ingame.to_vec(),
    };
    if ingame.len() != 0x1_0000 {
        return Err(format!("the patched program image has {} bytes, expected 65536", ingame.len()));
    }
    let f2 = match &module.title_patch {
        Some(p) => bps::decode(base_f2, p).map_err(|e| format!("title patch: {e}"))?,
        None => base_f2.to_vec(),
    };
    check_f2(&f2)?;
    let mut files = module.files.clone();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let engine = module.meta.engine.clone();
    let id = module_id_with_files(&ingame, &f2, &files, engine.as_deref());
    let family = detect_family(&ingame);
    let mut ingame = ingame;
    complete_sound_driver(base_ingame, &mut ingame);
    let code_changes = code_changes(base_ingame, &ingame);
    if engine.as_deref() == Some(crate::tnw::ENGINE_T) {
        // family T: the stored files refer to the original stages (the id above covers them as they are stored)
        files = crate::tnw::expand_files(&files, base_ingame)?;
    }
    Ok(Applied { ingame, f2, id, code_changes, family, files, engine })
}
