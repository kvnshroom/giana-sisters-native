//! One interface over the two disk image formats (`.g64`, `.d64`) and the content check that decides whether a disk
//! holds the original game.
//!
//! A disk counts as the original when the raw directory files `1` (program) and `2` (title) hash to the expected
//! values, whatever the container: another dump of the same disk, or the same files in a `.d64`, passes; a cracked or
//! modified version does not. Only data is parsed here; nothing from the disk is ever executed.

use crate::g64::{self, DirEntry};
use crate::sha256::sha256_hex;
use crate::{d64, image, GAME_FILE1_SHA256, GAME_FILE2_SHA256, KNOWN_IMAGES};
use std::fmt;

/// A disk image from which directory entries and files can be read.
pub trait DiskImage {
    /// Directory entries (all file types, deleted entries skipped).
    fn directory(&self) -> Result<Vec<DirEntry>, String>;
    /// Contents of a directory entry's file (PRG: includes the two byte load address).
    fn read_entry(&self, e: &DirEntry) -> Result<Vec<u8>, String>;

    /// Contents of the first PRG file with this name (including its two byte load address).
    fn read_prg(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self
            .directory()?
            .into_iter()
            .find(|e| e.name == name && e.file_type & 0x07 == 2)
            .ok_or_else(|| format!("file \"{name}\" not found in the directory"))?;
        self.read_entry(&e)
    }
}

impl DiskImage for g64::Disk {
    fn directory(&self) -> Result<Vec<DirEntry>, String> {
        g64::Disk::directory(self)
    }
    fn read_entry(&self, e: &DirEntry) -> Result<Vec<u8>, String> {
        g64::Disk::read_entry(self, e)
    }
}

impl DiskImage for d64::D64 {
    fn directory(&self) -> Result<Vec<DirEntry>, String> {
        d64::D64::directory(self)
    }
    fn read_entry(&self, e: &DirEntry) -> Result<Vec<u8>, String> {
        d64::D64::read_entry(self, e)
    }
}

/// Why a file is not accepted as the original game disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    /// neither a G64 nor a D64 image, or its structure cannot be read
    NotADisk(String),
    /// a disk image, but the game's files `1` and `2` are not on it
    NoGameFiles(String),
    /// the game's files are there but differ from the original (cracked, trained, hacked)
    Modified { file1: String, file2: String },
    /// the files are listed but their sectors cannot be read (damaged dump)
    Unreadable(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::NotADisk(e) => write!(f, "not a G64 or D64 disk image ({e})"),
            CheckError::NoGameFiles(e) => write!(f, "this disk does not hold the game files ({e})"),
            CheckError::Modified { file1, file2 } => write!(
                f,
                "modified or cracked version: the game files differ from the original \
                 (file 1 SHA-256 {file1}, file 2 SHA-256 {file2}; expected {GAME_FILE1_SHA256} and {GAME_FILE2_SHA256})"
            ),
            CheckError::Unreadable(e) => write!(f, "the game files cannot be read from this disk ({e})"),
        }
    }
}

/// Smallest and largest size of a D64 file, with or without error bytes (35 to 40 tracks).
pub const D64_SIZE_RANGE: std::ops::RangeInclusive<usize> = 174_848..=197_376;

/// Parses either format (detected by the signature, then by the size).
pub fn parse(bytes: &[u8]) -> Result<Box<dyn DiskImage>, CheckError> {
    if bytes.starts_with(b"GCR-1541") {
        return g64::Disk::parse(bytes).map(|d| Box::new(d) as Box<dyn DiskImage>).map_err(CheckError::NotADisk);
    }
    d64::D64::parse(bytes).map(|d| Box::new(d) as Box<dyn DiskImage>).map_err(CheckError::NotADisk)
}

/// The raw files `1` and `2` of the original disk, as read from it.
pub struct GameFiles {
    pub program: Vec<u8>,
    pub title: Vec<u8>,
}

/// The content check: reads the raw directory files `1` and `2` from the disk and compares their SHA-256 with the
/// original. Runs before anything else touches the data.
pub fn read_game_files(disk: &dyn DiskImage) -> Result<GameFiles, CheckError> {
    let dir = disk.directory().map_err(CheckError::NotADisk)?;
    let find = |name: &str| dir.iter().find(|e| e.name == name && e.file_type & 0x07 == 2);
    let (Some(e1), Some(e2)) = (find(image::PROGRAM_FILE), find("2")) else {
        return Err(CheckError::NoGameFiles("file \"1\" or \"2\" is missing in the directory".into()));
    };
    let program = disk.read_entry(e1).map_err(CheckError::Unreadable)?;
    let title = disk.read_entry(e2).map_err(CheckError::Unreadable)?;
    let (h1, h2) = (sha256_hex(&program), sha256_hex(&title));
    if h1 != GAME_FILE1_SHA256 || h2 != GAME_FILE2_SHA256 {
        return Err(CheckError::Modified { file1: h1, file2: h2 });
    }
    Ok(GameFiles { program, title })
}

/// Parses `bytes` and runs the content check. Returns the parsed disk too, for reading the other files.
pub fn check_content(bytes: &[u8]) -> Result<(Box<dyn DiskImage>, GameFiles), CheckError> {
    let disk = parse(bytes)?;
    let files = read_game_files(disk.as_ref())?;
    Ok((disk, files))
}

/// What a file found while searching is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identity {
    /// the original game (known image, or the game files are byte-identical)
    Original,
    /// has the game's files, but modified
    Modified,
    /// anything else
    Other,
}

/// Cheap identification for a folder search: known whole-file hash first, then the content check. Never runs
/// the 6502 emulation.
pub fn identify(bytes: &[u8]) -> Identity {
    let hash = sha256_hex(bytes);
    if KNOWN_IMAGES.iter().any(|k| k.sha256 == hash) {
        return Identity::Original;
    }
    match check_content(bytes) {
        Ok(_) => Identity::Original,
        Err(CheckError::Modified { .. }) => Identity::Modified,
        Err(_) => Identity::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_never_panics() {
        // xorshift, no dependencies
        let mut x = 0x2545F4914F6CDD1Du64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for len in [0usize, 1, 7, 8, 11, 12, 13, 100, 5000, 174_847, 174_848, 175_531, 196_608, 197_376] {
            for with_sig in [false, true] {
                let mut v: Vec<u8> = (0..len).map(|_| next() as u8).collect();
                if with_sig && len >= 8 {
                    v[..8].copy_from_slice(b"GCR-1541");
                }
                assert!(check_content(&v).is_err());
                assert_eq!(identify(&v), Identity::Other);
            }
        }
    }

    #[test]
    fn g64_header_with_hostile_offsets_is_rejected() {
        // every track offset points at the very end of the file, with a huge track count and length
        let mut v = b"GCR-1541".to_vec();
        v.extend_from_slice(&[0, 84, 0xF8, 0x1E]);
        for _ in 0..84 {
            v.extend_from_slice(&u32::MAX.to_le_bytes());
        }
        v.extend_from_slice(&[0xFF; 64]);
        assert!(parse(&v).is_err());
        let mut w = v.clone();
        w[12..16].copy_from_slice(&(12u32 + 4 * 84).to_le_bytes());
        w.truncate(12 + 4 * 84 + 3);
        assert!(parse(&w).is_err());
    }

    #[test]
    fn directory_and_sector_chain_loops_end() {
        // a directory whose next-sector link points at itself
        let mut img = d64::build_d64(&[("A", &[1, 8, 9])]);
        let dir = 17 * 21 * 256; // track 18 starts after 17 tracks of 21 sectors
        let d = dir + 256; // sector 1
        img[d] = 18;
        img[d + 1] = 1;
        let disk = parse(&img).unwrap();
        assert!(disk.directory().unwrap_err().contains("loops"));
        assert!(matches!(read_game_files(disk.as_ref()), Err(CheckError::NotADisk(_))));

        // a file whose chain points back at its own first sector
        let mut img = d64::build_d64(&[("1", &vec![7u8; 600])]);
        img[0] = 1; // track 1 sector 0 -> track 1 sector 0
        img[1] = 0;
        let disk = parse(&img).unwrap();
        let e = disk.directory().unwrap().remove(0);
        assert!(disk.read_entry(&e).unwrap_err().contains("loops"));
    }

    #[test]
    fn disk_without_the_game_files() {
        let img = d64::build_d64(&[("HELLO", &[1, 8, 0]), ("1", &[1, 8, 0])]);
        assert!(matches!(check_content(&img), Err(CheckError::NoGameFiles(_))));
        assert_eq!(identify(&img), Identity::Other);
    }

    #[test]
    fn game_files_with_other_content_are_modified() {
        let img = d64::build_d64(&[("1", &[0x20, 0x08, 1, 2, 3]), ("2", &[0, 4, 5, 6])]);
        let Err(e) = check_content(&img) else { panic!() };
        assert!(matches!(e, CheckError::Modified { .. }), "{e}");
        assert!(e.to_string().contains("modified or cracked"));
        assert_eq!(identify(&img), Identity::Modified);
    }

    #[test]
    fn truncated_real_image_is_an_error_not_a_panic() {
        let Ok(g64) = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../orig/Great_Giana_Sisters_The.g64")) else {
            eprintln!("skipped: orig/ not present");
            return;
        };
        let mut n = g64.len();
        while n > 0 {
            let _ = check_content(&g64[..n]);
            n = if n > 20_000 { n - 20_011 } else { n / 2 };
        }
        assert!(check_content(&g64[..g64.len() / 2]).is_err());
        assert!(check_content(&g64).is_ok());
    }
}
