//! The `.grom` file: a ZIP with `module.json`, `ingame.bps` and/or `f_2.bps`, and an optional `readme.txt`.

use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

/// Maximum size of a `.grom` file and of each entry once unpacked.
pub const MAX_SIZE: usize = 1 << 20;
pub const MAX_NAME: usize = 24;
pub const MAX_AUTHOR: usize = 24;
pub const MAX_VERSION: usize = 12;
pub const MAX_DESCRIPTION: usize = 200;
pub const MAX_URL: usize = 200;

/// Module metadata (`module.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// Container format, always 1.
    pub format: u32,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub url: Option<String>,
    /// SHA-256 (hex) of the base `ingame.bin`.
    pub base: String,
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
        }
    }

    /// Checks format, non-empty name and the length limits (in characters).
    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err(format!("unsupported module format {}", self.format));
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

    fn to_json(&self) -> String {
        let mut v = json!({
            "format": self.format, "name": self.name, "author": self.author, "version": self.version,
            "description": self.description, "base": self.base,
        });
        if let (Some(url), Some(o)) = (&self.url, v.as_object_mut()) {
            o.insert("url".into(), Value::String(url.clone()));
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
        if format != 1 {
            return Err(format!("unsupported module format {format}"));
        }
        let meta = Meta {
            format: 1,
            name: text("name", true)?,
            author: text("author", false)?,
            version: text("version", false)?,
            description: text("description", false)?,
            url: match o.get("url") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            },
            base: text("base", true)?,
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
        Ok(Module { meta, ingame_patch, title_patch, readme })
    }

    /// Serializes to a `.grom` file (deflate).
    pub fn write(&self) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut entries: Vec<(&str, Vec<u8>)> = vec![("module.json", self.meta.to_json().into_bytes())];
        if let Some(p) = &self.ingame_patch {
            entries.push(("ingame.bps", p.clone()));
        }
        if let Some(p) = &self.title_patch {
            entries.push(("f_2.bps", p.clone()));
        }
        if let Some(r) = &self.readme {
            entries.push(("readme.txt", r.clone().into_bytes()));
        }
        for (name, data) in entries {
            // writing into a Vec cannot fail
            let _ = zip.start_file(name, opts);
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
        let good = meta().to_json();
        let ok = |j: &str| Module::read(&zip_with(&[("module.json", j.as_bytes()), ("ingame.bps", b"x")]));
        assert!(ok(&good).is_ok());
        assert!(ok("not json").is_err());
        assert!(ok(&good.replace("\"format\":1", "\"format\":2")).is_err());
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
        let good = meta().to_json();
        let z = zip_with(&[("module.json", good.as_bytes()), ("f_2.bps", b"p"), ("extra.txt", b"?")]);
        assert!(Module::read(&z).is_ok());
    }

    #[test]
    fn zip_bomb_is_refused() {
        let good = meta().to_json();
        let big = vec![0u8; 5 << 20]; // compresses to a few KB
        let z = zip_with(&[("module.json", good.as_bytes()), ("ingame.bps", &big)]);
        assert!(z.len() < MAX_SIZE);
        assert!(Module::read(&z).unwrap_err().contains("too large"));
        assert!(Module::read(&vec![0u8; MAX_SIZE + 1]).is_err());
    }
}
