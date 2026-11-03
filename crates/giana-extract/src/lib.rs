//! Extractor for the program image of "The Great Giana Sisters" (C64) from the user's own `.g64` disk image.
//!
//! The disk holds the program as the plain PRG file `1` (load address `$0820`, 63441 bytes). The game does
//! not decrypt or unpack it: the 64K image the port embeds is that file placed at `$0820`, followed by
//! the explicitly listed patches. No C64 ROM and no CPU emulation is needed. See `image` for what is and is not
//! part of the result.

pub mod d64;
pub mod derive;
pub mod g64;
pub mod image;
pub mod mini6502;
pub mod sha256;

pub use derive::{missing_optional, verify_dir, write_data_dir, DataFile, MANIFEST};

use sha256::sha256_hex;

/// A disk image version that is known to extract correctly.
pub struct KnownVersion {
    pub name: &'static str,
    /// SHA-256 of the whole `.g64` file.
    pub g64_sha256: &'static str,
    /// SHA-256 of the unpatched image produced from it (self check of the extraction).
    pub unpatched_sha256: &'static str,
}

pub const KNOWN_VERSIONS: &[KnownVersion] = &[KnownVersion {
    name: "The Great Giana Sisters (G64, 35 tracks + protection tracks 36..42)",
    g64_sha256: "5ce29ce04786eca6518fb08dfe659abb3eee079b4135a3f7606f9d17a501ec77",
    unpatched_sha256: "5a64ea8d487890bb1913bbeaf7992b42770a8a111bdcfeda714eb28a9cbb2bd2",
}];

pub struct Extracted {
    pub version: &'static KnownVersion,
    pub unpatched: Vec<u8>,
    pub patched: Vec<u8>,
    /// disk file `2`: packed title picture (and title tune), unpacked by `derive`
    pub title_prg: Vec<u8>,
    /// disk file `ATG`: the high score table (optional: only the AUTHENTIC presentation needs it)
    pub atg: Option<Vec<u8>>,
}

/// Checks the input file against the list of known versions, reads the program file from the disk and builds
/// both images.
pub fn extract(g64: &[u8]) -> Result<Extracted, String> {
    if g64.len() < 12 || &g64[..8] != b"GCR-1541" {
        return Err("this is not a G64 disk image (the file does not start with GCR-1541)".into());
    }
    let hash = sha256_hex(g64);
    let version = KNOWN_VERSIONS.iter().find(|v| v.g64_sha256 == hash).ok_or_else(|| {
        let known: Vec<String> = KNOWN_VERSIONS.iter().map(|v| format!("  {}  {}", v.g64_sha256, v.name)).collect();
        format!(
            "unknown disk image version (SHA-256 {hash}).\nOnly these versions are supported:\n{}",
            known.join("\n")
        )
    })?;
    let disk = g64::Disk::parse(g64)?;
    let prg = disk.read_prg(image::PROGRAM_FILE)?;
    let unpatched = image::build_unpatched(&prg)?;
    let got = sha256_hex(&unpatched);
    if got != version.unpatched_sha256 {
        return Err(format!(
            "internal check failed: extracted image has SHA-256 {got}, expected {}",
            version.unpatched_sha256
        ));
    }
    let patched = image::apply_patches(&unpatched);
    let title_prg = disk.read_prg("2")?;
    let atg = disk.read_prg("ATG").ok();
    Ok(Extracted { version, unpatched, patched, title_prg, atg })
}

/// Everything at once: checks the disk image, extracts the program and derives all files of the manifest (verified).
pub fn extract_all(g64: &[u8]) -> Result<Vec<DataFile>, String> {
    let ex = extract(g64)?;
    derive::derive_all_with(ex.patched, ex.title_prg, ex.atg)
}
