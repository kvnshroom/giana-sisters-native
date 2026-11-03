//! Reader for G64 disk images (raw GCR bit streams of a 1541 drive): finds the sectors of the 35 standard
//! tracks, checks header and data checksums and follows the directory and file chains.
//!
//! Only what is needed to read a normal file is implemented. Tracks above 35 (copy protection data) and
//! half tracks are ignored.

use std::collections::{BTreeMap, HashSet};

const SIGNATURE: &[u8; 8] = b"GCR-1541";
const DIR_TRACK: u8 = 18;

/// GCR 5-bit code to nibble, `0xFF` for codes that are not valid.
const GCR_DECODE: [u8; 32] = {
    let mut t = [0xFFu8; 32];
    let codes: [u8; 16] = [0x0A, 0x0B, 0x12, 0x13, 0x0E, 0x0F, 0x16, 0x17, 0x09, 0x19, 0x1A, 0x1B, 0x0D, 0x1D, 0x1E, 0x15];
    let mut n = 0;
    while n < 16 {
        t[codes[n] as usize] = n as u8;
        n += 1;
    }
    t
};

/// Sectors on a track of a standard 35 track 1541 disk.
pub fn sectors_per_track(track: u8) -> u8 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

/// All sectors that could be read from a G64 image, keyed by (track, sector).
pub struct Disk {
    sectors: BTreeMap<(u8, u8), [u8; 256]>,
}

/// One directory entry.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub file_type: u8,
    pub track: u8,
    pub sector: u8,
    pub blocks: u16,
}

struct Track<'a> {
    data: &'a [u8],
}

impl Track<'_> {
    fn nbits(&self) -> usize {
        self.data.len() * 8
    }

    fn bit(&self, pos: usize) -> u8 {
        let p = pos % self.nbits();
        (self.data[p / 8] >> (7 - p % 8)) & 1
    }

    /// Reads `n` GCR-encoded bytes starting at bit `pos`, `None` on an invalid code.
    fn decode(&self, pos: usize, n: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(n);
        for k in 0..n {
            let mut nib = [0u8; 2];
            for (h, slot) in nib.iter_mut().enumerate() {
                let mut code = 0usize;
                for b in 0..5 {
                    code = code << 1 | self.bit(pos + k * 10 + h * 5 + b) as usize;
                }
                let v = GCR_DECODE[code];
                if v == 0xFF {
                    return None;
                }
                *slot = v;
            }
            out.push(nib[0] << 4 | nib[1]);
        }
        Some(out)
    }

    /// Bit positions just after each sync mark (a run of at least 10 one bits), in increasing order.
    fn syncs(&self) -> Vec<usize> {
        let n = self.nbits();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut ones = 0;
        // two laps so that a sync mark across the end of the track is seen whole
        for i in 0..2 * n {
            if self.bit(i) == 1 {
                ones += 1;
            } else {
                if ones >= 10 && seen.insert(i % n) {
                    out.push(i % n);
                }
                ones = 0;
            }
        }
        out.sort_unstable();
        out
    }
}

impl Disk {
    /// Parses the image. Fails when it is not a G64 or has no readable directory track.
    pub fn parse(g64: &[u8]) -> Result<Disk, String> {
        if g64.len() < 12 || &g64[..8] != SIGNATURE {
            return Err("not a G64 image (signature GCR-1541 missing)".into());
        }
        let ntracks = g64[9] as usize;
        let table_end = 12 + 4 * ntracks;
        if g64.len() < table_end {
            return Err("G64 header is truncated".into());
        }
        let mut sectors = BTreeMap::new();
        for track in 1..=35u8 {
            let entry = 2 * (track as usize - 1);
            if entry >= ntracks {
                break;
            }
            let o = u32::from_le_bytes(g64[12 + 4 * entry..16 + 4 * entry].try_into().unwrap()) as usize;
            if o == 0 || o + 2 > g64.len() {
                continue;
            }
            let len = u16::from_le_bytes([g64[o], g64[o + 1]]) as usize;
            if o + 2 + len > g64.len() || len < 16 {
                return Err(format!("track {track} is truncated"));
            }
            read_track(track, &Track { data: &g64[o + 2..o + 2 + len] }, &mut sectors);
        }
        if !sectors.contains_key(&(DIR_TRACK, 1)) {
            return Err("directory sector 18/1 could not be read".into());
        }
        Ok(Disk { sectors })
    }

    pub fn sector(&self, t: u8, s: u8) -> Option<&[u8; 256]> {
        self.sectors.get(&(t, s))
    }

    pub fn sector_count(&self) -> usize {
        self.sectors.len()
    }

    /// Directory entries (all file types, deleted entries skipped).
    pub fn directory(&self) -> Result<Vec<DirEntry>, String> {
        let mut out = Vec::new();
        let (mut t, mut s) = (DIR_TRACK, 1u8);
        let mut seen = HashSet::new();
        while t != 0 {
            if !seen.insert((t, s)) {
                return Err("directory chain loops".into());
            }
            let b = self.sectors.get(&(t, s)).ok_or_else(|| format!("directory sector {t}/{s} unreadable"))?;
            for i in 0..8 {
                let e = &b[i * 32..(i + 1) * 32];
                if e[2] == 0 {
                    continue;
                }
                let name: String = e[5..21].iter().take_while(|&&c| c != 0xA0).map(|&c| c as char).collect();
                out.push(DirEntry {
                    name,
                    file_type: e[2],
                    track: e[3],
                    sector: e[4],
                    blocks: u16::from_le_bytes([e[30], e[31]]),
                });
            }
            (t, s) = (b[0], b[1]);
        }
        Ok(out)
    }

    /// Contents of the first PRG file with this name (including its two byte load address).
    pub fn read_prg(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self
            .directory()?
            .into_iter()
            .find(|e| e.name == name && e.file_type & 0x07 == 2)
            .ok_or_else(|| format!("file \"{name}\" not found in the directory"))?;
        let mut out = Vec::new();
        let (mut t, mut s) = (e.track, e.sector);
        let mut seen = HashSet::new();
        while t != 0 {
            if !seen.insert((t, s)) {
                return Err(format!("file \"{name}\": sector chain loops"));
            }
            let b = self.sectors.get(&(t, s)).ok_or_else(|| format!("file \"{name}\": sector {t}/{s} unreadable"))?;
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
}

fn read_track(track: u8, tr: &Track, sectors: &mut BTreeMap<(u8, u8), [u8; 256]>) {
    let syncs = tr.syncs();
    for (i, &p) in syncs.iter().enumerate() {
        let Some(h) = tr.decode(p, 8) else { continue };
        if h[0] != 0x08 || h[1] != h[2] ^ h[3] ^ h[4] ^ h[5] || h[3] != track || h[2] >= sectors_per_track(track) {
            continue;
        }
        let sector = h[2];
        // the data block is the next sync mark after the header
        let Some(&dp) = syncs.get((i + 1) % syncs.len()) else { continue };
        let Some(d) = tr.decode(dp, 258) else { continue };
        if d[0] != 0x07 || d[1..257].iter().fold(0u8, |a, b| a ^ b) != d[257] {
            continue;
        }
        sectors.entry((track, sector)).or_insert_with(|| d[1..257].try_into().unwrap());
    }
}
