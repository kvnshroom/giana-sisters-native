//! Building the 64K program image from the PRG file `1`, and comparing it with a reference image.
//!
//! What the image contains:
//!
//! - `$0820..=$FFF0`: the bytes of file `1`, unchanged (the "unpatched" image).
//! - `$0000..=$081F` and `$FFF1..=$FFFF`: zero. The reference image `re/ingame.bin` holds a RAM snapshot there
//!   (zero page, stack, screen, BASIC and KERNAL variables, copies of KERNAL ROM bytes, vectors); that is run-time
//!   state, not part of the program, and the port does not depend on it.
//! - The patches in [`PATCHES`] are a separate step ([`apply_patches`]).

/// Name of the program file on the disk.
pub const PROGRAM_FILE: &str = "1";
/// Load address of the program file.
pub const LOAD_ADDRESS: usize = 0x0820;

/// A documented change of the original program bytes.
pub struct Patch {
    pub addr: usize,
    pub bytes: &'static [u8],
    pub why: &'static str,
}

/// Patches that the reference image `re/ingame.bin` contains on top of the file.
pub const PATCHES: &[Patch] = &[
    Patch {
        addr: 0x0820,
        bytes: &[0x4C, 0x32, 0x08],
        why: "JSR $C800 -> JMP $0832: skips the copy protection check at the entry (analysis/skip_prot.mon)",
    },
    Patch {
        addr: 0x1277,
        bytes: &[0x04],
        why: "ORA $0405 -> ORA $0404 at $1276: cloud scroll patch (the port does not depend on it)",
    },
];

/// Places the PRG (with its two byte load address) into a 64K image.
pub fn build_unpatched(prg: &[u8]) -> Result<Vec<u8>, String> {
    if prg.len() < 3 {
        return Err("program file is empty".into());
    }
    let load = prg[0] as usize | (prg[1] as usize) << 8;
    if load != LOAD_ADDRESS {
        return Err(format!("program file loads at ${load:04X}, expected ${LOAD_ADDRESS:04X}"));
    }
    let body = &prg[2..];
    if LOAD_ADDRESS + body.len() > 0x1_0000 {
        return Err("program file does not fit into 64K".into());
    }
    let mut img = vec![0u8; 0x1_0000];
    img[LOAD_ADDRESS..LOAD_ADDRESS + body.len()].copy_from_slice(body);
    Ok(img)
}

/// Applies [`PATCHES`] to a copy of the image.
pub fn apply_patches(unpatched: &[u8]) -> Vec<u8> {
    let mut img = unpatched.to_vec();
    for p in PATCHES {
        img[p.addr..p.addr + p.bytes.len()].copy_from_slice(p.bytes);
    }
    img
}

/// Byte ranges (start, length) in which two images differ.
pub fn diff_ranges(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    let n = a.len().min(b.len());
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if a[i] != b[i] {
            let s = i;
            while i < n && a[i] != b[i] {
                i += 1;
            }
            out.push((s, i - s));
        } else {
            i += 1;
        }
    }
    out
}
