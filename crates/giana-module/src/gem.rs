//! Giana Sisters II (Gem Nightmare, the older variant of the 7A3 engine): recognising the image and the constants of
//! its stage loader. The engine itself is in `giana-engine-b` (`gem.rs`, which has the memory model and the stage
//! reader); the importer needs only this much: which files of the disk the game loads.

/// The opcodes and operands this variant is recognised by (the stage hook of the loader at `$312B`, the hook of the
/// stage builder `$9936` and the table reads listed in the module documentation).
pub const SIGNATURE: [(usize, u8); 26] = [
    (0x9936, 0xA9),
    (0x993A, 0x20),
    (0x993B, 0x2C),
    (0x993C, 0x9A),
    (0x9952, 0xA5),
    (0x9956, 0xB9),
    (0x9A2C, 0xA5),
    (0x9A2E, 0xC9),
    (0x9A4C, 0x4C),
    (0x312B, 0xA5),
    (0x312D, 0xC9),
    (0x312F, 0xB0),
    (0x3131, 0xC9),
    (0x3135, 0xCD),
    (0x313A, 0x8D),
    (0x313E, 0x69),
    (0x3140, 0x8D),
    (0x3143, 0x20),
    (0x3147, 0xA0),
    (0x788E, 0xB9),
    (0x0ADB, 0xBD),
    (0x0AE1, 0xBD),
    (0x0B37, 0xB9),
    (0x0B89, 0x7D),
    (0x0D30, 0xB9),
    (0x1426, 0xB9),
];

/// True when `img` has the stage hook, loader and tables of Gem Nightmare (and not the header-in-the-file layout of
/// the other hacks, whose builder ends in `JMP $9010`).
pub fn is_gem(img: &[u8]) -> bool {
    img.len() == 0x1_0000 && SIGNATURE.iter().all(|&(a, v)| img[a] == v)
}

/// Absolute operand of the instruction at `at`.
fn abs(img: &[u8], at: usize) -> usize {
    img[at + 1] as usize | (img[at + 2] as usize) << 8
}

/// Immediate operand of the instruction at `at`.
fn imm(img: &[u8], at: usize) -> u8 {
    img[at + 1]
}

/// The constants of the stage hook and loader, read from the instruction operands of the image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GemLayout {
    /// stage of the title backdrop (`$2F63`: `CMP #$67`), skips the trainer replacement
    pub title_stage: u8,
    /// first stage number that is a resident room (`$2F67`: `CMP #$2A`)
    pub first_room: u8,
    /// highest stage the trainer remembers is below this (`$309E`: `CMP #$28`)
    pub highest_cap: u8,
    /// stages `first_loaded..last_loaded_end` are loaded from the disk (`$3131`: `CMP #$0F`, `$312D`: `CMP #$2A`)
    pub first_loaded: u8,
    pub loaded_end: u8,
    /// added to the stage number to get the file name letter (`$313E`: `ADC #$31`)
    pub letter_add: u8,
    /// address of the letter inside the file names (`$3140`: `STA $08B9`)
    pub letter_at: usize,
    /// stage file name: address and length of the `SETNAM` at `$317C`
    pub name1: (usize, usize),
    /// sprite file name (`$31D2`)
    pub name2: (usize, usize),
    /// where the stage file lands (`$317F`: `LDX #$13; LDY #$29`)
    pub load_at: usize,
}

impl GemLayout {
    pub fn read(img: &[u8]) -> GemLayout {
        GemLayout {
            title_stage: imm(img, 0x2F63),
            first_room: imm(img, 0x2F67),
            highest_cap: imm(img, 0x309E),
            first_loaded: imm(img, 0x3131),
            loaded_end: imm(img, 0x312D),
            letter_add: imm(img, 0x313E),
            letter_at: abs(img, 0x3140),
            name1: (imm(img, 0x3178) as usize | (imm(img, 0x317A) as usize) << 8, imm(img, 0x3176) as usize),
            name2: (imm(img, 0x31CE) as usize | (imm(img, 0x31D0) as usize) << 8, imm(img, 0x31CC) as usize),
            load_at: imm(img, 0x317F) as usize | (imm(img, 0x3181) as usize) << 8,
        }
    }

    /// True for the stage numbers whose stage is loaded from the disk.
    pub fn is_loaded(&self, stage: u8) -> bool {
        stage >= self.first_loaded && stage < self.loaded_end
    }

    /// True for a resident room (a bonus room or the title backdrop): no object list is spawned there.
    pub fn is_room(&self, stage: u8) -> bool {
        stage >= self.first_room
    }

    /// The names (stage file, sprite file) the loader asks the drive for when it loads `stage`, built from the name
    /// templates in the image.
    pub fn file_names(&self, img: &[u8], stage: u8) -> Option<(Vec<u8>, Vec<u8>)> {
        let letter = stage.wrapping_add(self.letter_add);
        let make = |(at, len): (usize, usize)| -> Option<Vec<u8>> {
            let mut v = img.get(at..at.checked_add(len)?)?.to_vec();
            if let Some(i) = self.letter_at.checked_sub(at).filter(|&i| i < len) {
                v[i] = letter;
            }
            Some(v)
        };
        Some((make(self.name1)?, make(self.name2)?))
    }
}


impl GemLayout {
    /// Every file name the loader can ask for: for each loaded stage the stage file and the sprite file.
    pub fn wanted_names(&self, img: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for stage in self.first_loaded..self.loaded_end {
            if let Some((a, b)) = self.file_names(img, stage) {
                out.push(a);
                out.push(b);
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_stage_letter() {
        let mut img = vec![0u8; 0x1_0000];
        for &(a, v) in &SIGNATURE {
            img[a] = v;
        }
        for (a, v) in [(0x3131usize, 0x0Fu8), (0x312D, 0x12), (0x313E, 0x31)] {
            img[a + 1] = v;
        }
        // letter slot at $08B9, names at $08B2 (13 bytes) and $08B6 (5 bytes)
        img[0x3141] = 0xB9;
        img[0x3142] = 0x08;
        img[0x3177] = 13;
        img[0x3179] = 0xB2;
        img[0x317B] = 0x08;
        img[0x31CD] = 5;
        img[0x31CF] = 0xB6;
        img[0x31D1] = 0x08;
        img[0x08B2..0x08BF].copy_from_slice(b"STAGE (U)/7A3");
        assert!(is_gem(&img));
        let l = GemLayout::read(&img);
        assert_eq!((l.first_loaded, l.loaded_end, l.letter_at, l.name1, l.name2), (0x0F, 0x12, 0x08B9, (0x08B2, 13), (0x08B6, 5)));
        let n = l.wanted_names(&img);
        assert_eq!(n.len(), 6);
        assert!(n.contains(&b"STAGE (@)/7A3".to_vec()) && n.contains(&b"E (B)".to_vec()));
    }
}
