//! Saving files so that a crash or a full disk cannot leave half a file behind.

use std::path::Path;

/// Write to a temporary file next to `path`, then rename it over `path`.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    write_atomic_bytes(path, contents.as_bytes())
}

/// The same for binary contents.
pub fn write_atomic_bytes(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let result = std::fs::write(&tmp, contents).and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_replaces_and_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("giana-fsutil-{}", std::process::id()));
        let file = dir.join("sub").join("a.txt");
        write_atomic(&file, "one").unwrap();
        write_atomic(&file, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failure_is_reported() {
        // the target is a directory: the rename cannot work
        let dir = std::env::temp_dir().join(format!("giana-fsutil-fail-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("x")).unwrap();
        assert!(write_atomic(&dir.join("x"), "data").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
