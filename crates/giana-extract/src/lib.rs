//! Extractor for the program image of "The Great Giana Sisters" (C64) from the user's own `.g64` or `.d64` disk image.
//!
//! The disk holds the program as the plain PRG file `1` (load address `$0820`, 63441 bytes). The game does
//! not decrypt or unpack it: the 64K image the port embeds is that file placed at `$0820`, followed by
//! the explicitly listed patches. No C64 ROM and no CPU emulation is needed. See `image` for what is and is not
//! part of the result.

pub mod d64;
pub mod derive;
pub mod disk;
pub mod g64;
pub mod image;
pub mod mini6502;
pub mod sha256;

pub use derive::{missing_optional, verify_dir, write_data_dir, DataFile, MANIFEST};
pub use disk::{check_content, identify, CheckError, DiskImage, Identity};

use sha256::sha256_hex;

/// The identity of the game: SHA-256 of the raw directory file `1` (the program, 63443 bytes) and of file `2` (the
/// title picture, 20088 bytes) as read from the disk. Any dump, in any container, with these two files is the original.
pub const GAME_FILE1_SHA256: &str = "0f1a720961fa52102fcfd52e46e40df07e5e307328ad087d55cc00506f904880";
pub const GAME_FILE2_SHA256: &str = "389ec667bab8bf119facad9706191d0ac79103950f3fa45dd3fe74e20c2676e0";

/// A whole disk image file that is known to hold the original files (a fast path for searching only; the content
/// check decides everything else).
pub struct KnownImage {
    pub name: &'static str,
    /// SHA-256 of the whole file
    pub sha256: &'static str,
}

pub const KNOWN_IMAGES: &[KnownImage] = &[
    KnownImage {
        name: "The Great Giana Sisters (G64, 35 tracks + protection tracks 36..42)",
        sha256: "5ce29ce04786eca6518fb08dfe659abb3eee079b4135a3f7606f9d17a501ec77",
    },
    KnownImage {
        name: "The Great Giana Sisters [Time Warp 1987] (PAL) (r1) (C64 Preservation Project G64)",
        sha256: "59432fc6ebf5b6b6c3c594f00e733695b438ebe86f39218b955a8383379dea1a",
    },
];

/// The game version the extraction is built and verified for.
pub struct KnownVersion {
    pub name: &'static str,
    /// SHA-256 of the unpatched image produced from it (self check of the extraction).
    pub unpatched_sha256: &'static str,
}

pub const KNOWN_VERSIONS: &[KnownVersion] = &[KnownVersion {
    name: "The Great Giana Sisters (original disk)",
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

/// Checks the disk image (`.g64` or `.d64`), reads the program file from it and builds both images.
///
/// The content check (SHA-256 of the raw files `1` and `2`) comes first, so nothing of an unknown disk is ever
/// unpacked or run on the 6502 emulation.
pub fn extract(disk_image: &[u8]) -> Result<Extracted, String> {
    let (disk, files) = check_content(disk_image).map_err(|e| e.to_string())?;
    let version = &KNOWN_VERSIONS[0];
    let unpatched = image::build_unpatched(&files.program)?;
    let got = sha256_hex(&unpatched);
    if got != version.unpatched_sha256 {
        return Err(format!(
            "internal check failed: extracted image has SHA-256 {got}, expected {}",
            version.unpatched_sha256
        ));
    }
    let patched = image::apply_patches(&unpatched);
    let atg = disk.read_prg("ATG").ok();
    Ok(Extracted { version, unpatched, patched, title_prg: files.title, atg })
}

/// Everything at once: checks the disk image, extracts the program and derives all files of the manifest (verified).
pub fn extract_all(disk_image: &[u8]) -> Result<Vec<DataFile>, String> {
    let ex = extract(disk_image)?;
    derive::derive_all_with(ex.patched, ex.title_prg, ex.atg)
}
