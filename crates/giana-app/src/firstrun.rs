//! First start: the game contains none of the original data, so it asks for the player's own disk image (.g64 or .d64),
//! checks it and extracts the data into the data folder (see `giana_core::data`). This module is the logic and the
//! page; the window loop that drives it is in `main.rs`.

use crate::menu::{clip, safe_text};
use crate::ui::{self, Hint, ItemState, Panel};
use giana_core::data;
use giana_extract::{CheckError, Identity};
use giana_render::framebuffer::Framebuffer;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// Largest file taken for a disk image (a G64 of this game is about 300 KB, a D64 at most 197376 bytes).
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
    // a path dragged into a terminal: "My\ Games/x.g64"
    if cfg!(not(windows)) && s.contains("\\ ") {
        s = s.replace("\\ ", " ");
    }
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

/// Why an import failed, in plain words: what is wrong, then what to do about it. `detail` is only for the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError {
    pub problem: String,
    pub fix: String,
    pub detail: String,
}

impl ImportError {
    fn new(problem: &str, fix: &str) -> ImportError {
        ImportError { problem: problem.into(), fix: fix.into(), detail: String::new() }
    }

    fn detail(mut self, detail: impl Into<String>) -> ImportError {
        self.detail = detail.into();
        self
    }

    /// For the terminal: one plain sentence group.
    pub fn plain(&self) -> String {
        let mut t = format!("{} {}", self.problem, self.fix);
        if !self.detail.is_empty() {
            t.push(' ');
            t.push_str(&self.detail);
        }
        t
    }

    /// For the first-start page: the problem, an empty line, then the fix, in capitals and wrapped.
    pub fn screen(&self) -> Vec<String> {
        let mut out = wrap(&safe_text(&self.problem.to_uppercase()), LINE);
        out.push(String::new());
        out.extend(wrap(&safe_text(&self.fix.to_uppercase()), LINE));
        out
    }
}

/// Checks the disk image at `path`, extracts everything and writes the data folder `dest`. The error is the message
/// for the player, as lines of at most 34 characters (see [`ImportError::screen`]).
pub fn import_file(path: &Path, dest: &Path) -> Result<(), Vec<String>> {
    try_import_file(path, dest).map_err(|e| e.screen())
}

/// Same for the content of a file already in memory (`ext` is only used for the wording of one message).
pub fn import_bytes(bytes: &[u8], dest: &Path, ext: &str) -> Result<(), Vec<String>> {
    try_import_bytes(bytes, dest, ext).map_err(|e| e.screen())
}

/// [`import_file`] with the error in plain words (for the terminal as well).
pub fn try_import_file(path: &Path, dest: &Path) -> Result<(), ImportError> {
    let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ImportError::new("Could not find that file.", "Check the path, or drop the disk image on this window.")
            .detail(format!("({})", path.display())),
        std::io::ErrorKind::PermissionDenied => ImportError::new("The game may not open that file.", "Copy it to your Downloads folder and choose it there.")
            .detail(format!("({})", path.display())),
        _ => ImportError::new("Could not open that file.", "Choose it again, or copy it to your Downloads folder.").detail(format!("({e})")),
    })?;
    if meta.is_dir() {
        return Err(ImportError::new("That is a folder, not a file.", "Choose the .g64 or .d64 file inside it."));
    }
    if meta.len() > MAX_IMAGE {
        return Err(ImportError::new("That file is too big.", "Choose the .g64 or .d64 file."));
    }
    let bytes = std::fs::read(path).map_err(|e| ImportError::new("Could not read that file.", "Copy it to your Downloads folder and choose it there.").detail(format!("({e})")))?;
    try_import_bytes(&bytes, dest, path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

/// [`import_bytes`] with the error in plain words.
pub fn try_import_bytes(bytes: &[u8], dest: &Path, ext: &str) -> Result<(), ImportError> {
    // the content check (SHA-256 of the game's files on the disk) comes first: an unknown disk is never unpacked
    if let Err(e) = giana_extract::check_content(bytes) {
        eprintln!("Disk image not accepted: {e}");
        return Err(match &e {
            CheckError::NotADisk(_) if ext.eq_ignore_ascii_case("zip") || bytes.starts_with(b"PK\x03\x04") => {
                ImportError::new("That is a ZIP archive.", "Unpack it first, then choose the .g64 or .d64 file inside.")
            }
            CheckError::NotADisk(_) => ImportError::new("That is not a disk image.", "Choose a file that ends in .g64 or .d64.").detail(format!("({e})")),
            CheckError::NoGameFiles(_) => ImportError::new(
                "This disk image does not hold The Great Giana Sisters.",
                "Choose the .g64 or .d64 of the game disk.",
            )
            .detail(format!("({e})")),
            CheckError::Modified { .. } => ImportError::new(
                "This is a modified or cracked version.",
                "Please use a dump of the original disk.",
            )
            .detail(format!("({e})")),
            CheckError::Unreadable(_) => ImportError::new("The disk image could not be read.", "The file may be damaged. Try another copy.").detail(format!("({e})")),
        });
    }
    let files = giana_extract::extract_all(bytes).map_err(|e| {
        eprintln!("Extraction failed: {e}");
        ImportError::new("The disk image could not be read.", "The file may be damaged. Try another copy.").detail(format!("({e})"))
    })?;
    giana_extract::write_data_dir(&files, dest).map_err(|e| {
        eprintln!("Could not write the data folder: {e}");
        ImportError::new("Could not save the game data.", "Check free space and permissions.")
            .detail(format!("(folder {}: {})", dest.display(), e.rsplit(": ").next().unwrap_or("")))
    })?;
    let problems = giana_extract::verify_dir(dest);
    if !problems.is_empty() {
        return Err(ImportError::new("The saved game data is not valid.", "Delete the data folder and try again.").detail(format!("({})", problems.join(", "))));
    }
    Ok(())
}

/// What a search for the player's disk image found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// a disk image with the unmodified game files
    Original(PathBuf),
    /// no original, but a disk with the game's files in a modified (cracked, trained) version
    Modified(PathBuf),
    Nothing,
}

/// Looks for a `.g64` or `.d64` of the original disk in the usual places: Downloads, Documents, Desktop, the home
/// folder, the folder of the program and the current folder (two levels deep, except in the home folder itself).
pub fn search() -> Found {
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
    find_disk(&dirs)
}

/// The first disk image of the original in the folders (folder, how many levels below it to look as well). Files
/// are identified by the content of the game's files, never by running anything. When there is no original, the
/// first modified version that was seen is reported.
pub fn find_disk(dirs: &[(PathBuf, usize)]) -> Found {
    let mut visited = 0usize;
    let mut modified = None;
    for (dir, depth) in dirs {
        if let Some(p) = scan(dir, *depth, &mut visited, &mut modified) {
            return Found::Original(p);
        }
    }
    modified.map_or(Found::Nothing, Found::Modified)
}

/// Whether a file of this extension and size can be a disk image (so that nothing else is read at all).
fn plausible_image(ext: &str, len: u64) -> bool {
    if ext.eq_ignore_ascii_case("g64") {
        (12..=MAX_IMAGE).contains(&len)
    } else if ext.eq_ignore_ascii_case("d64") {
        usize::try_from(len).is_ok_and(|n| giana_extract::disk::D64_SIZE_RANGE.contains(&n))
    } else {
        false
    }
}

fn scan(dir: &Path, depth: usize, visited: &mut usize, modified: &mut Option<PathBuf>) -> Option<PathBuf> {
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
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| plausible_image(e, meta.len())) {
            if let Ok(bytes) = std::fs::read(&path) {
                match giana_extract::identify(&bytes) {
                    Identity::Original => return Some(path),
                    Identity::Modified => {
                        modified.get_or_insert(path);
                    }
                    Identity::Other => {}
                }
            }
        }
    }
    subdirs.sort();
    subdirs.into_iter().find_map(|d| scan(&d, depth - 1, visited, modified))
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
    /// Open the file dialog (the caller does it on the main thread and reports back with `picked`).
    ChooseFile,
}

/// The items of the page, top to bottom: choose file, the path field, the search, quit.
const ITEMS: usize = 4;
const CHOOSE: usize = 0;
const FIELD: usize = 1;
const SEARCH: usize = 2;

/// Shown when the file dialog returned without a file (cancelled, or it could not be shown at all).
const NO_FILE: &str = "NO FILE CHOSEN.";
const NO_FILE_FIX: &str = "IF NO WINDOW OPENED, DROP THE FILE ON THIS WINDOW OR TYPE ITS PATH.";
const NO_PATH: &str = "NO PATH TYPED YET.";
const NO_PATH_FIX: &str = "TYPE OR PASTE IT, OR DROP THE FILE ON THIS WINDOW.";
const NOT_FOUND: &str = "NO DISK IMAGE FOUND.";
const NOT_FOUND_FIX: &str = "USE CHOOSE FILE, OR DROP THE FILE ON THIS WINDOW.";
const MODIFIED_FIX: &str = "PLEASE USE AN ORIGINAL DUMP.";

/// A failure message for the page: the problem, an empty line, what to do.
fn failure(problem: &str, fix: &str) -> Status {
    let mut lines = wrap(problem, LINE);
    lines.push(String::new());
    lines.extend(wrap(fix, LINE));
    Status::Failed(lines)
}

/// The search found only a modified version: says so with the file name (cut to one line).
fn modified_found(file_name: &str) -> Status {
    let name = clip(&safe_text(&file_name.to_uppercase()), LINE);
    failure(&format!("FOUND A MODIFIED VERSION: {name}"), MODIFIED_FIX)
}

/// What the file dialog asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// the player's own disk image (first start)
    DiskImage,
    /// a ROM module (`ADD ROM MODULE...`)
    RomModule,
}

/// Title and file filter of the dialog: the files of the kind first, then all files.
fn dialog_texts(kind: FileKind) -> (&'static str, &'static str, &'static [&'static str]) {
    match kind {
        FileKind::DiskImage => ("Choose your The Great Giana Sisters disk image", "C64 disk image (.g64, .d64)", &["g64", "G64", "d64", "D64"]),
        FileKind::RomModule => ("Choose a ROM module", "ROM module (.grom)", &["grom", "GROM"]),
    }
}

/// Opens the system file dialog (call on the main thread; it blocks until it is closed). `None`: cancelled, or the
/// dialog could not be shown (on Linux it needs the desktop portal or zenity, and rfd cannot tell the two apart).
pub fn choose_file(kind: FileKind) -> Option<PathBuf> {
    let start = home_dir().map(|h| h.join("Downloads")).filter(|d| d.is_dir()).or_else(home_dir);
    let (title, filter, extensions) = dialog_texts(kind);
    let mut dialog = rfd::FileDialog::new().set_title(title).add_filter(filter, extensions).add_filter("All files", &["*"]);
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    // a backend problem must not take the game down
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dialog.pick_file())).ok().flatten()
}

pub struct Setup {
    pub path: String,
    pub cursor: usize,
    pub status: Status,
    dest: PathBuf,
    job: Option<Receiver<Result<(), Vec<String>>>>,
}

impl Setup {
    pub fn new(dest: PathBuf) -> Setup {
        Setup { path: String::new(), cursor: CHOOSE, status: Status::Idle, dest, job: None }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    pub fn type_text(&mut self, text: &str) {
        if self.cursor == FIELD && !self.busy() {
            self.path.extend(text.chars().filter(|c| !c.is_control()));
            self.status = Status::Idle;
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor == FIELD && !self.busy() {
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
            CHOOSE => return Action::ChooseFile,
            FIELD if self.path.trim().is_empty() => self.status = failure(NO_PATH, NO_PATH_FIX),
            FIELD => {
                let p = clean_path(&self.path);
                self.start_import(p);
            }
            SEARCH => self.start_search(),
            _ => return Action::Quit,
        }
        Action::None
    }

    /// The result of the file dialog: a file starts the import, nothing explains what to do instead.
    pub fn picked(&mut self, file: Option<PathBuf>) {
        match file {
            Some(p) if !self.busy() => {
                self.path = p.display().to_string();
                self.start_import(p);
            }
            Some(_) => {}
            None => self.status = failure(NO_FILE, NO_FILE_FIX),
        }
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
        self.status = Status::Working("CHECKING THE DISK IMAGE - A MOMENT");
    }

    fn start_search(&mut self) {
        let dest = self.dest.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = match search() {
                Found::Original(p) => {
                    println!("Found a disk image: {}", p.display());
                    import_file(&p, &dest)
                }
                Found::Modified(p) => {
                    println!("Found a modified version of the game, not usable: {}", p.display());
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let Status::Failed(lines) = modified_found(&name) else { unreachable!() };
                    Err(lines)
                }
                Found::Nothing => {
                    let Status::Failed(lines) = failure(NOT_FOUND, NOT_FOUND_FIX) else { unreachable!() };
                    Err(lines)
                }
            };
            let _ = tx.send(r);
        });
        self.job = Some(rx);
        self.status = Status::Working("LOOKING FOR YOUR DISK IMAGE");
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
                    self.status = failure("SOMETHING WENT WRONG WHILE UNPACKING.", "START THE GAME AND TRY AGAIN.");
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
    let mut h = vec![Hint::new("UP/DN", "MOVE"), Hint::new("ENTER", "SELECT"), Hint::new("ESC", "QUIT")];
    if setup.cursor == FIELD {
        h.push(Hint::new(if cfg!(target_os = "macos") { "CMD+V" } else { "CTRL+V" }, "PASTE"));
    }
    h
}

/// Instruction lines of the page (each at most `LINE` characters).
const PAGE_TEXT: [&str; 6] = [
    "THE GREAT GIANA SISTERS NEEDS",
    "YOUR ORIGINAL DISK (.G64 OR .D64)",
    "OR DROP IT HERE OR TYPE ITS PATH:",
    "ANY DUMP WORKS IF UNMODIFIED.",
    "IT IS NOT PART OF THIS DOWNLOAD.",
    "THIS IS DONE ONLY ONCE.",
];

/// Draws the page. `tick` counts frames (for the dots of the progress message).
pub fn draw(fb: &mut Framebuffer, setup: &Setup, tick: u32, nudge: usize) {
    const W: usize = 38;
    ui::backdrop_black(fb);
    let p = Panel::centered(W, 25, true);
    p.draw(fb, Some("FIRST START"));
    p.message_colored(fb, 3, PAGE_TEXT[0], ui::color::TEXT);
    p.message_colored(fb, 5, PAGE_TEXT[1], ui::color::TEXT);
    let state = |i: usize| if setup.cursor == i && i != FIELD { ItemState::Selected } else { ItemState::Normal };
    p.item_centered(fb, p.row_y(7), "CHOOSE FILE...", state(CHOOSE), nudge);
    p.message_colored(fb, 10, PAGE_TEXT[2], ui::color::DIM);
    let shown: String = {
        let n = setup.path.chars().count();
        setup.path.chars().skip(n.saturating_sub(LINE - 1)).collect()
    };
    p.field(fb, p.row_y(12), (W - LINE) / 2, LINE, &safe_text(&shown), setup.cursor == FIELD && !setup.busy());
    p.item_centered(fb, p.row_y(14), "SEARCH THIS COMPUTER", state(SEARCH), nudge);
    p.item_centered(fb, p.row_y(16), "QUIT", state(ITEMS - 1), nudge);
    p.separator(fb, 17);
    match &setup.status {
        Status::Idle => {
            p.message_colored(fb, 19, PAGE_TEXT[3], ui::color::TEXT);
            p.message_colored(fb, 21, PAGE_TEXT[4], ui::color::DIM);
            p.message_colored(fb, 23, PAGE_TEXT[5], ui::color::DIM);
        }
        Status::Working(what) => {
            let dots = ".".repeat((tick / 12 % 4) as usize);
            for (i, l) in wrap(what, LINE).iter().enumerate() {
                p.message_colored(fb, 19 + i, l, ui::color::YELLOW);
            }
            p.message_colored(fb, 21, &format!("{dots:<3}"), ui::color::YELLOW);
        }
        Status::Failed(lines) => {
            // the problem (up to the empty line) in yellow, what to do about it in white
            // (one text line takes two rows; the empty line only switches the colour; room for three lines)
            let mut fix = false;
            let mut row = 19;
            for l in lines {
                if l.is_empty() {
                    fix = true;
                    continue;
                }
                if row > 23 {
                    break;
                }
                p.message_colored(fb, row, &clip(l, LINE), if fix { ui::color::TEXT } else { ui::color::YELLOW });
                row += 2;
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
    fn the_dialog_offers_the_files_of_its_kind() {
        let (_, filter, ext) = dialog_texts(FileKind::RomModule);
        assert!(filter.contains(".grom") && ext.contains(&"grom"));
        let (_, filter, ext) = dialog_texts(FileKind::DiskImage);
        assert!(filter.contains(".g64") && filter.contains(".d64") && ext.contains(&"g64") && ext.contains(&"d64"));
    }

    #[test]
    fn choose_file_is_first_and_selected() {
        let mut s = Setup::new(temp("choose"));
        assert_eq!(s.cursor, 0);
        assert_eq!(s.activate(), Action::ChooseFile);
        // typing goes nowhere until the path field is selected
        s.type_text("abc");
        assert_eq!(s.path, "");
        s.move_cursor(true);
        s.type_text("abc");
        assert_eq!(s.path, "abc");
        s.move_cursor(true);
        s.move_cursor(true);
        assert_eq!(s.activate(), Action::Quit);
        s.move_cursor(true);
        assert_eq!(s.cursor, 0);
        s.move_cursor(false);
        assert_eq!(s.cursor, ITEMS - 1);
    }

    #[test]
    fn a_cancelled_dialog_explains_the_alternatives() {
        let mut s = Setup::new(temp("cancel"));
        s.picked(None);
        let Status::Failed(lines) = &s.status else { panic!() };
        assert!(lines.join(" ").contains("DROP"));
        assert!(lines.iter().filter(|l| !l.is_empty()).count() <= 3);
        assert!(!s.busy());
    }

    #[test]
    fn a_picked_file_is_imported() {
        let dest = temp("picked");
        let mut s = Setup::new(dest.clone());
        s.picked(Some(PathBuf::from("/nonexistent/x.g64")));
        assert!(s.busy());
        wait(&mut s);
        assert!(matches!(s.status, Status::Failed(_)));
        assert!(!dest.exists());
    }

    #[test]
    fn page_texts_fit_and_use_known_glyphs() {
        for t in PAGE_TEXT.iter().chain([&NO_FILE, &NO_FILE_FIX, &NO_PATH, &NO_PATH_FIX, &NOT_FOUND, &NOT_FOUND_FIX, &MODIFIED_FIX]) {
            assert_eq!(safe_text(t), *t, "unsupported characters in {t}");
        }
        for t in PAGE_TEXT {
            assert!(t.chars().count() <= LINE, "{t}");
        }
        for t in [NO_FILE_FIX, NOT_FOUND, NOT_FOUND_FIX, MODIFIED_FIX] {
            for l in wrap(t, LINE) {
                assert!(l.chars().count() <= LINE, "{l}");
            }
        }
    }

    #[test]
    fn paths_are_cleaned() {
        assert_eq!(clean_path("  \"/a/b c.g64\" "), PathBuf::from("/a/b c.g64"));
        assert_eq!(clean_path("file:///home/me/My%20Games/x.g64"), PathBuf::from("/home/me/My Games/x.g64"));
        assert_eq!(clean_path("file:///C:/Games/x.g64"), PathBuf::from("C:/Games/x.g64"));
        #[cfg(not(windows))]
        assert_eq!(clean_path("/a/My\\ Games/x.g64"), PathBuf::from("/a/My Games/x.g64"));
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
        let text = |r: Result<(), ImportError>| r.unwrap_err();
        let not_a_disk = text(try_import_bytes(b"hello", &dest, "txt"));
        assert_eq!(not_a_disk.problem, "That is not a disk image.");
        assert!(not_a_disk.fix.contains(".g64 or .d64"));
        let zip = text(try_import_bytes(b"PK\x03\x04 zipped", &dest, "g64"));
        assert!(zip.problem.contains("ZIP"), "{zip:?}");
        // a .d64 name on garbage is not mistaken for anything special
        assert_eq!(text(try_import_bytes(b"hello", &dest, "d64")).problem, not_a_disk.problem);
        let mut fake = b"GCR-1541".to_vec();
        fake.extend_from_slice(&[0; 100]);
        assert_eq!(text(try_import_bytes(&fake, &dest, "g64")).problem, not_a_disk.problem);

        // a disk image without the game
        let other = giana_extract::d64::build_d64(&[("HELLO", &[1, 8, 0])]);
        let e = text(try_import_bytes(&other, &dest, "d64"));
        assert!(e.problem.contains("does not hold The Great Giana Sisters"), "{e:?}");
        // the game's files, modified
        let cracked = giana_extract::d64::build_d64(&[("1", &[0x20, 0x08, 1, 2, 3]), ("2", &[0, 4, 5, 6])]);
        let e = text(try_import_bytes(&cracked, &dest, "d64"));
        assert_eq!(e.problem, "This is a modified or cracked version.");
        assert_eq!(e.fix, "Please use a dump of the original disk.");
        assert!(e.detail.contains(giana_extract::GAME_FILE1_SHA256), "{e:?}");
        assert!(e.screen().join(" ").contains("MODIFIED OR CRACKED"));

        assert!(!dest.exists());
        assert!(import_file(Path::new("/nonexistent/x.g64"), &dest).is_err());
    }

    /// The original disk as a D64 made from the G64 (sectors of tracks 1-35).
    fn original_as_d64() -> Option<Vec<u8>> {
        let g64 = std::fs::read(repo(G64)).ok()?;
        let disk = giana_extract::g64::Disk::parse(&g64).unwrap();
        let mut out = Vec::new();
        for t in 1..=35u8 {
            for s in 0..giana_extract::g64::sectors_per_track(t) {
                out.extend_from_slice(disk.sector(t, s).unwrap_or(&[0; 256]));
            }
        }
        Some(out)
    }

    #[test]
    fn imports_a_d64_of_the_original() {
        let Some(d64) = original_as_d64() else { return };
        let dest = temp("d64");
        try_import_bytes(&d64, &dest, "d64").unwrap();
        assert!(giana_extract::verify_dir(&dest).is_empty());
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn imports_another_dump_of_the_original() {
        let Some(path) = std::env::var_os("GIANA_ALT_G64") else { return };
        let dest = temp("alt");
        try_import_file(Path::new(&path), &dest).unwrap();
        assert!(giana_extract::verify_dir(&dest).is_empty());
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn reports_an_unwritable_data_folder() {
        let Ok(g64) = std::fs::read(repo(G64)) else { return };
        let blocker = temp("blocker");
        std::fs::write(&blocker, b"a file, not a folder").unwrap();
        let err = import_bytes(&g64, &blocker.join("data"), "g64").unwrap_err().join(" ");
        assert!(err.contains("COULD NOT SAVE"), "{err}");
        std::fs::remove_file(&blocker).unwrap();
    }

    #[test]
    fn typed_path_and_drop_extract_the_data() {
        if !repo(G64).exists() {
            return;
        }
        let dest = temp("typed");
        let mut s = Setup::new(dest.clone());
        s.move_cursor(true);
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
        assert_eq!(find_disk(&[(root.clone(), 2)]), Found::Nothing);
        std::fs::copy(repo(G64), root.join("Games/sub/Giana.G64")).unwrap();
        assert_eq!(find_disk(&[(root.clone(), 2)]), Found::Original(root.join("Games/sub/Giana.G64")));
        assert_eq!(find_disk(&[(root.clone(), 1)]), Found::Nothing);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_finds_a_d64_by_its_content() {
        let Some(d64) = original_as_d64() else { return };
        let root = temp("search-d64");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/Giana (other dump).d64"), &d64).unwrap();
        // the same bytes under another extension, or a D64 of the wrong size, are not considered
        std::fs::write(root.join("a/notes.bin"), &d64).unwrap();
        std::fs::write(root.join("a/short.d64"), &d64[..1000]).unwrap();
        assert_eq!(find_disk(&[(root.clone(), 1)]), Found::Original(root.join("a/Giana (other dump).d64")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn search_reports_a_modified_version_only_when_there_is_no_original() {
        let root = temp("search-mod");
        std::fs::create_dir_all(&root).unwrap();
        let cracked = giana_extract::d64::build_d64(&[("1", &[0x20, 0x08, 1, 2, 3]), ("2", &[0, 4, 5, 6])]);
        let unrelated = giana_extract::d64::build_d64(&[("HELLO", &[1, 8, 0])]);
        std::fs::write(root.join("a_unrelated.d64"), &unrelated).unwrap();
        std::fs::write(root.join("b_cracked.d64"), &cracked).unwrap();
        assert_eq!(find_disk(&[(root.clone(), 0)]), Found::Modified(root.join("b_cracked.d64")));
        if let Some(d64) = original_as_d64() {
            std::fs::write(root.join("c_original.d64"), &d64).unwrap();
            assert_eq!(find_disk(&[(root.clone(), 0)]), Found::Original(root.join("c_original.d64")));
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_modified_message_names_the_file_and_fits_the_page() {
        let Status::Failed(lines) = modified_found("A_VERY_LONG_FILE_NAME_OF_A_CRACKED_DISK_(1987).d64") else { panic!() };
        assert!(lines.join(" ").contains("FOUND A MODIFIED VERSION"));
        assert!(lines.join(" ").contains("PLEASE USE AN ORIGINAL DUMP"));
        assert!(lines.iter().filter(|l| !l.is_empty()).count() <= 3, "{lines:?}");
        assert!(lines.iter().all(|l| l.chars().count() <= LINE));
    }

    #[test]
    fn only_plausible_files_are_read() {
        assert!(plausible_image("G64", 300_000) && !plausible_image("g64", MAX_IMAGE + 1));
        assert!(plausible_image("d64", 174_848) && plausible_image("D64", 175_531) && plausible_image("d64", 197_376));
        assert!(!plausible_image("d64", 174_847) && !plausible_image("d64", 197_377) && !plausible_image("zip", 174_848));
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
