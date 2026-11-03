//! Enemy and object type records.
//!
//! The hack image has a pointer table at `$21D8` (two bytes per type, types `$00..=$36`) to 21-byte records. Types
//! `$00..=$17` are the types of the original game (same record layout as the original's table at `$303B`, sprite
//! numbers shifted); `$18..=$31` and `$33..=$36` are new; type `$32` is the "loaded monster": its pointer is `$7505`,
//! where the `.NLM` file of the stage's monster is loaded (the file is the 21-byte record behind a load address).
//!
//! Field roles, from the routines that read them (see the doc, section "Sprite control program"):
//!
//! | byte | role | slot array |
//! |---|---|---|
//! | 0 | sprite shape number of the first frame (`$0605+slot`, bank 3: address = `$C000 + 64 * n`) | |
//! | 1 | flags: bit 0 = hires (clear: multicolour), bit 1 = behind the background, bit 6 -> `$0714`, bit 7 = animation runs backwards (`$0718`) | |
//! | 2 | colour (bit 7 -> `$07CC` flag, low bits to `$D027+slot`) | |
//! | 3 | kind -> `$071C`: 0..2 single sprite, 3 and 4 composite bosses | `$071C` |
//! | 4,5 | animation range (first, last shape) facing one way | `$0724`, `$072C` |
//! | 6,7 | animation range facing the other way | `$0734`, `$073C` |
//! | 8 | class -> `$0744`: 0 stompable, 1 deadly, 2 pipe plant, `$7F` platform, `$11`/`$12` plant variants, bit 7 set = boss state counter | `$0744` |
//! | 9 | score, low BCD byte | `$074C` |
//! | 10,11 | pointer to an animation/script table (0 = none) | `$0754`, `$075C` |
//! | 12,13 | shape when squashed (one per facing) | `$0764`, `$076C` |
//! | 14 | horizontal speed in pixels per step (low 7 bits) | `$0774` |
//! | 15 | animation delay | `$077C` |
//! | 16 | motion mode (0 walks and falls, 1 jumps on the arc table `$9ADE`, 2 ...) | `$0784` |
//! | 17 | extra flag | `$078C` |
//! | 18 | score, high BCD byte | `$0794` |
//! | 19 | number of chained follower sprites | `$079C` |
//! | 20 | extra | `$07A4` |

pub const TYPE_TABLE: usize = 0x21D8;
pub const TYPE_COUNT: u8 = 0x37;
/// The loaded monster type.
pub const MONSTER_TYPE: u8 = 0x32;
pub const MONSTER_DEF_ADDRESS: u16 = 0x7505;
pub const MONSTER_SPRITE_ADDRESS: u16 = 0xFB80;
pub const RECORD_LEN: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeRecord {
    pub raw: [u8; RECORD_LEN],
}

impl TypeRecord {
    pub fn sprite(&self) -> u8 {
        self.raw[0]
    }
    pub fn flags(&self) -> u8 {
        self.raw[1]
    }
    pub fn colour(&self) -> u8 {
        self.raw[2]
    }
    pub fn kind(&self) -> u8 {
        self.raw[3]
    }
    /// Animation ranges `[(first, last); 2]` for the two facings.
    pub fn animation(&self) -> [(u8, u8); 2] {
        [(self.raw[4], self.raw[5]), (self.raw[6], self.raw[7])]
    }
    pub fn class(&self) -> u8 {
        self.raw[8]
    }
    /// Score as BCD (`0x0500` = 500 points).
    pub fn score_bcd(&self) -> u16 {
        u16::from(self.raw[18]) << 8 | u16::from(self.raw[9])
    }
    pub fn squashed(&self) -> [u8; 2] {
        [self.raw[12], self.raw[13]]
    }
    pub fn speed(&self) -> u8 {
        self.raw[14] & 0x7F
    }
    pub fn anim_delay(&self) -> u8 {
        self.raw[15]
    }
    pub fn motion(&self) -> u8 {
        self.raw[16]
    }
    pub fn chain_len(&self) -> u8 {
        self.raw[19]
    }
}

/// The record of type `kind` from the pointer table of the hack image. For the loaded monster (`$32`, pointer
/// `$7505`) the image only holds what the last load left there; use [`MonsterFile`] instead.
pub fn type_record(img: &[u8], kind: u8) -> Option<TypeRecord> {
    if kind >= TYPE_COUNT || img.len() != 0x1_0000 {
        return None;
    }
    let t = TYPE_TABLE + 2 * kind as usize;
    let p = img[t] as usize | (img[t + 1] as usize) << 8;
    let raw: [u8; RECORD_LEN] = img.get(p..p + RECORD_LEN)?.try_into().ok()?;
    Some(TypeRecord { raw })
}

/// A `.NLM` file: the record of the loaded monster behind the load address `$7505`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonsterFile {
    pub record: TypeRecord,
}

impl MonsterFile {
    pub fn parse(file: &[u8]) -> Result<MonsterFile, String> {
        if file.len() < 2 + RECORD_LEN {
            return Err("monster file too short".into());
        }
        let load = u16::from_le_bytes([file[0], file[1]]);
        if load != MONSTER_DEF_ADDRESS {
            return Err(format!("monster file load address is ${load:04X}, expected ${MONSTER_DEF_ADDRESS:04X}"));
        }
        Ok(MonsterFile { record: TypeRecord { raw: file[2..2 + RECORD_LEN].try_into().unwrap() } })
    }
}

/// A `.SPR` file: sprite shapes (64 bytes each) loaded at `$FB80`, the first monster shape number is `$EE`.
pub fn parse_sprite_file(file: &[u8]) -> Result<(u16, &[u8]), String> {
    if file.len() < 2 {
        return Err("sprite file too short".into());
    }
    let load = u16::from_le_bytes([file[0], file[1]]);
    if load != MONSTER_SPRITE_ADDRESS {
        return Err(format!("sprite file load address is ${load:04X}, expected ${MONSTER_SPRITE_ADDRESS:04X}"));
    }
    Ok((load, &file[2..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        // type 0 at $2400, type 1 at $2420
        for (t, at) in [(0usize, 0x2400usize), (1, 0x2420)] {
            img[TYPE_TABLE + 2 * t] = at as u8;
            img[TYPE_TABLE + 2 * t + 1] = (at >> 8) as u8;
        }
        let r0 = [0x40, 0, 5, 0, 0x41, 0x43, 0x44, 0x46, 0, 0x25, 0, 0, 0x48, 0x49, 3, 4, 1, 0, 0, 0, 0];
        img[0x2400..0x2400 + 21].copy_from_slice(&r0);
        let mut r1 = r0;
        r1[8] = 1;
        r1[9] = 0;
        r1[18] = 5;
        r1[14] = 0x85;
        img[0x2420..0x2420 + 21].copy_from_slice(&r1);
        img
    }

    #[test]
    fn reads_records_through_the_pointer_table() {
        let img = image();
        let r = type_record(&img, 0).unwrap();
        assert_eq!((r.sprite(), r.kind(), r.class(), r.score_bcd(), r.speed()), (0x40, 0, 0, 0x0025, 3));
        assert_eq!(r.animation(), [(0x41, 0x43), (0x44, 0x46)]);
        assert_eq!(r.squashed(), [0x48, 0x49]);
        let r = type_record(&img, 1).unwrap();
        assert_eq!((r.class(), r.score_bcd(), r.speed()), (1, 0x0500, 5), "speed uses the low 7 bits");
        assert!(type_record(&img, TYPE_COUNT).is_none());
        assert!(type_record(&img[..100], 0).is_none());
    }

    #[test]
    fn monster_files() {
        let mut f = vec![0x05, 0x75];
        f.extend_from_slice(&[0xEE, 0x80, 1, 2, 0xEE, 0xEF, 0xF0, 0xF1, 0x20, 0, 0, 9, 0xF2, 0xF3, 1, 5, 0, 1, 0x40, 2, 1]);
        let m = MonsterFile::parse(&f).unwrap();
        assert_eq!((m.record.sprite(), m.record.class(), m.record.kind()), (0xEE, 0x20, 2));
        assert!(MonsterFile::parse(&f[..10]).is_err());
        f[1] = 0x76;
        assert!(MonsterFile::parse(&f).is_err());
        assert_eq!(parse_sprite_file(&[0x80, 0xFB, 1, 2]).unwrap(), (0xFB80, &[1u8, 2][..]));
        assert!(parse_sprite_file(&[0x80, 0xFC, 1]).is_err());
    }

    /// Real image (`GIANA_B_IMAGE`): every pointer of the table points into the image and the original types keep
    /// their well known shape (type 0 walks and is stompable).
    #[test]
    fn real_type_table() {
        let Some(p) = std::env::var_os("GIANA_B_IMAGE") else {
            eprintln!("skipped: GIANA_B_IMAGE not set");
            return;
        };
        let mut img = std::fs::read(p).unwrap();
        if img.len() == 0x1_0002 {
            img.drain(..2);
        }
        for k in 0..TYPE_COUNT {
            assert!(type_record(&img, k).is_some(), "type {k:#x}");
        }
        assert_eq!(type_record(&img, 0).unwrap().class(), 0);
    }
}
