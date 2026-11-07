//! Runs against the private development disk image; skipped where it is absent (e.g. a public checkout).

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn unknown_version_is_rejected_with_a_clear_error() {
    let err = giana_extract::extract(b"definitely not a disk").err().unwrap();
    assert!(err.contains("not a G64 or D64"), "{err}");
    let mut fake = b"GCR-1541".to_vec();
    fake.extend_from_slice(&[0; 64]);
    let err = giana_extract::extract(&fake).err().unwrap();
    assert!(err.contains("not a G64 or D64"), "{err}");
}

#[test]
fn a_disk_with_other_game_files_is_rejected_before_any_unpacking() {
    let img = giana_extract::d64::build_d64(&[("1", &[0x20, 0x08, 1, 2, 3]), ("2", &[0, 4, 5, 6])]);
    let err = giana_extract::extract(&img).err().unwrap();
    assert!(err.contains("modified or cracked") && err.contains(giana_extract::GAME_FILE1_SHA256), "{err}");
    let img = giana_extract::d64::build_d64(&[("X", &[1, 2, 3])]);
    let err = giana_extract::extract(&img).err().unwrap();
    assert!(err.contains("does not hold the game files"), "{err}");
}

/// Writes the 35 standard tracks of a G64 as a D64 (missing sectors stay zero), optionally with error bytes.
fn g64_to_d64(g64: &[u8], error_bytes: bool) -> Vec<u8> {
    let disk = giana_extract::g64::Disk::parse(g64).unwrap();
    let mut out = Vec::new();
    for t in 1..=35u8 {
        for s in 0..giana_extract::g64::sectors_per_track(t) {
            out.extend_from_slice(disk.sector(t, s).unwrap_or(&[0; 256]));
        }
    }
    assert_eq!(out.len(), 174_848);
    if error_bytes {
        out.extend(std::iter::repeat_n(1u8, 683));
    }
    out
}

#[test]
fn a_d64_made_from_the_original_is_extracted_identically() {
    let Ok(g64) = std::fs::read(root().join("orig/Great_Giana_Sisters_The.g64")) else {
        eprintln!("skipped: orig/ not present");
        return;
    };
    let from_g64 = giana_extract::extract_all(&g64).unwrap();
    for error_bytes in [false, true] {
        let d64 = g64_to_d64(&g64, error_bytes);
        let files = giana_extract::extract_all(&d64).unwrap();
        assert_eq!(files.len(), from_g64.len());
        for (a, b) in files.iter().zip(&from_g64) {
            assert_eq!((a.name, &a.data), (b.name, &b.data));
        }
        assert_eq!(giana_extract::identify(&d64), giana_extract::Identity::Original);
    }
}

#[test]
fn another_dump_of_the_same_disk_is_accepted() {
    let Some(path) = std::env::var_os("GIANA_ALT_G64") else {
        eprintln!("skipped: GIANA_ALT_G64 is not set");
        return;
    };
    let g64 = std::fs::read(path).unwrap();
    let files = giana_extract::extract_all(&g64).unwrap();
    for f in &files {
        assert!(giana_extract::derive::matches_manifest(f.name, &f.data), "{}", f.name);
    }
}

#[test]
fn extracts_the_reference_image() {
    let (Ok(g64), Ok(reference)) = (
        std::fs::read(root().join("orig/Great_Giana_Sisters_The.g64")),
        std::fs::read(root().join("re/ingame.bin")),
    ) else {
        eprintln!("skipped: orig/ or re/ not present");
        return;
    };
    let ex = giana_extract::extract(&g64).unwrap();
    let d = giana_extract::image::diff_ranges(&ex.patched, &reference);
    // identical inside the program area; outside it the reference holds a RAM snapshot
    assert!(d.iter().all(|&(s, l)| s + l <= 0x0820 || s > 0xFFF0), "{d:?}");
    // the unpatched image differs from the patched one exactly at the patches
    let p = giana_extract::image::diff_ranges(&ex.unpatched, &ex.patched);
    assert_eq!(p, vec![(0x0820, 3), (0x1277, 1)]);
}

#[test]
fn derives_every_manifest_file_from_the_disk_image() {
    let Ok(g64) = std::fs::read(root().join("orig/Great_Giana_Sisters_The.g64")) else {
        eprintln!("skipped: orig/ not present");
        return;
    };
    let files = giana_extract::extract_all(&g64).unwrap();
    assert_eq!(files.len(), giana_extract::MANIFEST.len());
    for f in &files {
        assert!(giana_extract::derive::matches_manifest(f.name, &f.data), "{}", f.name);
    }
}

#[test]
fn derive_files_accepts_a_modified_image_and_derive_all_rejects_it() {
    let Ok(g64) = std::fs::read(root().join("orig/Great_Giana_Sisters_The.g64")) else {
        eprintln!("skipped: orig/ not present");
        return;
    };
    let files = giana_extract::extract_all(&g64).unwrap();
    let get = |n: &str| files.iter().find(|f| f.name == n).unwrap().data.clone();
    let (ingame, f2) = (get("ingame.bin"), get("f_2.prg"));
    let same = giana_extract::derive::derive_files(ingame.clone(), f2.clone()).unwrap();
    // the optional ATG file only comes with the disk, not with derive_files
    assert_eq!(same.iter().map(|f| f.name).collect::<Vec<_>>(), files.iter().map(|f| f.name).filter(|n| *n != "atg.prg").collect::<Vec<_>>());
    let mut hacked = ingame.clone();
    hacked[0xAC10] ^= 1;
    let out = giana_extract::derive::derive_files(hacked.clone(), f2.clone()).unwrap();
    let map = out.iter().find(|f| f.name == "stage1_map.bin").unwrap();
    assert_eq!(map.data[0x10], ingame[0xAC10] ^ 1);
    assert!(giana_extract::derive::derive_all(hacked, f2).is_err());
}
