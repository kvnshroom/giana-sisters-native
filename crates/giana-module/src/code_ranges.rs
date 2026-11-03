//! Which changes of a module touch 6502 code the port replaces with its own Rust code (such changes have no effect).

include!("code_ranges_table.rs");

/// Changed byte ranges `(start, length)` between `base` and `target` that overlap [`CODE_RANGES`], clipped to them.
pub fn code_changes(base: &[u8], target: &[u8]) -> Vec<(usize, usize)> {
    code_changes_in(CODE_RANGES, base, target)
}

pub(crate) fn code_changes_in(table: &[(u16, u16)], base: &[u8], target: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (start, len) in giana_extract::image::diff_ranges(base, target) {
        let (c0, c1) = (start, start + len); // [c0, c1)
        for &(a, b) in table {
            let (r0, r1) = (a as usize, b as usize + 1);
            let (s, e) = (c0.max(r0), c1.min(r1));
            if s < e {
                out.push((s, e - s));
            }
        }
    }
    out.sort_unstable();
    // merge touching/overlapping pieces
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, l) in out {
        match merged.last_mut() {
            Some(last) if s <= last.0 + last.1 => last.1 = last.1.max(s + l - last.0),
            _ => merged.push((s, l)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_with_table() {
        let base = vec![0u8; 0x300];
        let mut t = base.clone();
        t[0x10] = 1; // outside
        for b in &mut t[0x1F0..0x230] {
            *b = 9; // straddles the start of the range 0x200..=0x20F and covers it
        }
        let table = [(0x0200u16, 0x020Fu16), (0x0280, 0x0280)];
        assert_eq!(code_changes_in(&table, &base, &t), vec![(0x200, 16)]);
        t[0x280] = 5;
        assert_eq!(code_changes_in(&table, &base, &t), vec![(0x200, 16), (0x280, 1)]);
        assert!(code_changes_in(&[], &base, &t).is_empty());
        assert!(code_changes(&base, &base).is_empty());
    }
}
