//! The `.stg` stage file and the map it expands to.
//!
//! File layout (PRG, load address `$83F0`; the hack loads it with secondary address 0 to `$6DFD` and shifts the object
//! list pointer by `$6DFD - $83F0`):
//!
//! ```text
//! +0      monster file number (hex byte, 0 = none): loads "NN???????????NLM" and "...SPR"
//! +1      diamond colour scheme (index into the tables at $91E4 / $91ED / $91F6)
//! +2      warp step: stages added by a warp pipe / stage exit (1 = next stage)
//! +3      music track
//! +4,+5   time in BCD, low byte first (0x0200 = 200 seconds, up to 9999)
//! +6,+7   address of the object list, in the $83F0 based file coordinates
//! +8      underground / palette flag ($1E)
//! +9..+11 background colours $D021, $D022, $D023 (high nibble ignored)
//! +12,+13 Giana's start position X, Y (sprite coordinates, X without the +5 the hack adds)
//! +14,+15 the second start position (used when $0456 = 2)
//! +16 ... map records of 5 bytes, up to a record whose first byte has bit 7 set
//! ... the object list: 3 bytes per object (trigger column + 1, row, type), ends with $FF
//! ```
//!
//! A map record is `block, column, row, horizontal repeat, vertical repeat`. The block comes from the block table of
//! the hack image (`$9200`: a pointer per block to `width - 1`, `height - 1` and `width * height` tiles). The map has
//! 20 rows of 256 columns at `$AC00`.

/// Load address in the first two bytes of a stage file.
pub const STAGE_LOAD_ADDRESS: u16 = 0x83F0;
pub const MAP_COLS: usize = 256;
pub const MAP_ROWS: usize = 20;
pub const MAP_SIZE: usize = MAP_COLS * MAP_ROWS;
/// Where the map lives in the C64 (`$AC00..$BFFF`).
pub const MAP_BASE: usize = 0xAC00;
/// Block pointer table of the hack image.
pub const BLOCK_TABLE: usize = 0x9200;
const HEADER_LEN: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageHeader {
    pub monster: u8,
    pub diamond_scheme: u8,
    pub warp_step: u8,
    pub music: u8,
    /// Time as stored (BCD, `0x0200` = 200).
    pub time_bcd: u16,
    pub object_list: u16,
    pub underground: u8,
    pub bg: [u8; 3],
    /// Start positions `[x, y]` (two of them).
    pub start: [[u8; 2]; 2],
}

impl StageHeader {
    /// Stage time in seconds (the BCD value decoded; digits above 9 count as stored).
    pub fn time_seconds(&self) -> u32 {
        let t = self.time_bcd as u32;
        (t >> 12 & 15) * 1000 + (t >> 8 & 15) * 100 + (t >> 4 & 15) * 10 + (t & 15)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRecord {
    pub block: u8,
    pub col: u8,
    pub row: u8,
    pub hrep: u8,
    pub vrep: u8,
}

/// Rows of the object Y table `$3205` (25 entries; the map has 20 rows).
pub const MAX_SPAWN_ROW: u8 = 25;

/// An enemy/object of the stage. `trigger` is stored as column + 1: the object appears when the scroll column
/// (`$21`) reaches `trigger - 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub trigger: u8,
    pub row: u8,
    pub kind: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    pub header: StageHeader,
    pub records: Vec<MapRecord>,
    pub spawns: Vec<Spawn>,
}

impl Stage {
    /// Parses a stage file as it is on the disk (with the load address).
    pub fn parse(file: &[u8]) -> Result<Stage, String> {
        if file.len() < 2 + HEADER_LEN + 1 {
            return Err("stage file too short".into());
        }
        let load = u16::from_le_bytes([file[0], file[1]]);
        if load != STAGE_LOAD_ADDRESS {
            return Err(format!("stage file load address is ${load:04X}, expected ${STAGE_LOAD_ADDRESS:04X}"));
        }
        let d = &file[2..];
        let header = StageHeader {
            monster: d[0],
            diamond_scheme: d[1],
            warp_step: d[2],
            music: d[3],
            time_bcd: u16::from_le_bytes([d[4], d[5]]),
            object_list: u16::from_le_bytes([d[6], d[7]]),
            underground: d[8],
            bg: [d[9], d[10], d[11]],
            start: [[d[12], d[13]], [d[14], d[15]]],
        };
        let mut records = Vec::new();
        let mut p = HEADER_LEN;
        loop {
            let Some(r) = d.get(p..p + 1) else {
                return Err("map records are not terminated".into());
            };
            if r[0] & 0x80 != 0 {
                break;
            }
            let Some(r) = d.get(p..p + 5) else {
                return Err("truncated map record".into());
            };
            records.push(MapRecord { block: r[0], col: r[1], row: r[2], hrep: r[3], vrep: r[4] });
            p += 5;
        }
        let list = (header.object_list as usize)
            .checked_sub(STAGE_LOAD_ADDRESS as usize)
            .filter(|&o| o >= p && o < d.len())
            .ok_or_else(|| format!("object list address ${:04X} is outside the file", header.object_list))?;
        let mut spawns = Vec::new();
        let mut q = list;
        loop {
            match d.get(q) {
                None => return Err("object list is not terminated".into()),
                Some(0xFF) => break,
                Some(_) => {
                    let Some(r) = d.get(q..q + 3) else {
                        return Err("truncated object entry".into());
                    };
                    // out-of-range rows and types (junk in untrusted files) are dropped, the list goes on
                    if r[1] < MAX_SPAWN_ROW && r[2] < crate::types::TYPE_COUNT {
                        spawns.push(Spawn { trigger: r[0], row: r[1], kind: r[2] });
                    }
                    q += 3;
                }
            }
        }
        Ok(Stage { header, records, spawns })
    }
}

/// Address of the resident room table of the hack image (`$3BF1`): one pointer per room, stage numbers `$64` and up.
pub const ROOM_TABLE: usize = 0x3BF1;
/// First stage number that is a resident room (`$64` is the title backdrop, `$65` and up the treasure rooms).
pub const FIRST_ROOM: u8 = 0x64;

/// Start address in the image of the resident room of `stage` (`>= $64`), from the table at `$3BF1`.
pub fn room_address(img: &[u8], stage: u8) -> Option<usize> {
    if stage < FIRST_ROOM {
        return None;
    }
    let t = ROOM_TABLE + 2 * (stage - FIRST_ROOM) as usize;
    let p = *img.get(t)? as usize | (*img.get(t + 1)? as usize) << 8;
    (p >= 0x0800).then_some(p)
}

impl Stage {
    /// Reads a resident room (title backdrop or treasure room) from the image: the same 16 header bytes and map
    /// records as a stage file, but the object list address in the header is relative to the start of the room
    /// (`$0061` = the byte behind the records). `None` for a room that does not parse. The object list of a room is
    /// returned as found; whether the hack spawns it is not known (see `docs/rom-hack-family-b.md`, 5.4).
    pub fn parse_room(img: &[u8], at: usize) -> Option<Stage> {
        let d = img.get(at..)?;
        let h = d.get(..HEADER_LEN)?;
        let header = StageHeader {
            monster: h[0],
            diamond_scheme: h[1],
            warp_step: h[2],
            music: h[3],
            time_bcd: u16::from_le_bytes([h[4], h[5]]),
            object_list: u16::from_le_bytes([h[6], h[7]]),
            underground: h[8],
            bg: [h[9], h[10], h[11]],
            start: [[h[12], h[13]], [h[14], h[15]]],
        };
        let mut records = Vec::new();
        let mut p = HEADER_LEN;
        loop {
            let r = d.get(p..p + 5)?;
            if r[0] & 0x80 != 0 {
                break;
            }
            records.push(MapRecord { block: r[0], col: r[1], row: r[2], hrep: r[3], vrep: r[4] });
            p += 5;
        }
        let mut spawns = Vec::new();
        let mut q = header.object_list as usize;
        if q >= p && q < d.len() {
            while let Some(&t) = d.get(q) {
                if t == 0xFF {
                    break;
                }
                let Some(r) = d.get(q..q + 3) else {
                    spawns.clear();
                    break;
                };
                if r[1] < MAX_SPAWN_ROW && r[2] < crate::types::TYPE_COUNT {
                    spawns.push(Spawn { trigger: r[0], row: r[1], kind: r[2] });
                }
                q += 3;
            }
        }
        Some(Stage { header, records, spawns })
    }
}

/// One block of the block table.
#[derive(Clone, Copy, Debug)]
pub struct Block<'a> {
    pub width: usize,
    pub height: usize,
    /// `width * height` tiles, row by row.
    pub tiles: &'a [u8],
}

/// The block table of a hack image (64K, file offset = C64 address).
pub struct BlockTable<'a> {
    img: &'a [u8],
}

impl<'a> BlockTable<'a> {
    pub fn new(img: &'a [u8]) -> BlockTable<'a> {
        BlockTable { img }
    }

    pub fn block(&self, id: u8) -> Option<Block<'a>> {
        let t = BLOCK_TABLE + 2 * id as usize;
        let p = *self.img.get(t)? as usize | (*self.img.get(t + 1)? as usize) << 8;
        let w = *self.img.get(p)? as usize + 1;
        let h = *self.img.get(p + 1)? as usize + 1;
        let tiles = self.img.get(p + 2..p + 2 + w * h)?;
        Some(Block { width: w, height: h, tiles })
    }
}

/// Expands the map records to the 256 x 20 map (`$AC00..$BFFF`, row after row), like the stage builder `$9960..$9A14`
/// of the hack: per block row, the row is drawn `hrep` times side by side (the column wraps within the row) and the
/// whole thing `vrep` times downwards, a block height apart. A row that would land above `$AC00` or below `$BFFF`
/// ends the record (the hack flashes the border and goes on with the next record).
pub fn expand_map(img: &[u8], stage: &Stage) -> Vec<u8> {
    let mut map = vec![0u8; MAP_SIZE];
    let blocks = BlockTable::new(img);
    for rec in &stage.records {
        let Some(b) = blocks.block(rec.block) else { continue };
        let hcount = if rec.hrep == 0 { 256 } else { rec.hrep as usize };
        let vcount = if rec.vrep == 0 { 256 } else { rec.vrep as usize };
        let mut base_page = rec.row.wrapping_add(0xAC);
        'rows: for r in 0..b.height {
            let src = &b.tiles[r * b.width..(r + 1) * b.width];
            let mut page = base_page;
            for _ in 0..vcount {
                if !(0xAC..0xC0).contains(&page) {
                    break 'rows;
                }
                let mut lo = rec.col;
                for _ in 0..hcount {
                    for (y, &t) in src.iter().enumerate() {
                        let a = ((page as usize) << 8 | lo as usize) + y;
                        if (MAP_BASE..MAP_BASE + MAP_SIZE).contains(&a) {
                            map[a - MAP_BASE] = t;
                        }
                    }
                    lo = lo.wrapping_add(b.width as u8);
                }
                page = page.wrapping_add(b.height as u8);
            }
            base_page = base_page.wrapping_add(1);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic image: block 0 = 1x1 tile $11, block 1 = 2x2 tiles $21 $22 / $23 $24, block 2 = 3x1 tiles $31..$33.
    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        let blocks: [(&[u8], u16); 3] = [(&[0, 0, 0x11], 0x9300), (&[1, 1, 0x21, 0x22, 0x23, 0x24], 0x9310), (&[2, 0, 0x31, 0x32, 0x33], 0x9320)];
        for (i, (data, at)) in blocks.iter().enumerate() {
            img[0x9200 + 2 * i] = *at as u8;
            img[0x9201 + 2 * i] = (*at >> 8) as u8;
            img[*at as usize..*at as usize + data.len()].copy_from_slice(data);
        }
        img
    }

    fn stage_file(records: &[[u8; 5]], spawns: &[[u8; 3]]) -> Vec<u8> {
        let list = 0x83F0 + 16 + records.len() * 5 + 1;
        let mut f = vec![0xF0, 0x83];
        f.extend_from_slice(&[0x14, 3, 2, 1, 0x50, 0x01, list as u8, (list >> 8) as u8, 0, 0xFE, 0xF8, 0xF9, 0x20, 0xC0, 0x70, 0x90]);
        for r in records {
            f.extend_from_slice(r);
        }
        f.push(0xFF);
        for s in spawns {
            f.extend_from_slice(s);
        }
        f.push(0xFF);
        f
    }

    #[test]
    fn parses_header_records_and_objects() {
        let f = stage_file(&[[0, 0, 17, 85, 1], [1, 8, 14, 1, 1]], &[[0x35, 10, 11], [0x45, 16, 5]]);
        let s = Stage::parse(&f).unwrap();
        assert_eq!(s.header.monster, 0x14);
        assert_eq!(s.header.time_bcd, 0x0150);
        assert_eq!(s.header.time_seconds(), 150);
        assert_eq!(s.header.bg, [0xFE, 0xF8, 0xF9]);
        assert_eq!(s.header.start, [[0x20, 0xC0], [0x70, 0x90]]);
        assert_eq!(s.records.len(), 2);
        assert_eq!(s.records[0], MapRecord { block: 0, col: 0, row: 17, hrep: 85, vrep: 1 });
        assert_eq!(s.spawns, [Spawn { trigger: 0x35, row: 10, kind: 11 }, Spawn { trigger: 0x45, row: 16, kind: 5 }]);
        let h = StageHeader { time_bcd: 0x9999, ..s.header.clone() };
        assert_eq!(h.time_seconds(), 9999);
    }

    #[test]
    fn drops_objects_with_out_of_range_row_or_type() {
        let f = stage_file(&[[0, 0, 17, 1, 1]], &[[0x35, 200, 11], [0x36, 3, 0xF0], [0x37, 4, 5]]);
        let s = Stage::parse(&f).unwrap();
        assert_eq!(s.spawns, [Spawn { trigger: 0x37, row: 4, kind: 5 }]);
    }

    #[test]
    fn rejects_bad_files() {
        let good = stage_file(&[[0, 0, 17, 1, 1]], &[]);
        assert!(Stage::parse(&good).is_ok());
        assert!(Stage::parse(&good[..10]).is_err());
        let mut wrong = good.clone();
        wrong[1] = 0x84;
        assert!(Stage::parse(&wrong).unwrap_err().contains("load address"));
        // unterminated records
        let mut cut = good.clone();
        cut.truncate(2 + 16 + 5);
        assert!(Stage::parse(&cut).is_err());
        // object list pointer outside the file / inside the header
        let mut bad = good.clone();
        bad[2 + 6] = 0xFF;
        bad[2 + 7] = 0xFF;
        assert!(Stage::parse(&bad).is_err());
        bad[2 + 6] = 0xF0;
        bad[2 + 7] = 0x83;
        assert!(Stage::parse(&bad).is_err());
        // unterminated object list
        let mut open = good.clone();
        open.pop();
        assert!(Stage::parse(&open).is_err());
    }

    #[test]
    fn expands_repeats_and_rows() {
        let img = image();
        // ground: block 0 (1x1) at row 17, 85 times to the right; a 2x2 block at col 8 row 14; a 3x1 block repeated 2x3
        let f = stage_file(&[[0, 0, 17, 85, 1], [1, 8, 14, 1, 1], [2, 100, 3, 2, 3]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert_eq!(m.len(), MAP_SIZE);
        assert!(m[17 * 256..17 * 256 + 85].iter().all(|&t| t == 0x11));
        assert_eq!(m[17 * 256 + 85], 0);
        assert_eq!(&m[14 * 256 + 8..14 * 256 + 10], &[0x21, 0x22]);
        assert_eq!(&m[15 * 256 + 8..15 * 256 + 10], &[0x23, 0x24]);
        // 3x1 block: 2 side by side (6 tiles), 3 downwards (a block height of 1 apart)
        for row in 3..6 {
            assert_eq!(&m[row * 256 + 100..row * 256 + 106], &[0x31, 0x32, 0x33, 0x31, 0x32, 0x33]);
        }
        assert_eq!(m[6 * 256 + 100], 0);
        // the 2x2 block repeated 2 downwards lands two rows apart: rows 0,1 at row r and r+2
        let f = stage_file(&[[1, 0, 0, 1, 2]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert_eq!((m[0], m[256 + 1], m[2 * 256], m[3 * 256 + 1]), (0x21, 0x24, 0x21, 0x24));
    }

    #[test]
    fn column_wraps_in_the_row_and_overflow_aborts_the_record() {
        let img = image();
        // 3x1 block at column 254: the first copy spills into the next row (pointer arithmetic), later copies wrap
        let f = stage_file(&[[2, 254, 2, 2, 1]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert_eq!(&m[2 * 256 + 254..2 * 256 + 256], &[0x31, 0x32]);
        assert_eq!(m[3 * 256], 0x33, "the third tile crosses into the next row");
        // second copy starts at (254 + 3) & 255 = 1 of the same row
        assert_eq!(&m[2 * 256 + 1..2 * 256 + 4], &[0x31, 0x32, 0x33]);
        // a record below the map ends there: row 19 + 2x2 block: only the first row fits, the second is dropped
        let f = stage_file(&[[1, 0, 19, 1, 1], [0, 5, 20, 1, 1], [0, 6, 0, 1, 1]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert_eq!(m[19 * 256], 0x21);
        assert_eq!(m.iter().filter(|&&t| t == 0x23).count(), 0, "second row would be page $C0");
        assert_eq!(m[5], 0, "row 20 does not exist, the record is skipped");
        assert_eq!(m[6], 0x11, "the next record still runs");
        // a missing block is skipped
        let f = stage_file(&[[9, 0, 0, 1, 1]], &[]);
        assert!(expand_map(&img, &Stage::parse(&f).unwrap()).iter().all(|&t| t == 0));
    }

    #[test]
    fn zero_repeat_counts_mean_256() {
        let img = image();
        let f = stage_file(&[[0, 0, 0, 0, 1]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert!(m[..256].iter().all(|&t| t == 0x11), "hrep 0 = 256 columns");
        // vrep 0 = 256 rows: stops at the map end (the record is aborted on the first row past it)
        let f = stage_file(&[[0, 0, 0, 1, 0]], &[]);
        let m = expand_map(&img, &Stage::parse(&f).unwrap());
        assert_eq!((m[0], m[19 * 256], m[0..5120].iter().filter(|&&t| t == 0x11).count()), (0x11, 0x11, 20));
    }

    #[test]
    fn rooms_come_from_the_image() {
        let mut img = image();
        // room table: room $65 at $6100; header with a relative object list address, one record, one object
        img[ROOM_TABLE + 2] = 0x00;
        img[ROOM_TABLE + 3] = 0x61;
        let mut room = vec![0, 2, 2, 5, 0x00, 0x01, 0x16, 0x00, 0, 6, 7, 8, 0x20, 0xC0, 0x30, 0x90];
        room.extend_from_slice(&[0, 0, 17, 5, 1, 0xFF]);
        room.extend_from_slice(&[0x10, 9, 3, 0xFF]);
        img[0x6100..0x6100 + room.len()].copy_from_slice(&room);
        assert_eq!(room_address(&img, 0x65), Some(0x6100));
        assert_eq!(room_address(&img, 0x63), None);
        let s = Stage::parse_room(&img, 0x6100).unwrap();
        assert_eq!((s.header.music, s.header.bg, s.header.time_seconds()), (5, [6, 7, 8], 100));
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.spawns, [Spawn { trigger: 0x10, row: 9, kind: 3 }]);
        let m = expand_map(&img, &s);
        assert!(m[17 * 256..17 * 256 + 5].iter().all(|&t| t == 0x11));
        // unterminated records
        assert!(Stage::parse_room(&img[..0x6110], 0x6100).is_none());
    }

    #[test]
    fn block_table_checks_bounds() {
        let mut img = image();
        let t = BlockTable::new(&img);
        assert_eq!(t.block(1).unwrap().width, 2);
        assert!(t.block(0xFF).is_none() || t.block(0xFF).unwrap().tiles.len() == 1);
        img[0x9200] = 0xFF;
        img[0x9201] = 0xFF;
        assert!(BlockTable::new(&img).block(0).is_none(), "tiles run over the end of the image");
        assert!(BlockTable::new(&img[..0x9000]).block(0).is_none());
    }

    /// With the real data (`GIANA_B_IMAGE` = a 64K image or a VICE `save` dump of a running hack at the start of
    /// stage 1, `GIANA_B_STAGE` = its `.stg` file): the expanded map must equal the map in the dump byte for byte.
    #[test]
    fn real_stage_matches_the_dump() {
        let (Some(img), Some(stg)) = (std::env::var_os("GIANA_B_IMAGE"), std::env::var_os("GIANA_B_STAGE")) else {
            eprintln!("skipped: GIANA_B_IMAGE / GIANA_B_STAGE not set");
            return;
        };
        let mut img = std::fs::read(img).unwrap();
        if img.len() == 0x1_0002 {
            img.drain(..2);
        }
        let stage = Stage::parse(&std::fs::read(stg).unwrap()).unwrap();
        let map = expand_map(&img, &stage);
        let real = &img[MAP_BASE..MAP_BASE + MAP_SIZE];
        let diff: Vec<usize> = (0..MAP_SIZE).filter(|&i| map[i] != real[i]).collect();
        assert!(diff.is_empty(), "{} tiles differ, first at {:?}", diff.len(), diff.first().map(|&i| (i / 256, i % 256)));
    }
}
