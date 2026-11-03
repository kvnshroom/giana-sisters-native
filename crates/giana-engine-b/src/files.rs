//! How the hack finds its stage and monster files on the disk.
//!
//! The stage loader (`$9053`) uses KERNAL `SETNAM`/`LOAD` with a 16 character pattern that has `?` wildcards, so the
//! authors' file names do not matter, only the two leading characters and the end:
//!
//! - stage file: the template at `$91C5` (`00???????????STG` in Ridley X), the first two characters replaced by the
//!   stage number as two decimal digits (`01` .. `99`);
//! - monster definition: the template at `$91D5` (`00???????????NLM`), the first two characters replaced by the
//!   monster number from the stage header as two upper case hex digits, the last three by `NLM`, then `SPR` for the
//!   sprite shapes.
//!
//! Other hacks use other templates in the same two places (Junior: `00STG?????-ROLE-`); the template of the image is the
//! one to use.

pub use giana_module::container::ModFile;

pub const STAGE_TEMPLATE: usize = 0x91C5;
pub const MONSTER_TEMPLATE: usize = 0x91D5;
pub const PATTERN_LEN: usize = 16;

fn template(img: &[u8], at: usize) -> Option<[u8; PATTERN_LEN]> {
    img.get(at..at + PATTERN_LEN)?.try_into().ok()
}

/// The pattern for the stage file of `stage` (1..=99).
pub fn stage_pattern(img: &[u8], stage: u8) -> Option<[u8; PATTERN_LEN]> {
    if stage == 0 || stage > 99 {
        return None;
    }
    let mut p = template(img, STAGE_TEMPLATE)?;
    p[0] = b'0' + stage / 10;
    p[1] = b'0' + stage % 10;
    Some(p)
}

/// The pattern for a monster file: `ext` is `b"NLM"` or `b"SPR"`. Monster 0 means "none".
pub fn monster_pattern(img: &[u8], monster: u8, ext: &[u8; 3]) -> Option<[u8; PATTERN_LEN]> {
    if monster == 0 {
        return None;
    }
    let mut p = template(img, MONSTER_TEMPLATE)?;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    p[0] = HEX[(monster >> 4) as usize];
    p[1] = HEX[(monster & 15) as usize];
    p[PATTERN_LEN - 3..].copy_from_slice(ext);
    Some(p)
}

/// CBM DOS pattern match as `LOAD "pattern",8` does: `?` matches one character, `*` the rest; otherwise the lengths
/// must be equal.
pub fn name_matches(pattern: &[u8], name: &[u8]) -> bool {
    for (i, &p) in pattern.iter().enumerate() {
        if p == b'*' {
            return true;
        }
        match name.get(i) {
            Some(&c) if p == b'?' || p == c => {}
            _ => return false,
        }
    }
    name.len() == pattern.len()
}

/// The first file (in name order) that matches.
pub fn find<'a>(files: &'a [ModFile], pattern: &[u8]) -> Option<&'a ModFile> {
    files.iter().find(|f| name_matches(pattern, &f.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        img[STAGE_TEMPLATE..STAGE_TEMPLATE + 16].copy_from_slice(b"00???????????STG");
        img[MONSTER_TEMPLATE..MONSTER_TEMPLATE + 16].copy_from_slice(b"00???????????NLM");
        img
    }

    #[test]
    fn patterns() {
        let img = image();
        assert_eq!(&stage_pattern(&img, 1).unwrap(), b"01???????????STG");
        assert_eq!(&stage_pattern(&img, 43).unwrap(), b"43???????????STG");
        assert!(stage_pattern(&img, 0).is_none() && stage_pattern(&img, 100).is_none());
        assert_eq!(&monster_pattern(&img, 0x14, b"NLM").unwrap(), b"14???????????NLM");
        assert_eq!(&monster_pattern(&img, 0x1F, b"SPR").unwrap(), b"1F???????????SPR");
        assert_eq!(&monster_pattern(&img, 0x0D, b"NLM").unwrap(), b"0D???????????NLM");
        assert!(monster_pattern(&img, 0, b"NLM").is_none());
        assert!(stage_pattern(&img[..100], 1).is_none());
    }

    #[test]
    fn matching_and_lookup() {
        assert!(name_matches(b"01???????????STG", b"01 RIDLEYX01.STG"));
        assert!(!name_matches(b"01???????????STG", b"02 RIDLEYX02.STG"));
        assert!(!name_matches(b"01???????????STG", b"01 RIDLEYX01.NLM"));
        assert!(!name_matches(b"01???????????STG", b"01 SHORT.STG"));
        assert!(name_matches(b"01*", b"01 ANYTHING"));
        let files = vec![
            ModFile::new(b"01 BIG MBALL.NLM", vec![1]),
            ModFile::new(b"01 RIDLEYX01.STG", vec![2]),
            ModFile::new(b"02 RIDLEYX02.STG", vec![3]),
        ];
        assert_eq!(find(&files, b"01???????????STG").unwrap().data, [2]);
        assert_eq!(find(&files, b"01???????????NLM").unwrap().data, [1]);
        assert!(find(&files, b"03???????????STG").is_none());
    }
}
