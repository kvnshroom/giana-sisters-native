//! The `.grom` file: a ZIP with `module.json`, `ingame.bps` and/or `f_2.bps`, and an optional `readme.txt`.
//!
//! Format 2 (only written when needed) adds files the hack loads from its disk while playing (`files/<name>`: stage
//! files, monster files of the 7A3 construction-set engine, "family B") and an `engine` tag in `module.json`. Format 1
//! files stay valid; a build that only knows format 1 refuses a format 2 module instead of playing it wrongly.

use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

/// Maximum size of a `.grom` file and of each entry once unpacked.
pub const MAX_SIZE: usize = 1 << 20;
pub const MAX_NAME: usize = 24;
pub const MAX_AUTHOR: usize = 24;
pub const MAX_VERSION: usize = 12;
pub const MAX_DESCRIPTION: usize = 200;
pub const MAX_URL: usize = 200;
/// Highest container format this build reads.
pub const FORMAT_MAX: u32 = 2;
/// Engine tag of the 7A3 construction-set engine (Giana Sisters II, Giana Power Edition, Ridley X, ...).
pub const ENGINE_B: &str = "b";
/// Limits for the disk files of a format 2 module (a stage file is 200..1500 bytes, a monster sprite file up to 800).
pub const MAX_FILES: usize = 256;
pub const MAX_FILE_SIZE: usize = 32 * 1024;
pub const MAX_FILES_TOTAL: usize = 512 * 1024;
/// Longest CBM disk file name.
pub const MAX_FILE_NAME: usize = 16;
pub const MAX_ENGINE: usize = 16;

/// Module metadata (`module.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// Container format read from `module.json` (1 or 2). Writing picks the lowest format that can hold the module.
    pub format: u32,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub url: Option<String>,
    /// SHA-256 (hex) of the base `ingame.bin`.
    pub base: String,
    /// Engine the module needs (format 2): `None` is the original engine, [`ENGINE_B`] the 7A3 construction-set one.
    pub engine: Option<String>,
}

/// A disk file the hack loads while it plays: the CBM file name (up to 16 bytes, as in the directory, without the
/// padding) and the content including the two-byte load address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModFile {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}

impl ModFile {
    pub fn new(name: &[u8], data: Vec<u8>) -> ModFile {
        ModFile { name: name.to_vec(), data }
    }
}

/// Zip entry name of a disk file: `files/` plus the CBM name with every byte outside `!`..`~` (and `%`, `/`, `\`)
/// written as `%XX`, so any PETSCII name survives a round trip.
pub fn file_entry_name(name: &[u8]) -> String {
    let mut s = String::from("files/");
    for &b in name {
        if (0x21..=0x7E).contains(&b) && !matches!(b, b'%' | b'/' | b'\\' | b':') {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// Inverse of [`file_entry_name`]; `None` when the entry is not under `files/` or the escape is malformed.
pub fn parse_entry_name(entry: &str) -> Option<Vec<u8>> {
    let rest = entry.strip_prefix("files/")?;
    let b = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = rest.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

impl Meta {
    /// A format-1 meta without base hash (the maker fills it in).
    pub fn new(name: &str, author: &str, version: &str, description: &str) -> Meta {
        Meta {
            format: 1,
            name: name.into(),
            author: author.into(),
            version: version.into(),
            description: description.into(),
            url: None,
            base: String::new(),
            engine: None,
        }
    }

    /// Checks format, non-empty name and the length limits (in characters).
    pub fn validate(&self) -> Result<(), String> {
        if self.format == 0 || self.format > FORMAT_MAX {
            return Err(format!("unsupported module format {}", self.format));
        }
        if let Some(e) = &self.engine {
            if e.is_empty() || e.len() > MAX_ENGINE || !e.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                return Err("module engine must be 1 to 16 lowercase letters or digits".into());
            }
        }
        if self.name.trim().is_empty() {
            return Err("the module needs a name".into());
        }
        let limits = [
            ("name", self.name.chars().count(), MAX_NAME),
            ("author", self.author.chars().count(), MAX_AUTHOR),
            ("version", self.version.chars().count(), MAX_VERSION),
            ("description", self.description.chars().count(), MAX_DESCRIPTION),
            ("url", self.url.as_deref().map_or(0, |u| u.chars().count()), MAX_URL),
        ];
        for (what, n, max) in limits {
            if n > max {
                return Err(format!("module {what} is too long ({n} characters, at most {max})"));
            }
        }
        Ok(())
    }

    fn to_json(&self, format: u32) -> String {
        let mut v = json!({
            "format": format, "name": self.name, "author": self.author, "version": self.version,
            "description": self.description, "base": self.base,
        });
        if let (Some(url), Some(o)) = (&self.url, v.as_object_mut()) {
            o.insert("url".into(), Value::String(url.clone()));
        }
        if let (Some(e), Some(o)) = (&self.engine, v.as_object_mut()) {
            o.insert("engine".into(), Value::String(e.clone()));
        }
        v.to_string()
    }

    fn from_json(bytes: &[u8]) -> Result<Meta, String> {
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("module.json is not valid JSON: {e}"))?;
        let o = v.as_object().ok_or("module.json must be an object")?;
        let text = |k: &str, required: bool| -> Result<String, String> {
            match o.get(k) {
                Some(Value::String(s)) => Ok(s.clone()),
                None | Some(Value::Null) if !required => Ok(String::new()),
                _ => Err(format!("module.json: \"{k}\" is missing or not a string")),
            }
        };
        let format = o.get("format").and_then(Value::as_u64).ok_or("module.json: \"format\" is missing")?;
        if format == 0 || format > FORMAT_MAX as u64 {
            return Err(format!("unsupported module format {format}"));
        }
        let meta = Meta {
            format: format as u32,
            name: text("name", true)?,
            author: text("author", false)?,
            version: text("version", false)?,
            description: text("description", false)?,
            url: match o.get("url") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            },
            base: text("base", true)?,
            engine: match o.get("engine") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            },
        };
        meta.validate()?;
        Ok(meta)
    }
}

/// A parsed `.grom` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub meta: Meta,
    pub ingame_patch: Option<Vec<u8>>,
    pub title_patch: Option<Vec<u8>>,
    pub readme: Option<String>,
    /// Disk files of a format 2 module, sorted by name.
    pub files: Vec<ModFile>,
}

fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let f = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("cannot read {name}: {e}")),
    };
    if f.size() > MAX_SIZE as u64 {
        return Err(format!("{name} is too large"));
    }
    let mut buf = Vec::new();
    f.take(MAX_SIZE as u64 + 1).read_to_end(&mut buf).map_err(|e| format!("cannot read {name}: {e}"))?;
    if buf.len() > MAX_SIZE {
        return Err(format!("{name} is too large"));
    }
    Ok(Some(buf))
}

impl Module {
    /// Parses and validates a `.grom` file. Unknown entries are ignored.
    pub fn read(bytes: &[u8]) -> Result<Module, String> {
        if bytes.len() > MAX_SIZE {
            return Err("module file is larger than 1 MB".into());
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a ROM module (bad ZIP): {e}"))?;
        let json = read_entry(&mut zip, "module.json")?.ok_or("not a ROM module (module.json is missing)")?;
        let meta = Meta::from_json(&json)?;
        let ingame_patch = read_entry(&mut zip, "ingame.bps")?;
        let title_patch = read_entry(&mut zip, "f_2.bps")?;
        if ingame_patch.is_none() && title_patch.is_none() {
            return Err("the module contains no patch".into());
        }
        let readme = read_entry(&mut zip, "readme.txt")?.map(|b| String::from_utf8_lossy(&b).into_owned());
        let mut files = Vec::new();
        if meta.format >= 2 {
            let mut names: Vec<String> = Vec::new();
            for i in 0..zip.len() {
                let f = zip.by_index(i).map_err(|e| format!("cannot read the module: {e}"))?;
                if let Ok(n) = f.name() {
                    if n.starts_with("files/") {
                        names.push(n.to_string());
                    }
                }
            }
            let mut total = 0usize;
            for entry in names {
                let Some(name) = parse_entry_name(&entry) else {
                    return Err(format!("bad disk file entry name {entry:?}"));
                };
                if name.len() > MAX_FILE_NAME {
                    return Err(format!("disk file name too long: {entry:?}"));
                }
                let data = read_entry(&mut zip, &entry)?.unwrap_or_default();
                if data.len() > MAX_FILE_SIZE {
                    return Err(format!("disk file {entry:?} is too large"));
                }
                total += data.len();
                if total > MAX_FILES_TOTAL {
                    return Err("the disk files of the module are too large".into());
                }
                files.push(ModFile { name, data });
            }
            if files.len() > MAX_FILES {
                return Err(format!("too many disk files ({}, at most {MAX_FILES})", files.len()));
            }
            files.sort_by(|a, b| a.name.cmp(&b.name));
            if files.windows(2).any(|w| w[0].name == w[1].name) {
                return Err("two disk files have the same name".into());
            }
        }
        Ok(Module { meta, ingame_patch, title_patch, readme, files })
    }

    /// The lowest container format that can hold this module: 2 when it has disk files or needs another engine.
    pub fn needed_format(&self) -> u32 {
        if self.files.is_empty() && self.meta.engine.is_none() {
            1
        } else {
            2
        }
    }

    /// Serializes to a `.grom` file (deflate).
    pub fn write(&self) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut entries: Vec<(String, Vec<u8>)> =
            vec![("module.json".into(), self.meta.to_json(self.needed_format()).into_bytes())];
        if let Some(p) = &self.ingame_patch {
            entries.push(("ingame.bps".into(), p.clone()));
        }
        if let Some(p) = &self.title_patch {
            entries.push(("f_2.bps".into(), p.clone()));
        }
        if let Some(r) = &self.readme {
            entries.push(("readme.txt".into(), r.clone().into_bytes()));
        }
        let mut files: Vec<&ModFile> = self.files.iter().collect();
        files.sort_by(|a, b| a.name.cmp(&b.name));
        for f in files {
            entries.push((file_entry_name(&f.name), f.data.clone()));
        }
        for (name, data) in entries {
            // writing into a Vec cannot fail
            let _ = zip.start_file(name.as_str(), opts);
            let _ = zip.write_all(&data);
        }
        zip.finish().map(Cursor::into_inner).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Meta {
        let mut m = Meta::new("MORE LEVELS", "someone", "1.0", "33 new stages");
        m.url = Some("https://example.org".into());
        m.base = "ab".repeat(32);
        m
    }

    fn module() -> Module {
        Module {
            meta: meta(),
            ingame_patch: Some(vec![1, 2, 3]),
            title_patch: Some(vec![4; 5000]),
            readme: Some("hello".into()),
            files: Vec::new(),
        }
    }

    #[test]
    fn round_trip() {
        let m = module();
        let bytes = m.write();
        assert_eq!(Module::read(&bytes).unwrap(), m);
        assert_eq!(m.write(), bytes, "deterministic");
        let mut m2 = module();
        m2.meta.url = None;
        m2.title_patch = None;
        m2.readme = None;
        assert_eq!(Module::read(&m2.write()).unwrap(), m2);
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (n, d) in entries {
            z.start_file(*n, o).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn rejects_bad_input() {
        let good = meta().to_json(1);
        let ok = |j: &str| Module::read(&zip_with(&[("module.json", j.as_bytes()), ("ingame.bps", b"x")]));
        assert!(ok(&good).is_ok());
        assert!(ok("not json").is_err());
        assert!(ok(&good.replace("\"format\":1", "\"format\":3")).is_err());
        assert!(ok(&good.replace("\"format\":1", "\"format\":0")).is_err());
        assert!(ok(&good.replace("MORE LEVELS", "")).is_err());
        assert!(ok(&good.replace("MORE LEVELS", &"X".repeat(25))).is_err());
        assert!(ok(&good.replace("33 new stages", &"d".repeat(201))).is_err());
        assert!(Module::read(&zip_with(&[("module.json", good.as_bytes())])).is_err(), "no patch");
        assert!(Module::read(&zip_with(&[("ingame.bps", b"x")])).is_err(), "no json");
        assert!(Module::read(b"hello").is_err());
        assert!(Module::read(&[]).is_err());
    }

    #[test]
    fn unknown_entries_ignored() {
        let good = meta().to_json(1);
        let z = zip_with(&[("module.json", good.as_bytes()), ("f_2.bps", b"p"), ("extra.txt", b"?")]);
        assert!(Module::read(&z).is_ok());
    }

    fn module_b() -> Module {
        let mut m = module();
        m.meta.engine = Some(ENGINE_B.into());
        m.files = vec![
            ModFile::new(b"02 RIDLEYX02.STG", vec![0xF0, 0x83, 1, 2, 3]),
            ModFile::new(b"01 RIDLEYX01.STG", vec![0xF0, 0x83, 9, 9]),
            ModFile::new(b"01STG\x14\x14 -ROLE-", vec![0x05, 0x75, 7]),
            ModFile::new(b"%/\\:x y", vec![1]),
        ];
        m
    }

    #[test]
    fn format_2_round_trip_and_minimal_format() {
        let m = module_b();
        let bytes = m.write();
        let back = Module::read(&bytes).unwrap();
        assert_eq!(back.meta.format, 2);
        assert_eq!(back.meta.engine.as_deref(), Some("b"));
        let mut want = m.clone();
        want.files.sort_by(|a, b| a.name.cmp(&b.name));
        want.meta.format = 2;
        assert_eq!(back, want);
        assert_eq!(m.write(), bytes, "deterministic whatever the order of the files");
        let mut reversed = m.clone();
        reversed.files.reverse();
        assert_eq!(reversed.write(), bytes);
        // a module without files or engine is still written as format 1
        assert_eq!(module().needed_format(), 1);
        assert_eq!(Module::read(&module().write()).unwrap().meta.format, 1);
        // engine without files is format 2 too
        let mut e = module();
        e.meta.engine = Some("b".into());
        assert_eq!(Module::read(&e.write()).unwrap().meta.format, 2);
    }

    #[test]
    fn entry_names_round_trip() {
        for name in [&b"01 RIDLEYX01.STG"[..], b"%/\\:", b"\x01\xA0\xFF", b"a b", b"01STG\x14\x14"] {
            let e = file_entry_name(name);
            assert!(e.is_ascii() && !e["files/".len()..].contains(['/', '\\', ':']), "{e}");
            assert_eq!(parse_entry_name(&e).unwrap(), name);
        }
        assert_eq!(file_entry_name(b"A B"), "files/A%20B");
        assert!(parse_entry_name("other/x").is_none());
        assert!(parse_entry_name("files/").is_none());
        assert!(parse_entry_name("files/%G1").is_none());
        assert!(parse_entry_name("files/%4").is_none());
    }

    #[test]
    fn format_1_ignores_files_and_old_json_reads() {
        // format 1 json with a stray files/ entry: ignored, like any unknown entry
        let good = meta().to_json(1);
        let z = zip_with(&[("module.json", good.as_bytes()), ("ingame.bps", b"p"), ("files/X", b"?")]);
        assert!(Module::read(&z).unwrap().files.is_empty());
    }

    #[test]
    fn disk_file_limits() {
        let json = {
            let mut m = meta();
            m.engine = Some("b".into());
            m.to_json(2)
        };
        let ok = |entries: &[(&str, &[u8])]| {
            let mut v: Vec<(&str, &[u8])> = vec![("module.json", json.as_bytes()), ("ingame.bps", b"p")];
            v.extend_from_slice(entries);
            Module::read(&zip_with(&v))
        };
        assert_eq!(ok(&[("files/A", b"1")]).unwrap().files.len(), 1);
        let big = vec![7u8; MAX_FILE_SIZE + 1];
        assert!(ok(&[("files/A", &big)]).unwrap_err().contains("too large"));
        let long = "files/".to_string() + &"N".repeat(17);
        assert!(ok(&[(long.as_str(), b"1")]).unwrap_err().contains("too long"));
        assert!(ok(&[("files/%ZZ", b"1")]).is_err());
        // total size cap: 20 files of 30 KB
        let chunk = vec![1u8; 30 * 1024];
        let names: Vec<String> = (0..20).map(|i| format!("files/F{i:02}")).collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), chunk.as_slice())).collect();
        let mut rng = crate::test_rng(5);
        let noisy: Vec<Vec<u8>> = (0..20).map(|_| (0..30 * 1024).map(|_| rng()).collect()).collect();
        let entries_noisy: Vec<(&str, &[u8])> = names.iter().zip(&noisy).map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let _ = entries;
        // 600 KB of random data compresses to ~600 KB: over the 512 KB cap but under the 1 MB file size
        let r = ok(&entries_noisy);
        assert!(r.is_err(), "{:?}", r.map(|m| m.files.len()));
    }

    #[test]
    fn engine_tag_is_validated() {
        let mut m = meta();
        m.format = 2;
        m.engine = Some("B!".into());
        assert!(m.validate().is_err());
        m.engine = Some("".into());
        assert!(m.validate().is_err());
        m.engine = Some("b".into());
        assert!(m.validate().is_ok());
    }

    #[test]
    fn zip_bomb_is_refused() {
        let good = meta().to_json(1);
        let big = vec![0u8; 5 << 20]; // compresses to a few KB
        let z = zip_with(&[("module.json", good.as_bytes()), ("ingame.bps", &big)]);
        assert!(z.len() < MAX_SIZE);
        assert!(Module::read(&z).unwrap_err().contains("too large"));
        assert!(Module::read(&vec![0u8; MAX_SIZE + 1]).is_err());
    }
}
