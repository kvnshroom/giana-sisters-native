//! One interface over the two disk image formats the boot emulation reads (`.d64`, `.g64`).

use giana_extract::d64::{self, D64};
use giana_extract::g64::{self, DirEntry};

pub enum Disk {
    D64(D64),
    G64(g64::Disk),
}

impl Disk {
    /// Detects the format by magic / size.
    pub fn parse(image: &[u8]) -> Result<Disk, String> {
        if image.starts_with(b"GCR-1541") {
            Ok(Disk::G64(g64::Disk::parse(image)?))
        } else {
            Ok(Disk::D64(D64::parse(image)?))
        }
    }

    pub fn directory(&self) -> Result<Vec<DirEntry>, String> {
        match self {
            Disk::D64(d) => d.directory(),
            Disk::G64(d) => d.directory(),
        }
    }

    pub fn read_entry(&self, e: &DirEntry) -> Result<Vec<u8>, String> {
        match self {
            Disk::D64(d) => d.read_entry(e),
            Disk::G64(d) => d64::follow_chain(&e.name, (e.track, e.sector), |t, s| d.sector(t, s)),
        }
    }

    /// PRG entries of the directory, in disk order.
    pub fn prgs(&self) -> Result<Vec<DirEntry>, String> {
        Ok(self.directory()?.into_iter().filter(|e| e.file_type & 0x07 == 2).collect())
    }

    /// Looks a file up like the drive does for `LOAD"name",8`: exact name, or a pattern with `?` and a trailing `*`.
    /// `$` (the directory) is not supported. Only PRG (and SEQ/USR) files match.
    pub fn find(&self, pattern: &[u8]) -> Result<Option<DirEntry>, String> {
        let pat: Vec<u8> = match pattern.iter().position(|&c| c == b',') {
            Some(i) => pattern[..i].to_vec(),
            None => pattern.to_vec(),
        };
        let pat: Vec<u8> = if pat.first() == Some(&b'0') && pat.get(1) == Some(&b':') { pat[2..].to_vec() } else { pat };
        for e in self.directory()? {
            if e.file_type & 0x07 == 0 || e.file_type & 0x07 > 3 {
                continue;
            }
            if name_matches(&pat, &e.name) {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
}

/// Compares a file name with a CBM DOS pattern. Names from the directory are ASCII/PETSCII bytes as `char`s.
fn name_matches(pat: &[u8], name: &str) -> bool {
    let n: Vec<u8> = name.chars().map(|c| c as u32 as u8).collect();
    for (i, &p) in pat.iter().enumerate() {
        if p == b'*' {
            return true;
        }
        match n.get(i) {
            Some(&c) if p == b'?' || p == c => {}
            _ => return false,
        }
    }
    n.len() == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(name_matches(b"GIANA", "GIANA"));
        assert!(!name_matches(b"GIANA", "GIANA2"));
        assert!(name_matches(b"GI*", "GIANA2"));
        assert!(name_matches(b"G?ANA", "GIANA"));
        assert!(name_matches(b"*", "X"));
        assert!(!name_matches(b"GIANAX", "GIANA"));
    }
}
