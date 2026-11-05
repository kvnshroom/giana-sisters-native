//! Families of fan hacks whose code changes the port re-implements (see `docs/rom-hack-families.md`).
//!
//! A family is recognised from a handful of bytes at the places where the hack hooks into the game (addresses and
//! the operands of the jumps; nothing is copied from a hack beyond these short signatures). The app switches the
//! matching features of `giana_core::features` on; code changes that belong to a family are not reported as
//! "changes game code" ([`covered_ranges`]).

/// Which part of the Construction Kit 1989 a hack uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CkVariant {
    /// Per-stage time and warp tables (`$0A5E`, `$1300` hooks) only.
    Basic,
    /// Plus the boss bonus (`$291B`) and the final bonus (`$302E`) hooks.
    Full,
}

/// The family of a program image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// All the places a family hooks into are as in the original game.
    Original,
    /// "Gianna Sisters Construction Kit", 1989.
    ConstructionKit1989 { variant: CkVariant },
    /// The 7A3 "Great Giana Sisters Construction-Set" engine (Giana Sisters II, Power Edition, Ridley X, Junior, ...):
    /// played by its own engine (see `make::engine_tag`, docs/rom-hack-family-b.md), not by hooks into the original.
    SevenA3,
    /// "The Great Giana Sisters 2" by The New Wave, 1989 (family T): the original layout, a start menu with three modes
    /// and every stage loaded from a disk file (`tnw.rs`).
    Tnw1989,
    /// Hooks that no family explains.
    Unknown,
}

impl Family {
    /// Short name for messages.
    pub fn name(&self) -> &'static str {
        match self {
            Family::Original => "original",
            Family::ConstructionKit1989 { .. } => "Construction Kit 1989",
            Family::SevenA3 => "7A3 Construction-Set",
            Family::Tnw1989 => "TNW Giana Sisters 2",
            Family::Unknown => "unknown",
        }
    }
}

fn at(image: &[u8], addr: usize, bytes: &[u8]) -> bool {
    image.get(addr..addr + bytes.len()) == Some(bytes)
}

// the hook sites: original instruction, hack instruction
const ORIG_0A5E: [u8; 3] = [0x20, 0x39, 0x0E]; // JSR $0E39
const ORIG_1300: [u8; 3] = [0x20, 0x0A, 0x1D]; // JSR $1D0A
const ORIG_291B: [u8; 3] = [0xAE, 0x6B, 0x07]; // LDX $076B
const ORIG_302E: [u8; 3] = [0x8D, 0x31, 0x04]; // STA $0431
const CK_0A5E: [u8; 3] = [0x20, 0x50, 0x8F]; // JSR $8F50
const CK_1300: [u8; 3] = [0x20, 0x21, 0x8F]; // JSR $8F21
const CK_291B: [u8; 3] = [0x20, 0x00, 0xF9]; // JSR $F900
const CK_302E: [u8; 4] = [0x20, 0x0B, 0xF9, 0x60]; // JSR $F90B; RTS
/// `$8F50`: the stage set-up reads the three tables `$8F9D`, `$8FBE`, `$8FDF` and stores them (time, warp opcode and operand).
const CK_SETUP: [u8; 21] = [0xA6, 0x20, 0xCA, 0xBD, 0xDF, 0x8F, 0x8D, 0x9B, 0x0B, 0xBD, 0x9D, 0x8F, 0x8D, 0x49, 0x04, 0xBD, 0xBE, 0x8F, 0x8D, 0x9A, 0x0B];
/// `$F900`: +1000 points, then the original instruction.
const CK_BOSS: [u8; 11] = [0xA9, 0x00, 0xA2, 0x10, 0x20, 0x1D, 0x99, 0xAE, 0x6B, 0x07, 0x60];
/// `$F90B`: the original `STA $0431`, then the final bonus.
const CK_FINAL: [u8; 7] = [0x8D, 0x31, 0x04, 0xA9, 0x00, 0xA2, 0xA0];

/// Recognises the family of a 64K program image from its hook sites.
pub fn detect_family(image: &[u8]) -> Family {
    if image.len() != 0x1_0000 {
        return Family::Unknown;
    }
    // before the original: TNW leaves the hook sites of the other families as they are
    if crate::tnw::signature(image) {
        return Family::Tnw1989;
    }
    let orig = at(image, 0x0A5E, &ORIG_0A5E) && at(image, 0x1300, &ORIG_1300) && at(image, 0x291B, &ORIG_291B) && at(image, 0x302E, &ORIG_302E);
    if orig {
        return Family::Original;
    }
    if crate::make::engine_tag(image) == Some(crate::container::ENGINE_B) {
        return Family::SevenA3;
    }
    let tables = at(image, 0x0A5E, &CK_0A5E) && at(image, 0x1300, &CK_1300) && at(image, 0x8F50, &CK_SETUP) && at(image, 0x8F21, &ORIG_1300);
    if !tables {
        return Family::Unknown;
    }
    let boss_orig = at(image, 0x291B, &ORIG_291B);
    let final_orig = at(image, 0x302E, &ORIG_302E);
    if boss_orig && final_orig {
        return Family::ConstructionKit1989 { variant: CkVariant::Basic };
    }
    if at(image, 0x291B, &CK_291B) && at(image, 0x302E, &CK_302E) && at(image, 0xF900, &CK_BOSS) && at(image, 0xF90B, &CK_FINAL) {
        return Family::ConstructionKit1989 { variant: CkVariant::Full };
    }
    Family::Unknown
}

/// Address ranges (inclusive) of code the port implements itself that a family changes and that the port covers:
/// the hook sites it reproduces and the disk-loader / high-score code that the port replaces with its own storage.
pub fn covered_ranges(family: Family) -> &'static [(u16, u16)] {
    match family {
        Family::ConstructionKit1989 { .. } => &[
            (0x08D1, 0x08D1), // palette of the loading screen
            (0x0A5F, 0x0A60), // hook: per-stage time
            (0x0B9A, 0x0B9B), // warp instruction (rewritten by the tables)
            (0x1301, 0x1302), // hook: hundreds digit of the time
            (0x291B, 0x291D), // hook: boss bonus
            (0x302E, 0x3030), // hook: final bonus
            (0x325D, 0x3613), // disk loader and high score screen (the port has its own)
        ],
        // family T: the hooks and tables the port reproduces (`features/tnw.rs`). The three ending text positions
        // (`$0AE4`, `$0AFF`, `$0B0F`: the opcode of `LDX #$09` was overwritten, its operand stayed) are covered as well: what
        // the hack's own ending runs there is whatever the byte holds after its unpacker (a JAM in VICE: the game freezes), so
        // the port plays the original ending, see docs/rom-hack-families.md.
        Family::Tnw1989 => &[
            (0x08D8, 0x08D8), // start-up palette: the stage of the title backdrop (the port has its own title)
            (0x08DF, 0x08E3), // start-up: loads the backdrop stage
            (0x0AE4, 0x0AE4), // ending text positions: the port shows the original ending
            (0x0AFF, 0x0AFF),
            (0x0B0F, 0x0B0F),
            (0x0996, 0x0998), // hook: the start menu
            (0x0A59, 0x0A5A), // stage number table `$6F00`
            (0x0AA5, 0x0AA7), // music track from the stage file
            (0x0BBD, 0x0BBE), // stage number table
            (0x0D3C, 0x0D4F), // colours from the stage file
            (0x0D95, 0x0D9F), // run-time state (self-modifying operands)
            (0x0E09, 0x0E0A), // stage number table
            (0x0E23, 0x0E24), // stage number table
            (0x13F8, 0x1417), // start position from the stage file
            (0x1E5A, 0x1E71), // stage and bonus room numbers (`$40`, `$41`, `$47`, `$42`)
            (0x1F4E, 0x1F50), // colours again after a bonus room
            (0x229C, 0x22A3), // enemy list pointer from the stage file
            (0x33F2, 0x33F5), // copy protection (not reproduced on purpose)
            (0x3482, 0x3483), // stage number table
            (0x989B, 0x989B), // run-time state (self-modifying operands)
            (0x98CD, 0x98D2),
            (0x9936, 0x9939), // hook: the stage loader
            (0x9953, 0x995A), // the stage stream comes from the file
        ],
        // the 7A3 engine is played by engine B (`giana-core/src/engine_b`), a port of the hack's own routines: its code
        // changes are what that engine implements, so none of them is left uncovered
        // ... except the sound driver (`$41C0..$4A79`): the port plays every module with the original's driver translated to Rust
        // (`giana-core/src/sound_driver.rs`), so a hack with another driver does not sound as intended and says so (none of the
        // known ones: the construction-set game's snapshot has an empty driver area, `apply` completes it with the original's,
        // which is what the game's own `GAME PART 2` file holds)
        Family::SevenA3 => &[(0x0000, 0x41BF), (0x4A7A, 0xFFFF)],
        _ => &[],
    }
}

/// `(start, len)` pieces of `changes` that lie outside `covered`.
pub fn subtract_covered(changes: &[(usize, usize)], covered: &[(u16, u16)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for &(s, l) in changes {
        let mut pieces = vec![(s, s + l)];
        for &(a, b) in covered {
            let (a, b) = (a as usize, b as usize + 1);
            pieces = pieces
                .into_iter()
                .flat_map(|(p0, p1)| {
                    if b <= p0 || a >= p1 {
                        vec![(p0, p1)]
                    } else {
                        let mut v = Vec::new();
                        if p0 < a {
                            v.push((p0, a));
                        }
                        if b < p1 {
                            v.push((b, p1));
                        }
                        v
                    }
                })
                .collect();
        }
        out.extend(pieces.into_iter().map(|(p0, p1)| (p0, p1 - p0)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original_like() -> Vec<u8> {
        let mut m = vec![0u8; 0x1_0000];
        m[0x0A5E..0x0A61].copy_from_slice(&ORIG_0A5E);
        m[0x1300..0x1303].copy_from_slice(&ORIG_1300);
        m[0x291B..0x291E].copy_from_slice(&ORIG_291B);
        m[0x302E..0x3031].copy_from_slice(&ORIG_302E);
        m
    }

    fn basic() -> Vec<u8> {
        let mut m = original_like();
        m[0x0A5E..0x0A61].copy_from_slice(&CK_0A5E);
        m[0x1300..0x1303].copy_from_slice(&CK_1300);
        m[0x8F21..0x8F24].copy_from_slice(&ORIG_1300);
        m[0x8F50..0x8F50 + CK_SETUP.len()].copy_from_slice(&CK_SETUP);
        m
    }

    fn full() -> Vec<u8> {
        let mut m = basic();
        m[0x291B..0x291E].copy_from_slice(&CK_291B);
        m[0x302E..0x3032].copy_from_slice(&CK_302E);
        m[0xF900..0xF900 + CK_BOSS.len()].copy_from_slice(&CK_BOSS);
        m[0xF90B..0xF90B + CK_FINAL.len()].copy_from_slice(&CK_FINAL);
        m
    }

    #[test]
    fn detects_synthetic_images() {
        assert_eq!(detect_family(&original_like()), Family::Original);
        assert_eq!(detect_family(&basic()), Family::ConstructionKit1989 { variant: CkVariant::Basic });
        assert_eq!(detect_family(&full()), Family::ConstructionKit1989 { variant: CkVariant::Full });
    }

    #[test]
    fn detects_tnw_before_the_original() {
        let mut m = original_like(); // TNW leaves the hook sites of the other families as they are
        assert_eq!(detect_family(&m), Family::Original);
        for (a, b) in [(0x0996usize, [0x20u8, 0xC0, 0x7A]), (0x9936, [0x20, 0x80, 0x6F]), (0x33F2, [0x20, 0xC8, 0x6F]), (0x0A58, [0xBD, 0x00, 0x6F])] {
            m[a..a + 3].copy_from_slice(&b);
        }
        assert_eq!(detect_family(&m), Family::Tnw1989);
        assert!(!covered_ranges(Family::Tnw1989).is_empty());
        m[0x33F2] = 0xA5; // without the protection hook it is not the hack
        assert_eq!(detect_family(&m), Family::Original);
    }

    #[test]
    fn half_hooks_and_wrong_sizes_are_unknown() {
        let mut m = full();
        m[0xF903] = 0x99; // changed boss routine
        assert_eq!(detect_family(&m), Family::Unknown);
        let mut m = basic();
        m[0x8F55] ^= 1; // the set-up routine is another one
        assert_eq!(detect_family(&m), Family::Unknown);
        let mut m = original_like();
        m[0x1301] = 0x21; // a hook without the tables
        assert_eq!(detect_family(&m), Family::Unknown);
        assert_eq!(detect_family(&[0u8; 100]), Family::Unknown);
    }

    #[test]
    fn subtracts_covered_pieces() {
        let covered = [(0x0A5Fu16, 0x0A60u16), (0x1301, 0x1302)];
        // 0A5F..0A60 entirely covered, 0B00..0B03 not, 12FF..1303 partly
        let changes = [(0x0A5F, 2), (0x0B00, 4), (0x12FF, 5)];
        assert_eq!(subtract_covered(&changes, &covered), vec![(0x0B00, 4), (0x12FF, 2), (0x1303, 1)]);
        assert!(subtract_covered(&changes, &[]).len() == 3);
    }
}
