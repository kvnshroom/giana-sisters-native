//! Reader for D64 disk images (plain sector dumps of a 1541 disk): 35, 40 or 42 tracks, optionally followed by one
//! error byte per sector. Follows the directory and file chains.

use crate::g64::{sectors_per_track, DirEntry};
use std::collections::{BTreeMap, HashSet};

const DIR_TRACK: u8 = 18;

pub struct D64 {
    sectors: BTreeMap<(u8, u8), [u8; 256]>,
}

/// Follows a file's sector chain through `get` and returns the file contents.
pub fn follow_chain<'a>(
    name: &str,
    start: (u8, u8),
    get: impl Fn(u8, u8) -> Option<&'a [u8; 256]>,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let (mut t, mut s) = start;
    let mut seen = HashSet::new();
    while t != 0 {
        if !seen.insert((t, s)) {
            return Err(format!("file \"{name}\": sector chain loops"));
        }
        let b = get(t, s).ok_or_else(|| format!("file \"{name}\": sector {t}/{s} unreadable"))?;
        if b[0] != 0 {
            out.extend_from_slice(&b[2..]);
            (t, s) = (b[0], b[1]);
        } else {
            let last = b[1] as usize;
            if !(1..=255).contains(&last) {
                return Err(format!("file \"{name}\": bad length byte in last sector"));
            }
            out.extend_from_slice(&b[2..=last]);
            t = 0;
        }
    }
    Ok(out)
}

/// Parses the directory out of any sector source.
pub fn read_directory<'a>(get: impl Fn(u8, u8) -> Option<&'a [u8; 256]>) -> Result<Vec<DirEntry>, String> {
    let mut out = Vec::new();
    let (mut t, mut s) = (DIR_TRACK, 1u8);
    let mut seen = HashSet::new();
    while t != 0 {
        if !seen.insert((t, s)) {
            return Err("directory chain loops".into());
        }
        let b = get(t, s).ok_or_else(|| format!("directory sector {t}/{s} unreadable"))?;
        for i in 0..8 {
            let e = &b[i * 32..(i + 1) * 32];
            if e[2] == 0 {
                continue;
            }
            let name: String = e[5..21].iter().take_while(|&&c| c != 0xA0).map(|&c| c as char).collect();
            out.push(DirEntry { name, file_type: e[2], track: e[3], sector: e[4], blocks: u16::from_le_bytes([e[30], e[31]]) });
        }
        (t, s) = (b[0], b[1]);
    }
    Ok(out)
}

impl D64 {
    pub fn parse(d: &[u8]) -> Result<D64, String> {
        let tracks: u8 = match d.len() {
            174_848 | 175_531 => 35,
            196_608 | 197_376 => 40,
            205_312 | 206_114 => 42,
            n => return Err(format!("not a D64 image (unexpected size {n})")),
        };
        let mut sectors = BTreeMap::new();
        let mut off = 0;
        for t in 1..=tracks {
            for s in 0..sectors_per_track(t) {
                sectors.insert((t, s), d[off..off + 256].try_into().unwrap());
                off += 256;
            }
        }
        Ok(D64 { sectors })
    }

    pub fn sector(&self, t: u8, s: u8) -> Option<&[u8; 256]> {
        self.sectors.get(&(t, s))
    }

    pub fn directory(&self) -> Result<Vec<DirEntry>, String> {
        read_directory(|t, s| self.sector(t, s))
    }

    /// Contents of a directory entry's file (PRG: includes the two byte load address).
    pub fn read_entry(&self, e: &DirEntry) -> Result<Vec<u8>, String> {
        follow_chain(&e.name, (e.track, e.sector), |t, s| self.sector(t, s))
    }
}

/// Builds a minimal 35 track D64 (a writer for tests and tools) with the given files (name, PRG bytes incl. load address).
pub fn build_d64(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut img = vec![0u8; 174_848];
    let off = |t: u8, s: u8| -> usize {
        let mut o = 0;
        for k in 1..t {
            o += sectors_per_track(k) as usize * 256;
        }
        o + s as usize * 256
    };
    let mut next = (1u8, 0u8);
    let mut dir_entries = Vec::new();
    for (name, data) in files {
        let start = next;
        let chunks: Vec<&[u8]> = data.chunks(254).collect();
        for (i, c) in chunks.iter().enumerate() {
            let (t, s) = next;
            let mut n = (t, s + 1);
            if n.1 >= sectors_per_track(n.0) {
                n = (n.0 + 1, 0);
            }
            if n.0 == 18 {
                n = (19, 0);
            }
            let o = off(t, s);
            if i + 1 < chunks.len() {
                img[o] = n.0;
                img[o + 1] = n.1;
            } else {
                img[o] = 0;
                img[o + 1] = (c.len() + 1) as u8;
            }
            img[o + 2..o + 2 + c.len()].copy_from_slice(c);
            next = n;
        }
        dir_entries.push((name, start, chunks.len()));
    }
    let d = off(18, 1);
    img[d] = 0;
    img[d + 1] = 0xFF;
    for (i, (name, st, blocks)) in dir_entries.iter().enumerate() {
        let e = d + i * 32;
        img[e + 2] = 0x82;
        img[e + 3] = st.0;
        img[e + 4] = st.1;
        for k in 0..16 {
            img[e + 5 + k] = name.as_bytes().get(k).copied().unwrap_or(0xA0);
        }
        img[e + 30] = *blocks as u8;
    }
    img
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let big: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let img = build_d64(&[("A", &[1, 8, 9, 9]), ("BIG", &big)]);
        let d = D64::parse(&img).unwrap();
        let dir = d.directory().unwrap();
        assert_eq!(dir.len(), 2);
        assert_eq!(d.read_entry(&dir[0]).unwrap(), vec![1, 8, 9, 9]);
        assert_eq!(d.read_entry(&dir[1]).unwrap(), big);
    }

    #[test]
    fn rejects_bad_size() {
        assert!(D64::parse(&[0; 100]).is_err());
    }
}
