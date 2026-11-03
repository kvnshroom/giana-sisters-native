//! Engine B: the 7A3 construction-set engine of the fan hacks "Ridley X", "Giana Power Edition", "Giana Sisters II",
//! "Junior" and others (see `docs/rom-hack-family-b.md`).
//!
//! The original engine keeps running for everything the hack kept (player, tiles, items, HUD, scroll, sound). `GameState`
//! owns an `Option<Box<EngineB>>` (`None` = original engine, switched on by [`crate::features::set_engine_b`]); the
//! places where the hack's code differs call into it behind a cheap `if self.engine_b.is_none()` check:
//!
//! | Place in the port | Hack code | What engine B does |
//! |---|---|---|
//! | `decompress_stage_layout` (`$9936`) | stage builder `$9936`, hook `$2F54`/`$306A`, loader `$9053` | [`EngineB::build`]: hook, stage file or resident room, map |
//! | `stage_colours_sub_0d39` (`$0D39`) | `$0D2E` | palette, underground flag and diamond colours from the stage header |
//! | `start_stage_after_post_fire` | `$0AD9..$0AF0` | 4-digit time from the header, stage number table `$8F99` |
//! | `setup_sprites` (`$13F0`) | `$1413` | start position from the header |
//! | `start_stage_after_banner_timed` | `$0B35..$0B3A` | music track from the header |
//! | `status_bar_sub_1cd7` | `$1CD7` | time shown with four digits |
//! | `update_timer_sub_1d0a` | `$1D0A` | time-out when the 4-digit time reaches 0 |
//! | `warp_target_stage` (features.rs) | `$0B85` | warp by the header's warp step |
//! | `stage_exit_l09a3` | stage exit | ends the game when the next stage has no file |
//! | `enemy_main_sub_2194` | `$2704` -> `$776E` | [`GameState::b_sprite_frame`]: the place of the new sprite program |
//! | colour RAM writes, tile classes | `$AB00`, `$9C20` copies | per game copies (`EngineB::colours`, `EngineB::classes`) |
//!
//! Work packages and the interface for the next steps are in `docs/rom-hack-family-b.md`, section 11.
//!
//! # What is not there yet
//!
//! Walkers, hoppers and items of the stage's object list are spawned and collide (E4, `sprites.rs`); chains, platforms, plants/ladders (E5), bosses and the homing shot (E6), water and the
//! other player hooks (E3). Treasure rooms and the title backdrop are built (map, header, colours) but their object lists
//! are not used. The trainer menu is not offered: [`EngineB::trainer_start_stage`] is the setting for its start stage.

mod glue;
pub mod sprites;
mod boss;
pub mod platform;
pub mod water;

use giana_engine_b::files::{self, ModFile};
use giana_engine_b::hooks::{apply_stage_constants, StageTables};
use giana_engine_b::stage::{room_address, FIRST_ROOM};
use giana_engine_b::types::{type_record, MonsterFile, MONSTER_TYPE, TYPE_COUNT};
use giana_engine_b::{expand_map, Spawn, Stage, StageHeader, TypeRecord};
use std::sync::Arc;

/// Header and object list of the stage that was built last (`$9000..$900F` and the list `$9006/7` points to).
#[derive(Clone, Debug)]
pub struct Current {
    /// stage number (`$20`) it was built for; `$64` is the title backdrop, `$65..` treasure rooms
    pub number: u8,
    pub header: StageHeader,
    /// object list of the stage: `(trigger column + 1, row, type)`, spawned when the map column reaches `trigger - 1`
    pub spawns: Vec<Spawn>,
}

/// Result of a stage build (`$9936`): everything the caller has to put into the game.
pub struct Built {
    /// the stage number after the hook (the trainer start stage can replace it)
    pub stage: u8,
    /// the hook changed the lives (`$043C`)
    pub lives: Option<u8>,
    /// the 256 x 20 map
    pub map: Vec<u8>,
    /// sprite shapes of a monster file that was just loaded: load address and data (`$FB80`)
    pub sprites: Option<(u16, Vec<u8>)>,
}

#[derive(Clone)]
pub struct EngineB {
    files: Arc<Vec<ModFile>>,
    /// colour attribute per character (`$AB00`): the image's table with the stage constants applied
    pub colours: [u8; 256],
    /// tile classes (`$9C20`, bit 0 = solid): the image's table with the stage constants applied
    pub classes: [u8; 256],
    /// type records `$00..=$36` (table `$21D8`); `$32` is the monster of the last stage that had one
    pub types: Vec<Option<TypeRecord>>,
    /// the loaded monster (`$7505`), `None` until a stage with a monster was loaded
    pub monster: Option<TypeRecord>,
    /// the monster file as in memory from `$7505` (record and what follows it, e.g. the movement script)
    pub monster_mem: Vec<u8>,
    /// the stage file in the stage buffer `$6DFD`: stage number and content (`$8FFD`)
    loaded: Option<(u8, Stage)>,
    /// header and object list of the last build
    pub current: Current,
    /// `$9FCF`: stage number the hook ran for last
    last_hook: Option<u8>,
    /// Start stage of the trainer (`$8FFE`, 0 = none): used once, at the next stage start that is not the title backdrop
    /// or a room; the hack then also sets the lives to 1. (The hack's trainer menu is not offered.)
    pub trainer_start_stage: u8,
    /// `$8FFF`: highest stage reached (up to `$2C`)
    pub highest_stage: u8,
    /// The last problem the loader had (a stage file that is missing or does not parse), for the app to show. The hack
    /// shows a message and waits for fire in a loop; the port builds an empty stage instead.
    pub error: Option<String>,
    /// byte offset of the next entry of the object list to spawn (`$0474`; wraps at 256 like the hack, see `sprites.rs`)
    pub spawn_next: usize,
    /// slot arrays and scratch of the sprite program (E4, `sprites.rs`)
    pub sprites: sprites::Sprites,
    /// water physics state (`$FF`, ladder patch), see `water.rs`
    pub water: water::Water,
    /// moving platforms and the zero page items `$F8..$FD`, see `platform.rs`
    pub plat: platform::Platforms,
}

impl EngineB {
    /// Engine B for the image (`img`: the module's 64K program image) and the disk files of the module.
    pub fn new(files: Arc<Vec<ModFile>>, img: &[u8]) -> EngineB {
        let mut colours = [0u8; 256];
        colours.copy_from_slice(&img[0xAB00..0xAC00]);
        let mut classes = [0u8; 256];
        classes.copy_from_slice(&img[0x9C20..0x9D20]);
        let types = (0..TYPE_COUNT).map(|k| type_record(img, k)).collect();
        EngineB {
            files,
            colours,
            classes,
            types,
            monster: None,
            monster_mem: Vec::new(),
            loaded: None,
            current: Current { number: 0, header: blank_header(), spawns: Vec::new() },
            last_hook: None,
            trainer_start_stage: 0,
            highest_stage: 0,
            error: None,
            spawn_next: 0,
            sprites: sprites::Sprites::new(),
            water: water::Water::default(),
            plat: platform::Platforms::default(),
        }
    }

    /// The installed engine B of the process (`features::set_engine_b`), if any.
    pub fn from_installed(img: &[u8]) -> Option<Box<EngineB>> {
        crate::features::engine_b_files().map(|f| Box::new(EngineB::new(f, img)))
    }

    /// The type record of object type `kind` (`$00..=$36`); `$32` is the loaded monster.
    pub fn type_record(&self, kind: u8) -> Option<&TypeRecord> {
        self.types.get(kind as usize)?.as_ref()
    }

    /// True when the module has a stage file for `stage` (1..=99).
    pub fn has_stage(&self, img: &[u8], stage: u8) -> bool {
        files::stage_pattern(img, stage).is_some_and(|p| files::find(&self.files, &p).is_some())
    }

    /// The stage builder `$9936` with its hook (`$2F54`, `$306A`, `$30AA..`) and loader (`$9053`) for stage number
    /// `stage` (`$20`): decides the stage number, applies the per-stage constants to the colour and class tables and
    /// to `charset` (the first 2 KiB of bank 3), loads the stage file and monster files when the stage changed (stage
    /// files are looked up with the pattern of the image), sets `current` (header and object list) and expands the map.
    /// Stage numbers `>= $64` are the resident rooms of the image.
    pub fn build(&mut self, img: &[u8], charset: &mut [u8], stage: u8) -> Built {
        let mut s = stage;
        let mut lives = None;
        // `$2F81`/`$3090`: the trainer start stage replaces a normal stage (once, see the field)
        if s < FIRST_ROOM && self.trainer_start_stage != 0 {
            s = std::mem::take(&mut self.trainer_start_stage);
            lives = Some(1);
        }
        // `$2F8F`: the stage hook `$306A` runs when the stage number changed (rooms skip it)
        if s <= FIRST_ROOM && self.last_hook != Some(s) {
            self.last_hook = Some(s);
            if s < 0x2C && s >= self.highest_stage {
                self.highest_stage = s;
            }
        }
        // `$30AA..$3125`: the per-stage constants (colours, tile classes)
        let mut t = StageTables { colours: &mut self.colours, classes: &mut self.classes, charset };
        apply_stage_constants(img, s, &mut t);
        // `$312B..`: load the stage file when it is not the one in the buffer
        let mut sprites = None;
        if s < FIRST_ROOM && self.loaded.as_ref().map(|l| l.0) != Some(s) {
            sprites = self.load_stage_file(img, s);
        }
        let stage_data: Stage = if s >= FIRST_ROOM {
            match room_address(img, s).and_then(|a| Stage::parse_room(img, a)) {
                Some(r) => r,
                None => {
                    self.error = Some(format!("the room of stage {s} is not in the program image"));
                    blank_stage()
                }
            }
        } else {
            self.loaded.as_ref().map_or_else(blank_stage, |l| l.1.clone())
        };
        let map = expand_map(img, &stage_data);
        self.current = Current { number: s, header: stage_data.header.clone(), spawns: stage_data.spawns.clone() };
        self.spawn_next = 0;
        self.sprites = sprites::Sprites::new();
        // the stage hook (`$2F5D`, `$2F77`): `$FD = 0`, the ladder patch of the water code is undone
        self.water.reset();
        self.plat.reset();
        Built { stage: s, lives, map, sprites }
    }

    /// The loader `$9053`: the stage file of `stage`, then the monster record and shapes named by the header. A file that
    /// is missing or broken leaves an error message and an empty stage; a missing monster file leaves the previous
    /// monster in place (the hack does not check either). Returns the shapes of a monster file that was loaded.
    fn load_stage_file(&mut self, img: &[u8], stage: u8) -> Option<(u16, Vec<u8>)> {
        let found = files::stage_pattern(img, stage).and_then(|p| files::find(&self.files, &p));
        let st = match found.map(|f| Stage::parse(&f.data)) {
            Some(Ok(st)) => {
                self.error = None;
                st
            }
            Some(Err(e)) => {
                self.error = Some(format!("stage {stage}: {e}"));
                blank_stage()
            }
            None => {
                self.error = Some(format!("no stage file for stage {stage}"));
                blank_stage()
            }
        };
        let monster = st.header.monster;
        self.loaded = Some((stage, st));
        let mut sprites = None;
        if monster != 0 {
            let def = files::monster_pattern(img, monster, b"NLM").and_then(|p| files::find(&self.files, &p));
            if let Some((m, f)) = def.and_then(|f| MonsterFile::parse(&f.data).ok().map(|m| (m, f))) {
                self.monster = Some(m.record);
                self.monster_mem = f.data.get(2..).unwrap_or_default().to_vec();
                self.types[MONSTER_TYPE as usize] = Some(m.record);
            }
            let spr = files::monster_pattern(img, monster, b"SPR").and_then(|p| files::find(&self.files, &p));
            if let Some(Ok((addr, data))) = spr.map(|f| giana_engine_b::types::parse_sprite_file(&f.data)) {
                sprites = Some((addr, data.to_vec()));
            }
        }
        sprites
    }
}

fn blank_header() -> StageHeader {
    StageHeader {
        monster: 0,
        diamond_scheme: 0,
        warp_step: 1,
        music: 0,
        time_bcd: 0x0100,
        object_list: 0,
        underground: 0,
        bg: [0, 0, 0],
        start: [[0x1C, 0xCB], [0x1C, 0xCB]],
    }
}

/// The stage used when a file is missing: no tiles, a start position on the ground line of the original's first stage.
fn blank_stage() -> Stage {
    Stage { header: blank_header(), records: Vec::new(), spawns: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image with block 0 = 1x1 tile `$11`, the stage hook layout of Ridley X with invented values, a stage template
    /// and a monster template.
    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x1_0000];
        img[0x9200] = 0x00;
        img[0x9201] = 0x93;
        img[0x9300..0x9303].copy_from_slice(&[0, 0, 0x11]);
        img[0x91C5..0x91D5].copy_from_slice(b"00???????????STG");
        img[0x91D5..0x91E5].copy_from_slice(b"00???????????NLM");
        for (a, op) in [(0x30AC, 0xC9), (0x30B0, 0xA9), (0x30B2, 0xA0), (0x30C2, 0xA9), (0x30C4, 0xA0), (0x30CA, 0xC9), (0x30CE, 0xA9)] {
            img[a] = op;
        }
        for (a, op) in [(0x30D1, 0xA9), (0x30E2, 0xDD), (0x30E7, 0xBC), (0x30F0, 0xA0), (0x30FA, 0xC9), (0x30FE, 0xA9), (0x3101, 0xA9), (0x310B, 0xC9), (0x310F, 0xA9), (0x3112, 0xA9)] {
            img[a] = op;
        }
        // room table: title backdrop at $6100 with one record
        img[0x3BF1] = 0x00;
        img[0x3BF2] = 0x61;
        let mut room = vec![0, 0, 1, 7, 0x00, 0x03, 0x16, 0, 1, 2, 3, 4, 0x20, 0xC0, 0x20, 0xC0];
        room.extend_from_slice(&[0, 0, 5, 4, 1, 0xFF, 0xFF]);
        img[0x6100..0x6100 + room.len()].copy_from_slice(&room);
        img
    }

    fn stage_file(monster: u8, time: u16, warp: u8, ground_row: u8) -> Vec<u8> {
        let list = 0x83F0 + 16 + 5 + 1;
        let mut f = vec![0xF0, 0x83, monster, 2, warp, 9, time as u8, (time >> 8) as u8, list as u8, (list >> 8) as u8, 1, 0xF6, 0xF7, 0xF8, 0x20, 0xC0, 0x40, 0x90];
        f.extend_from_slice(&[0, 0, ground_row, 10, 1, 0xFF, 0x12, 5, 3, 0xFF]);
        f
    }

    /// A 16 character disk name: `base` padded to 12, a dot and the 3 letter type.
    fn name(base: &str, ext: &str) -> Vec<u8> {
        format!("{base:<12}.{ext}").into_bytes()
    }

    fn engine(files: Vec<ModFile>) -> (EngineB, Vec<u8>) {
        let img = image();
        (EngineB::new(Arc::new(files), &img), img)
    }

    #[test]
    fn builds_a_stage_from_its_file() {
        let (mut b, img) = engine(vec![ModFile::new(&name("01 STAGE ONE", "STG"), stage_file(0, 0x0250, 2, 17))]);
        let mut charset = vec![5u8; 0x800];
        let r = b.build(&img, &mut charset, 1);
        assert_eq!((r.stage, r.lives), (1, None));
        assert_eq!(r.map[17 * 256], 0x11);
        assert_eq!(r.map[17 * 256 + 10], 0);
        assert_eq!(b.current.number, 1);
        assert_eq!(b.current.header.time_seconds(), 250);
        assert_eq!(b.current.header.warp_step, 2);
        assert_eq!(b.current.spawns, [Spawn { trigger: 0x12, row: 5, kind: 3 }]);
        assert!(b.error.is_none());
        // the same stage again does not load it again: remove the file, the buffer stays
        b.files = Arc::new(Vec::new());
        let r = b.build(&img, &mut charset, 1);
        assert_eq!(r.map[17 * 256], 0x11);
        // another stage without a file: an error and an empty stage
        let r = b.build(&img, &mut charset, 2);
        assert!(r.map.iter().all(|&t| t == 0));
        assert!(b.error.as_deref().unwrap().contains("stage 2"));
        assert!(!b.has_stage(&img, 1));
    }

    #[test]
    fn loads_the_monster_of_a_stage() {
        let mut nlm = vec![0x05, 0x75];
        nlm.extend((0..21).map(|i| 0x40 + i as u8));
        let spr = vec![0x80, 0xFB, 1, 2, 3];
        let files = vec![
            ModFile::new(&name("01 ONE", "STG"), stage_file(0x14, 0x0100, 2, 17)),
            ModFile::new(&name("14 FISH", "NLM"), nlm),
            ModFile::new(&name("14 FISH", "SPR"), spr),
        ];
        let (mut b, img) = engine(files);
        let r = b.build(&img, &mut vec![0; 0x800], 1);
        assert_eq!(r.sprites, Some((0xFB80, vec![1, 2, 3])));
        assert_eq!(b.monster.unwrap().raw[0], 0x40);
        assert_eq!(b.type_record(MONSTER_TYPE).unwrap().raw[20], 0x54);
    }

    #[test]
    fn title_backdrop_and_trainer_stage() {
        let (mut b, img) = engine(vec![ModFile::new(&name("05 FIVE", "STG"), stage_file(0, 0x0100, 2, 17)), ModFile::new(&name("01 ONE", "STG"), stage_file(0, 0x0100, 1, 3))]);
        let mut charset = vec![0u8; 0x800];
        // title backdrop: room $64 from the image, header of the room, the hook ran for it
        let r = b.build(&img, &mut charset, 0x64);
        assert_eq!(r.stage, 0x64);
        assert_eq!(r.map[5 * 256], 0x11);
        assert_eq!((b.current.header.music, b.current.header.bg), (7, [2, 3, 4]));
        // a trainer start stage replaces the next real stage once and sets the lives to 1
        b.trainer_start_stage = 5;
        let r = b.build(&img, &mut charset, 1);
        assert_eq!((r.stage, r.lives), (5, Some(1)));
        assert_eq!(b.current.number, 5);
        let r = b.build(&img, &mut charset, 1);
        assert_eq!((r.stage, r.lives), (1, None));
        assert_eq!(b.highest_stage, 5);
        // a room that is not in the table
        let r = b.build(&img, &mut charset, 0x70);
        assert!(r.map.iter().all(|&t| t == 0) && b.error.is_some());
    }
}
