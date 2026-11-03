//! Applying a module to the player's base data.

use crate::bps;
use crate::code_ranges::code_changes;
use crate::container::Module;
use giana_extract::sha256::sha256_hex;

/// Result of applying a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// Patched `ingame.bin` (65536 bytes).
    pub ingame: Vec<u8>,
    /// Patched title program `f_2.prg`.
    pub f2: Vec<u8>,
    /// First 16 hex digits of SHA-256 over `ingame ‖ f2`.
    pub id: String,
    /// Changed ranges `(start, length)` of `ingame` that overlap code the port implements itself.
    pub code_changes: Vec<(usize, usize)>,
}

/// Module id of a patched pair of files.
pub fn module_id(ingame: &[u8], f2: &[u8]) -> String {
    let mut all = ingame.to_vec();
    all.extend_from_slice(f2);
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
    let code_changes = code_changes(base_ingame, &ingame);
    let id = module_id(&ingame, &f2);
    Ok(Applied { ingame, f2, id, code_changes })
}
