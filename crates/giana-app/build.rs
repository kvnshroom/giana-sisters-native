//! Gives every build an identity for the network game: a hash of the game logic (`giana-core`). Two computers with different
//! logic would drift apart, so they must refuse to play together.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../giana-core");
    let mut list = Vec::new();
    files(&core.join("src"), &mut list);
    list.push(core.join("Cargo.toml"));
    list.sort();
    // FNV-1a over the relative names and the contents
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for path in &list {
        let name = path.strip_prefix(&core).unwrap_or(path).to_string_lossy().replace('\\', "/");
        // carriage returns are skipped: a Windows checkout (CRLF) must give the same identity as the others
        for b in name.bytes().chain(std::fs::read(path).unwrap_or_default().into_iter().filter(|&b| b != b'\r')) {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=GIANA_CORE_HASH={:08x}", (h ^ (h >> 32)) as u32);
    println!("cargo:rerun-if-changed=../giana-core/src");
    println!("cargo:rerun-if-changed=../giana-core/Cargo.toml");
}
