//! ROM modules in the settings folder: `<config dir>/modules/<id>.grom`. A module is a patch against the player's own
//! original data (see the `giana-module` crate), so it contains none of it and can be shared.
//!
//! Listing needs no game data (the names are in the files); everything that applies a module reads `ingame.bin` and
//! `f_2.prg` from the data folder.

use crate::settings::ModuleSort;
use giana_core::data::GameData;
use giana_module::{Applied, Meta, Module};
use std::path::{Path, PathBuf};

/// Largest input file taken (a VICE snapshot of a C64 is about 70 KB; a `.grom` is at most 1 MB).
const MAX_INPUT: u64 = 8 << 20;

/// Largest `.grom` taken from the network.
pub const MAX_RECEIVED: usize = 1 << 20;

/// A module in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// 16 hex digits, also the name of the file
    pub id: String,
    pub meta: Meta,
    pub path: PathBuf,
    /// How many ranges of game code the module changes (`Some` only when the game data was at hand to find out)
    pub code_changes: Option<usize>,
    /// Disk files the module carries (stages of a hack of the construction-set engine); 0 for the other modules
    pub files: usize,
    /// Engine the module needs: `None` is the original game's, `Some("b")` the 7A3 construction-set engine
    pub engine: Option<String>,
}

impl Installed {
    /// What kind of game the module is, for lists: empty for a module of the original engine, otherwise a few words.
    pub fn kind_text(&self) -> String {
        match (self.engine.as_deref(), self.files) {
            (None, 0) => String::new(),
            (None, n) => format!("{n} files"),
            (Some(e), n) if e == giana_module::tnw::ENGINE_T => format!("TNW game, {n} stage files"),
            (Some(_), n) => format!("construction-set game, {n} stage and monster files"),
        }
    }
}

/// The original `ingame.bin` and `f_2.prg`, the base of every module.
pub struct Base {
    pub ingame: Vec<u8>,
    pub f2: Vec<u8>,
}

impl Base {
    pub fn read(data_dir: &Path) -> Result<Base, String> {
        let get = |name: &str| std::fs::read(data_dir.join(name)).map_err(|e| format!("cannot read {name} in the game data folder: {e}"));
        Ok(Base { ingame: get("ingame.bin")?, f2: get("f_2.prg")? })
    }
}

fn is_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Name for a module made from a file: the file name in capitals, letters and digits only.
fn name_from_file(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { ' ' }).collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(giana_module::container::MAX_NAME).collect();
    let name = name.trim().to_string();
    if name.is_empty() {
        "MY MODULE".to_string()
    } else {
        name
    }
}

fn read_input(path: &Path) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?.len();
    if size > MAX_INPUT {
        return Err(format!("{} is too large to be a ROM module or a game image", path.display()));
    }
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// A `.grom` file (a ZIP archive, whatever its name).
fn looks_like_module(path: &Path, bytes: &[u8]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("grom")) || bytes.starts_with(b"PK")
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Makes a module from a modified image, a RAM dump, a PRG, a VICE snapshot, a `.bps` patch or a disk image of a fan
/// hack (`.d64`/`.g64`), optionally with a modified title program. A disk of a hack of the 7A3 construction-set engine
/// is booted, and the stage, monster and treasure room files it loads while playing go into the module. The result is
/// checked against the base; nothing is written.
pub fn make_module(base: &Base, input: &Path, title: Option<&Path>, meta: Meta) -> Result<(Module, Applied), String> {
    let bytes = read_input(input)?;
    let title = title.map(read_input).transpose()?;
    let name = input.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let module = giana_module::make::make_from_file(&base.ingame, &base.f2, &bytes, name, title, meta).map_err(|e| in_words(&e))?;
    let applied = giana_module::apply(&module, &base.ingame, &base.f2)?;
    Ok((module, applied))
}

/// Error texts of the module maker for the menu: no jargon about boot emulation.
fn in_words(e: &str) -> String {
    if e.contains("FAST LOADER") || e.contains("DIRECT ACCESS") {
        "this disk loads with its own fast loader, which this program cannot run. Use a version of the hack that loads with the normal disk routines".to_string()
    } else if e.contains("does not look like a hack") {
        format!("{e}. If this is a disk, its game may need a construction tool or an engine this program does not know")
    } else {
        e.to_string()
    }
}

/// Starts this program again, to play another ROM module: the arguments of this run without those that only applied to it
/// (`--module`, `--stage`, the files to open; see `cli::restart_args`). The caller ends this process afterwards, once its
/// window and sound are closed.
pub fn spawn_restart() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe).args(crate::cli::restart_args(std::env::args().skip(1))).spawn().map(|_| ())
}

/// How popular the known modules are, for `SORT: POPULAR`: the number of downloads on CSDb (checked 2026-11-03). The
/// 1989 Construction Kit hacks have no download numbers there; they come after the others in the order the scene
/// rates them, with made-up weights below the smallest real count. The 30th Anniversary preview (4208 downloads) has no
/// module id yet.
const POPULARITY: [(&str, u32); 14] = [
    ("da6409a65d5d148d", 3118), // Power Edition 2023
    ("41220fc5c98bafe3", 2851), // Gem Nightmare
    ("f6d968ea0fe0aaf0", 1974), // construction-set game
    ("389365968042ca20", 1364), // Super Marioahner Sisters
    ("078bcb477d8ea024", 1082), // Giana Sisters Junior
    ("6c4f772fd8d56dea", 716),  // Ridley X
    ("5430d50d3b6dc351", 646),  // TNW Giana Sisters 2
    ("d4c6852c767aa63d", 600),  // The Furry Knibble Girls
    ("f932c6d7d251cfa5", 550),  // Power Sisters
    ("99bee9fb6d14e7d1", 500),  // Frankie's Horror Trip 1
    ("15a0495c23074971", 450),  // Frankie's Horror Trip 2
    ("a42d3adbef5a03c3", 400),  // Gitty's Dreams 2
    ("fe3765783b2ed064", 350),  // The Gitty's News
    ("b8227c4074989ca7", 300),  // Gitty's Dreams 1
];

/// The popularity weight of a module (bigger is more popular), `None` for a module the table does not know.
pub fn popularity(id: &str) -> Option<u32> {
    POPULARITY.iter().find(|(known, _)| *known == id).map(|&(_, w)| w)
}

/// Puts the modules in the order of the modules page: favorites first (in the order of the sort), then the rest.
/// `Popular`: known modules by weight, unknown ones after them by name. `Name`: by name. `LastPlayed`: the one started
/// last first, never played ones after them by name.
pub fn order(list: &mut [Installed], sort: ModuleSort, favorites: &[String], last_played: &std::collections::BTreeMap<String, u64>) {
    let name = |m: &Installed| m.meta.name.to_ascii_uppercase();
    list.sort_by(|a, b| {
        let fav = |m: &Installed| favorites.contains(&m.id);
        let by_name = || name(a).cmp(&name(b)).then(a.id.cmp(&b.id));
        fav(b).cmp(&fav(a)).then_with(|| match sort {
            ModuleSort::Name => by_name(),
            ModuleSort::Popular => popularity(&b.id).cmp(&popularity(&a.id)).then_with(by_name),
            ModuleSort::LastPlayed => last_played.get(&b.id).cmp(&last_played.get(&a.id)).then_with(by_name),
        })
    });
}

/// The folder with the modules.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    /// `modules/` next to `settings.conf`.
    pub fn default_location() -> Option<Store> {
        crate::bindings::config_path().map(|p| Store::new(p.with_file_name("modules")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.grom"))
    }

    /// The installed modules by name. Files that are not readable modules are left out. With the data folder the number
    /// of changed code ranges is filled in as well.
    pub fn list(&self, data_dir: Option<&Path>) -> Vec<Installed> {
        let base = data_dir.and_then(|d| Base::read(d).ok());
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return out };
        for e in entries.flatten() {
            let path = e.path();
            let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".grom")).filter(|s| is_id(s)) else { continue };
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let Ok(module) = Module::read(&bytes) else { continue };
            let code_changes = base.as_ref().and_then(|b| giana_module::apply(&module, &b.ingame, &b.f2).ok()).map(|a| a.uncovered_code_changes().len());
            let (files, engine) = (module.files.len(), module.meta.engine.clone());
            out.push(Installed { id: id.to_ascii_lowercase(), meta: module.meta, path: path.clone(), code_changes, files, engine });
        }
        out.sort_by(|a, b| a.meta.name.to_ascii_uppercase().cmp(&b.meta.name.to_ascii_uppercase()).then(a.id.cmp(&b.id)));
        out
    }

    /// Adds a file: a `.grom`, or something to make a module from (a modified 64K image, a RAM dump, a PRG, a VICE
    /// snapshot, a `.bps`). The result is checked by applying it to the original data first.
    pub fn add(&self, path: &Path, data_dir: &Path) -> Result<Installed, String> {
        let base = Base::read(data_dir)?;
        let bytes = read_input(path)?;
        let (module, applied) = if looks_like_module(path, &bytes) {
            let module = Module::read(&bytes).map_err(|e| format!("this is not a valid ROM module: {e}"))?;
            let applied = giana_module::apply(&module, &base.ingame, &base.f2)?;
            (module, applied)
        } else {
            let meta = Meta::new(&name_from_file(path), "UNKNOWN", "", "");
            make_module(&base, path, None, meta)?
        };
        // data the game cannot use is found now, not at the next start
        giana_extract::derive::derive_files(applied.ingame.clone(), applied.f2.clone()).map_err(|e| format!("the game data of this module is not usable: {e}"))?;
        let dest = self.file(&applied.id);
        write_bytes_atomic(&dest, &module.write()).map_err(|e| format!("cannot store the module in {}: {e}", dest.display()))?;
        Ok(Installed { code_changes: Some(applied.uncovered_code_changes().len()), id: applied.id, files: module.files.len(), engine: module.meta.engine.clone(), meta: module.meta, path: dest })
    }

    /// The file of an installed module.
    pub fn read_module(&self, id: &str) -> Option<Vec<u8>> {
        is_id(id).then(|| std::fs::read(self.file(id)).ok()).flatten()
    }

    /// Keeps a `.grom` that came over the network (at most 1 MB). It has to be a valid module for the player's own game
    /// data, nothing is written otherwise. An installed module with the same id stays as it is.
    pub fn add_received(&self, bytes: &[u8], data_dir: &Path) -> Result<Installed, String> {
        if bytes.len() > MAX_RECEIVED {
            return Err("the module is too large".into());
        }
        let base = Base::read(data_dir)?;
        let module = Module::read(bytes).map_err(|e| format!("this is not a valid ROM module: {e}"))?;
        let applied = giana_module::apply(&module, &base.ingame, &base.f2)?;
        giana_extract::derive::derive_files(applied.ingame.clone(), applied.f2.clone()).map_err(|e| format!("the game data of this module is not usable: {e}"))?;
        let dest = self.file(&applied.id);
        let installed = Installed { id: applied.id.clone(), meta: module.meta.clone(), path: dest.clone(), code_changes: Some(applied.code_changes.len()), files: applied.files.len(), engine: applied.engine.clone() };
        if dest.exists() {
            return Ok(installed);
        }
        write_bytes_atomic(&dest, &module.write()).map_err(|e| format!("cannot store the module in {}: {e}", dest.display()))?;
        Ok(installed)
    }

    /// Deletes a module. An id that is not 16 hex digits does nothing.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        if !is_id(id) {
            return Err("not a module id".into());
        }
        std::fs::remove_file(self.file(id)).map_err(|e| format!("cannot remove the module: {e}"))
    }

    /// The module with this id, or the start of its id (at least 4 digits).
    pub fn find(&self, id_or_prefix: &str) -> Option<Installed> {
        let want = id_or_prefix.trim().to_ascii_lowercase();
        if want.len() < 4 {
            return None;
        }
        self.list(None).into_iter().find(|m| m.id == want || (want.len() < 16 && m.id.starts_with(&want)))
    }

    /// The patched game data of a module: reads `ingame.bin` and `f_2.prg` of the data folder, applies the module,
    /// derives the other files.
    pub fn load_selected(&self, id: &str, data_dir: &Path) -> Result<(GameData, Applied, Meta), String> {
        if !is_id(id) {
            return Err("not a module id".into());
        }
        let bytes = std::fs::read(self.file(id)).map_err(|_| "the module is not installed any more".to_string())?;
        let module = Module::read(&bytes)?;
        let base = Base::read(data_dir)?;
        let applied = giana_module::apply(&module, &base.ingame, &base.f2)?;
        let files = giana_extract::derive::derive_files(applied.ingame.clone(), applied.f2.clone())?;
        let refs: Vec<(&str, &[u8])> = files.iter().map(|f| (f.name, f.data.as_slice())).collect();
        let data = GameData::from_files(&refs)?;
        Ok((data, applied, module.meta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use giana_extract::image::{apply_patches, LOAD_ADDRESS};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("giana-modules-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A made-up base in a data folder (the real tables are not needed for storing).
    fn fake_data(dir: &Path) -> Base {
        let mut raw = vec![0u8; 0x1_0000];
        let mut x = 12345u32;
        for b in &mut raw[LOAD_ADDRESS..=0xFFF0] {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            *b = (x >> 16) as u8;
        }
        let ingame = apply_patches(&raw);
        let mut f2 = vec![0x01, 0x08];
        f2.extend((0..3000u32).map(|i| (i * 7) as u8));
        std::fs::write(dir.join("ingame.bin"), &ingame).unwrap();
        std::fs::write(dir.join("f_2.prg"), &f2).unwrap();
        Base { ingame, f2 }
    }

    fn entry(id: &str, name: &str) -> Installed {
        Installed { id: id.into(), meta: Meta::new(name, "A", "", ""), path: PathBuf::new(), code_changes: None, files: 0, engine: None }
    }

    fn ids(list: &[Installed]) -> Vec<&str> {
        list.iter().map(|m| m.id.as_str()).collect()
    }

    fn sample() -> Vec<Installed> {
        vec![
            entry("0000000000000001", "ZED UNKNOWN"),
            entry("b8227c4074989ca7", "GITTY'S DREAMS 1"),
            entry("0000000000000002", "ALPHA UNKNOWN"),
            entry("da6409a65d5d148d", "POWER EDITION 2023"),
            entry("41220fc5c98bafe3", "GEM NIGHTMARE"),
        ]
    }

    #[test]
    fn popular_order_puts_known_modules_by_weight_and_unknown_ones_last_by_name() {
        let mut l = sample();
        order(&mut l, ModuleSort::Popular, &[], &Default::default());
        assert_eq!(ids(&l), ["da6409a65d5d148d", "41220fc5c98bafe3", "b8227c4074989ca7", "0000000000000002", "0000000000000001"]);
        // every id of the table is a module id and the weights go down in the order written
        assert!(POPULARITY.iter().all(|(id, _)| is_id(id)));
        assert!(POPULARITY.windows(2).all(|w| w[0].1 > w[1].1));
        assert_eq!(popularity("da6409a65d5d148d"), Some(3118));
        assert_eq!(popularity("0000000000000001"), None);
    }

    #[test]
    fn name_order_ignores_the_popularity() {
        let mut l = sample();
        order(&mut l, ModuleSort::Name, &[], &Default::default());
        let names: Vec<&str> = l.iter().map(|m| m.meta.name.as_str()).collect();
        assert_eq!(names, ["ALPHA UNKNOWN", "GEM NIGHTMARE", "GITTY'S DREAMS 1", "POWER EDITION 2023", "ZED UNKNOWN"]);
    }

    #[test]
    fn last_played_order_puts_never_played_modules_last_by_name() {
        let mut l = sample();
        let played = std::collections::BTreeMap::from([("b8227c4074989ca7".to_string(), 50), ("0000000000000001".to_string(), 90)]);
        order(&mut l, ModuleSort::LastPlayed, &[], &played);
        assert_eq!(ids(&l), ["0000000000000001", "b8227c4074989ca7", "0000000000000002", "41220fc5c98bafe3", "da6409a65d5d148d"]);
    }

    #[test]
    fn favorites_come_first_in_every_sort() {
        let fav = vec!["0000000000000001".to_string(), "b8227c4074989ca7".to_string()];
        for sort in [ModuleSort::Popular, ModuleSort::Name, ModuleSort::LastPlayed] {
            let mut l = sample();
            order(&mut l, sort, &fav, &Default::default());
            assert!(l[..2].iter().all(|m| fav.contains(&m.id)), "{sort:?}: {:?}", ids(&l));
            assert!(l[2..].iter().all(|m| !fav.contains(&m.id)));
        }
        // inside the favorites the sort still decides
        let mut l = sample();
        order(&mut l, ModuleSort::Popular, &fav, &Default::default());
        assert_eq!(ids(&l)[..2], ["b8227c4074989ca7", "0000000000000001"]);
    }

    #[test]
    fn names_from_files() {
        assert_eq!(name_from_file(Path::new("/x/more-levels v2.bin")), "MORE LEVELS V2");
        assert_eq!(name_from_file(Path::new("/x/\u{e4}\u{f6}.bin")), "MY MODULE");
        assert!(name_from_file(Path::new(&format!("{}.bin", "a".repeat(80)))).len() <= 24);
    }

    #[test]
    fn an_empty_or_missing_store_lists_nothing() {
        let d = scratch("empty");
        assert!(Store::new(d.join("none")).list(None).is_empty());
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        std::fs::write(d.join("0123456789abcdef.grom"), "broken").unwrap();
        assert!(Store::new(d.clone()).list(None).is_empty());
        assert!(Store::new(d.clone()).find("0123").is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn listing_finding_and_removing_modules() {
        let (data, store_dir) = (scratch("data"), scratch("store"));
        let base = fake_data(&data);
        let store = Store::new(store_dir.join("modules"));
        let mut hack = base.ingame.clone();
        hack[0xAC10] ^= 0x55;
        let image = data.join("my hack.bin");
        std::fs::write(&image, &hack).unwrap();
        // the made-up base has no real tables, so the game data cannot be derived: nothing is stored
        assert!(store.add(&image, &data).is_err());
        assert!(store.list(None).is_empty(), "a module that cannot be used is not stored");
        // unknown and missing files
        std::fs::write(data.join("notes.txt"), "hello").unwrap();
        assert!(store.add(&data.join("notes.txt"), &data).is_err());
        assert!(store.add(&data.join("nothing.grom"), &data).is_err());
        // a made module put into the folder is listed, found and removed
        let (module, applied) = make_module(&base, &image, None, Meta::new("MY HACK", "ME", "1.0", "TEXT")).unwrap();
        std::fs::create_dir_all(store.dir()).unwrap();
        std::fs::write(store.dir().join(format!("{}.grom", applied.id)), module.write()).unwrap();
        let listed = store.list(None);
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].id.as_str(), listed[0].meta.name.as_str(), listed[0].code_changes), (applied.id.as_str(), "MY HACK", None));
        assert!(store.list(Some(&data))[0].code_changes.is_some());
        assert_eq!(store.find(&applied.id[..6]).map(|m| m.id), Some(applied.id.clone()));
        assert!(store.find("abc").is_none() && store.find("ffffffffffffffff").is_none());
        assert!(store.remove("../x").is_err());
        store.remove(&applied.id).unwrap();
        assert!(store.list(None).is_empty());
        for d in [data, store_dir] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// With the real data: a module that changes a map byte and a colour is added, listed, loaded; the data differs.
    #[test]
    fn a_module_is_added_loaded_and_changes_the_data() {
        giana_core::skip_without_data!();
        let Some(data_dir) = giana_core::data::candidate_dirs(None).into_iter().find(|d| giana_extract::verify_dir(d).is_empty()) else { return };
        let base = Base::read(&data_dir).unwrap();
        let tmp = scratch("real");
        let mut hack = base.ingame.clone();
        hack[0xAC10] ^= 0x55;
        hack[0x604E] = hack[0x604E].wrapping_add(1) & 15;
        let file = tmp.join("test hack.bin");
        std::fs::write(&file, &hack).unwrap();
        let store = Store::new(tmp.join("modules"));
        let added = store.add(&file, &data_dir).unwrap();
        assert_eq!((added.meta.name.as_str(), added.meta.author.as_str()), ("TEST HACK", "UNKNOWN"));
        // adding the stored file again is the same module
        let again = store.add(&added.path, &data_dir).unwrap();
        assert_eq!(again.id, added.id);
        assert_eq!(store.list(Some(&data_dir)).len(), 1);
        let (data, applied, meta) = store.load_selected(&added.id, &data_dir).unwrap();
        assert_eq!(meta.name, "TEST HACK");
        assert_eq!(data.ingame, applied.ingame);
        assert_eq!(data.ingame[0xAC10], base.ingame[0xAC10] ^ 0x55);
        assert!(store.load_selected("0000000000000000", &data_dir).is_err());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn kinds_and_plain_error_texts() {
        let mk = |files, engine: Option<&str>| Installed {
            id: "0".repeat(16),
            meta: Meta::new("X", "Y", "1", ""),
            path: PathBuf::new(),
            code_changes: None,
            files,
            engine: engine.map(str::to_string),
        };
        assert_eq!(mk(0, None).kind_text(), "");
        assert_eq!(mk(3, None).kind_text(), "3 files");
        assert!(mk(75, Some("b")).kind_text().contains("75 stage and monster files"));
        assert!(in_words("THIS DISK USES A FAST LOADER OR CUSTOM DRIVE CODE - IT CANNOT BE LOADED").contains("own fast loader"));
        assert!(in_words("the input does not look like a hack of this game (only 11%)").contains("construction tool"));
        assert_eq!(in_words("cannot read x"), "cannot read x");
    }

    /// With real family B disks (`GIANA_B_DISK=a.d64:b.d64`) and the original data: the disks are added to a store, listed
    /// with their files and engine, loaded with files and engine, and a second add is the same module.
    #[test]
    fn family_b_disks_are_added_listed_and_loaded() {
        let Some(disks) = std::env::var_os("GIANA_B_DISK") else { return };
        giana_core::skip_without_data!();
        let Some(data_dir) = giana_core::data::candidate_dirs(None).into_iter().find(|d| giana_extract::verify_dir(d).is_empty()) else { return };
        let tmp = scratch("bdisk");
        let store = Store::new(tmp.join("modules"));
        for disk in std::env::split_paths(&disks) {
            let added = store.add(&disk, &data_dir).unwrap();
            assert_eq!(added.engine.as_deref(), Some("b"), "{}", disk.display());
            assert!(added.files >= 20);
            assert_eq!(store.add(&disk, &data_dir).unwrap().id, added.id, "same id on a second import");
            assert_eq!(store.add(&added.path, &data_dir).unwrap().id, added.id, "the stored module is the same module");
            let listed = store.list(Some(&data_dir)).into_iter().find(|m| m.id == added.id).unwrap();
            assert_eq!((listed.files, listed.engine.as_deref()), (added.files, Some("b")));
            let (_, applied, _) = store.load_selected(&added.id, &data_dir).unwrap();
            assert_eq!((applied.files.len(), applied.engine.as_deref()), (added.files, Some("b")));
            assert_eq!(applied.id, added.id);
        }
        let _ = std::fs::remove_dir_all(tmp);
    }
}
