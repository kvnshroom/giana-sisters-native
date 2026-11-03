//! First start: the game contains none of the original data, so it asks for the player's own disk image (.g64),
//! checks it and extracts the data into the data folder (see `giana_core::data`). This module is the logic and the
//! page; the window loop that drives it is in `main.rs`.

use crate::menu::{clip, safe_text};
use crate::ui::{self, Hint, ItemState, Panel};
use giana_core::data;
use giana_extract::{sha256::sha256_hex, KNOWN_VERSIONS};
use giana_render::framebuffer::Framebuffer;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// Largest file taken for a disk image (a G64 of this game is about 300 KB).
const MAX_IMAGE: u64 = 4 << 20;
/// Characters per line on the page.
const LINE: usize = 34;

/// Where valid game data is, if anywhere. With an explicit folder only that folder counts.
pub fn locate(cli: Option<&Path>) -> Option<PathBuf> {
    data::candidate_dirs(cli)
        .into_iter()
        .filter(|d| cli.is_none() || Some(d.as_path()) == cli)
        .find(|d| giana_extract::verify_dir(d).is_empty())
}

/// Where extracted data goes: the folder given on the command line, else the user's data folder.
pub fn destination(cli: Option<&Path>) -> Option<PathBuf> {
    cli.map(Path::to_path_buf).or_else(data::user_data_dir)
}

/// Loads the data of `dir` and makes it the data of this process.
pub fn load(dir: &Path) -> Result<(), String> {
    data::install(data::GameData::load_dir(dir)?);
    Ok(())
}

/// Splits a text into lines of at most `width` characters (long words are cut).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        while word.chars().count() > width {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            out.push(word.chars().take(width).collect());
            word = word.chars().skip(width).collect();
        }
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// A path as typed, pasted or dropped: surrounding quotes, `file://` addresses (with %20 and so on) and `~/` are resolved.
pub fn clean_path(text: &str) -> PathBuf {
    let mut s = text.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
    if let Some(rest) = s.strip_prefix("file://") {
        s = rest.trim_start_matches("localhost").to_string();
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if bytes[i] == b'%' && i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() {
                out.push((hex(bytes[i + 1]).unwrap() * 16 + hex(bytes[i + 2]).unwrap()) as u8);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        s = String::from_utf8_lossy(&out).into_owned();
        // Windows: file:///C:/dir/file -> C:/dir/file
        let b = s.as_bytes();
        if b.len() > 3 && b[0] == b'/' && b[2] == b':' {
            s.remove(0);
        }
    }
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

fn home_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("HOME").or_else(|| var("USERPROFILE"))
}

/// Checks the disk image at `path`, extracts everything and writes the data folder `dest`. The error is the message
/// for the player, as lines of at most 34 characters.
pub fn import_file(path: &Path, dest: &Path) -> Result<(), Vec<String>> {
    let fail = |text: &str| Err(wrap(&safe_text(&text.to_uppercase()), LINE));
    let meta = std::fs::metadata(path).map_err(|e| wrap(&safe_text(&format!("COULD NOT FIND OR OPEN THE FILE ({})", short_io(&e))), LINE))?;
    if meta.is_dir() {
        return fail("THAT IS A FOLDER. GIVE THE .G64 FILE ITSELF.");
    }
    if meta.len() > MAX_IMAGE {
        return fail("THIS FILE IS MUCH TOO BIG FOR A DISK IMAGE OF THIS GAME (NOT A G64?)");
    }
    let bytes = std::fs::read(path).map_err(|e| wrap(&safe_text(&format!("COULD NOT READ THE FILE ({})", short_io(&e))), LINE))?;
    import_bytes(&bytes, dest, path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

/// Same for the content of a file already in memory (`ext` is only used for the wording of one message).
pub fn import_bytes(bytes: &[u8], dest: &Path, ext: &str) -> Result<(), Vec<String>> {
    let lines = |t: &str| wrap(&safe_text(&t.to_uppercase()), LINE);
    if bytes.len() < 12 || &bytes[..8] != b"GCR-1541" {
        let mut m = lines("THIS IS NOT A G64 DISK IMAGE.");
        if ext.eq_ignore_ascii_case("d64") {
            m.extend(lines("A D64 FILE CANNOT BE USED. YOU NEED THE G64 VERSION."));
        } else {
            m.extend(lines("THE FILE MUST END IN .G64."));
        }
        return Err(m);
    }
    let hash = sha256_hex(bytes);
    if !KNOWN_VERSIONS.iter().any(|v| v.g64_sha256 == hash) {
        eprintln!("Unknown disk image version, SHA-256 {hash}");
        let mut m = lines("UNKNOWN VERSION OF THE DISK IMAGE. ONLY THIS ONE IS SUPPORTED (SHA-256):");
        for v in KNOWN_VERSIONS {
            m.push(v.g64_sha256[..32].to_uppercase());
            m.push(v.g64_sha256[32..].to_uppercase());
        }
        return Err(m);
    }
    let files = giana_extract::extract_all(bytes).map_err(|e| {
        eprintln!("Extraction failed: {e}");
        lines(&format!("THE DISK IMAGE COULD NOT BE READ: {e}"))
    })?;
    giana_extract::write_data_dir(&files, dest).map_err(|e| {
        eprintln!("Could not write the data folder: {e}");
        let mut m = lines("COULD NOT WRITE THE GAME DATA TO");
        let shown = dest.display().to_string();
        let tail: String = shown.chars().rev().take(LINE * 2).collect::<Vec<_>>().into_iter().rev().collect();
        m.extend(wrap(&safe_text(&tail.to_uppercase()), LINE));
        m.extend(lines(&format!("({})", e.rsplit(": ").next().unwrap_or(""))));
        m
    })?;
    let problems = giana_extract::verify_dir(dest);
    if !problems.is_empty() {
        return Err(lines(&format!("THE WRITTEN DATA IS NOT VALID: {}", problems.join(", "))));
    }
    Ok(())
}

fn short_io(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => "NO SUCH FILE".into(),
        PermissionDenied => "NO PERMISSION".into(),
        _ => e.to_string(),
    }
}

/// Looks for a `.g64` of a known version in the usual places: Downloads, Documents, Desktop, the home folder, the
/// folder of the program and the current folder (two levels deep, except in the home folder itself).
pub fn search() -> Option<PathBuf> {
    let mut dirs: Vec<(PathBuf, usize)> = Vec::new();
    if let Some(home) = home_dir() {
        for d in ["Downloads", "Documents", "Desktop"] {
            dirs.push((home.join(d), 2));
        }
        dirs.push((home, 0));
    }
    if let Some(exe) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push((exe, 1));
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push((cwd, 1));
    }
    find_g64(&dirs)
}

/// The first `.g64` of a known version in the folders (folder, how many levels below it to look as well).
pub fn find_g64(dirs: &[(PathBuf, usize)]) -> Option<PathBuf> {
    let mut visited = 0usize;
    for (dir, depth) in dirs {
        if let Some(p) = scan(dir, *depth, &mut visited) {
            return Some(p);
        }
    }
    None
}

fn scan(dir: &Path, depth: usize, visited: &mut usize) -> Option<PathBuf> {
    let mut subdirs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        *visited += 1;
        if *visited > 20_000 {
            return None;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 && !entry.file_name().to_string_lossy().starts_with('.') {
                subdirs.push(path);
            }
        } else if meta.len() <= MAX_IMAGE
            && path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("g64"))
        {
            if let Ok(bytes) = std::fs::read(&path) {
                let h = sha256_hex(&bytes);
                if KNOWN_VERSIONS.iter().any(|v| v.g64_sha256 == h) {
                    return Some(path);
                }
            }
        }
    }
    subdirs.sort();
    subdirs.into_iter().find_map(|d| scan(&d, depth - 1, visited))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Working(&'static str),
    Failed(Vec<String>),
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
}

/// The items of the page, top to bottom: the path field, the search, quit.
const ITEMS: usize = 3;

pub struct Setup {
    pub path: String,
    pub cursor: usize,
    pub status: Status,
    dest: PathBuf,
    job: Option<Receiver<Result<(), Vec<String>>>>,
}

impl Setup {
    pub fn new(dest: PathBuf) -> Setup {
        Setup { path: String::new(), cursor: 0, status: Status::Idle, dest, job: None }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn type_text(&mut self, text: &str) {
        if self.cursor == 0 && !self.busy() {
            self.path.extend(text.chars().filter(|c| !c.is_control()));
            self.status = Status::Idle;
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 && !self.busy() {
            self.path.pop();
        }
    }

    pub fn move_cursor(&mut self, down: bool) {
        if !self.busy() {
            self.cursor = if down { (self.cursor + 1) % ITEMS } else { (self.cursor + ITEMS - 1) % ITEMS };
        }
    }

    /// Enter on the selected item.
    pub fn activate(&mut self) -> Action {
        if self.busy() {
            return Action::None;
        }
        match self.cursor {
            0 if self.path.trim().is_empty() => self.status = Status::Failed(wrap("TYPE OR PASTE THE PATH FIRST, OR DROP THE FILE ON THE WINDOW", LINE)),
            0 => {
                let p = clean_path(&self.path);
                self.start_import(p);
            }
            1 => self.start_search(),
            _ => return Action::Quit,
        }
        Action::None
    }

    /// A file dropped on the window.
    pub fn drop_file(&mut self, name: &str) {
        if !self.busy() {
            self.path = name.to_string();
            self.start_import(clean_path(name));
        }
    }

    fn start_import(&mut self, path: PathBuf) {
        let dest = self.dest.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(import_file(&path, &dest));
        });
        self.job = Some(rx);
        self.status = Status::Working("CHECKING AND UNPACKING THE DISK IMAGE");
    }

    fn start_search(&mut self) {
        let dest = self.dest.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = match search() {
                Some(p) => {
                    println!("Found a disk image: {}", p.display());
                    import_file(&p, &dest)
                }
                None => Err(wrap(
                    "NO KNOWN G64 FOUND IN DOWNLOADS, DOCUMENTS, DESKTOP, YOUR HOME FOLDER OR NEXT TO THE GAME. DROP THE FILE ON THE WINDOW OR TYPE ITS PATH.",
                    LINE,
                )),
            };
            let _ = tx.send(r);
        });
        self.job = Some(rx);
        self.status = Status::Working("SEARCHING FOR YOUR DISK IMAGE");
    }

    /// Call every frame. True once the data is in place.
    pub fn poll(&mut self) -> bool {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    self.job = None;
                    self.status = Status::Done;
                }
                Ok(Err(lines)) => {
                    self.job = None;
                    self.status = Status::Failed(lines);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = Status::Failed(wrap("SOMETHING WENT WRONG WHILE UNPACKING", LINE));
                }
            }
        }
        self.status == Status::Done
    }
}

pub fn footer_hints(setup: &Setup) -> Vec<Hint> {
    if setup.busy() {
        return vec![Hint::new("PLEASE", "WAIT")];
    }
    let mut h = vec![Hint::new("UP/DN", "MOVE"), Hint::new("ENTER", "SELECT")];
    if setup.cursor == 0 {
        h.push(Hint::new("CTRL+V", "PASTE"));
    }
    h
}

/// Draws the page. `tick` counts frames (for the dots of the progress message).
pub fn draw(fb: &mut Framebuffer, setup: &Setup, tick: u32, nudge: usize) {
    const W: usize = 38;
    ui::backdrop_black(fb);
    let p = Panel::centered(W, 25, true);
    p.draw(fb, Some("FIRST START"));
    p.message_colored(fb, 3, "THE GREAT GIANA SISTERS NEEDS", ui::color::TEXT);
    p.message_colored(fb, 5, "YOUR ORIGINAL DISK IMAGE", ui::color::TEXT);
    p.message_colored(fb, 8, "DROP THE .G64 FILE ON THIS WINDOW", ui::color::DIM);
    p.message_colored(fb, 9, "OR TYPE OR PASTE ITS PATH", ui::color::DIM);
    let shown: String = {
        let n = setup.path.chars().count();
        setup.path.chars().skip(n.saturating_sub(LINE - 1)).collect()
    };
    p.field(fb, p.row_y(11), (W - LINE) / 2, LINE, &safe_text(&shown), setup.cursor == 0 && !setup.busy());
    let state = |i: usize| if setup.cursor == i && i != 0 { ItemState::Selected } else { ItemState::Normal };
    p.item_centered(fb, p.row_y(13), "SEARCH THIS COMPUTER", state(1), nudge);
    p.item_centered(fb, p.row_y(15), "QUIT", state(2), nudge);
    p.separator(fb, 17);
    match &setup.status {
        Status::Idle => {
            p.message_colored(fb, 19, "THE DATA STAYS ON YOUR COMPUTER.", ui::color::DIM);
            p.message_colored(fb, 21, "THIS IS DONE ONLY ONCE.", ui::color::DIM);
        }
        Status::Working(what) => {
            let dots = ".".repeat((tick / 12 % 4) as usize);
            for (i, l) in wrap(what, LINE).iter().enumerate() {
                p.message_colored(fb, 19 + i, l, ui::color::YELLOW);
            }
            p.message_colored(fb, 22, &format!("{dots:<3}"), ui::color::YELLOW);
        }
        Status::Failed(lines) => {
            for (i, l) in lines.iter().take(5).enumerate() {
                p.message_colored(fb, 18 + i, &clip(l, LINE), ui::color::YELLOW);
            }
        }
        Status::Done => p.message_colored(fb, 19, "ALL SET - STARTING THE GAME", ui::color::CYAN),
    }
    ui::footer(fb, &footer_hints(setup), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    const G64: &str = "orig/Great_Giana_Sisters_The.g64";

    fn repo(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("giana-firstrun-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn wait(s: &mut Setup) {
        for _ in 0..2000 {
            if s.poll() || matches!(s.status, Status::Failed(_)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("setup did not finish");
    }

    #[test]
    fn paths_are_cleaned() {
        assert_eq!(clean_path("  \"/a/b c.g64\" "), PathBuf::from("/a/b c.g64"));
        assert_eq!(clean_path("file:///home/me/My%20Games/x.g64"), PathBuf::from("/home/me/My Games/x.g64"));
        assert_eq!(clean_path("file:///C:/Games/x.g64"), PathBuf::from("C:/Games/x.g64"));
    }

    #[test]
    fn wrap_keeps_lines_short() {
        for l in wrap("A VERY LONG TEXT WITH SOMEVERYLONGWORDTHATDOESNOTFITINONELINEATALL AND MORE", 20) {
            assert!(l.chars().count() <= 20, "{l}");
        }
    }

    #[test]
    fn rejects_files_in_plain_words() {
        let dest = temp("reject");
        let not_g64 = import_bytes(b"hello", &dest, "txt").unwrap_err().join(" ");
        assert!(not_g64.contains("NOT A G64"), "{not_g64}");
        let d64 = import_bytes(b"hello", &dest, "d64").unwrap_err().join(" ");
        assert!(d64.contains("D64"), "{d64}");
        let mut fake = b"GCR-1541".to_vec();
        fake.extend_from_slice(&[0; 100]);
        let unknown = import_bytes(&fake, &dest, "g64").unwrap_err().join(" ");
        assert!(unknown.contains("UNKNOWN VERSION") && unknown.contains("5CE29CE0"), "{unknown}");
        assert!(!dest.exists());
        assert!(import_file(Path::new("/nonexistent/x.g64"), &dest).is_err());
    }

    #[test]
    fn reports_an_unwritable_data_folder() {
        let Ok(g64) = std::fs::read(repo(G64)) else { return };
        let blocker = temp("blocker");
        std::fs::write(&blocker, b"a file, not a folder").unwrap();
        let err = import_bytes(&g64, &blocker.join("data"), "g64").unwrap_err().join(" ");
        assert!(err.contains("COULD NOT WRITE"), "{err}");
        std::fs::remove_file(&blocker).unwrap();
    }

    #[test]
    fn typed_path_and_drop_extract_the_data() {
        if !repo(G64).exists() {
            return;
        }
        let dest = temp("typed");
        let mut s = Setup::new(dest.clone());
        s.type_text(&format!("\"{}\"", repo(G64).display()));
        s.activate();
        wait(&mut s);
        assert_eq!(s.status, Status::Done);
        assert!(giana_extract::verify_dir(&dest).is_empty());
        std::fs::remove_dir_all(&dest).unwrap();

        let mut s = Setup::new(dest.clone());
        s.drop_file(&repo(G64).display().to_string());
        wait(&mut s);
        assert_eq!(s.status, Status::Done);
        assert!(giana_extract::verify_dir(&dest).is_empty());
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn search_finds_a_known_image_and_skips_others() {
        if !repo(G64).exists() {
            return;
        }
        let root = temp("search");
        std::fs::create_dir_all(root.join("Games/sub")).unwrap();
        std::fs::write(root.join("Games/other.g64"), b"GCR-1541 not the right one").unwrap();
        assert_eq!(find_g64(&[(root.clone(), 2)]), None);
        std::fs::copy(repo(G64), root.join("Games/sub/Giana.G64")).unwrap();
        assert_eq!(find_g64(&[(root.clone(), 2)]), Some(root.join("Games/sub/Giana.G64")));
        assert_eq!(find_g64(&[(root.clone(), 1)]), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn locate_accepts_only_valid_folders() {
        let empty = temp("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(locate(Some(&empty)), None);
        if repo("data/ingame.bin").exists() {
            assert_eq!(locate(Some(&repo("data"))), Some(repo("data")));
        }
        std::fs::remove_dir_all(&empty).unwrap();
    }
}
