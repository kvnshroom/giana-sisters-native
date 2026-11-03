//! Engine of "Giana Sisters 30th Anniversary Extended Preview" (The Seven-A-Three, 2018), "family C".
//!
//! This is a fan sequel that shares only its look with the original game, so it has its own engine. **It is local
//! only**: the program *is* the hack (it contains original material such as the music driver), so nothing of it is
//! ever indexed, shared or put into a patch. The player's own `.d64` is read at run time and unpacked in memory
//! ([`Image::from_d64`]); the module store may keep that disk (or the unpacked RAM image) locally, marked with the
//! engine tag [`ENGINE_C`], and must never offer it for sharing.
//!
//! Modules: [`decrunch`] (native port of the cruncher), [`stage`] (stage records and the 256x20 level buffer),
//! [`music`] (the Huelsbeck driver, recompiled by `tools/recompile6502.py`). Nothing here contains game bytes; every
//! test that needs the disk is gated on the environment variable `GIANA_C30_DISK`.

pub mod capture;
pub mod decrunch;
pub mod music;
pub mod render;
pub mod stage;

use giana_module::disk::Disk;

/// Engine tag of the 30A preview in `module.json` / the module store. Modules of this engine are never shareable.
pub const ENGINE_C: &str = "c";

/// The unpacked program: all of the C64's RAM as the game sees it when it starts.
pub struct Image {
    pub ram: Vec<u8>,
    /// Start address of the game (the `SYS` target of the BASIC stub).
    pub entry: u16,
}

impl Image {
    /// Reads the preview's program from the player's `.d64` (the disk has one big PRG, found by its `SYS 2064` start
    /// stub) and unpacks it.
    pub fn from_d64(d64: &[u8]) -> Result<Image, String> {
        let disk = Disk::parse(d64)?;
        let mut last = String::from("no PRG file on the disk");
        for e in disk.prgs()? {
            let prg = disk.read_entry(&e)?;
            match decrunch::decrunch(&prg) {
                Ok(d) => return Ok(Image { ram: d.ram, entry: d.entry }),
                Err(err) => last = err.to_string(),
            }
        }
        Err(last)
    }

    /// Reads a 64 KB RAM image (`$0000..$FFFF`, optionally with a two byte load address in front, as VICE saves it).
    pub fn from_ram(bytes: &[u8]) -> Result<Image, String> {
        let ram = match bytes.len() {
            0x1_0000 => bytes.to_vec(),
            0x1_0002 => bytes[2..].to_vec(),
            _ => return Err("a RAM image is 65536 bytes".into()),
        };
        Ok(Image { ram, entry: 0x1D10 })
    }

    pub fn byte(&self, a: u16) -> u8 {
        self.ram[a as usize]
    }

    pub fn word(&self, a: u16) -> u16 {
        u16::from_le_bytes([self.ram[a as usize], self.ram[a.wrapping_add(1) as usize]])
    }
}
