//! Refuses patches that embed original data as literal bytes.

use crate::bps::get_varint;
use std::collections::HashSet;

const WINDOW: usize = 32;
const MIN_DISTINCT: usize = 4;

/// The payloads of all TargetRead actions of a BPS patch.
pub fn literal_bytes(patch: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if patch.len() < 4 + 3 + 12 || &patch[..4] != b"BPS1" {
        return Err("BPS: not a BPS patch".into());
    }
    let end = patch.len() - 12;
    let mut pos = 4;
    get_varint(patch, &mut pos, end)?;
    get_varint(patch, &mut pos, end)?;
    let meta = get_varint(patch, &mut pos, end)?;
    if meta > (end - pos) as u64 {
        return Err("BPS: metadata exceeds the patch".into());
    }
    pos += meta as usize;
    let mut out = Vec::new();
    while pos < end {
        let cmd = get_varint(patch, &mut pos, end)?;
        let len = usize::try_from(cmd >> 2).ok().and_then(|l| l.checked_add(1)).ok_or("BPS: action too long")?;
        match cmd & 3 {
            0 => {}
            1 => {
                if len > end - pos {
                    return Err("BPS: literal data exceeds the patch".into());
                }
                out.push(patch[pos..pos + len].to_vec());
                pos += len;
            }
            _ => {
                get_varint(patch, &mut pos, end)?;
            }
        }
    }
    Ok(out)
}

fn distinct(w: &[u8]) -> usize {
    let mut seen = [false; 256];
    w.iter().filter(|&&b| !std::mem::replace(&mut seen[b as usize], true)).count()
}

/// Fails when a literal payload of `patch` contains a 32-byte window of `base` that has at least 4 distinct byte
/// values (zero runs, padding and the like are not evidence).
pub fn leak_check(base: &[u8], patch: &[u8]) -> Result<(), String> {
    let lits = literal_bytes(patch)?;
    let windows: HashSet<&[u8]> = base.windows(WINDOW).filter(|w| distinct(w) >= MIN_DISTINCT).collect();
    for lit in &lits {
        for w in lit.windows(WINDOW) {
            if windows.contains(w) {
                return Err("the patch would contain original game data as literal bytes".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bps::{action_target_read, encode, wrap};
    use crate::test_rng;

    fn base() -> Vec<u8> {
        let mut r = test_rng(21);
        (0..65536).map(|_| r()).collect()
    }

    #[test]
    fn moved_block_passes() {
        let s = base();
        let mut t = s.clone();
        let block: Vec<u8> = t.drain(2000..12000).collect();
        t.splice(30000..30000, block);
        t[5] ^= 1;
        let p = encode(&s, &t);
        leak_check(&s, &p).unwrap();
        assert!(literal_bytes(&p).unwrap().iter().all(|l| l.len() < 32));
    }

    #[test]
    fn embedded_block_fails() {
        let s = base();
        let mut t = s.clone();
        t[0] ^= 1;
        let mut actions = Vec::new();
        action_target_read(&mut actions, &t);
        let p = wrap(&s, &t, &actions);
        assert!(leak_check(&s, &p).is_err());
        assert_eq!(literal_bytes(&p).unwrap().len(), 1);
    }

    #[test]
    fn boring_windows_are_ignored() {
        let s = vec![0u8; 4096];
        let t = vec![0u8; 4096];
        let mut actions = Vec::new();
        action_target_read(&mut actions, &t);
        leak_check(&s, &wrap(&s, &t, &actions)).unwrap();
    }
}
