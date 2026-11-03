//! Making a module from a modified image, RAM dump, PRG, VICE snapshot or BPS patch.

use crate::apply::{apply, check_f2};
use crate::bps;
use crate::container::{ModFile, Meta, Module, ENGINE_B};
use crate::disk::Disk;
use crate::leak::leak_check;
use giana_extract::image::{apply_patches, build_unpatched, LOAD_ADDRESS};
use giana_extract::sha256::sha256_hex;

const VSF_MAGIC: &[u8] = b"VICE Snapshot File\x1a";
const IMAGE_END: usize = 0xFFF0;

/// Signature of the 7A3 construction-set engine ("family B": Giana Sisters II, Giana Power Edition, Ridley X, Giana
/// Sisters Junior, Super Marioahner Sisters, the C-Set tool): its new stage builder starts at `$9936` with
/// `LDA #0; STA $9D; JSR $2F54; LDA #$AC; LDX #$13` and ends its map clearing with `JMP $9010` at `$9952`
/// (checked against four hacks; the original has other code there).
const FAMILY_B_SIGNATURE: [(usize, &[u8]); 2] = [
    (0x9936, &[0xA9, 0x00, 0x85, 0x9D, 0x20, 0x54, 0x2F, 0xA9, 0xAC, 0xA2, 0x13]),
    (0x9952, &[0x4C, 0x10, 0x90]),
];

/// The new frame handler of the 7A3 engine at `$776E` (`LDA $1D; BEQ +3; JMP $7804; LDA #$FD`): the sprite program of
/// every version of the engine. Giana Sisters II (Gem Nightmare, V1.2) has it but another stage builder and another
/// stage file format; it is recognised so that it can be refused with a clear message.
const FAMILY_B_PROGRAM: (usize, &[u8]) = (0x776E, &[0xA5, 0x1D, 0xF0, 0x03, 0x4C, 0x04, 0x78, 0xA9, 0xFD]);

const OLDER_ENGINE: &str = "this game uses an older version of the construction-set engine (Giana Sisters II, Gem Nightmare): \
    its stage files have another format, which this program cannot play yet";

/// The sprite program of the 7A3 engine is there, whatever its stage builder looks like.
fn has_b_program(img: &[u8]) -> bool {
    img.len() == 0x1_0000 && &img[FAMILY_B_PROGRAM.0..FAMILY_B_PROGRAM.0 + FAMILY_B_PROGRAM.1.len()] == FAMILY_B_PROGRAM.1
}

/// Load addresses (first two bytes of a disk file) of the files family B loads while it plays: stage and treasure room
/// files (`$83F0`), monster definition files (`$7505`), monster sprite files (`$FB80`). Hack authors name the files as
/// they like (`01 RIDLEYX01.STG`, `01STG.....-ROLE-`, `01 GIANA I  .STG`), the load address is the reliable mark.
pub const FAMILY_B_LOAD_ADDRESSES: [u16; 3] = [0x83F0, 0x7505, 0xFB80];

/// Which engine a 64K program image needs: `Some("b")` for the 7A3 construction-set engine, `Some("t")` for family T
/// (TNW, loads its stages from the disk: see `tnw.rs`), `None` for the original engine and everything else.
pub fn engine_tag(img: &[u8]) -> Option<&'static str> {
    if img.len() != 0x1_0000 {
        return None;
    }
    if crate::tnw::signature(img) {
        return Some(crate::tnw::ENGINE_T);
    }
    // the other engines' builders end in `JMP $9010`; Gem Nightmare's (the older variant of the same engine, which
    // keeps the original game's tables) has its own hook, see `gem.rs`
    (FAMILY_B_SIGNATURE.iter().all(|(a, sig)| &img[*a..*a + sig.len()] == *sig) || crate::gem::is_gem(img)).then_some(ENGINE_B)
}

/// The disk files the engine of `img` loads while playing: [`collect_family_b_files`] by load address for the hacks
/// with stage files (`$83F0`, `$7505`, `$FB80`); for Gem Nightmare the files whose names the loader builds (stage and
/// sprite files; the disk also holds editor files with similar load addresses).
pub fn collect_engine_b_files(img: &[u8], disk_image: &[u8]) -> Result<Vec<ModFile>, String> {
    if !crate::gem::is_gem(img) {
        return collect_family_b_files(disk_image);
    }
    let wanted = crate::gem::GemLayout::read(img).wanted_names(img);
    let disk = Disk::parse(disk_image)?;
    let mut files = Vec::new();
    for e in disk.prgs()? {
        let name: Vec<u8> = e.name.chars().map(|c| c as u32 as u8).collect();
        if !wanted.contains(&name) {
            continue;
        }
        let data = disk.read_entry(&e)?;
        if data.len() < 2 || data.len() > crate::container::MAX_FILE_SIZE {
            return Err(format!("disk file {:?} cannot be stored", e.name));
        }
        files.push(ModFile { name, data });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files.dedup_by(|a, b| a.name == b.name);
    let total: usize = files.iter().map(|f| f.data.len()).sum();
    if files.len() > crate::container::MAX_FILES || total > crate::container::MAX_FILES_TOTAL {
        return Err(format!("the disk has too many stage files ({} files, {total} bytes)", files.len()));
    }
    Ok(files)
}

/// The disk files of a family B hack: every PRG of the directory whose load address is one of
/// [`FAMILY_B_LOAD_ADDRESSES`], with the directory name as the file name. Sorted by name.
pub fn collect_family_b_files(disk_image: &[u8]) -> Result<Vec<ModFile>, String> {
    let disk = Disk::parse(disk_image)?;
    let mut files = Vec::new();
    for e in disk.prgs()? {
        let data = disk.read_entry(&e)?;
        if data.len() < 2 {
            continue;
        }
        let load = u16::from_le_bytes([data[0], data[1]]);
        if !FAMILY_B_LOAD_ADDRESSES.contains(&load) {
            continue;
        }
        let name: Vec<u8> = e.name.chars().map(|c| c as u32 as u8).collect();
        if name.is_empty() || name.len() > crate::container::MAX_FILE_NAME {
            return Err(format!("disk file name {:?} cannot be stored", e.name));
        }
        if data.len() > crate::container::MAX_FILE_SIZE {
            return Err(format!("disk file {:?} is larger than {} bytes", e.name, crate::container::MAX_FILE_SIZE));
        }
        files.push(ModFile { name, data });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files.dedup_by(|a, b| a.name == b.name);
    let total: usize = files.iter().map(|f| f.data.len()).sum();
    if files.len() > crate::container::MAX_FILES || total > crate::container::MAX_FILES_TOTAL {
        return Err(format!("the disk has too many stage files ({} files, {total} bytes)", files.len()));
    }
    Ok(files)
}

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
    /// The final RAM of a CPU-only disk boot (the game waiting for input for the first time). It still holds run-time
    /// state of the game's own start-up; [`clean_boot`] removes that.
    Boot(Vec<u8>),
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
    // a disk image (or a PRG with a BASIC stub) is booted on the CPU-only emulation of `runner`
    let is_disk = bytes.starts_with(b"GCR-1541")
        || lower.ends_with(".d64")
        || lower.ends_with(".g64")
        || matches!(bytes.len(), 174_848 | 175_531 | 196_608 | 197_376 | 205_312 | 206_114);
    if is_disk {
        if giana_extract::extract(bytes).is_ok() {
            return Err("this is the original game disk: its data is extracted automatically on the first start; \
                        a module needs a fan hack"
                .into());
        }
        return boot_input(crate::runner::boot_disk(bytes, file_name)?);
    }
    if bytes.len() > 2 && bytes[0] == 0x01 && bytes[1] == 0x08 {
        return boot_input(crate::runner::boot_prg(bytes, file_name)?);
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

fn boot_input(r: crate::runner::BootResult) -> Result<Input, String> {
    if !r.entry_reached && !matches!(r.ram[0x0820], 0x4C | 0x20 | 0x85 | 0xA9 | 0x78) {
        return Err(format!("the program never started the game ({}); is this a Giana Sisters hack?", r.stop));
    }
    Ok(Input::Boot(r.ram))
}

/// Cleans the final RAM of a boot: the addresses in `RUNTIME_RANGES` (variables, buffers, sound driver and IRQ state,
/// operands the start-up patches into the code) get the base byte, so they never show up as changes. Everything else
/// is taken from the final state, not from the snapshot at the game entry (`BootResult::entry_ram`):
/// - some hacks build their level data in their own start-up (Frankie's Horror Trip 2 and Gitty's Dreams 1 expand the
///   stage map `$AC00..$BFFF` there), which the entry state does not have yet;
/// - for packed hacks the entry event fires when the main program has been loaded, before the hack's own unpacker has
///   patched the code (Frankie's Horror Trip 2 changes `$08D1` afterwards), so even the code is more complete in the
///   final state;
/// - checked against VICE dumps taken at `$0820` for all eight known hacks: apart from `RUNTIME_RANGES` the final state
///   equals them byte for byte.
pub fn clean_boot(base: &[u8], last: &[u8]) -> Vec<u8> {
    let mut img = last.to_vec();
    reset_runtime(base, &mut img);
    img
}

/// The run-time ranges that go with the engine of `img`: the 7A3 engine (family B) has its own layout.
fn runtime_ranges_for(img: &[u8]) -> &'static [(u16, u16)] {
    // family T keeps the layout of the original game (and its table and loader at `$6F00..`): the ranges of the 7A3 engine
    // would wipe them; its own extras come in `reset_runtime`. Gem Nightmare (an older 7A3 engine) has its own ranges.
    if crate::gem::is_gem(img) {
        crate::runtime_ranges::RUNTIME_RANGES_GEM
    } else if engine_tag(img).is_some_and(|e| e != crate::tnw::ENGINE_T) {
        crate::runtime_ranges::RUNTIME_RANGES_B
    } else {
        crate::runtime_ranges::RUNTIME_RANGES
    }
}

/// Puts the base byte into every run-time range of the image's engine.
fn reset_runtime(base: &[u8], img: &mut [u8]) {
    if base.len() == img.len() && img.len() == 0x1_0000 {
        let extra: &[(u16, u16)] = if engine_tag(img) == Some(crate::tnw::ENGINE_T) { crate::tnw::RUNTIME_RANGES_T } else { &[] };
        for &(a, b) in runtime_ranges_for(img).iter().chain(extra) {
            img[a as usize..=b as usize].copy_from_slice(&base[a as usize..=b as usize]);
        }
    }
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
///
/// A hack of the 7A3 engine (family B) cannot play without the stage files of its disk: use [`make_with_files`] or
/// [`make_from_file`] for it.
pub fn make(
    base_ingame: &[u8],
    base_f2: &[u8],
    input: Input,
    title_f2: Option<Vec<u8>>,
    meta: Meta,
) -> Result<Module, String> {
    make_with_files(base_ingame, base_f2, input, title_f2, meta, Vec::new())
}

/// Like [`make`], with the disk files the hack loads while playing (family B: stage, monster and treasure room files).
/// The engine tag is set from the code signature of the image; a family B image without files is refused, files with
/// another engine are refused too.
pub fn make_with_files(
    base_ingame: &[u8],
    base_f2: &[u8],
    input: Input,
    title_f2: Option<Vec<u8>>,
    mut meta: Meta,
    files: Vec<ModFile>,
) -> Result<Module, String> {
    meta.base = sha256_hex(base_ingame);
    meta.validate()?;
    let target = match input {
        Input::Image(img) => normalize_image(&img)?,
        Input::Prg(prg) => apply_patches(&build_unpatched(&prg)?),
        Input::ViceSnapshot(snap) => {
            let mut img = normalize_image(&vsf_ram(&snap)?)?;
            // a snapshot of a family B game holds its run-time state (stage buffer, header copy, loaded monster, map)
            if engine_tag(&img).is_some() {
                reset_runtime(base_ingame, &mut img);
            }
            img
        }
        Input::Bps(p) => bps::decode(base_ingame, &p)?,
        Input::Boot(last) => normalize_image(&clean_boot(base_ingame, &last))?,
    };
    if target.len() == 0x1_0000 && base_ingame.len() == 0x1_0000 && !looks_like_hack(base_ingame, &target) {
        if has_b_program(&target) {
            return Err(OLDER_ENGINE.into());
        }
        // a hack of this game keeps most of the program; a disk of another game (or a failed boot) does not
        let same = (LOAD_ADDRESS..=IMAGE_END).filter(|&i| target[i] == base_ingame[i]).count();
        return Err(format!(
            "the input does not look like a hack of this game (only {}% of the program is the same as the original)",
            same * 100 / (IMAGE_END - LOAD_ADDRESS)
        ));
    }
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
    let family = engine_tag(&target);
    if family.is_some() && files.is_empty() {
        return Err("this hack loads its stage files from the disk while playing (Giana Sisters II / construction-set \
                    engine, TNW Giana Sisters 2), so it can only be imported from the disk image (.d64/.g64) that holds them"
            .into());
    }
    if family.is_none() && !files.is_empty() {
        return Err("the program is not a hack of the construction-set engine, but disk files were given".into());
    }
    meta.engine = family.map(str::to_string);
    let module = Module { meta, ingame_patch, title_patch, readme: None, files };
    let applied = apply(&module, base_ingame, base_f2)?;
    if applied.ingame != target {
        return Err("internal error: the module does not reproduce the input".into());
    }
    Ok(module)
}

/// Does the image look like a hack of this game (or of its construction-set engine)? A hack keeps most of the program;
/// the 7A3 engine rewrites more than half of it and is recognised by its code signature instead.
fn looks_like_hack(base_ingame: &[u8], target: &[u8]) -> bool {
    if target.len() != 0x1_0000 || base_ingame.len() != 0x1_0000 {
        return true; // reported elsewhere
    }
    let same = (LOAD_ADDRESS..=IMAGE_END).filter(|&i| target[i] == base_ingame[i]).count();
    let needed = if engine_tag(target).is_some() { 4 } else { 2 };
    same * needed >= IMAGE_END - LOAD_ADDRESS
}

/// How many programs of a disk are tried to find the game (the first one is often an intro, a menu or an editor).
const MAX_BOOT_TRIES: usize = 6;

fn is_disk_image(bytes: &[u8], file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    bytes.starts_with(b"GCR-1541")
        || lower.ends_with(".d64")
        || lower.ends_with(".g64")
        || matches!(bytes.len(), 174_848 | 175_531 | 196_608 | 197_376 | 205_312 | 206_114)
}

/// Imports a file the user dropped: like [`detect`] followed by [`make`], but a disk image of a family B hack also
/// brings its stage, monster and treasure room files into the module. A disk is booted program by program (the
/// first one, then the other programs that start with BASIC, at most [`MAX_BOOT_TRIES`]) until one gives a game
/// that looks like a hack of this one: disks with several programs keep the game behind an intro or a menu.
pub fn make_from_file(
    base_ingame: &[u8],
    base_f2: &[u8],
    bytes: &[u8],
    file_name: &str,
    title_f2: Option<Vec<u8>>,
    meta: Meta,
) -> Result<Module, String> {
    let first = detect(bytes, file_name);
    let first_is_game = match &first {
        Ok(Input::Boot(ram)) => normalize_image(&clean_boot(base_ingame, ram)).is_ok_and(|img| looks_like_hack(base_ingame, &img)),
        _ => true,
    };
    let kind = if is_disk_image(bytes, file_name) && !first_is_game || first.is_err() && is_disk_image(bytes, file_name) {
        // the first program was not the game: try the others, keep the first result for the error message
        let tries = crate::runner::boot_candidates(bytes).unwrap_or_default().len().min(MAX_BOOT_TRIES);
        let mut older = false;
        let found = (1..tries).find_map(|n| {
            let r = crate::runner::boot_disk_nth(bytes, file_name, n, &Default::default()).ok()?;
            let img = normalize_image(&clean_boot(base_ingame, &r.ram)).ok()?;
            older |= has_b_program(&img) && engine_tag(&img).is_none();
            (r.entry_reached && looks_like_hack(base_ingame, &img)).then_some(r.ram)
        });
        match found {
            Some(ram) => Input::Boot(ram),
            None if older => return Err(OLDER_ENGINE.into()),
            None => first?,
        }
    } else {
        first?
    };
    let files = match &kind {
        Input::Boot(ram) => match engine_tag(&normalize_image(&clean_boot(base_ingame, ram))?) {
            Some(crate::tnw::ENGINE_T) => crate::tnw::collect_files(bytes, base_ingame)?,
            Some(_) => collect_engine_b_files(&normalize_image(&clean_boot(base_ingame, ram))?, bytes)?,
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    make_with_files(base_ingame, base_f2, kind, title_f2, meta, files)
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
    fn clean_boot_resets_runtime_state_only() {
        let (base, mut last) = (vec![1u8; 0x1_0000], vec![3u8; 0x1_0000]);
        last[0x9B10] = 8; // run-time state: base byte wins
        last[0xAC10] = 9; // level data built by the start-up stays
        last[0x1000] = 4; // code changed by the hack stays
        let out = clean_boot(&base, &last);
        assert_eq!((out[0x9B10], out[0xAC10], out[0x1000]), (1, 9, 4));
    }

    #[test]
    fn runtime_ranges_keep_level_data_out() {
        for (a, b) in [(0x6000u16, 0x60E7u16), (0x9C20, 0x9D1F), (0xAB00, 0xBFFF)] {
            for &(s, e) in crate::runtime_ranges::RUNTIME_RANGES {
                assert!(e < a || s > b, "runtime range {s:04X}..{e:04X} overlaps level data {a:04X}..{b:04X}");
            }
        }
    }

    #[test]
    fn family_b_runtime_ranges_keep_its_data_out() {
        // the sprite program, type tables, loader file name patterns, block table and the high score screen are the
        // hack's own; the original table's `$7CAA` is code in this engine and must not be in the B table
        for (a, b) in [(0x2167u16, 0x2703u16), (0x3600, 0x37E6), (0x7700, 0x8EDD), (0x91C5, 0x93FF), (0x9C20, 0x9D1F), (0xAB00, 0xABA5)] {
            for &(s, e) in crate::runtime_ranges::RUNTIME_RANGES_B {
                assert!(e < a || s > b, "runtime range {s:04X}..{e:04X} overlaps data {a:04X}..{b:04X}");
            }
        }
        let b = crate::runtime_ranges::RUNTIME_RANGES_B;
        assert!(b.iter().any(|&(s, e)| s <= 0x6DFD && e >= 0x7519), "stage buffer");
        assert!(b.iter().any(|&(s, e)| s <= 0xAC00 && e >= 0xBFFF), "stage map");
        assert!(b.windows(2).all(|w| w[0].1 < w[1].0), "sorted and disjoint");
    }

    #[test]
    fn clean_boot_uses_the_ranges_of_the_engine() {
        let base = base_ingame();
        let (mut b_run, mut orig_run) = (family_b_image(), base_ingame());
        // a byte only the original table covers (sound driver state `$41E4` is in both; `$7CAA` only in the original's)
        // and one only the B table covers (the header copy `$9000`)
        for img in [&mut b_run, &mut orig_run] {
            img[0x7CAA] = base[0x7CAA].wrapping_add(1);
            img[0x9000] = base[0x9000].wrapping_add(1);
            img[0x41E4] = base[0x41E4].wrapping_add(1);
        }
        let b = clean_boot(&base, &b_run);
        assert_eq!((b[0x7CAA], b[0x9000], b[0x41E4]), (b_run[0x7CAA], base[0x9000], base[0x41E4]), "family B: code at $7CAA stays");
        let o = clean_boot(&base, &orig_run);
        assert_eq!((o[0x7CAA], o[0x9000], o[0x41E4]), (base[0x7CAA], orig_run[0x9000], base[0x41E4]), "original engine");
        // a snapshot of a family B game is cleaned the same way
        let mut snap_ram = b_run.clone();
        snap_ram[0x6E00] = 0x99;
        let files = collect_family_b_files(&stage_disk()).unwrap();
        let m = make_with_files(&base, &base_f2(), Input::ViceSnapshot(vsf(&snap_ram, 37)), None, meta(), files).unwrap();
        let a = apply(&m, &base, &base_f2()).unwrap();
        assert_eq!(a.ingame[0x6E00], base[0x6E00], "stage buffer reset");
        assert_eq!(a.ingame[0x7CAA], b_run[0x7CAA]);
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

    // ---- family B (7A3 construction-set engine) -------------------------------------------------------------------

    /// A "hack" of the base: half of the program rewritten, the family B signature in place.
    fn family_b_image() -> Vec<u8> {
        let mut t = base_ingame();
        let mut r = test_rng(77);
        for i in (LOAD_ADDRESS..=IMAGE_END).step_by(2) {
            t[i] = r();
        }
        for (a, sig) in FAMILY_B_SIGNATURE {
            t[a..a + sig.len()].copy_from_slice(sig);
        }
        t
    }

    fn stage_disk() -> Vec<u8> {
        let stg1: Vec<u8> = [&[0xF0u8, 0x83][..], &[1, 2, 3, 4, 5, 6, 7, 8][..]].concat();
        let stg2: Vec<u8> = [&[0xF0u8, 0x83][..], &vec![9u8; 600][..]].concat();
        let nlm = vec![0x05u8, 0x75, 0xEE, 0x80, 1, 2];
        let spr = vec![0x80u8, 0xFB, 0, 0, 1];
        let other = vec![0x01u8, 0x08, 0x0B, 0x08];
        giana_extract::d64::build_d64(&[
            ("RIDLEYXHACK", &other),
            ("01 RIDLEYX01.STG", &stg1),
            ("02 RIDLEYX02.STG", &stg2),
            ("01 BIG MBALL.NLM", &nlm),
            ("01 BIG MBALL.SPR", &spr),
            ("HI!", &[0xF7, 0x36, 1, 2]),
        ])
    }

    #[test]
    fn family_b_signature_and_disk_files() {
        let img = family_b_image();
        assert_eq!(engine_tag(&img), Some("b"));
        assert_eq!(engine_tag(&base_ingame()), None);
        assert_eq!(engine_tag(&img[..100]), None);
        let mut broken = img.clone();
        broken[0x9952] = 0x4D;
        assert_eq!(engine_tag(&broken), None);
        let files = collect_family_b_files(&stage_disk()).unwrap();
        let names: Vec<String> = files.iter().map(|f| String::from_utf8_lossy(&f.name).into_owned()).collect();
        assert_eq!(names, ["01 BIG MBALL.NLM", "01 BIG MBALL.SPR", "01 RIDLEYX01.STG", "02 RIDLEYX02.STG"], "boot file and HI! are not collected");
        assert_eq!(files[3].data.len(), 602);
        assert!(collect_family_b_files(b"junk").is_err());
    }

    #[test]
    fn family_b_module_round_trip() {
        let (base, f2) = (base_ingame(), base_f2());
        let img = family_b_image();
        let files = collect_family_b_files(&stage_disk()).unwrap();
        // without the disk files a B hack is refused with an explanation
        let e = make(&base, &f2, Input::Image(img.clone()), None, meta()).unwrap_err();
        assert!(e.contains("disk image"), "{e}");
        // files with the original engine are refused
        assert!(make_with_files(&base, &f2, Input::Image(edited()), None, meta(), files.clone()).is_err());
        let m = make_with_files(&base, &f2, Input::Image(img.clone()), None, meta(), files.clone()).unwrap();
        assert_eq!(m.meta.engine.as_deref(), Some("b"));
        assert_eq!(m.files.len(), 4);
        let bytes = m.write();
        let back = Module::read(&bytes).unwrap();
        assert_eq!(back.meta.format, 2);
        let a = apply(&back, &base, &f2).unwrap();
        assert_eq!(a.ingame, normalize_image(&img).unwrap());
        assert_eq!(a.files, files);
        assert_eq!(a.engine.as_deref(), Some("b"));
        assert!(a.code_changes.len() > 1, "the engine changes code the port runs itself");
        // the id covers the files: another stage file is another module
        let mut files2 = files.clone();
        files2[2].data[3] ^= 1;
        let m2 = make_with_files(&base, &f2, Input::Image(img.clone()), None, meta(), files2).unwrap();
        let a2 = apply(&m2, &base, &f2).unwrap();
        assert_ne!(a.id, a2.id);
        assert_eq!(a.id, apply(&back, &base, &f2).unwrap().id, "stable");
        // an original-engine module keeps the old id formula
        let o = make(&base, &f2, Input::Image(edited()), None, meta()).unwrap();
        let ao = apply(&o, &base, &f2).unwrap();
        assert_eq!(ao.id, crate::apply::module_id(&ao.ingame, &ao.f2));
        assert!(ao.files.is_empty() && ao.engine.is_none());
        assert_eq!(Module::read(&o.write()).unwrap().meta.format, 1, "no format 2 unless needed");
    }

    fn real_base() -> Option<(Vec<u8>, Vec<u8>)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = if dir.join("ingame.bin").exists() { dir } else { std::path::PathBuf::from("/home/user/Projekte/giana-native/data") };
        match (std::fs::read(dir.join("ingame.bin")), std::fs::read(dir.join("f_2.prg"))) {
            (Ok(b), Ok(f)) => Some((b, f)),
            _ => {
                eprintln!("skipped: no original data");
                None
            }
        }
    }

    /// With real family B disks (`GIANA_B_DISK=a.d64:b.d64:...`) and the original data: import every disk like the app
    /// does and check the result (engine, files, size, the same id on a second import, the leak check inside the
    /// maker passes). Skipped without both.
    #[test]
    fn real_family_b_disk() {
        let Some(paths) = std::env::var_os("GIANA_B_DISK") else {
            eprintln!("skipped: GIANA_B_DISK not set");
            return;
        };
        let Some((b, f)) = real_base() else { return };
        for path in std::env::split_paths(&paths) {
            let disk = std::fs::read(&path).unwrap();
            let m = make_from_file(&b, &f, &disk, "disk.d64", None, meta()).unwrap();
            assert_eq!(m.meta.engine.as_deref(), Some("b"), "{}", path.display());
            assert!(m.files.len() >= 20, "{} files", m.files.len());
            let bytes = m.write();
            assert!(bytes.len() < crate::container::MAX_SIZE, "{} bytes", bytes.len());
            let a = apply(&Module::read(&bytes).unwrap(), &b, &f).unwrap();
            assert_eq!(engine_tag(&a.ingame), Some("b"));
            let again = make_from_file(&b, &f, &disk, "disk.d64", None, meta()).unwrap();
            assert_eq!(apply(&again, &b, &f).unwrap().id, a.id, "the id is the same on a second import");
            eprintln!("family B module {}: {} bytes, {} files, id {}", path.display(), bytes.len(), a.files.len(), a.id);
        }
    }

    /// Disks that must be refused with a clear message (`GIANA_B_REFUSED=a.d64:...`: Giana Sisters II of 1993).
    #[test]
    fn real_older_engine_disk_is_refused() {
        let Some(paths) = std::env::var_os("GIANA_B_REFUSED") else {
            eprintln!("skipped: GIANA_B_REFUSED not set");
            return;
        };
        let Some((b, f)) = real_base() else { return };
        for path in std::env::split_paths(&paths) {
            let disk = std::fs::read(&path).unwrap();
            let e = make_from_file(&b, &f, &disk, "disk.d64", None, meta()).unwrap_err();
            assert!(e.contains("older version of the construction-set engine"), "{e}");
        }
    }
}
