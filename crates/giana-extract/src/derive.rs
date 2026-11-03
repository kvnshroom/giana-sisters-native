//! Derives every data file of the manifest from the two disk files and verifies it against the expected size and
//! SHA-256 (the hashes are embedded here, never the data).
//!
//! `ingame.bin` is the program image (see `image`). The four tables are cut out of it. `title_koala.bin` comes from the
//! title program `f_2.prg`, a crunched BASIC program (`SYS 2064`): its own unpacker runs on a small 6502 interpreter
//! (`mini6502`) until it jumps to the program at `$080E`. Then the title picture lies in memory in Koala layout: bitmap
//! `$2000..$3F3F`, screen RAM `$3F40..$4327`, colour RAM `$4328..$470F`, background colour `$4710`.

use crate::mini6502::Cpu;
use crate::sha256::sha256_hex;
use std::path::Path;

/// One file of the data directory: name, size and SHA-256 it must have.
pub struct ManifestEntry {
    pub name: &'static str,
    pub size: usize,
    pub sha256: &'static str,
}

/// Same content as `tools/data/manifest.txt` (a unit test keeps them equal).
pub const MANIFEST: &[ManifestEntry] = &[
    ManifestEntry { name: "collision_table.bin", size: 256, sha256: "667526cbc31e27d88d7e5871b646f96ff904d5b76b81494f29e425f41083b22b" },
    ManifestEntry { name: "colortable.bin", size: 256, sha256: "74ccc485e71ad8505c1c30932f07f728624f822e9277fecb659f63209287966a" },
    ManifestEntry { name: "f_2.prg", size: 20088, sha256: "389ec667bab8bf119facad9706191d0ac79103950f3fa45dd3fe74e20c2676e0" },
    ManifestEntry { name: "ingame.bin", size: 65536, sha256: "5fe942fa64cd73395f852d49156fac3b0fc0bac120f7375efd52248f1c142c6e" },
    ManifestEntry { name: "stage1_map.bin", size: 5120, sha256: "e75d388d6b80e7ec82064fc6a9309c16dadedf34a73425b52e4712be1fe7c99c" },
    ManifestEntry { name: "stage_palettes.bin", size: 132, sha256: "e75786ccc9337b8a90c84b7bccc6b3c9d25de4b586234a4c85d72af5e6d0e456" },
    ManifestEntry { name: "title_koala.bin", size: 10001, sha256: "88975b52bb92932020cba739f4136e425fa1aec97e9cca3361ea3ddf54ef6cbb" },
];

pub struct DataFile {
    pub name: &'static str,
    pub data: Vec<u8>,
}

fn entry(name: &str) -> &'static ManifestEntry {
    MANIFEST.iter().find(|e| e.name == name).expect("name is in the manifest")
}

/// Does `data` have the size and hash of the manifest entry `name`?
pub fn matches_manifest(name: &str, data: &[u8]) -> bool {
    let e = entry(name);
    data.len() == e.size && sha256_hex(data) == e.sha256
}

/// The four tables cut out of the program image, plus the per-stage palette table.
pub fn derive_tables(img: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let mut pal = Vec::with_capacity(33 * 4);
    for s in 0..33 {
        pal.extend_from_slice(&[img[0x604E + s] & 15, img[0x6075 + s] & 15, img[0x609C + s] & 15, img[0x60C3 + s]]);
    }
    vec![
        ("collision_table.bin", img[0x9C20..0x9D20].to_vec()),
        ("colortable.bin", img[0xAB00..0xAC00].to_vec()),
        ("stage1_map.bin", img[0xAC00..0xC000].to_vec()),
        ("stage_palettes.bin", pal),
    ]
}

/// Runs the unpacker of `f_2.prg` and returns the title picture (Koala layout, 10001 bytes).
pub fn title_koala(prg: &[u8]) -> Result<Vec<u8>, String> {
    if prg.len() < 3 || prg[0] != 0x01 || prg[1] != 0x08 || 0x801 + prg.len() - 2 > 0x1_0000 {
        return Err("title program (disk file 2) has an unexpected format".into());
    }
    let mut cpu = Cpu::new();
    cpu.m[0x801..0x801 + prg.len() - 2].copy_from_slice(&prg[2..]);
    cpu.pc = 0x810; // SYS 2064
    cpu.run_until(0x080E, 20_000_000).map_err(|e| format!("unpacking the title picture failed: {e}"))?;
    Ok(cpu.m[0x2000..0x4711].to_vec())
}

/// All manifest files, derived from `ingame.bin` and `f_2.prg`, sorted by name, **without** the manifest check. Used for
/// ROM modules, whose images differ from the original on purpose.
pub fn derive_files(ingame: Vec<u8>, f2: Vec<u8>) -> Result<Vec<DataFile>, String> {
    if ingame.len() != entry("ingame.bin").size {
        return Err(format!("ingame.bin has {} bytes, expected {}", ingame.len(), entry("ingame.bin").size));
    }
    let mut files: Vec<DataFile> = Vec::new();
    for (name, data) in derive_tables(&ingame) {
        files.push(DataFile { name: entry(name).name, data });
    }
    files.push(DataFile { name: "title_koala.bin", data: title_koala(&f2)? });
    files.push(DataFile { name: "ingame.bin", data: ingame });
    files.push(DataFile { name: "f_2.prg", data: f2 });
    files.sort_by_key(|f| f.name);
    Ok(files)
}

/// All manifest files, derived from `ingame.bin` and `f_2.prg` and verified against the manifest.
pub fn derive_all(ingame: Vec<u8>, f2: Vec<u8>) -> Result<Vec<DataFile>, String> {
    let files = derive_files(ingame, f2)?;
    for f in &files {
        if !matches_manifest(f.name, &f.data) {
            return Err(format!("derived {} does not match the expected size/SHA-256", f.name));
        }
    }
    Ok(files)
}

/// Writes the files into `dir` (created if needed). Writes to temporary names first, so an interrupted run does not
/// leave a half-valid directory that looks complete.
pub fn write_data_dir(files: &[DataFile], dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the data folder {}: {e}", dir.display()))?;
    for f in files {
        let tmp = dir.join(format!("{}.part", f.name));
        std::fs::write(&tmp, &f.data).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    }
    for f in files {
        let (tmp, dst) = (dir.join(format!("{}.part", f.name)), dir.join(f.name));
        std::fs::rename(&tmp, &dst).map_err(|e| format!("cannot write {}: {e}", dst.display()))?;
    }
    Ok(())
}

/// Checks all manifest files in `dir`. Returns a list of problems (empty = fine).
pub fn verify_dir(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for e in MANIFEST {
        match std::fs::read(dir.join(e.name)) {
            Err(_) => problems.push(format!("missing {}", e.name)),
            Ok(d) if !matches_manifest(e.name, &d) => problems.push(format!("{} has the wrong content", e.name)),
            Ok(_) => {}
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_equals_manifest_txt() {
        let text = include_str!("../../../tools/data/manifest.txt");
        let rows: Vec<Vec<&str>> =
            text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).map(|l| l.split_whitespace().collect()).collect();
        assert_eq!(rows.len(), MANIFEST.len());
        for (row, e) in rows.iter().zip(MANIFEST) {
            assert_eq!((row[0], row[1].parse::<usize>().unwrap(), row[2]), (e.name, e.size, e.sha256));
        }
    }
}
