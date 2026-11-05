//! The older variant of the 7A3 engine: "Giana Sisters II - Gem Nightmare" (V1.2, The Seven-A-Three, CSDb 29190).
//!
//! The sprite program (`$776E`), the player code and the map builder (`$9960..$9A14`) are the ones of the other
//! hacks of the family; what differs is where a stage comes from. Gem Nightmare still has the original game's
//! layout: its stage data is addressed through tables in the program image, and a stage file is not a self
//! contained record but a piece of memory that the loader drops at `$2913`:
//!
//! | What | Where (Gem Nightmare) |
//! |---|---|
//! | map records of stage `n` | pointer `$5FFE + 2n` (`$9956`); records `block, column, row, hrep, vrep` up to a byte with bit 7 |
//! | object list of stage `n` | pointer `$3D20 + 2(n-1)` (`$788E`), `column + 1, row, type` up to `$FF` (the spawner `$7869`) |
//! | time | low byte `$8FC2 + n` (`$0ADB`), hundreds from the high nibble of `$3BD7 + n` (`$0AE1`) |
//! | music | `$8FEB + n` (`$0B37`) |
//! | warp step | `$3C2E + n` (`$0B89`: `stage += table[stage]`) |
//! | palette | `$1E` = `$3C09 + n`, `$D021` = `$6065 + n`, `$D022` = `$3BD7 + n`, `$D023` = `$6098 + n` (`$0D2E`) |
//! | diamond colours | `$ABA9` = `$D021` value `>> 4`, `$ABA7 = $ABA8` = `$D023` value `>> 4` (`$0D2E`) |
//! | start position | `$60EA + 4(n-1) + $0456`, X then Y (`$1426`, `$1438`) |
//! | monster (type `$32`) | the 21 byte record at `$2914`, the first bytes of a stage file that has a monster |
//!
//! Stages `1..$0E` are resident in the image. The stage loader `$3147` (called from the stage hook `$312B`) loads
//! the files of stages `$0F..$29` (the file `STAGE (c)/7A3` with `c = stage + $31`, `@` for `$0F`, `A` for `$10`
//! ...) with the KERNAL's secondary address 0, i.e. to `$2913` whatever the load address in the file says. The first
//! byte of the file (`$2913`) is the stage number, with bit 7 set when the stage also needs a sprite file
//! (`E (c)`, loaded to its own address, `$FB80`). Everything else in the file is placed at the address the tables
//! of the image name (`$2914` for the records of most stages, other offsets for stages with a monster record at
//! `$2914`; the stage of the last boss has its records inside the sprite file at `$FCDC`).
//!
//! So this module keeps a copy of the image and drops the files into it like the loader does; the tables are read
//! from that memory. Every address that comes out of a table is bounds checked (the data is untrusted).

use crate::stage::{MapRecord, Spawn, Stage, StageHeader, MAX_SPAWN_ROW};
use crate::types::TYPE_COUNT;
use giana_module::container::ModFile;

pub use giana_module::gem::{is_gem, GemLayout};

fn abs(img: &[u8], at: usize) -> usize {
    img[at + 1] as usize | (img[at + 2] as usize) << 8
}

fn imm(img: &[u8], at: usize) -> u8 {
    img[at + 1]
}

/// Table addresses (operands of the instructions at the given places).
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Tables {
    records: usize,
    objects: usize,
    time_lo: usize,
    time_hi: usize,
    music: usize,
    warp: usize,
    underground: usize,
    bg0: usize,
    bg1: usize,
    bg2: usize,
    start_x: usize,
    start_y: usize,
}

impl Tables {
    fn read(img: &[u8]) -> Tables {
        Tables {
            records: abs(img, 0x9956),
            objects: abs(img, 0x788E),
            time_lo: abs(img, 0x0ADB),
            time_hi: abs(img, 0x0AE1),
            music: abs(img, 0x0B37),
            warp: abs(img, 0x0B89),
            underground: abs(img, 0x0D30),
            bg0: abs(img, 0x0D35),
            bg1: abs(img, 0x0D42),
            bg2: abs(img, 0x0D48),
            start_x: abs(img, 0x1426),
            start_y: abs(img, 0x1438),
        }
    }
}

/// Problems the loader can have (the hack shows a message and waits for fire; the port shows `error`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// sprite shapes loaded by this call: address and data (to be copied into the character/sprite bank)
    pub sprites: Option<(u16, Vec<u8>)>,
    pub error: Option<String>,
}

/// The memory of a running Gem Nightmare: the program image with the stage files dropped in.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GemMemory {
    pub layout: GemLayout,
    tables: Tables,
    mem: Vec<u8>,
    /// stage number the loader ran for last (`$60E7`); `None` after start
    loaded: Option<u8>,
}

impl GemMemory {
    /// `img`: the module's 64K program image (must pass [`is_gem`]).
    pub fn new(img: &[u8]) -> GemMemory {
        let mut mem = img.to_vec();
        mem.resize(0x1_0000, 0);
        GemMemory { layout: GemLayout::read(&mem), tables: Tables::read(&mem), mem, loaded: None }
    }

    /// One byte of the memory (0 above the end).
    pub fn byte(&self, a: usize) -> u8 {
        self.mem.get(a).copied().unwrap_or(0)
    }

    fn word(&self, a: usize) -> usize {
        self.byte(a) as usize | (self.byte(a + 1) as usize) << 8
    }

    /// The whole memory (64K), for readers of record scripts and the like.
    pub fn memory(&self) -> &[u8] {
        &self.mem
    }

    /// The monster record (type `$32`): 21 bytes at `$2914`.
    pub fn monster_record(&self) -> [u8; 21] {
        let mut r = [0u8; 21];
        for (i, b) in r.iter_mut().enumerate() {
            *b = self.byte(self.layout.load_at + 1 + i);
        }
        r
    }

    /// Drops a PRG file into memory like the KERNAL's `LOAD`: with `secondary0` to `dest` (the address in the file is
    /// ignored), otherwise to the address in the file. Stops at the end of memory.
    fn load_prg(&mut self, file: &[u8], dest: Option<usize>) -> Option<u16> {
        if file.len() < 2 {
            return None;
        }
        let at = dest.unwrap_or(file[0] as usize | (file[1] as usize) << 8);
        let data = &file[2..];
        let n = data.len().min(0x1_0000usize.saturating_sub(at));
        self.mem[at..at + n].copy_from_slice(&data[..n]);
        Some(at as u16)
    }

    /// The stage hook `$312B..$3146` and the loader `$3147`: loads the file of `stage` (and its sprite file) when
    /// the stage is one of the loaded ones and not the one loaded last.
    pub fn load_for_stage(&mut self, files: &[ModFile], stage: u8) -> LoadReport {
        let mut rep = LoadReport::default();
        if !self.layout.is_loaded(stage) || self.loaded == Some(stage) {
            return rep;
        }
        self.loaded = Some(stage);
        let Some((n1, n2)) = self.layout.file_names(&self.mem, stage) else {
            rep.error = Some(format!("stage {stage}: the file names are not in the program"));
            return rep;
        };
        let find = |n: &[u8]| files.iter().find(|f| f.name == n);
        match find(&n1) {
            None => {
                rep.error = Some(format!("no stage file {:?} for stage {stage}", String::from_utf8_lossy(&n1)));
                // like the hack's loader failing, nothing was loaded; the stage that was loaded before stays in memory,
                // so a retry is possible
                self.loaded = None;
                return rep;
            }
            Some(f) => {
                self.load_prg(&f.data, Some(self.layout.load_at));
            }
        }
        // `$31BF`: bit 7 of the first byte asks for the sprite file
        if self.byte(self.layout.load_at) & 0x80 != 0 {
            match find(&n2) {
                Some(f) => {
                    if let Some(at) = self.load_prg(&f.data, None) {
                        let end = (at as usize + f.data.len() - 2).min(0x1_0000);
                        rep.sprites = Some((at, self.mem[at as usize..end].to_vec()));
                    }
                }
                None => rep.error = Some(format!("no sprite file {:?} for stage {stage}", String::from_utf8_lossy(&n2))),
            }
        }
        rep
    }

    /// True when the stage can be played: resident, or its file is among `files`.
    pub fn has_stage(&self, files: &[ModFile], stage: u8) -> bool {
        if stage == 0 || stage >= self.layout.first_room {
            return false;
        }
        if !self.layout.is_loaded(stage) {
            return stage < self.layout.first_loaded;
        }
        self.layout.file_names(&self.mem, stage).is_some_and(|(n, _)| files.iter().any(|f| f.name == n))
    }

    /// The hook `$9A2C` that runs before the stage builder: stage 4 raises Giana's power to at least 2, and the
    /// start X of stage `$22` is set to one of two values depending on the stage being built (`$616E`). Returns
    /// the power Giana must have at least.
    pub fn builder_hook(&mut self, stage: u8) -> Option<u8> {
        let mut min_power = None;
        if stage == imm(&self.mem, 0x9A2E) {
            min_power = Some(imm(&self.mem, 0x9A38));
        }
        if stage != imm(&self.mem, 0x9A3C) {
            let v = if stage == imm(&self.mem, 0x9A40) { imm(&self.mem, 0x9A44) } else { imm(&self.mem, 0x9A47) };
            let at = abs(&self.mem, 0x9A49);
            if let Some(b) = self.mem.get_mut(at) {
                *b = v;
            }
        }
        min_power
    }

    /// Header values of `stage` from the tables (`$0ADB`, `$0B37`, `$0B89`, `$0D2E`, `$1426`).
    pub fn header(&self, stage: u8) -> StageHeader {
        let t = &self.tables;
        let s = stage as usize;
        let lo = self.byte(t.time_lo + s);
        let hi = self.byte(t.time_hi + s) >> 4;
        let i = (stage.wrapping_sub(1).wrapping_mul(4)) as usize;
        let at = |base: usize, k: usize| self.byte(base + ((i + k) & 0xFF));
        StageHeader {
            monster: self.byte(self.layout.load_at) >> 7,
            diamond_scheme: 0,
            warp_step: self.byte(t.warp + s),
            music: self.byte(t.music + s),
            time_bcd: lo as u16 | (hi as u16) << 8,
            object_list: self.word(t.objects + 2 * (stage.wrapping_sub(1) as usize)) as u16,
            underground: self.byte(t.underground + s),
            bg: [self.byte(t.bg0 + s), self.byte(t.bg1 + s), self.byte(t.bg2 + s)],
            start: [[at(t.start_x, 0), at(t.start_y, 0)], [at(t.start_x, 2), at(t.start_y, 2)]],
        }
    }

    /// The map records of `stage` (`$9952..$9966`): through the pointer at `$5FFE + 2 * stage`. `Err` for records
    /// that run off the end of memory.
    pub fn records(&self, stage: u8) -> Result<Vec<MapRecord>, String> {
        let mut p = self.word(self.tables.records + (stage as usize).wrapping_mul(2) % 0x100);
        let mut out = Vec::new();
        loop {
            let first = *self.mem.get(p).ok_or("map records are not terminated")?;
            if first & 0x80 != 0 {
                return Ok(out);
            }
            let r = self.mem.get(p..p + 5).ok_or("truncated map record")?;
            out.push(MapRecord { block: r[0], col: r[1], row: r[2], hrep: r[3], vrep: r[4] });
            p += 5;
            if out.len() > 0x4000 {
                return Err("too many map records".into());
            }
        }
    }

    /// The object list of `stage` (`$788E`): `column + 1, row, type` through the pointer at `$3D20 + 2 * (stage - 1)`.
    /// Entries with a row or type the sprite program has no table for are dropped (as in `Stage::parse`).
    pub fn spawns(&self, stage: u8) -> Vec<Spawn> {
        let mut q = self.word(self.tables.objects + (stage.wrapping_sub(1) as usize).wrapping_mul(2) % 0x100);
        let mut out = Vec::new();
        while let Some(&t) = self.mem.get(q) {
            if t == 0xFF {
                break;
            }
            let Some(r) = self.mem.get(q..q + 3) else { break };
            if r[1] < MAX_SPAWN_ROW && r[2] < TYPE_COUNT {
                out.push(Spawn { trigger: r[0], row: r[1], kind: r[2] });
            }
            q += 3;
            if out.len() > 0x2000 {
                break;
            }
        }
        out
    }

    /// Header, records and objects of `stage` as the builder `$9936` sees them. The objects of a resident room
    /// (title backdrop, bonus rooms) are not spawned.
    pub fn stage(&self, stage: u8) -> Result<Stage, String> {
        let header = self.header(stage);
        let records = self.records(stage)?;
        let spawns = if self.layout.is_room(stage) { Vec::new() } else { self.spawns(stage) };
        Ok(Stage { header, records, spawns })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An invented image with the operands this module reads (the real one is checked in the core tests with
    /// `GIANA_GEM_IMAGE`): tables at made-up places, loader constants like Gem Nightmare's.
    pub(crate) fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        for &(a, v) in &giana_module::gem::SIGNATURE {
            img[a] = v;
        }
        let set = |img: &mut Vec<u8>, a: usize, v: &[u8]| img[a..a + v.len()].copy_from_slice(v);
        // loader constants
        set(&mut img, 0x2F63, &[0xC9, 0x67]);
        set(&mut img, 0x2F67, &[0xC9, 0x2A]);
        set(&mut img, 0x309E, &[0xC9, 0x28]);
        set(&mut img, 0x312D, &[0xC9, 0x2A]);
        set(&mut img, 0x3131, &[0xC9, 0x0F]);
        set(&mut img, 0x313E, &[0x69, 0x31]);
        set(&mut img, 0x3140, &[0x8D, 0xB9, 0x08]);
        set(&mut img, 0x3176, &[0xA9, 0x0D, 0xA2, 0xB2, 0xA0, 0x08]);
        set(&mut img, 0x31CC, &[0xA9, 0x05, 0xA2, 0xB6, 0xA0, 0x08]);
        set(&mut img, 0x317F, &[0xA2, 0x13, 0xA0, 0x29]);
        img[0x08B2..0x08BF].copy_from_slice(b"STAGE (U)/7A3");
        // table operands
        for (at, table) in [(0x9956usize, 0x5FFEusize), (0x788E, 0x3D20), (0x0ADB, 0x8FC2), (0x0AE1, 0x3BD7), (0x0B37, 0x8FEB), (0x0B89, 0x3C2E), (0x0D30, 0x3C09), (0x0D35, 0x6065), (0x0D42, 0x3BD7), (0x0D48, 0x6098), (0x1426, 0x60EA), (0x1438, 0x60EB)] {
            img[at + 1] = table as u8;
            img[at + 2] = (table >> 8) as u8;
        }
        // the builder hook
        set(&mut img, 0x9A2E, &[0xC9, 0x04]);
        set(&mut img, 0x9A38, &[0xA2, 0x02]);
        set(&mut img, 0x9A3C, &[0xC9, 0x22]);
        set(&mut img, 0x9A40, &[0xC9, 0x12]);
        set(&mut img, 0x9A44, &[0xA9, 0x86]);
        set(&mut img, 0x9A47, &[0xA9, 0x42]);
        set(&mut img, 0x9A49, &[0x8D, 0x6E, 0x61]);
        img
    }

    fn put(img: &mut [u8], a: usize, v: &[u8]) {
        img[a..a + v.len()].copy_from_slice(v);
    }

    #[test]
    fn signature_and_layout() {
        let img = image();
        assert!(is_gem(&img));
        let mut other = img.clone();
        other[0x993B] = 0x54;
        assert!(!is_gem(&other));
        assert!(!is_gem(&img[..100]));
        let l = GemLayout::read(&img);
        assert_eq!((l.title_stage, l.first_room, l.first_loaded, l.loaded_end, l.load_at, l.letter_at), (0x67, 0x2A, 0x0F, 0x2A, 0x2913, 0x08B9));
        assert!(l.is_loaded(0x0F) && l.is_loaded(0x29) && !l.is_loaded(0x0E) && !l.is_loaded(0x2A));
        assert!(l.is_room(0x2A) && l.is_room(0x67) && !l.is_room(0x29));
        let (a, b) = l.file_names(&img, 0x0F).unwrap();
        assert_eq!((&a[..], &b[..]), (&b"STAGE (@)/7A3"[..], &b"E (@)"[..]));
        assert_eq!(l.file_names(&img, 0x29).unwrap().0, b"STAGE (Z)/7A3");
    }

    /// Resident stage 1 (records and objects in the image), stage `$10` from a file with a sprite file.
    #[test]
    fn builds_resident_and_loaded_stages() {
        let mut img = image();
        // stage 1: record table pointer $5FFE + 2 -> $6200, objects $3D20 -> $6300, tables
        put(&mut img, 0x6000, &[0x00, 0x62]);
        put(&mut img, 0x6200, &[3, 0, 17, 85, 1, 0xFF]);
        put(&mut img, 0x3D20, &[0x00, 0x63]);
        put(&mut img, 0x6300, &[0x45, 10, 5, 0x50, 40, 200, 0x51, 4, 0x77, 0xFF]);
        img[0x8FC2 + 1] = 0x50;
        img[0x3BD7 + 1] = 0x20;
        img[0x8FEB + 1] = 7;
        img[0x3C2E + 1] = 2;
        img[0x3C09 + 1] = 1;
        put(&mut img, 0x60EA, &[0x1C, 0xCB, 0x50, 0x60]);
        let g = GemMemory::new(&img);
        let st = g.stage(1).unwrap();
        assert_eq!(st.header.time_bcd, 0x0250);
        assert_eq!((st.header.music, st.header.warp_step, st.header.underground), (7, 2, 1));
        assert_eq!(st.header.start, [[0x1C, 0xCB], [0x50, 0x60]]);
        assert_eq!(st.records, [MapRecord { block: 3, col: 0, row: 17, hrep: 85, vrep: 1 }]);
        // row 200 and type $77 are dropped, the rest is kept
        assert_eq!(st.spawns, [Spawn { trigger: 0x45, row: 10, kind: 5 }]);
        // stage $10 (letter A): the pointer tables say $2914 and $2930
        put(&mut img, 0x5FFE + 2 * 0x10, &[0x14, 0x29]);
        put(&mut img, 0x3D20 + 2 * 0x0F, &[0x30, 0x29]);
        let mut g2 = GemMemory::new(&img);
        let mut file = vec![0x13, 0x29, 0x90 | 0x80];
        file.extend_from_slice(&[5, 1, 2, 3, 3, 0xFF]); // records at $2914 ... one record, terminator
        file.resize(2 + 0x1D, 0);
        file.extend_from_slice(&[0x33, 9, 3, 0xFF]); // objects at $2930? (offset 0x1D + 1)
        let sprites = vec![0x80, 0xFB, 1, 2, 3, 4];
        let files = vec![ModFile::new(b"STAGE (A)/7A3", file.clone()), ModFile::new(b"E (A)", sprites)];
        assert!(g2.has_stage(&files, 0x10) && !g2.has_stage(&files, 0x11) && g2.has_stage(&files, 5) && !g2.has_stage(&files, 0x2A));
        let rep = g2.load_for_stage(&files, 0x10);
        assert_eq!(rep.error, None);
        assert_eq!(rep.sprites, Some((0xFB80, vec![1, 2, 3, 4])));
        assert_eq!(g2.byte(0x2913), 0x90 | 0x80);
        let st = g2.stage(0x10).unwrap();
        assert_eq!(st.records, [MapRecord { block: 5, col: 1, row: 2, hrep: 3, vrep: 3 }]);
        assert_eq!(st.spawns, [Spawn { trigger: 0x33, row: 9, kind: 3 }]);
        assert_eq!(st.header.monster, 1);
        // loading the same stage again does nothing; a later file replaces the memory
        assert_eq!(g2.load_for_stage(&files, 0x10), LoadReport::default());
    }

    #[test]
    fn missing_file_and_hostile_pointers() {
        let mut img = image();
        put(&mut img, 0x5FFE + 2 * 0x0F, &[0xFE, 0xFF]); // records at the very end of memory, unterminated
        let mut g = GemMemory::new(&img);
        let rep = g.load_for_stage(&[], 0x0F);
        assert!(rep.error.unwrap().contains("STAGE (@)/7A3"));
        assert!(g.stage(0x0F).is_err(), "records run off the end of memory");
        // a pointer into the zero page and a table index beyond the stage numbers do not panic
        for s in 0..=255u8 {
            let _ = g.stage(s);
            let _ = g.header(s);
            let _ = g.spawns(s);
        }
    }

    /// Hostile data: random tables (pointers anywhere, also into the zero page and over the end of memory), random stage and
    /// sprite files of every size loaded for every stage. Nothing may panic and the lists stay bounded.
    #[test]
    fn random_memory_and_files_do_not_panic() {
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..40 {
            let mut img = image();
            // random bytes in the tables and the data areas the module reads
            for a in (0x3BD7..0x3C60).chain(0x3D20..0x3D80).chain(0x5FFE..0x6200).chain(0x8FC2..0x9016).chain(0x2914..0x2A00) {
                img[a] = rnd() as u8;
            }
            if round % 5 == 0 {
                // pointers that end in the last bytes of memory
                for a in (0x5FFE..0x6060).step_by(2) {
                    img[a] = 0xFD + (rnd() % 3) as u8;
                    img[a + 1] = 0xFF;
                }
            }
            let mut g = GemMemory::new(&img);
            let mut files = Vec::new();
            for stage in 0x0F..0x2A {
                let (a, b) = g.layout.file_names(&img, stage).unwrap();
                let len = (rnd() % 3000) as usize;
                let mut data: Vec<u8> = (0..len).map(|_| rnd() as u8).collect();
                if data.len() >= 2 && round % 3 == 0 {
                    data[0] = 0x13;
                    data[1] = 0x29;
                }
                files.push(ModFile::new(&a, data));
                let spr: Vec<u8> = (0..(rnd() % 2000) as usize + 2).map(|_| rnd() as u8).collect();
                files.push(ModFile::new(&b, spr));
            }
            for stage in 0..=255u8 {
                let rep = g.load_for_stage(&files, stage);
                assert!(rep.error.is_none() || !files.is_empty());
                if let Ok(st) = g.stage(stage) {
                    assert!(st.spawns.len() <= 0x2001);
                    assert!(st.records.len() <= 0x4001);
                }
                let _ = g.header(stage);
                let _ = g.builder_hook(stage);
                let _ = g.has_stage(&files, stage);
                let _ = g.monster_record();
            }
        }
    }

    #[test]
    fn builder_hook_pokes_the_start_of_the_bonus_stage() {
        let img = image();
        let mut g = GemMemory::new(&img);
        assert_eq!(g.builder_hook(4), Some(2));
        assert_eq!(g.byte(0x616E), 0x42);
        assert_eq!(g.builder_hook(0x12), None);
        assert_eq!(g.byte(0x616E), 0x86);
        assert_eq!(g.builder_hook(0x22), None);
        assert_eq!(g.byte(0x616E), 0x86, "stage $22 itself leaves it alone");
    }
}
