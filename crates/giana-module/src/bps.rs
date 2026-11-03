//! BPS1 patches (byuu's format): `decode` validates everything, `encode` produces compact patches.
//!
//! Layout: `"BPS1"`, varint source size, varint target size, varint metadata size, metadata, actions, then three
//! little-endian CRC32 values (source, target, patch over everything before the last four bytes).

use crate::crc32::crc32;
use std::collections::HashMap;

/// Largest target the decoder is willing to allocate.
const MAX_TARGET: u64 = 64 << 20;

pub(crate) fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let x = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(0x80 | x);
            return;
        }
        out.push(x);
        n -= 1;
    }
}

pub(crate) fn get_varint(data: &[u8], pos: &mut usize, end: usize) -> Result<u64, String> {
    let mut value: u64 = 0;
    let mut shift: u64 = 1;
    loop {
        if *pos >= end {
            return Err("BPS: patch ends inside a number".into());
        }
        let x = data[*pos];
        *pos += 1;
        value = ((x & 0x7F) as u64)
            .checked_mul(shift)
            .and_then(|add| value.checked_add(add))
            .ok_or("BPS: number too large")?;
        if x & 0x80 != 0 {
            return Ok(value);
        }
        shift = shift.checked_mul(128).ok_or("BPS: number too large")?;
        value = value.checked_add(shift).ok_or("BPS: number too large")?;
    }
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn apply_delta(rel: usize, v: u64) -> Result<usize, String> {
    let delta = (v >> 1) as i64;
    let rel = rel as i64;
    let r = if v & 1 != 0 { rel.checked_sub(delta) } else { rel.checked_add(delta) };
    match r {
        Some(r) if r >= 0 => Ok(r as usize),
        _ => Err("BPS: copy offset out of range".into()),
    }
}

/// Applies `patch` to `source`; checks magic, sizes and all three CRC32 values.
pub fn decode(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    decode_max(source, patch, MAX_TARGET)
}

/// [`decode`] for a patch whose target must not be larger than `max_target` bytes (checked on the header, before anything
/// is allocated).
pub fn decode_max(source: &[u8], patch: &[u8], max_target: u64) -> Result<Vec<u8>, String> {
    if patch.len() < 4 + 3 + 12 || &patch[..4] != b"BPS1" {
        return Err("BPS: not a BPS patch".into());
    }
    let end = patch.len() - 12;
    if crc32(&patch[..patch.len() - 4]) != le32(&patch[patch.len() - 4..]) {
        return Err("BPS: patch checksum mismatch (damaged file)".into());
    }
    if crc32(source) != le32(&patch[end..]) {
        return Err("BPS: the patch was made for different original data (source checksum mismatch)".into());
    }
    let mut pos = 4;
    let src_size = get_varint(patch, &mut pos, end)?;
    let tgt_size = get_varint(patch, &mut pos, end)?;
    let meta_size = get_varint(patch, &mut pos, end)?;
    if src_size != source.len() as u64 {
        return Err("BPS: source size mismatch".into());
    }
    if tgt_size > max_target.min(MAX_TARGET) {
        return Err("BPS: target too large".into());
    }
    if meta_size > (end - pos) as u64 {
        return Err("BPS: metadata exceeds the patch".into());
    }
    pos += meta_size as usize;
    let tgt_size = tgt_size as usize;
    let mut out: Vec<u8> = Vec::with_capacity(tgt_size);
    let (mut src_rel, mut tgt_rel) = (0usize, 0usize);
    while pos < end {
        let cmd = get_varint(patch, &mut pos, end)?;
        let len = (cmd >> 2)
            .checked_add(1)
            .filter(|&l| l <= (tgt_size - out.len()) as u64)
            .ok_or("BPS: action writes past the end of the target")? as usize;
        let o = out.len();
        match cmd & 3 {
            0 => {
                if o + len > source.len() {
                    return Err("BPS: SourceRead past the end of the source".into());
                }
                out.extend_from_slice(&source[o..o + len]);
            }
            1 => {
                if len > end - pos {
                    return Err("BPS: literal data exceeds the patch".into());
                }
                out.extend_from_slice(&patch[pos..pos + len]);
                pos += len;
            }
            2 => {
                let v = get_varint(patch, &mut pos, end)?;
                src_rel = apply_delta(src_rel, v)?;
                if src_rel.checked_add(len).is_none_or(|e| e > source.len()) {
                    return Err("BPS: SourceCopy outside the source".into());
                }
                out.extend_from_slice(&source[src_rel..src_rel + len]);
                src_rel += len;
            }
            _ => {
                let v = get_varint(patch, &mut pos, end)?;
                tgt_rel = apply_delta(tgt_rel, v)?;
                if tgt_rel >= o {
                    return Err("BPS: TargetCopy from data not yet written".into());
                }
                for k in 0..len {
                    let b = out[tgt_rel + k];
                    out.push(b);
                }
                tgt_rel += len;
            }
        }
    }
    if out.len() != tgt_size {
        return Err("BPS: patch produces the wrong number of bytes".into());
    }
    if crc32(&out) != le32(&patch[end + 4..]) {
        return Err("BPS: result checksum mismatch".into());
    }
    Ok(out)
}

/// Header + actions + footer. Used by the encoder and by tests that hand-build patches.
pub(crate) fn wrap(source: &[u8], target: &[u8], actions: &[u8]) -> Vec<u8> {
    let mut p = b"BPS1".to_vec();
    put_varint(&mut p, source.len() as u64);
    put_varint(&mut p, target.len() as u64);
    put_varint(&mut p, 0);
    p.extend_from_slice(actions);
    p.extend_from_slice(&crc32(source).to_le_bytes());
    p.extend_from_slice(&crc32(target).to_le_bytes());
    let c = crc32(&p);
    p.extend_from_slice(&c.to_le_bytes());
    p
}

/// Hand-built action: a literal run (tests).
#[cfg(test)]
pub(crate) fn action_target_read(out: &mut Vec<u8>, bytes: &[u8]) {
    put_varint(out, (((bytes.len() - 1) as u64) << 2) | 1);
    out.extend_from_slice(bytes);
}

const KEY: usize = 4;
const MIN_COPY: usize = 8;
const MIN_SOURCE_READ: usize = 4;
const CANDIDATES: usize = 8;

fn key(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]])
}

fn common(a: &[u8], ai: usize, b: &[u8], bi: usize) -> usize {
    let mut n = 0;
    while ai + n < a.len() && bi + n < b.len() && a[ai + n] == b[bi + n] {
        n += 1;
    }
    n
}

struct Enc<'a> {
    tgt: &'a [u8],
    out: Vec<u8>,
    lit_start: usize,
    src_rel: usize,
    tgt_rel: usize,
}

impl Enc<'_> {
    fn flush(&mut self, upto: usize) {
        if upto > self.lit_start {
            let n = upto - self.lit_start;
            put_varint(&mut self.out, (((n - 1) as u64) << 2) | 1);
            self.out.extend_from_slice(&self.tgt[self.lit_start..upto]);
        }
    }

    fn copy(&mut self, at: usize, mode: u64, from: usize, len: usize) {
        self.flush(at);
        put_varint(&mut self.out, (((len - 1) as u64) << 2) | mode);
        let rel = if mode == 2 { &mut self.src_rel } else { &mut self.tgt_rel };
        let (neg, mag) = if from >= *rel { (0, from - *rel) } else { (1, *rel - from) };
        put_varint(&mut self.out, ((mag as u64) << 1) | neg);
        *rel = from + len;
        self.lit_start = at + len;
    }
}

/// Makes a BPS patch from `source` to `target`. Prefers SourceRead/SourceCopy/TargetCopy over literals, so data that
/// merely moved is referenced and never embedded.
pub fn encode(source: &[u8], target: &[u8]) -> Vec<u8> {
    let mut sidx: HashMap<u32, Vec<u32>> = HashMap::new();
    for i in 0..source.len().saturating_sub(KEY - 1) {
        sidx.entry(key(source, i)).or_default().push(i as u32);
    }
    let mut tidx: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut tindexed = 0usize; // positions < tindexed are in tidx
    let mut e = Enc { tgt: target, out: Vec::new(), lit_start: 0, src_rel: 0, tgt_rel: 0 };
    let mut o = 0usize;
    while o < target.len() {
        // positions whose 4 bytes lie fully before o can be copy sources
        while tindexed + KEY <= o {
            tidx.entry(key(target, tindexed)).or_default().push(tindexed as u32);
            tindexed += 1;
        }
        let sr = if o < source.len() { common(source, o, target, o) } else { 0 };
        let (mut sc_len, mut sc_from) = (0, 0);
        let (mut tc_len, mut tc_from) = (0, 0);
        if o + KEY <= target.len() {
            let k = key(target, o);
            let mut cands: Vec<usize> = Vec::new();
            if e.src_rel < source.len() {
                cands.push(e.src_rel);
            }
            if let Some(list) = sidx.get(&k) {
                let mid = list.partition_point(|&p| (p as usize) < o);
                let lo = mid.saturating_sub(CANDIDATES);
                let hi = (mid + CANDIDATES).min(list.len());
                cands.extend(list[lo..hi].iter().map(|&p| p as usize));
                cands.extend(list[..CANDIDATES.min(list.len())].iter().map(|&p| p as usize));
            }
            for c in cands {
                let l = common(source, c, target, o);
                if l > sc_len {
                    sc_len = l;
                    sc_from = c;
                }
            }
            if let Some(list) = tidx.get(&k) {
                for &p in list.iter().rev().take(CANDIDATES * 2) {
                    let l = common(target, p as usize, target, o);
                    if l > tc_len {
                        tc_len = l;
                        tc_from = p as usize;
                    }
                }
            }
        }
        let best_copy = sc_len.max(tc_len);
        if sr >= MIN_SOURCE_READ && sr + 2 >= best_copy {
            e.flush(o);
            put_varint(&mut e.out, ((sr - 1) as u64) << 2);
            o += sr;
            e.lit_start = o;
        } else if best_copy >= MIN_COPY {
            if sc_len >= tc_len {
                e.copy(o, 2, sc_from, sc_len);
                o += sc_len;
            } else {
                e.copy(o, 3, tc_from, tc_len);
                o += tc_len;
            }
        } else {
            o += 1;
        }
    }
    e.flush(target.len());
    wrap(source, target, &e.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rng;

    fn rand_vec(seed: u64, n: usize) -> Vec<u8> {
        let mut r = test_rng(seed);
        (0..n).map(|_| r()).collect()
    }

    fn roundtrip(s: &[u8], t: &[u8]) -> Vec<u8> {
        let p = encode(s, t);
        assert_eq!(decode(s, &p).unwrap(), t);
        p
    }

    #[test]
    fn varint_roundtrip() {
        for n in [0u64, 1, 127, 128, 129, 16511, 16512, 1 << 32, u64::MAX / 4] {
            let mut v = Vec::new();
            put_varint(&mut v, n);
            let mut pos = 0;
            assert_eq!(get_varint(&v, &mut pos, v.len()).unwrap(), n);
            assert_eq!(pos, v.len());
        }
        // byuu's reference: 128 -> 0x00 0x80
        let mut v = Vec::new();
        put_varint(&mut v, 128);
        assert_eq!(v, [0x00, 0x80]);
    }

    #[test]
    fn identical_is_tiny() {
        let s = rand_vec(1, 65536);
        let p = roundtrip(&s, &s);
        assert!(p.len() < 40, "{}", p.len());
    }

    #[test]
    fn small_edits_are_small() {
        let s = rand_vec(2, 65536);
        let mut t = s.clone();
        t[100] ^= 0xFF;
        t[30000] = 7;
        t[30001] = 8;
        let p = roundtrip(&s, &t);
        assert!(p.len() < 80, "{}", p.len());
    }

    #[test]
    fn random_unrelated() {
        let s = rand_vec(3, 3000);
        let t = rand_vec(4, 5000);
        roundtrip(&s, &t);
    }

    #[test]
    fn moved_blocks_growth_shrink() {
        let s = rand_vec(5, 65536);
        let mut t = s.clone();
        let block: Vec<u8> = t.drain(1000..9000).collect();
        t.splice(40000..40000, block);
        let p = roundtrip(&s, &t);
        assert!(p.len() < 100, "{}", p.len());
        let mut grown = s.clone();
        grown.splice(20000..20000, rand_vec(6, 500));
        roundtrip(&s, &grown);
        let shrunk = s[..50000].to_vec();
        roundtrip(&s, &shrunk);
        roundtrip(&s, &[]);
        roundtrip(&[], &s[..1000]);
        roundtrip(&[], &[]);
    }

    #[test]
    fn repeating_target_uses_target_copy() {
        let s = rand_vec(7, 1000);
        let mut t = Vec::new();
        for _ in 0..200 {
            t.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        }
        let p = roundtrip(&s, &t);
        assert!(p.len() < 80, "{}", p.len());
        roundtrip(&s, &vec![0u8; 40000]);
    }

    #[test]
    fn zero_heavy_sources() {
        let mut s = vec![0u8; 65536];
        s[5000..5100].copy_from_slice(&rand_vec(8, 100));
        let mut t = vec![0u8; 65536];
        t[9000..9100].copy_from_slice(&s[5000..5100]);
        let started = std::time::Instant::now();
        let p = roundtrip(&s, &t);
        assert!(started.elapsed().as_secs() < 2);
        assert!(p.len() < 120, "{}", p.len());
    }

    #[test]
    fn corrupt_patches_are_rejected() {
        let s = rand_vec(9, 4000);
        let mut t = s.clone();
        t[10] ^= 1;
        let p = encode(&s, &t);
        assert!(decode(&s, &p).is_ok());
        let mut other = s.clone();
        other[0] ^= 1;
        assert!(decode(&other, &p).unwrap_err().contains("different original"));
        for i in 0..p.len() {
            let mut q = p.clone();
            q[i] ^= 0x10;
            assert!(decode(&s, &q).is_err(), "flip at {i}");
        }
        for n in 0..p.len() {
            assert!(decode(&s, &p[..n]).is_err());
        }
        assert!(decode(&s, b"nope").is_err());
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let s = rand_vec(10, 300);
        let mut r = test_rng(11);
        for round in 0..3000u64 {
            let n = (r() as usize) % 64;
            let junk: Vec<u8> = (0..n).map(|_| r()).collect();
            let _ = decode(&s, &junk);
            // valid framing (header + CRCs) around random actions
            let actions: Vec<u8> = (0..(r() as usize % 40)).map(|_| r()).collect();
            let target = rand_vec(round, (r() as usize) % 50);
            let p = wrap(&s, &target, &actions);
            let _ = decode(&s, &p);
        }
    }
}
