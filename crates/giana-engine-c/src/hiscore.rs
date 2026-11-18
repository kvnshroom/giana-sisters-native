//! High scores of the 30A preview (package F, `docs/rom-hack-family-c.md` 7.9): the two tables of five entries, the
//! player's record, the rank, the insertion, and the file. The original keeps `$5192..$5227` (150 bytes: both tables,
//! the highest stage reached `$520F`, a saved game `$5210..$5227`) in the disk file `hid2!g30a`: loaded once by the
//! intro, saved by the KERNAL (`$4CBE`) from the high-score screen and the pause menu. The native game asks a
//! [`ScoreStore`] instead (the app keeps the bytes in its own file).
//!
//! A table entry is 21 bytes, the same as the record the game builds in the zero page at `$39..$4D`: the score as six
//! digit characters, 13 name characters, the stage as two digits. The entries start at the offsets `$52BB` (3, 28, 53,
//! 78, 103) from the table's base (`$523E` the first table, `$5192` the second).

use crate::layout::C30aLayout;

/// Where the high-score bytes live between runs (the app implements it; the original's disk file).
pub trait ScoreStore {
    /// The 150 bytes of `$5192..$5227` saved last, if any.
    fn load(&mut self) -> Option<Vec<u8>>;
    /// Keeps the 150 bytes of `$5192..$5227`.
    fn save(&mut self, bytes: &[u8]);
}

/// A [`ScoreStore`] that keeps nothing (every game starts with the disk's table).
pub struct NoStore;

impl ScoreStore for NoStore {
    fn load(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn save(&mut self, _: &[u8]) {}
}

/// A [`ScoreStore`] in memory (tests, or an app that persists it itself).
#[derive(Default, Clone)]
pub struct MemoryStore(pub Option<Vec<u8>>);

impl ScoreStore for MemoryStore {
    fn load(&mut self) -> Option<Vec<u8>> {
        self.0.clone()
    }
    fn save(&mut self, bytes: &[u8]) {
        self.0 = Some(bytes.to_vec());
    }
}

/// The bytes the file holds (`$5192..$5227`).
pub fn file_bytes(ram: &[u8], l: &C30aLayout) -> Vec<u8> {
    ram[l.score_file as usize..l.score_file as usize + FILE_LEN].to_vec()
}

/// Length of the high-score file's data.
pub const FILE_LEN: usize = 150;

/// The file's bytes back into memory (what the intro's `LOAD` does).
pub fn load_file(ram: &mut [u8], l: &C30aLayout, bytes: &[u8]) {
    let n = bytes.len().min(FILE_LEN);
    ram[l.score_file as usize..l.score_file as usize + n].copy_from_slice(&bytes[..n]);
}

/// `$236F`/`$237D`: a BCD byte as two digits through the print pointer (`$04/$05`, index `y`); returns the next index.
fn bcd_to(ram: &mut [u8], l: &C30aLayout, a: u8, y: u8) -> u8 {
    let p = u16::from_le_bytes([ram[l.print_ptr as usize], ram[l.print_ptr as usize + 1]]);
    ram[p.wrapping_add(y as u16) as usize] = a >> 4 | 0x30;
    ram[p.wrapping_add(y as u16 + 1) as usize] = (a & 15) | 0x30;
    ram[l.print_col as usize] = y + 2;
    y + 2
}

/// `$4FA8`: the player's record in the zero page: the score as six digits at `$39..$3E`, the stage (`$FC`, binary) as two
/// digits at `$4C/$4D` (a tens digit of 5 or more is `$18`; after the end of the game, `$FC` negative: 5, `$0E`).
/// (`$39..$4D` is the player probe's cell table `probe_tiles` during play.)
pub fn record(ram: &mut [u8], l: &C30aLayout) {
    let rec = l.probe_tiles as usize;
    ram[l.print_ptr as usize] = rec as u8;
    ram[l.print_ptr as usize + 1] = 0;
    let mut y = 0;
    for i in [2u16, 1, 0] {
        let v = ram[(l.score + i) as usize];
        y = bcd_to(ram, l, v, y);
    }
    let st = ram[l.stage as usize];
    if st & 0x80 == 0 {
        ram[l.print_ptr as usize] = rec as u8 + 0x13;
        let (mut a, mut tens) = (st, 0u8);
        while a >= 10 {
            a -= 10;
            tens += 1;
        }
        let mut t = tens | 0x30;
        if t >= 0x35 {
            t = 0x18;
        }
        ram[rec + 0x13] = t;
        ram[rec + 0x14] = a | 0x30;
        ram[l.print_col as usize] = 2;
    } else {
        ram[rec + 0x13] = 5;
        ram[rec + 0x14] = 0x0E;
    }
}

/// `$4FD2` (`second = false`, the table at `$523E`) / `$4FD8` (`true`, `$5192`): `$02/$03` = the table, `$04/$05` = 25
/// bytes before it (the entry above), `$48` = the table's high byte (a leftover the cursor starts from).
pub fn select_table(ram: &mut [u8], l: &C30aLayout, second: bool) {
    let base = if second { l.score_file } else { l.score_table };
    ram[l.pointer2 as usize] = base as u8;
    ram[l.pointer2 as usize + 1] = (base >> 8) as u8;
    ram[l.entry_cursor as usize] = (base >> 8) as u8;
    let p = base.wrapping_sub(0x19);
    ram[l.print_ptr as usize] = p as u8;
    ram[l.print_ptr as usize + 1] = (p >> 8) as u8;
}

/// `$4FED`: the rank of the record `$39..$3E` in the table at `$02/$03` (0 = first; it goes above an entry it beats or
/// equals): kept in `$49`; `None` = 5, not in the table.
pub fn rank(ram: &mut [u8], l: &C30aLayout) -> Option<u8> {
    let base = u16::from_le_bytes([ram[l.pointer2 as usize], ram[l.pointer2 as usize + 1]]);
    let mut x = 5u8;
    loop {
        ram[l.entry_rank as usize] = x;
        if x == 0 {
            break;
        }
        let i = x - 1;
        let y = ram[(l.entry_offsets + i as u16) as usize] as u16;
        let mut ge = true;
        for k in 0..6u16 {
            let a = ram[(l.probe_tiles + k) as usize];
            let b = ram[base.wrapping_add(y + k) as usize];
            if a != b {
                ge = a > b;
                break;
            }
        }
        if !ge {
            break;
        }
        x = i;
    }
    let r = ram[l.entry_rank as usize];
    (r < 5).then_some(r)
}

/// `$500F`: the record into the table at `$02/$03` at its rank (the entries below move down one; the name from
/// `$CBD6..$CBE2`, `.` = blank).
pub fn insert(ram: &mut [u8], l: &C30aLayout) {
    let Some(r) = rank(ram, l) else { return };
    let base = u16::from_le_bytes([ram[l.pointer2 as usize], ram[l.pointer2 as usize + 1]]);
    let above = u16::from_le_bytes([ram[l.print_ptr as usize], ram[l.print_ptr as usize + 1]]);
    let mut x = 4u8;
    let mut y;
    loop {
        y = ram[(l.entry_offsets + x as u16) as usize] as u16;
        if x == r {
            break;
        }
        ram[l.entry_rank as usize + 1] = x;
        for k in 0..21u16 {
            ram[base.wrapping_add(y + k) as usize] = ram[above.wrapping_add(y + k) as usize];
        }
        x -= 1;
    }
    for k in 0..0x15u16 {
        let v = if (6..0x13).contains(&k) {
            let c = ram[(l.entry_name - 7 + k) as usize];
            if c == 0x2E {
                0
            } else {
                c
            }
        } else {
            ram[(l.probe_tiles + k) as usize]
        };
        ram[base.wrapping_add(y + k) as usize] = v;
    }
}
