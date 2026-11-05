//! Data formats of the 7A3 construction-set engine ("family B": Giana Sisters II, Giana Power Edition 2023, Ridley X,
//! Giana Sisters Junior, Super Marioahner Sisters, the C-Set tool). See `docs/rom-hack-family-b.md`.
//!
//! This crate only reads data; it contains no game data and no game logic. It is the first step of the native engine B:
//!
//! - [`stage`]: the `.stg` stage file (header, map records, enemy/object list) and the expansion of the records into
//!   the 256 x 20 tile map, exactly like the hack's own stage builder at `$9936`;
//! - [`types`]: the 21-byte enemy/object type records (table at `$21D8` of the hack image, plus the loaded monster
//!   files `.NLM`);
//! - [`hooks`]: the per-stage constants of the stage change hook (`$30AA..$3125`) and the diamond colour scheme,
//!   read from the hack image;
//! - [`files`]: how the hack finds its stage and monster files on the disk (name patterns with `?` wildcards).
//!
//! Addresses in the docs are C64 addresses of the hack's 64K program image.

pub mod snap;
pub mod files;
pub mod gem;
pub mod hooks;
pub mod stage;
pub mod types;

pub use stage::{expand_map, room_address, BlockTable, MapRecord, Spawn, Stage, StageHeader, FIRST_ROOM, MAP_COLS, MAP_ROWS, MAP_SIZE};
pub use types::{type_record, MonsterFile, TypeRecord};

#[cfg(test)]
mod tests {
    use super::*;

    /// With a real disk (`GIANA_B_DISK`): every stage file the importer collects parses, every monster file parses,
    /// every object type of every stage has a record in the type table (the `$32` monster is covered by its file), and
    /// the file name patterns of the stage loader find one file per stage number present on the disk.
    #[test]
    fn real_disk_files_parse() {
        let Some(path) = std::env::var_os("GIANA_B_DISK") else {
            eprintln!("skipped: GIANA_B_DISK not set");
            return;
        };
        let files = giana_module::make::collect_family_b_files(&std::fs::read(path).unwrap()).unwrap();
        let (mut stages, mut monsters) = (0, 0);
        for f in &files {
            match u16::from_le_bytes([f.data[0], f.data[1]]) {
                0x83F0 => {
                    let st = Stage::parse(&f.data).unwrap_or_else(|e| panic!("{:?}: {e}", String::from_utf8_lossy(&f.name)));
                    assert!(st.header.time_seconds() > 0 || st.header.time_bcd == 0);
                    assert!(st.spawns.iter().all(|s| s.kind < types::TYPE_COUNT), "{:?}", String::from_utf8_lossy(&f.name));
                    stages += 1;
                }
                0x7505 => {
                    MonsterFile::parse(&f.data).unwrap();
                    monsters += 1;
                }
                0xFB80 => {
                    types::parse_sprite_file(&f.data).unwrap();
                }
                other => panic!("unexpected load address {other:#x}"),
            }
        }
        eprintln!("{stages} stage files, {monsters} monster files");
        assert!(stages > 0);
    }
}
