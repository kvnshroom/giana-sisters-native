//! Making a module from a modified image, RAM dump, PRG, VICE snapshot or BPS patch.

use crate::apply::{apply, check_f2};
use crate::bps;
use crate::container::{Meta, Module};
use crate::leak::leak_check;
use giana_extract::image::{apply_patches, build_unpatched, LOAD_ADDRESS};
use giana_extract::sha256::sha256_hex;

const VSF_MAGIC: &[u8] = b"VICE Snapshot File\x1a";
const IMAGE_END: usize = 0xFFF0;

/// What the user handed to the maker.
#[derive(Clone, Debug)]
pub enum Input {
    /// A 64K program image or raw RAM dump.
    Image(Vec<u8>),
    /// Disk file `1` (load address `$0820`).
    Prg(Vec<u8>),
    /// A VICE snapshot (`.vsf`).
    ViceSnapshot(Vec<u8>),
    /// A BPS patch against the base `ingame.bin`.
    Bps(Vec<u8>),
}

/// Guesses the kind of input from magic bytes, size and extension.
pub fn detect(bytes: &[u8], file_name: &str) -> Result<Input, String> {
    let lower = file_name.to_ascii_lowercase();
    if bytes.starts_with(VSF_MAGIC) {
        return Ok(Input::ViceSnapshot(bytes.to_vec()));
    }
    if bytes.starts_with(b"BPS1") || lower.ends_with(".bps") {
        return Ok(Input::Bps(bytes.to_vec()));
    }
    if bytes.len() == 0x1_0000 {
        return Ok(Input::Image(bytes.to_vec()));
    }
    // VICE monitor `save "x" 0 0 ffff` writes a PRG with load address $0000: a 64K dump plus two bytes
    if bytes.len() == 0x1_0002 && bytes[0] == 0 && bytes[1] == 0 {
        return Ok(Input::Image(bytes[2..].to_vec()));
    }
    if bytes.len() > 2 && bytes[0] == 0x20 && bytes[1] == 0x08 {
        return Ok(Input::Prg(bytes.to_vec()));
    }
    Err("unrecognized input: expected a 64K image or RAM dump, a PRG of disk file 1, a VICE snapshot or a .bps patch"
        .into())
}

/// Extracts the 64K RAM of a VICE snapshot (module `C64MEM`).
///
/// Layout, from memory of VICE's `snapshot.c` / `c64memsnapshot.c` (not verified against a real file here, so it is
/// parsed defensively):
/// - header: magic (19 bytes) + version major/minor (2) + machine name (16, zero padded); VICE 3.x adds the VICE
///   version (4 bytes) and the SVN revision (4 bytes LE) after that. Both header lengths (37 and 45) are tried; the
///   one whose module chain parses is used.
/// - modules: name (16, zero padded), major, minor, size (u32 LE, includes these 22 header bytes), data.
/// - `C64MEM` data: CPU port data, CPU port direction, EXROM, GAME (4 bytes), then 65536 bytes of RAM (newer
///   versions may append more after the RAM, which is ignored).
fn vsf_ram(snap: &[u8]) -> Result<Vec<u8>, String> {
    for header in [37usize, 45] {
        let mut pos = header;
        while pos + 22 <= snap.len() {
            let name = &snap[pos..pos + 16];
            let size = u32::from_le_bytes([snap[pos + 18], snap[pos + 19], snap[pos + 20], snap[pos + 21]]) as usize;
            if size < 22 || size > snap.len() - pos {
                break;
            }
            let trimmed: &[u8] = &name[..name.iter().position(|&c| c == 0).unwrap_or(16)];
            if trimmed == b"C64MEM" {
                let data = &snap[pos + 22..pos + size];
                if data.len() < 4 + 0x1_0000 {
                    return Err("VICE snapshot: the C64MEM module is too small (not a C64 snapshot?)".into());
                }
                return Ok(data[4..4 + 0x1_0000].to_vec());
            }
            pos += size;
        }
    }
    Err("VICE snapshot: no C64MEM module found (is it a C64 snapshot?)".into())
}

/// Normalizes a 64K image like the extractor: zero outside `$0820..=$FFF0`, then the documented patches.
fn normalize_image(ram: &[u8]) -> Result<Vec<u8>, String> {
    if ram.len() != 0x1_0000 {
        return Err("the image must be exactly 65536 bytes".into());
    }
    let mut img = vec![0u8; 0x1_0000];
    img[LOAD_ADDRESS..=IMAGE_END].copy_from_slice(&ram[LOAD_ADDRESS..=IMAGE_END]);
    Ok(apply_patches(&img))
}

/// Builds a module from `input`, optionally with a modified title program. Refuses patches that embed original data
/// and modules that change nothing; fills `meta.base`; verifies the result by applying it.
pub fn make(
    base_ingame: &[u8],
    base_f2: &[u8],
    input: Input,
    title_f2: Option<Vec<u8>>,
    mut meta: Meta,
) -> Result<Module, String> {
    meta.base = sha256_hex(base_ingame);
    meta.validate()?;
    let target = match input {
        Input::Image(img) => normalize_image(&img)?,
        Input::Prg(prg) => apply_patches(&build_unpatched(&prg)?),
        Input::ViceSnapshot(snap) => normalize_image(&vsf_ram(&snap)?)?,
        Input::Bps(p) => bps::decode(base_ingame, &p)?,
    };
    if target.len() != 0x1_0000 {
        return Err(format!("the modified program image has {} bytes, expected 65536", target.len()));
    }
    let ingame_patch = if target != base_ingame {
        let p = bps::encode(base_ingame, &target);
        leak_check(base_ingame, &p).map_err(|e| format!("ingame: {e}"))?;
        Some(p)
    } else {
        None
    };
    let title_patch = match title_f2 {
        Some(t) if t != base_f2 => {
            check_f2(&t)?;
            let p = bps::encode(base_f2, &t);
            leak_check(base_f2, &p).map_err(|e| format!("title: {e}"))?;
            Some(p)
        }
        _ => None,
    };
    if ingame_patch.is_none() && title_patch.is_none() {
        return Err("the input is identical to the original game data: nothing to make a module from".into());
    }
    let module = Module { meta, ingame_patch, title_patch, readme: None };
    let applied = apply(&module, base_ingame, base_f2)?;
    if applied.ingame != target {
        return Err("internal error: the module does not reproduce the input".into());
    }
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng;

    fn base_ingame() -> Vec<u8> {
        let mut r = test_rng(31);
        let mut raw = vec![0u8; 0x1_0000];
        for b in &mut raw[LOAD_ADDRESS..=IMAGE_END] {
            *b = r();
        }
        apply_patches(&raw)
    }

    fn base_f2() -> Vec<u8> {
        let mut r = test_rng(32);
        let mut v = vec![0x01, 0x08];
        v.extend((0..5000).map(|_| r()));
        v
    }

    fn meta() -> Meta {
        Meta::new("TEST", "me", "1", "d")
    }

    fn edited() -> Vec<u8> {
        let mut t = base_ingame();
        t[0xAC10] ^= 0x55;
        t[0xAC11] = 1;
        t[0x604E] = 7;
        t
    }

    fn vsf(ram: &[u8], header: usize) -> Vec<u8> {
        let mut s = VSF_MAGIC.to_vec();
        s.extend_from_slice(&[2, 0]);
        s.extend_from_slice(b"C64\0\0\0\0\0\0\0\0\0\0\0\0\0");
        s.resize(header, 0);
        let mut module = |name: &[u8], data: &[u8]| {
            let mut n = name.to_vec();
            n.resize(16, 0);
            s.extend_from_slice(&n);
            s.extend_from_slice(&[1, 1]);
            s.extend_from_slice(&((data.len() + 22) as u32).to_le_bytes());
            s.extend_from_slice(data);
        };
        module(b"MAINCPU", &[0u8; 20]);
        let mut mem = vec![0x37, 0x2F, 0, 0];
        mem.extend_from_slice(ram);
        mem.extend_from_slice(&[9; 6]); // trailing data of newer versions
        module(b"C64MEM", &mem);
        module(b"VIC-II", &[1, 2, 3]);
        s
    }

    #[test]
    fn detects_kinds() {
        assert!(matches!(detect(&vsf(&[0; 65536], 37), "x").unwrap(), Input::ViceSnapshot(_)));
        assert!(matches!(detect(&bps::encode(b"a", b"b"), "x").unwrap(), Input::Bps(_)));
        assert!(matches!(detect(&[1, 2, 3], "hack.BPS").unwrap(), Input::Bps(_)));
        assert!(matches!(detect(&vec![5; 65536], "x.bin").unwrap(), Input::Image(_)));
        let mut dump = vec![0, 0];
        dump.extend(vec![5; 65536]);
        assert!(matches!(detect(&dump, "x").unwrap(), Input::Image(i) if i.len() == 65536));
        assert!(matches!(detect(&[0x20, 0x08, 1, 2, 3], "1").unwrap(), Input::Prg(_)));
        assert!(detect(&[1, 2, 3], "x").is_err());
        assert!(detect(&[], "x").is_err());
    }

    #[test]
    fn make_from_image_normalizes() {
        let (b, f) = (base_ingame(), base_f2());
        let mut dump = edited();
        dump[0x0400] = 0xAA; // screen RAM: run-time state, must be dropped
        dump[0xFFFF] = 0x12;
        dump[0x1277] = 0; // overwritten by the documented patch again
        let m = make(&b, &f, Input::Image(dump), None, meta()).unwrap();
        let a = apply(&m, &b, &f).unwrap();
        assert_eq!(a.ingame, edited());
        assert_eq!(a.f2, f);
        assert_eq!(a.id.len(), 16);
        assert!(m.title_patch.is_none());
        assert_eq!(m.meta.base, sha256_hex(&b));
        let again = apply(&Module::read(&m.write()).unwrap(), &b, &f).unwrap();
        assert_eq!(again, a);
    }

    #[test]
    fn make_from_prg_snapshot_and_bps() {
        let (b, f) = (base_ingame(), base_f2());
        let e = edited();
        let mut prg = vec![0x20, 0x08];
        prg.extend_from_slice(&e[LOAD_ADDRESS..=IMAGE_END]);
        let m = make(&b, &f, detect(&prg, "1").unwrap(), None, meta()).unwrap();
        assert_eq!(apply(&m, &b, &f).unwrap().ingame, e);
        for header in [37, 45] {
            let m = make(&b, &f, Input::ViceSnapshot(vsf(&e, header)), None, meta()).unwrap();
            assert_eq!(apply(&m, &b, &f).unwrap().ingame, e, "header {header}");
        }
        let m = make(&b, &f, Input::Bps(bps::encode(&b, &e)), None, meta()).unwrap();
        assert_eq!(apply(&m, &b, &f).unwrap().ingame, e);
        assert!(make(&b, &f, Input::ViceSnapshot(vsf(&e, 37)[..60].to_vec()), None, meta()).is_err());
        assert!(make(&b, &f, Input::ViceSnapshot(VSF_MAGIC.to_vec()), None, meta()).is_err());
    }

    #[test]
    fn title_patch_and_checks() {
        let (b, f) = (base_ingame(), base_f2());
        let mut t = f.clone();
        t[100] ^= 0xFF;
        let m = make(&b, &f, Input::Image(b.clone()), Some(t.clone()), meta()).unwrap();
        assert!(m.ingame_patch.is_none());
        let a = apply(&m, &b, &f).unwrap();
        assert_eq!((a.ingame, a.f2), (b.clone(), t));
        // nothing changed
        assert!(make(&b, &f, Input::Image(b.clone()), Some(f.clone()), meta()).is_err());
        // bad title program
        let mut bad = f.clone();
        bad[0] = 0x02;
        assert!(make(&b, &f, Input::Image(b.clone()), Some(bad), meta()).is_err());
        // bad meta
        assert!(make(&b, &f, Input::Image(edited()), None, Meta::new("", "", "", "")).is_err());
        // wrong size
        assert!(make(&b, &f, Input::Image(vec![0; 100]), None, meta()).is_err());
    }

    #[test]
    fn apply_rejects_wrong_base_and_size() {
        let (b, f) = (base_ingame(), base_f2());
        let m = make(&b, &f, Input::Image(edited()), None, meta()).unwrap();
        let mut other = b.clone();
        other[0x3000] ^= 1;
        assert!(apply(&m, &other, &f).is_err());
        let mut m2 = m.clone();
        m2.meta.base = "00".repeat(32);
        assert!(apply(&m2, &b, &f).is_err());
        // ingame target of the wrong length
        let mut m3 = m.clone();
        m3.ingame_patch = Some(bps::encode(&b, &b[..1000]));
        assert!(apply(&m3, &b, &f).unwrap_err().contains("65536"));
    }

    #[test]
    fn moved_blocks_do_not_leak() {
        let b = base_ingame();
        let mut e = b.clone();
        let blk: Vec<u8> = e[0x2000..0x3000].to_vec();
        e[0x8000..0x9000].copy_from_slice(&blk);
        let m = make(&b, &base_f2(), Input::Image(e), None, meta()).unwrap();
        assert!(m.ingame_patch.unwrap().len() < 200);
    }

    /// Needs the original data folder of the main checkout (git-ignored); skipped without it.
    #[test]
    fn real_data_round_trip() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = if dir.join("ingame.bin").exists() {
            dir
        } else {
            std::path::PathBuf::from("/home/user/Projekte/giana-native/data")
        };
        let (Ok(b), Ok(f)) = (std::fs::read(dir.join("ingame.bin")), std::fs::read(dir.join("f_2.prg"))) else {
            eprintln!("skipped: no original data");
            return;
        };
        let mut e = b.clone();
        e[0xAC00..0xAC10].iter_mut().for_each(|x| *x ^= 0x0F);
        e[0x604E] = (e[0x604E] + 1) & 15;
        let m = make(&b, &f, Input::Image(e.clone()), None, meta()).unwrap();
        let a = apply(&Module::read(&m.write()).unwrap(), &b, &f).unwrap();
        assert_eq!(a.ingame, e);
        assert_eq!(a.f2, f);
        assert!(m.write().len() < 1000);
    }
}
