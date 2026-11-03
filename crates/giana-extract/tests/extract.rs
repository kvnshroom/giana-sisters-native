//! Runs against the private development disk image; skipped where it is absent (e.g. a public checkout).

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn unknown_version_is_rejected_with_a_clear_error() {
    let err = giana_extract::extract(b"definitely not a disk").err().unwrap();
    assert!(err.contains("not a G64"), "{err}");
    let mut fake = b"GCR-1541".to_vec();
    fake.extend_from_slice(&[0; 64]);
    let err = giana_extract::extract(&fake).err().unwrap();
    assert!(err.contains("unknown disk image version"), "{err}");
    assert!(err.contains("5ce29ce0"), "{err}");
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
