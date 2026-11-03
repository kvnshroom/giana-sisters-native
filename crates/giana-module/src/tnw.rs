//! Family T: "The Great Giana Sisters 2" by The New Wave (TNW), 1989. See `docs/rom-hack-families.md`.
//!
//! The hack keeps the memory layout of the original game, but loads the data of one stage from its disk every time a
//! stage starts (`$6070`, KERNAL LOAD of a one-character file name = file number + `$40`, secondary address 1: the
//! file goes to the address in its first two bytes, always `$6182`). A stage file is
//!
//! ```text
//! $6182  background colour ($D021)            $6187  start position of the player, 4 bytes
//! $6183  multicolour 1 ($D022)                       (x, y) for the start and (x, y) after a bonus room
//! $6184  multicolour 2 ($D023)                $618B  address of the enemy list (word)
//! $6185  colour for the sprite code ($1E)     $618D  stage stream (5-byte records, `$FF` ends it)
//! $6186  music track                          then   enemy list (3-byte entries, `$FF` ends it)
//! ```
//!
//! The same fields sit in per-stage tables of the original (`$604E`, `$6075`, `$609C`, `$60C3`, `$1529`, `$60EA`, `$3D20`
//! and the stream pointer table `$6000`). The hack's "old stages" (menu entries 1 and 3) are copies of the original's
//! stages, some with changes, and a few of its new bonus rooms reuse original room streams. A module must not carry
//! original data, so [`original_stage_pool`] rebuilds all 39 original stages in this file layout from the player's own
//! `ingame.bin`, and the files of a module are stored either as they are (new data) or as a BPS patch against that pool
//! (data that is partly or wholly the original's; recognised by the `BPS1` magic, a stage file starts with its load
//! address `82 61`). [`expand_files`] turns them back into whole files.

use crate::bps;
use crate::container::{ModFile, MAX_FILES, MAX_FILES_TOTAL, MAX_FILE_NAME, MAX_FILE_SIZE};
use crate::disk::Disk;
use crate::leak::leak_check;

/// Engine tag of a family T module.
pub const ENGINE_T: &str = "t";
/// Load address of every stage file.
pub const LOAD_ADDRESS: u16 = 0x6182;
/// Largest content of a stage file the hack's loader can take (`$6182..$FFFF`).
pub const MAX_CONTENT: usize = 0x1_0000 - LOAD_ADDRESS as usize;
/// Offset of the header fields in a stage file (without the load address).
pub const HEADER_LEN: usize = 11;

/// Run-time state of family T on top of the original's ranges: the loader's variables (`$6061` saved `$01`, `$6063` the
/// file name), the stage file that is in memory (`$6182..`, the module stores the files instead) and the mode (`$7ABE` the
/// file number, `$7ABF` the menu choice). `$6F00` (the stage number table) and the code behind it stay.
pub const RUNTIME_RANGES_T: &[(u16, u16)] = &[(0x6061, 0x6063), (0x6182, 0x6EFF), (0x7ABE, 0x7ABF)];

// the hook sites (`$0996`: `JSR $7AC0`, the start menu; `$9936`: `JSR $6F80`, the stage loader; `$33F2`: `JSR $6FC8`,
// the copy protection) and the moved stage number table (`$0A58`: `LDA $6F00,X`)
const SIGNATURE: [(usize, &[u8]); 4] =
    [(0x0996, &[0x20, 0xC0, 0x7A]), (0x9936, &[0x20, 0x80, 0x6F]), (0x33F2, &[0x20, 0xC8, 0x6F]), (0x0A58, &[0xBD, 0x00, 0x6F])];

/// True when the 64K program image carries the hooks of family T.
pub fn signature(img: &[u8]) -> bool {
    img.len() == 0x1_0000 && SIGNATURE.iter().all(|(a, s)| &img[*a..*a + s.len()] == *s)
}

/// Name (CBM file name, one byte) of the file with number `n` (`n + $40`).
pub fn file_name(n: u8) -> Vec<u8> {
    vec![n.wrapping_add(0x40)]
}

fn word(img: &[u8], a: usize) -> usize {
    img[a] as usize | (img[a + 1] as usize) << 8
}

/// One original stage (1..=39; 33 is the last stage, 34..=39 are the bonus rooms) as a stage file (without load address).
pub fn original_stage_file(base: &[u8], stage: usize) -> Result<Vec<u8>, String> {
    if base.len() != 0x1_0000 || !(1..=39).contains(&stage) {
        return Err("original_stage_file: bad arguments".into());
    }
    let y = stage - 1;
    let end_of = |from: usize, step: usize| -> Result<usize, String> {
        let mut e = from;
        while *base.get(e).ok_or("stage data runs off the image")? != 0xFF {
            e += step;
            if e - from > 0x2000 {
                return Err("stage data without end".into());
            }
        }
        Ok(e + 1)
    };
    let rec = word(base, 0x6000 + 2 * y);
    let stream = &base[rec..end_of(rec, 5)?];
    // the rooms read their (unused) list pointers from behind the table: they have no enemies
    let list: &[u8] = if stage <= 33 {
        let l = word(base, 0x3D20 + 2 * y);
        &base[l..end_of(l, 3)?]
    } else {
        &[0xFF]
    };
    let mut f = vec![base[0x604E + y], base[0x6075 + y], base[0x609C + y], base[0x60C3 + y], base[0x1529 + stage]];
    f.extend_from_slice(&base[0x60EA + 4 * y..0x60EA + 4 * y + 4]);
    let list_at = LOAD_ADDRESS as usize + HEADER_LEN + stream.len() + 1;
    f.extend_from_slice(&(list_at as u16).to_le_bytes());
    f.extend_from_slice(stream);
    f.push(0);
    f.extend_from_slice(list);
    Ok(f)
}

/// The 39 original stages back to back (stage 1 first), each as a stage file without its load address.
pub fn original_stage_pool(base: &[u8]) -> Result<Vec<u8>, String> {
    let mut pool = Vec::new();
    for s in 1..=39 {
        pool.extend(original_stage_file(base, s)?);
    }
    Ok(pool)
}

/// The stage files of a family T disk (every PRG that loads at `$6182`) as a module stores them: raw when they hold no
/// original data, otherwise as a BPS patch against [`original_stage_pool`]. Sorted by name. Fails when a file cannot be
/// stored or when it would leak.
pub fn collect_files(disk_image: &[u8], base_ingame: &[u8]) -> Result<Vec<ModFile>, String> {
    let pool = original_stage_pool(base_ingame)?;
    let disk = Disk::parse(disk_image)?;
    let mut files = Vec::new();
    for e in disk.prgs()? {
        let data = disk.read_entry(&e)?;
        if data.len() < 2 || u16::from_le_bytes([data[0], data[1]]) != LOAD_ADDRESS {
            continue;
        }
        let name: Vec<u8> = e.name.chars().map(|c| c as u32 as u8).collect();
        if name.is_empty() || name.len() > MAX_FILE_NAME {
            return Err(format!("disk file name {:?} cannot be stored", e.name));
        }
        let stored = store_file(&pool, &data[2..])?;
        if stored.len() > MAX_FILE_SIZE {
            return Err(format!("disk file {:?} is larger than {} bytes", e.name, MAX_FILE_SIZE));
        }
        files.push(ModFile { name, data: stored });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files.dedup_by(|a, b| a.name == b.name);
    let total: usize = files.iter().map(|f| f.data.len()).sum();
    if files.is_empty() || files.len() > MAX_FILES || total > MAX_FILES_TOTAL {
        return Err(format!("the disk has {} stage files ({total} bytes), expected 1..{MAX_FILES}", files.len()));
    }
    Ok(files)
}

/// The module form of one stage file (the content after the load address, which is always `$6182`; the stored file starts
/// with the load address when it is raw).
pub fn store_file(pool: &[u8], content: &[u8]) -> Result<Vec<u8>, String> {
    // raw data passes when a patch made of nothing but its bytes as literals passes
    let raw_ok = leak_check(pool, &bps::encode(&[], content)).is_ok();
    if raw_ok {
        let mut v = LOAD_ADDRESS.to_le_bytes().to_vec();
        v.extend_from_slice(content);
        return Ok(v);
    }
    let patch = bps::encode(pool, content);
    leak_check(pool, &patch).map_err(|e| format!("stage file: {e}"))?;
    Ok(patch)
}

/// The content (without load address) of a stored file: raw files lose their load address, patches are applied to the pool.
pub fn load_file(pool: &[u8], stored: &[u8]) -> Result<Vec<u8>, String> {
    if stored.starts_with(b"BPS1") {
        bps::decode_max(pool, stored, MAX_CONTENT as u64).map_err(|e| format!("stage file patch: {e}"))
    } else if stored.len() >= 2 && stored[..2] == LOAD_ADDRESS.to_le_bytes() {
        Ok(stored[2..].to_vec())
    } else {
        Err("a stage file must start with its load address $6182 or be a BPS patch".into())
    }
}

/// The module's files as the game loads them: every patch applied to [`original_stage_pool`], raw files without their
/// load address (the content that goes to `$6182`).
pub fn expand_files(files: &[ModFile], base_ingame: &[u8]) -> Result<Vec<ModFile>, String> {
    let pool = original_stage_pool(base_ingame)?;
    let mut total = 0usize;
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let data = load_file(&pool, &f.data)?;
        total += data.len();
        if total > MAX_FILES_TOTAL {
            return Err("the stage files of the module are too large".into());
        }
        out.push(ModFile { name: f.name.clone(), data });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image shaped like the original: tables for 39 stages and some streams.
    fn base() -> Vec<u8> {
        let mut b = vec![0u8; 0x1_0000];
        let mut at = 0x6182usize;
        for s in 0..39usize {
            b[0x6000 + 2 * s..0x6002 + 2 * s].copy_from_slice(&(at as u16).to_le_bytes());
            for r in 0..(5 + s) {
                b[at..at + 5].copy_from_slice(&[(r % 7 + 1) as u8, (r * 3 + s) as u8, (r * 11 + s * 5) as u8, (r % 13 + 2) as u8, (r * 17 + 1) as u8]);
                at += 5;
            }
            b[at] = 0xFF;
            at += 1;
            b[0x604E + s] = s as u8;
            b[0x6075 + s] = (s * 2) as u8;
            b[0x609C + s] = (s * 3) as u8;
            b[0x60C3 + s] = (s * 5) as u8;
            b[0x1529 + s + 1] = (s % 4) as u8;
            b[0x60EA + 4 * s..0x60EA + 4 * s + 4].copy_from_slice(&[s as u8, 1, 2, 3]);
        }
        let mut at = 0x3D80usize;
        for s in 0..33usize {
            b[0x3D20 + 2 * s..0x3D22 + 2 * s].copy_from_slice(&(at as u16).to_le_bytes());
            for r in 0..(1 + s % 4) {
                b[at..at + 3].copy_from_slice(&[(r * 9 + s + 1) as u8, 10 + r as u8, 5]);
                at += 3;
            }
            b[at] = 0xFF;
            at += 1;
        }
        b
    }

    #[test]
    fn signature_needs_all_hooks() {
        let mut img = vec![0u8; 0x1_0000];
        assert!(!signature(&img));
        for (a, s) in SIGNATURE {
            img[a..a + s.len()].copy_from_slice(s);
        }
        assert!(signature(&img));
        img[0x33F2] = 0xA5;
        assert!(!signature(&img));
        assert!(!signature(&img[..100]));
    }

    #[test]
    fn original_stage_has_the_hacks_layout() {
        let b = base();
        let f = original_stage_file(&b, 2).unwrap();
        assert_eq!(&f[..5], &[1, 2, 3, 5, 1]); // bg0, bg1, bg2, $6185, music
        assert_eq!(&f[5..9], &[1, 1, 2, 3]); // start positions
        let list = u16::from_le_bytes([f[9], f[10]]) as usize - 0x6182;
        assert_eq!(f[list - 1], 0); // pad byte
        assert_eq!(f[HEADER_LEN + 5 * 6], 0xFF); // stage 2 has 6 records
        assert_eq!(*f.last().unwrap(), 0xFF);
        assert!(original_stage_file(&b, 40).is_err());
        assert!(original_stage_file(&b[..10], 1).is_err());
        assert_eq!(original_stage_pool(&b).unwrap().len(), (1..=39).map(|s| original_stage_file(&b, s).unwrap().len()).sum::<usize>());
    }

    #[test]
    fn original_data_is_stored_as_a_patch_and_new_data_raw() {
        let b = base();
        let pool = original_stage_pool(&b).unwrap();
        // an original stage with another start position: patch
        let mut changed = original_stage_file(&b, 20).unwrap();
        changed[5] = 99;
        let stored = store_file(&pool, &changed).unwrap();
        assert!(stored.starts_with(b"BPS1"));
        assert!(stored.len() < changed.len() / 2, "{} {}", stored.len(), changed.len());
        assert_eq!(load_file(&pool, &stored).unwrap(), changed);
        // an unchanged original stage is a tiny patch too
        let same = original_stage_file(&b, 7).unwrap();
        assert_eq!(load_file(&pool, &store_file(&pool, &same).unwrap()).unwrap(), same);
        // new data: raw with the load address
        let mut rng = crate::test_rng(5);
        let new: Vec<u8> = (0..400).map(|_| rng()).collect();
        let stored = store_file(&pool, &new).unwrap();
        assert_eq!(&stored[..2], &[0x82, 0x61]);
        assert_eq!(load_file(&pool, &stored).unwrap(), new);
        assert!(load_file(&pool, b"junk").is_err());
        let files = expand_files(&[ModFile::new(b"A", stored)], &b).unwrap();
        assert_eq!(files[0].data, new);
    }

    #[test]
    fn a_patch_with_a_huge_target_is_refused() {
        let b = base();
        let pool = original_stage_pool(&b).unwrap();
        // a valid patch that declares a 40 MB target (all zeros as one literal-free copy is not needed: the header decides)
        let mut p = b"BPS1".to_vec();
        bps::put_varint(&mut p, pool.len() as u64);
        bps::put_varint(&mut p, 40 << 20);
        bps::put_varint(&mut p, 0);
        p.extend_from_slice(&crate::crc32::crc32(&pool).to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        let c = crate::crc32::crc32(&p);
        p.extend_from_slice(&c.to_le_bytes());
        let err = load_file(&pool, &p).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        assert!(expand_files(&[ModFile::new(b"A", p)], &b).is_err());
        // the total is capped as well
        let one = store_file(&pool, &original_stage_file(&b, 3).unwrap()).unwrap();
        assert_eq!(expand_files(&[ModFile::new(b"A", one)], &b).unwrap().len(), 1);
    }

    #[test]
    fn file_names_are_number_plus_40() {
        assert_eq!(file_name(1), b"A");
        assert_eq!(file_name(101), vec![0xA5]);
        assert_eq!(file_name(171), vec![0xEB]);
    }
}
