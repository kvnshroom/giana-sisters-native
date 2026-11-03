//! Per-stage constants of the hack image: what the stage builder's hook (`$2F54` -> `$306A`, `$30AA..$3125`) and the
//! palette routine (`$0D2E`) write for a stage.
//!
//! The code writes only fixed addresses with values read from instruction operands and a small table, so the port
//! reads the same operands from the image instead of running the code (and never hard-codes them: other hacks of the
//! family have other numbers at the same places, Power Edition differs exactly here). Every write is unconditional,
//! so the result depends on the stage number only:
//!
//! | Writes | Value |
//! |---|---|
//! | `$ABD0..$ABD2`, `$ABD3` | stage `== imm($30AC)`: `imm($30B0)`, `imm($30B2)`; else `imm($30C2)`, `imm($30C4)` |
//! | `$ABA1..$ABA5`, `$ABF3..$ABF7` | stage `== imm($30CA)`: `imm($30CE)`; else `imm($30D1)` |
//! | `$AB8F` | `$2D17[i]` for the first `i` (1, then 0) with `$2D14[i] == stage`; else `imm($30F0)` |
//! | `$ABF9`, `$ABFA` | stage `== imm($30FA)`: `imm($30FE)`; else `imm($3101)` |
//! | `$9CFF..$9D03` (tile classes) | stage `== imm($310B)`: `imm($310F)`; else `imm($3112)` |
//! | `$C7C0..$C7C7` | 0 (eight bytes of the character set) |
//! | `$ABA6`, `$ABA7`, `$ABA8`, `$ABA9` (`$0D2E`) | diamond scheme `n` of the header: `$91E4[n]`, `$91ED[n]`, `$91ED[n]`, `$91F6[n]` |
//!
//! `imm(a)` is the operand byte at address `a + 1` of the instruction at `a`.

/// Opcode bytes that must stand where the table above reads operands; anything else is a different code layout and
/// the hook is not applied (the stage then keeps the colours of the image).
const SIGNATURE: [(usize, u8); 17] = [
    (0x30AC, 0xC9),
    (0x30B0, 0xA9),
    (0x30B2, 0xA0),
    (0x30C2, 0xA9),
    (0x30C4, 0xA0),
    (0x30CA, 0xC9),
    (0x30CE, 0xA9),
    (0x30D1, 0xA9),
    (0x30E2, 0xDD),
    (0x30E7, 0xBC),
    (0x30F0, 0xA0),
    (0x30FA, 0xC9),
    (0x30FE, 0xA9),
    (0x3101, 0xA9),
    (0x310B, 0xC9),
    (0x310F, 0xA9),
    (0x3112, 0xA9),
];

/// True when the image has the code layout of the stage hook at `$30AA` that [`apply_stage_constants`] reads.
pub fn has_stage_hook(img: &[u8]) -> bool {
    img.len() == 0x1_0000 && SIGNATURE.iter().all(|&(a, op)| img[a] == op)
}

/// The writable copies of the tables the hook changes: character colour attributes (`$AB00`, 256 bytes), tile
/// classes (`$9C20`, 256 bytes) and the first 2 KiB of the character set (`$C000`).
pub struct StageTables<'a> {
    pub colours: &'a mut [u8; 256],
    pub classes: &'a mut [u8; 256],
    pub charset: &'a mut [u8],
}

/// Applies the writes of the stage hook for `stage`. Returns false (and changes nothing) when the image has another
/// code layout.
pub fn apply_stage_constants(img: &[u8], stage: u8, t: &mut StageTables) -> bool {
    if !has_stage_hook(img) {
        return false;
    }
    let imm = |a: usize| img[a + 1];
    // $30AA..$30C7
    let (a, y) = if stage == imm(0x30AC) { (imm(0x30B0), imm(0x30B2)) } else { (imm(0x30C2), imm(0x30C4)) };
    for c in 0xD0..=0xD2 {
        t.colours[c] = a;
    }
    t.colours[0xD3] = y;
    // $30C8..$30DD: `BNE $30D1` lands on the `LDA #imm` of $30D1 and skips the one of $30CE
    let a = if stage == imm(0x30CA) { imm(0x30CE) } else { imm(0x30D1) };
    for x in 1..=5usize {
        t.colours[0xA0 + x] = a;
        t.colours[0xF2 + x] = a;
    }
    // $30DE..$30F7
    let mut v = imm(0x30F0);
    for x in (0..=1usize).rev() {
        if img[0x2D14 + x] == stage {
            v = img[0x2D17 + x];
            break;
        }
    }
    t.colours[0x8F] = v;
    // $30F8..$3108
    let a = if stage == imm(0x30FA) { imm(0x30FE) } else { imm(0x3101) };
    t.colours[0xF9] = a;
    t.colours[0xFA] = a;
    // $3109..$311B: X = 4..0 -> $9CFF..$9D03 = table entries $DF..$E3
    let a = if stage == imm(0x310B) { imm(0x310F) } else { imm(0x3112) };
    for x in 0..=4usize {
        t.classes[0xDF + x] = a;
    }
    // $311C..$3125: X = 8..1 -> $C7BF + X
    for x in 1..=8usize {
        if let Some(b) = t.charset.get_mut(0x7BF + x) {
            *b = 0;
        }
    }
    true
}

/// The four diamond colour attributes (`$ABA6..$ABA9`) for scheme `n` of the stage header (`$0D45..$0D54`, `$9049`).
pub fn diamond_colours(img: &[u8], scheme: u8) -> [u8; 4] {
    let at = |base: usize| img.get(base + scheme as usize).copied().unwrap_or(0);
    [at(0x91E4), at(0x91ED), at(0x91ED), at(0x91F6)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image with the code layout of the hook and invented operand values.
    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        for &(a, op) in &SIGNATURE {
            img[a] = op;
        }
        for (a, v) in [
            (0x30AC, 11),
            (0x30B0, 21),
            (0x30B2, 22),
            (0x30C2, 31),
            (0x30C4, 32),
            (0x30CA, 12),
            (0x30CE, 41),
            (0x30D1, 42),
            (0x30F0, 51),
            (0x30FA, 13),
            (0x30FE, 61),
            (0x3101, 62),
            (0x310B, 14),
            (0x310F, 71),
            (0x3112, 72),
        ] {
            img[a + 1] = v;
        }
        img[0x2D14] = 15;
        img[0x2D15] = 16;
        img[0x2D17] = 81;
        img[0x2D18] = 82;
        img[0x91E4..0x91E6].copy_from_slice(&[100, 101]);
        img[0x91ED..0x91EF].copy_from_slice(&[101, 103]);
        img[0x91F6..0x91F8].copy_from_slice(&[102, 104]);
        img
    }

    fn run(img: &[u8], stage: u8) -> ([u8; 256], [u8; 256], Vec<u8>) {
        let (mut c, mut k, mut ch) = ([9u8; 256], [9u8; 256], vec![9u8; 0x800]);
        assert!(apply_stage_constants(img, stage, &mut StageTables { colours: &mut c, classes: &mut k, charset: &mut ch }));
        (c, k, ch)
    }

    #[test]
    fn writes_depend_on_the_stage() {
        let img = image();
        let (c, k, ch) = run(&img, 1);
        assert_eq!((c[0xD0], c[0xD2], c[0xD3]), (31, 31, 32));
        assert_eq!((c[0xA1], c[0xA5], c[0xA0], c[0xA6], c[0xF3], c[0xF7], c[0xF2], c[0xF8]), (42, 42, 9, 9, 42, 42, 9, 9));
        assert_eq!(c[0x8F], 51);
        assert_eq!((c[0xF9], c[0xFA]), (62, 62));
        assert_eq!(&k[0xDF..0xE4], &[72; 5]);
        assert_eq!(k[0xDE], 9);
        assert_eq!((ch[0x7C0], ch[0x7C7], ch[0x7BF], ch[0x7C8]), (0, 0, 9, 9));
        assert_eq!(run(&img, 11).0[0xD0], 21);
        assert_eq!(run(&img, 11).0[0xD3], 22);
        assert_eq!(run(&img, 12).0[0xA3], 41);
        assert_eq!(run(&img, 15).0[0x8F], 81, "table entry 0");
        assert_eq!(run(&img, 16).0[0x8F], 82, "table entry 1");
        assert_eq!(run(&img, 13).0[0xF9], 61);
        assert_eq!(run(&img, 14).1[0xE3], 71);
    }

    #[test]
    fn other_code_layouts_are_left_alone() {
        let mut img = image();
        img[0x30AC] = 0xEA;
        let (mut c, mut k, mut ch) = ([9u8; 256], [9u8; 256], vec![9u8; 0x800]);
        assert!(!apply_stage_constants(&img, 1, &mut StageTables { colours: &mut c, classes: &mut k, charset: &mut ch }));
        assert!(c.iter().all(|&b| b == 9));
        assert!(!has_stage_hook(&img[..100]));
    }

    #[test]
    fn diamonds() {
        assert_eq!(diamond_colours(&image(), 1), [101, 103, 103, 104]);
    }
}
