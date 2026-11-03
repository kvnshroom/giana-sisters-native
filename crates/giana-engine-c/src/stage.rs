//! Stage records and the level buffer of the 30A preview.
//!
//! Layout in the unpacked image (see `docs/rom-hack-family-c.md`):
//!
//! * `$8FBC + 2 * n`: word pointer to the record of stage `n` (0..=15); stage numbers `>= 100` are special rooms with
//!   their pointers at `$8FB0 + 2 * (n - 100)` (101, 103 and 104 are used, they sit between stage 0 and stage 1; the
//!   other three entries point at stage 0 and at RAM outside the records);
//! * record = 16 header bytes (copied to `$0100..$010F` by the game), the stage banner (one byte column and a
//!   screen-code text, `$FF` terminated; special rooms have none), the **map commands** (`$8C26`), a `$FF` and the object spawn list
//!   (special rooms end after the `$FF`)
//!   (3 bytes `{column + 1, parameter, type | special << 7}`, `$FF` terminated, sorted by column);
//! * a map command stamps a *shape* (a small rectangle of tile codes, table of word pointers at `$8757`, each shape =
//!   `width - 1`, `height - 1`, `height` rows of `width` tile codes, `$1C` = transparent) `x_rep` times to the right and
//!   `y_rep` times downwards into the level buffer: 256 columns x 20 rows at `$0400 + row * 256 + column`.
//!
//! The builder follows the game routine at `$8C0E`/`$8C26` step by step (including its quirks: a repeat count of 0 means
//! 256, the write pointer is 16 bits so a shape that runs past column 255 continues in the next row).

use crate::Image;

/// Address of the stage pointer table and of the special room table.
const STAGE_TABLE: u16 = 0x8FBC;
const ROOM_TABLE: u16 = 0x8FB0;
/// Word pointers to the shapes the map commands use.
const SHAPE_TABLE: u16 = 0x8757;
/// First address of the level buffer, columns and rows.
pub const LEVEL_BASE: u16 = 0x0400;
pub const LEVEL_COLS: usize = 256;
pub const LEVEL_ROWS: usize = 20;
/// Tile code that a shape leaves out ("transparent").
pub const SHAPE_SKIP: u8 = 0x1C;
/// Number of regular stages.
pub const STAGES: u8 = 16;

#[derive(Debug, PartialEq, Eq)]
pub enum StageError {
    /// Stage number without a record.
    NoSuchStage(u8),
    /// A pointer or a list leaves the image or does not end.
    Corrupt(&'static str),
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageError::NoSuchStage(n) => write!(f, "no stage {n}"),
            StageError::Corrupt(s) => write!(f, "corrupt stage data: {s}"),
        }
    }
}
impl std::error::Error for StageError {}

/// The 16 header bytes of a record (`$0100..$010F` while the stage runs). Names follow the code that reads them;
/// evidence: [D] = read from the disassembly, [I] = inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header(pub [u8; 16]);

impl Header {
    /// `$0101`: index into the colour tables at `$873B`, `$8744`, `$874D` (the three character colours) [D].
    pub fn palette(&self) -> u8 {
        self.0[1]
    }
    /// `$0102`: added to the stage number when the stage is left (stage advance) [I].
    pub fn advance(&self) -> u8 {
        self.0[2]
    }
    /// `$0103`: song started with the stage (`JSR $5C93` with Y = this) [D].
    pub fn song(&self) -> u8 {
        self.0[3]
    }
    /// `$0104/$0105`: time at the start, BCD, low byte first (stage 1: `$0150` = 150) [D, M].
    pub fn time_bcd(&self) -> u16 {
        u16::from_le_bytes([self.0[4], self.0[5]])
    }
    /// `$0108`: colour RAM value of the status/cloud area, 0 = none [D].
    pub fn colour_fill(&self) -> u8 {
        self.0[8]
    }
    /// `$0109`: low nibble = background colour (`$D021`), bits 4..7 = variant flags (character set swaps) [D].
    pub fn flags(&self) -> u8 {
        self.0[9]
    }
    pub fn background(&self) -> u8 {
        self.0[9] & 0x0F
    }
    /// `$010A/$010B`: multicolour colours 1 and 2 (`$D022/$D023`) [D].
    pub fn multicolour(&self) -> (u8, u8) {
        (self.0[10], self.0[11])
    }
    /// `$010C/$010D`: Giana's start: sprite x = first + 5, sprite y = second [D].
    pub fn start(&self) -> (u8, u8) {
        (self.0[12].wrapping_add(5), self.0[13])
    }
    /// `$010E/$010F`: column (`(first + 1) * 8 + 6`, 3 sprites 24 pixels apart) and row table index of the stage end
    /// markers [D, I].
    pub fn end_marker(&self) -> (u8, u8) {
        (self.0[14], self.0[15])
    }
}

/// One map command (see the module documentation). Counts are the raw values; 0 means 256 like in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapCommand {
    /// Index into the shape table.
    pub shape: u8,
    /// Start column (low byte of the write pointer).
    pub col: u8,
    /// Start row as the *page* number `$05` the game uses: buffer row + 4 (so values are `4..=23` for visible rows).
    pub page: u8,
    /// Repeats to the right (`$06`).
    pub x_rep: u8,
    /// Repeats downwards (`$0F`).
    pub y_rep: u8,
}

/// A spawn record: the object appears when the screen column reaches `column`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub column: u8,
    pub param: u8,
    /// Index into the descriptor table at `$42A8` (word pointers to 21 byte records).
    pub kind: u8,
    /// Bit 7 of the type byte (special spawn, `$75F4`).
    pub special: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    pub number: u8,
    pub header: Header,
    /// Banner: the column byte and the screen-code text (including control codes), without the `$FF` end. Empty for
    /// special rooms.
    pub banner: Vec<u8>,
    pub commands: Vec<MapCommand>,
    pub spawns: Vec<Spawn>,
    /// Address of the record and of its end (after the spawn list's `$FF`).
    pub start_addr: u16,
    pub end_addr: u16,
}

/// Address of the record of stage `n` (0..=15, or 100.. for special rooms).
pub fn record_addr(img: &Image, n: u8) -> Result<u16, StageError> {
    let p = if n >= 100 {
        let i = (n - 100) as u16;
        // the room table has 6 entries before the stage table
        if i >= 6 {
            return Err(StageError::NoSuchStage(n));
        }
        img.word(ROOM_TABLE + 2 * i)
    } else if n < STAGES {
        img.word(STAGE_TABLE + 2 * n as u16)
    } else {
        return Err(StageError::NoSuchStage(n));
    };
    if p < 0x8FDC {
        return Err(StageError::Corrupt("record pointer out of range"));
    }
    Ok(p)
}

impl Stage {
    pub fn parse(img: &Image, n: u8) -> Result<Stage, StageError> {
        let start = record_addr(img, n)?;
        let at = |a: u32| -> Result<u8, StageError> {
            img.ram.get(a as usize).copied().ok_or(StageError::Corrupt("record leaves RAM"))
        };
        let mut p = start as u32;
        let mut h = [0u8; 16];
        for (i, b) in h.iter_mut().enumerate() {
            *b = at(p + i as u32)?;
        }
        p += 16;
        // banner (stages only, special rooms have none): up to and including the first $FF, found by scanning like the
        // game does ($4A99)
        let mut banner = Vec::new();
        if n < 100 {
            loop {
                let b = at(p)?;
                p += 1;
                if b == 0xFF {
                    break;
                }
                banner.push(b);
                if banner.len() > 64 {
                    return Err(StageError::Corrupt("banner does not end"));
                }
            }
        }
        // map commands
        let mut commands = Vec::new();
        loop {
            let c = at(p)?;
            if c == 0xFF {
                p += 1;
                break;
            }
            let (shape, col, b2) = (c & 0x7F, at(p + 1)?, at(p + 2)?);
            if c & 0x80 != 0 {
                // 3 bytes: x repeat in bits 6..7 (+1), y repeat 1 or 2 in bit 5, row in bits 0..4
                commands.push(MapCommand {
                    shape,
                    col,
                    page: (b2 & 0x1F).wrapping_add(4),
                    x_rep: (b2 >> 6) + 1,
                    y_rep: if b2 & 0x20 != 0 { 2 } else { 1 },
                });
                p += 3;
            } else if b2 >= 0x14 {
                // 4 bytes: row in bits 0..4, y repeat in bits 5..7, x repeat byte
                commands.push(MapCommand { shape, col, page: (b2 & 0x1F).wrapping_add(4), x_rep: at(p + 3)?, y_rep: b2 >> 5 });
                p += 4;
            } else {
                // 5 bytes: row, x repeat, y repeat
                commands.push(MapCommand { shape, col, page: b2.wrapping_add(4), x_rep: at(p + 3)?, y_rep: at(p + 4)? });
                p += 5;
            }
            if commands.len() > 512 {
                return Err(StageError::Corrupt("command list does not end"));
            }
        }
        // object spawn list
        let mut spawns = Vec::new();
        while n < 100 {
            // special rooms have no list: the game still reads the following bytes as one but never reaches column 255
            let c = at(p)?;
            if c == 0xFF {
                p += 1;
                break;
            }
            let t = at(p + 2)?;
            spawns.push(Spawn { column: c.wrapping_sub(1), param: at(p + 1)?, kind: t & 0x7F, special: t & 0x80 != 0 });
            p += 3;
            if spawns.len() > 256 {
                return Err(StageError::Corrupt("spawn list does not end"));
            }
        }
        Ok(Stage { number: n, header: Header(h), banner, commands, spawns, start_addr: start, end_addr: p as u16 })
    }
}

/// The 256 x 20 level buffer, row-major (`cells[row * 256 + column]`).
#[derive(Clone, PartialEq, Eq)]
pub struct Level {
    pub cells: Vec<u8>,
}

impl Level {
    pub fn get(&self, row: usize, col: usize) -> u8 {
        self.cells[row * LEVEL_COLS + col]
    }

    /// The buffer as the game keeps it in RAM (`$0400..$17FF`).
    pub fn as_ram_bytes(&self) -> &[u8] {
        &self.cells
    }
}

impl std::fmt::Debug for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Level({} cells)", self.cells.len())
    }
}

/// Builds the level buffer of a stage (`$8C0E`/`$8C26`).
pub fn build_level(img: &Image, stage: &Stage) -> Level {
    // scratch RAM `$0400..$2000`: the 16 bit write pointer may run past the buffer; only `$0400..$17FF` is the level
    const SCRATCH: usize = 0x2000 - LEVEL_BASE as usize;
    let mut mem = vec![0u8; SCRATCH];
    for cmd in &stage.commands {
        let sp = img.word(SHAPE_TABLE + 2 * cmd.shape as u16);
        let (w1, h1) = (img.byte(sp), img.byte(sp + 1));
        let mut row_ptr = sp + 2;
        let mut row_page = cmd.page; // $0E: first page of the shape row
        for _ in 0..=h1 {
            // the y repeats of one shape row ($07 counts down from y_rep, 0 = 256)
            let mut page = row_page; // $05
            let mut y_left = cmd.y_rep;
            loop {
                if !(4..0x18).contains(&page) {
                    // $8CD0: outside the buffer, the game skips this and the following repeats
                    break;
                }
                let mut col = cmd.col; // $04
                let mut x_left = cmd.x_rep; // $06 in X
                loop {
                    for y in (0..=w1 as u16).rev() {
                        let v = img.byte(row_ptr + y);
                        if v != SHAPE_SKIP {
                            let addr = ((page as usize) << 8 | col as usize) + y as usize;
                            if let Some(c) = addr.checked_sub(LEVEL_BASE as usize).and_then(|i| mem.get_mut(i)) {
                                *c = v;
                            }
                        }
                    }
                    col = col.wrapping_add(w1).wrapping_add(1);
                    x_left = x_left.wrapping_sub(1);
                    if x_left == 0 {
                        break;
                    }
                }
                page = page.wrapping_add(h1).wrapping_add(1);
                y_left = y_left.wrapping_sub(1);
                if y_left == 0 {
                    break;
                }
            }
            row_page = row_page.wrapping_add(1);
            row_ptr = row_ptr.wrapping_add(w1 as u16 + 1);
        }
    }
    mem.truncate(LEVEL_COLS * LEVEL_ROWS);
    Level { cells: mem }
}
