//! Finding and installing new versions from the GitHub Releases of the project.
//!
//! The check runs in a background thread and never blocks or breaks the game: offline, rate limited or a missing
//! repository are ignored silently. The install downloads the package for this system and `SHA256SUMS.txt`, refuses
//! anything whose checksum does not match, and only then replaces the running executable. Data and settings are not touched.
//! Where the executable cannot be replaced (a package manager owns it, no write access) the release page is opened instead.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::Duration;

pub const REPO: &str = "kvnshroom/giana-sisters-native";
pub const API_URL: &str = "https://api.github.com/repos/kvnshroom/giana-sisters-native/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/kvnshroom/giana-sisters-native/releases";
/// Downloads are only accepted from the releases of this repository.
const DOWNLOAD_PREFIX: &str = "https://github.com/kvnshroom/giana-sisters-native/releases/download/";
pub const SUMS_NAME: &str = "SHA256SUMS.txt";
/// at most one automatic check per day
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_EXE: u64 = 512 * 1024 * 1024;

// ---------- versions ----------

/// `X.Y.Z` with an optional `-pre` part; a pre-release is older than the release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// false for a pre-release (so that it sorts before the release)
    release: bool,
}

impl Version {
    /// `v1.2.3`, `1.2.3`, `1.2` and `1.2.3-rc1` (build metadata after `+` is ignored).
    pub fn parse(text: &str) -> Option<Version> {
        let t = text.trim();
        let t = t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t);
        let t = t.split('+').next()?;
        let (core, pre) = match t.split_once('-') {
            Some((c, _)) => (c, true),
            None => (t, false),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version { major, minor, patch, release: !pre })
    }

    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is a version")
    }
}

/// The tag is newer than the running version. A tag that is not a version is never newer.
pub fn is_newer(tag: &str, current: &str) -> bool {
    match (Version::parse(tag), Version::parse(current)) {
        (Some(t), Some(c)) => t > c,
        _ => false,
    }
}

/// `v1.2.3` -> `1.2.3`
fn plain_version(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

// ---------- the release ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// without the `v`
    pub version: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The release has the package for `os`/`arch` and the checksum file. The packages of a release are uploaded one
    /// after the other (macOS is often the last), so a release without them is not offered yet.
    pub fn has_package_for(&self, os: &str, arch: &str) -> bool {
        asset_name(&self.version, os, arch).is_some_and(|n| self.asset(&n).is_some()) && self.asset(SUMS_NAME).is_some()
    }

    /// [`Release::has_package_for`] for the system this program runs on.
    pub fn has_package(&self) -> bool {
        self.has_package_for(std::env::consts::OS, std::env::consts::ARCH)
    }
}

/// The answer of `releases/latest`. Drafts and pre-releases are not offered.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    Version::parse(tag)?;
    let assets = v["assets"]
        .as_array()?
        .iter()
        .filter_map(|a| Some(Asset { name: a["name"].as_str()?.to_string(), url: a["browser_download_url"].as_str()?.to_string() }))
        .collect();
    let page = v["html_url"].as_str().filter(|u| u.starts_with("https://github.com/")).unwrap_or(RELEASES_URL).to_string();
    Some(Release { version: plain_version(tag), page, assets })
}

// ---------- assets and checksums ----------

/// The package for a system: `os` and `arch` as in `std::env::consts`. macOS has one universal package for both processors.
pub fn asset_name(version: &str, os: &str, arch: &str) -> Option<String> {
    let suffix = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64.tar.gz",
        ("windows", "x86_64") => "windows-x86_64.zip",
        ("macos", "aarch64" | "x86_64") => "macos-universal.tar.gz",
        _ => return None,
    };
    Some(format!("giana-sisters-native-{version}-{suffix}"))
}

/// The name of the executable inside the package.
pub fn exe_name(os: &str) -> &'static str {
    if os == "windows" {
        "giana-app.exe"
    } else {
        "giana-app"
    }
}

/// The checksum of `file` in a `SHA256SUMS.txt` (`<hex>  <name>` or `<hex> *<name>`).
pub fn parse_sums(text: &str, file: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        let name = name.strip_prefix("./").unwrap_or(name);
        (name == file && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

// ---------- errors ----------

#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// the executable cannot be replaced here (no write access, or a package manager owns it): show the release page
    NotWritable,
    /// the download does not match its checksum, or the checksum is missing: nothing was installed
    Checksum(String),
    Other(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::NotWritable => write!(f, "the program cannot be replaced here"),
            InstallError::Checksum(m) | InstallError::Other(m) => write!(f, "{m}"),
        }
    }
}

// ---------- unpacking ----------

fn read_limited(r: &mut impl Read, size_hint: u64) -> Result<Vec<u8>, InstallError> {
    if size_hint > MAX_EXE {
        return Err(InstallError::Other("the program in the package is too big".into()));
    }
    let mut out = Vec::new();
    r.take(MAX_EXE + 1).read_to_end(&mut out).map_err(|e| InstallError::Other(format!("could not unpack: {e}")))?;
    if out.len() as u64 > MAX_EXE {
        return Err(InstallError::Other("the program in the package is too big".into()));
    }
    Ok(out)
}

/// The executable (`exe`: its file name) out of a `.tar.gz` or `.zip` package. In a macOS package it is the one inside the app bundle.
pub fn extract_exe(archive_name: &str, data: &[u8], exe: &str) -> Result<Vec<u8>, InstallError> {
    let missing = || InstallError::Other(format!("{exe} is not in the package"));
    if archive_name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| InstallError::Other(format!("bad package: {e}")))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| InstallError::Other(format!("bad package: {e}")))?;
            let is_exe = f.is_file() && f.enclosed_name().is_some_and(|p| p.file_name().is_some_and(|n| n == exe));
            if is_exe {
                let size = f.size();
                return read_limited(&mut f, size);
            }
        }
        Err(missing())
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(data));
        let entries = tar.entries().map_err(|e| InstallError::Other(format!("bad package: {e}")))?;
        for entry in entries {
            let mut entry = entry.map_err(|e| InstallError::Other(format!("bad package: {e}")))?;
            let is_exe = entry.header().entry_type().is_file() && entry.path().ok().is_some_and(|p| p.file_name().is_some_and(|n| n == exe));
            if is_exe {
                let size = entry.size();
                return read_limited(&mut entry, size);
            }
        }
        Err(missing())
    }
}

// ---------- replacing the running program ----------

/// `.../X.app/Contents/MacOS/name` -> `.../X.app`
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app";
    is_bundle.then(|| app.to_path_buf())
}

/// Places that a package manager or a sandbox owns: never overwritten from inside the game.
pub fn is_managed_path(exe: &Path) -> bool {
    ["/usr/bin", "/usr/lib", "/usr/libexec", "/usr/sbin", "/bin", "/sbin", "/nix/store", "/snap", "/app"].iter().any(|p| exe.starts_with(p))
}

fn new_name(exe: &Path) -> PathBuf {
    let mut n = std::ffi::OsString::from(".");
    n.push(exe.file_name().unwrap_or_default());
    n.push(".new");
    exe.with_file_name(n)
}

/// The leftover of a Windows update.
pub fn old_name(exe: &Path) -> PathBuf {
    let mut n = exe.file_name().unwrap_or_default().to_os_string();
    n.push(".old");
    exe.with_file_name(n)
}

/// Delete what a former update left behind (the renamed old program on Windows). Call it at the start.
pub fn cleanup_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_name(&exe));
        let _ = std::fs::remove_file(new_name(&exe));
    }
}

/// Put `new` in place of the program at `exe`. The new file is written next to it first. Elsewhere it is renamed over the old one
/// (the running program keeps its old file); on Windows a running program cannot be overwritten but can be renamed, so it becomes
/// `.old` (deleted on the next start) and the new one takes its name. Nothing changes when a step fails.
pub fn replace_exe(exe: &Path, new: &[u8], windows: bool) -> Result<(), InstallError> {
    let staged = new_name(exe);
    if let Err(e) = write_staged(exe, &staged, new) {
        let _ = std::fs::remove_file(&staged);
        return Err(match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => InstallError::NotWritable,
            _ => InstallError::Other(format!("could not write the new program: {e}")),
        });
    }
    let result = if windows {
        let old = old_name(exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).and_then(|_| {
            std::fs::rename(&staged, exe).inspect_err(|_| {
                // put the old program back
                let _ = std::fs::rename(&old, exe);
            })
        })
    } else {
        std::fs::rename(&staged, exe)
    };
    result.map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => InstallError::NotWritable,
            _ => InstallError::Other(format!("could not replace the program: {e}")),
        }
    })
}

fn write_staged(exe: &Path, staged: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(staged)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(exe).map_or(0o755, |m| m.permissions().mode() & 0o7777) | 0o111;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = exe;
    Ok(())
}

/// Everything after the download: verify the checksum, unpack, replace. Nothing is touched before the checksum matches.
pub fn install_from_bytes(archive_name: &str, archive: &[u8], sums: &str, exe: &Path, os: &str) -> Result<(), InstallError> {
    let expected = parse_sums(sums, archive_name).ok_or_else(|| InstallError::Checksum(format!("no checksum for {archive_name}")))?;
    let actual = sha256_hex(archive);
    if actual != expected {
        return Err(InstallError::Checksum("the download does not match its checksum".into()));
    }
    if is_managed_path(exe) {
        return Err(InstallError::NotWritable);
    }
    let new = extract_exe(archive_name, archive, exe_name(os))?;
    if new.is_empty() {
        return Err(InstallError::Other("the program in the package is empty".into()));
    }
    replace_exe(exe, &new, os == "windows")
}

// ---------- network ----------

#[derive(Debug)]
pub enum CheckError {
    /// the server answered with an error (404 when the repository has no release yet, 403 when rate limited)
    Status(u16),
    /// no connection, a bad answer ...
    Failed(String),
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).user_agent(concat!("giana-app/", env!("CARGO_PKG_VERSION"), " (+https://github.com/kvnshroom/giana-sisters-native)")).build()
}

/// The latest release (not a draft or pre-release).
pub fn fetch_latest() -> Result<Release, CheckError> {
    let resp = agent(Duration::from_secs(5)).get(API_URL).set("Accept", "application/vnd.github+json").call().map_err(|e| match e {
        ureq::Error::Status(code, _) => CheckError::Status(code),
        other => CheckError::Failed(other.to_string()),
    })?;
    let mut body = String::new();
    resp.into_reader().take(2 * 1024 * 1024).read_to_string(&mut body).map_err(|e| CheckError::Failed(e.to_string()))?;
    parse_release(&body).ok_or_else(|| CheckError::Failed("unexpected answer".into()))
}

fn download(url: &str) -> Result<Vec<u8>, InstallError> {
    if !url.starts_with(DOWNLOAD_PREFIX) {
        return Err(InstallError::Other("the download is not from the project's releases".into()));
    }
    let resp = agent(Duration::from_secs(300)).get(url).call().map_err(|e| InstallError::Other(format!("download failed: {e}")))?;
    let mut data = Vec::new();
    resp.into_reader().take(MAX_DOWNLOAD + 1).read_to_end(&mut data).map_err(|e| InstallError::Other(format!("download failed: {e}")))?;
    if data.len() as u64 > MAX_DOWNLOAD {
        return Err(InstallError::Other("the download is too big".into()));
    }
    Ok(data)
}

/// Download, verify and install `release` over the running program.
pub fn install_release(release: &Release) -> Result<(), InstallError> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let name = asset_name(&release.version, os, arch).ok_or(InstallError::NotWritable)?;
    let asset = release.asset(&name).ok_or_else(|| InstallError::Other(format!("{name} is not part of the release")))?;
    let sums = release.asset(SUMS_NAME).ok_or_else(|| InstallError::Checksum(format!("{SUMS_NAME} is not part of the release")))?;
    let exe = std::env::current_exe().map_err(|e| InstallError::Other(e.to_string()))?;
    // fail early, before the big download
    if is_managed_path(&exe) {
        return Err(InstallError::NotWritable);
    }
    let sums = String::from_utf8(download(&sums.url)?).map_err(|_| InstallError::Checksum("unreadable checksum file".into()))?;
    let archive = download(&asset.url)?;
    install_from_bytes(&name, &archive, &sums, &exe, os)
}

/// Open the release page in the browser (best effort).
pub fn open_page(url: &str) {
    if !url.starts_with("https://github.com/") {
        return;
    }
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = cmd.arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

// ---------- the check, once a day ----------

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A check is due when none was made for a day (a clock that went back counts as due).
pub fn check_due(last: u64, now: u64) -> bool {
    last == 0 || now < last || now - last >= CHECK_INTERVAL_SECS
}

/// What the main menu shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    None,
    /// this version can be installed
    Available(String),
    Installing,
    Done,
    /// the program cannot be replaced here, the release page was opened
    Manual,
    Failed(String),
}

impl View {
    /// The text of the notice; `key` is the name of the key or button that starts the update.
    pub fn line(&self, key: &str) -> Option<String> {
        Some(match self {
            View::None => return None,
            View::Available(v) => format!("NEW VERSION {v} - PRESS {key} TO UPDATE"),
            View::Installing => "UPDATING...".to_string(),
            View::Done => "UPDATED - RESTART THE GAME".to_string(),
            View::Manual => "OPENED THE PAGE OF THE NEW VERSION".to_string(),
            View::Failed(why) => format!("UPDATE FAILED - {why}"),
        })
    }
}

enum Work {
    Check(Receiver<(Option<Release>, bool)>),
    Install(Receiver<View>),
}

/// The background work of the updater, polled from the main loop.
pub struct Updater {
    pub view: View,
    work: Option<Work>,
}

/// What a finished check changes in the settings.
pub struct SettingsChange {
    pub last_check: Option<u64>,
    pub seen: Option<String>,
}

impl Updater {
    /// Starts a check in the background when `enabled` and one is due; shows a version that an earlier check found.
    pub fn start(enabled: bool, last_check: u64, seen: &str) -> Updater {
        let mut view = View::None;
        if enabled && is_newer(seen, env!("CARGO_PKG_VERSION")) {
            view = View::Available(plain_version(seen));
        }
        let work = (enabled && check_due(last_check, now_secs())).then(|| {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                // (the release if it is newer, a check was really made)
                let answer = match fetch_latest() {
                    Ok(r) => (Some(r), true),
                    // no release yet: a real answer; any other status (rate limit, server trouble) is like being offline
                    Err(CheckError::Status(404)) => (None, true),
                    Err(CheckError::Status(_)) => (None, false),
                    Err(CheckError::Failed(_)) => (None, false),
                };
                let _ = tx.send(answer);
            });
            Work::Check(rx)
        });
        Updater { view, work }
    }

    /// Look at the background work. Returns what to store in the settings, if a check finished.
    pub fn poll(&mut self) -> Option<SettingsChange> {
        match self.work.as_ref()? {
            Work::Check(rx) => match rx.try_recv() {
                Ok((release, checked)) => {
                    self.work = None;
                    let seen = release.as_ref().map(|r| r.version.clone());
                    if let Some(r) = release.filter(|r| is_newer(&r.version, env!("CARGO_PKG_VERSION")) && r.has_package()) {
                        if !matches!(self.view, View::Installing | View::Done | View::Manual) {
                            self.view = View::Available(r.version);
                        }
                    } else if matches!(self.view, View::Available(_)) {
                        self.view = View::None;
                    }
                    checked.then(|| SettingsChange { last_check: Some(now_secs()), seen: Some(seen.unwrap_or_default()) })
                }
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.work = None;
                    None
                }
            },
            Work::Install(rx) => {
                match rx.try_recv() {
                    Ok(view) => {
                        self.view = view;
                        self.work = None;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        self.view = View::Failed("INTERNAL ERROR".into());
                        self.work = None;
                    }
                }
                None
            }
        }
    }

    /// Start the install of the offered version (`U` on the main menu).
    pub fn install(&mut self) {
        if !matches!(self.view, View::Available(_)) || matches!(self.work, Some(Work::Install(_))) {
            return;
        }
        self.view = View::Installing;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let view = match fetch_latest() {
                Ok(r) if !is_newer(&r.version, env!("CARGO_PKG_VERSION")) => View::None,
                Ok(r) => match install_release(&r) {
                    Ok(()) => View::Done,
                    Err(InstallError::NotWritable) => {
                        println!("Update {} available: {}", r.version, r.page);
                        open_page(&r.page);
                        View::Manual
                    }
                    Err(InstallError::Checksum(m)) => {
                        eprintln!("Update refused: {m}");
                        View::Failed("CHECKSUM MISMATCH".into())
                    }
                    Err(InstallError::Other(m)) => {
                        eprintln!("Update failed: {m}");
                        View::Failed("SEE THE TERMINAL".into())
                    }
                },
                Err(_) => View::Failed("NO CONNECTION".into()),
            };
            let _ = tx.send(view);
        });
        self.work = Some(Work::Install(rx));
    }
}

/// `--check-update`: the result as text, and whether it worked.
pub fn check_report() -> (String, bool) {
    let current = env!("CARGO_PKG_VERSION");
    match fetch_latest() {
        Ok(r) if is_newer(&r.version, current) && r.has_package() => (format!("A new version is available: {} (you have {current})\n{}", r.version, r.page), true),
        Ok(r) if is_newer(&r.version, current) => (format!("Version {} is out, its package for this system is still being uploaded (you have {current})", r.version), true),
        // (a newer release whose package for this system is not uploaded yet is reported as such)
        Ok(r) => (format!("Up to date: {current} (latest release: {})", r.version), true),
        Err(CheckError::Status(404)) => (format!("No release found (you have {current}). {RELEASES_URL}"), true),
        Err(CheckError::Status(code)) => (format!("The update check failed: the server answered {code}"), false),
        Err(CheckError::Failed(m)) => (format!("The update check failed: {m}"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE: &str = r#"{
      "url": "https://api.github.com/repos/kvnshroom/giana-sisters-native/releases/1",
      "html_url": "https://github.com/kvnshroom/giana-sisters-native/releases/tag/v0.2.0",
      "tag_name": "v0.2.0", "name": "0.2.0", "draft": false, "prerelease": false,
      "assets": [
        {"name": "giana-sisters-native-0.2.0-linux-x86_64.tar.gz", "size": 1, "browser_download_url": "https://github.com/kvnshroom/giana-sisters-native/releases/download/v0.2.0/giana-sisters-native-0.2.0-linux-x86_64.tar.gz"},
        {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/kvnshroom/giana-sisters-native/releases/download/v0.2.0/SHA256SUMS.txt"},
        {"name": "broken"}
      ]
    }"#;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("giana-update-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            tar.append_data(&mut h, path, *data).unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    fn zip_file(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, data) in files {
            z.start_file(*path, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn versions_compare_like_semver() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"), "numbers, not text");
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("v1.0.0-rc1", "1.0.0"), "a pre-release is older than its release");
        assert!(is_newer("v1.0.0", "1.0.0-rc1"));
        assert!(!is_newer("nightly", "0.1.0") && !is_newer("v0.2.0", "garbage"));
        assert!(is_newer("v2", "1.9.9"));
        assert_eq!(Version::parse("v1.2.3+build5"), Version::parse("1.2.3"));
        assert_eq!(Version::parse("1.2.3.4"), None);
    }

    #[test]
    fn the_asset_depends_on_the_system() {
        assert_eq!(asset_name("0.2.0", "linux", "x86_64").unwrap(), "giana-sisters-native-0.2.0-linux-x86_64.tar.gz");
        assert_eq!(asset_name("0.2.0", "windows", "x86_64").unwrap(), "giana-sisters-native-0.2.0-windows-x86_64.zip");
        assert_eq!(asset_name("0.2.0", "macos", "aarch64").unwrap(), "giana-sisters-native-0.2.0-macos-universal.tar.gz");
        assert_eq!(asset_name("0.2.0", "macos", "x86_64"), asset_name("0.2.0", "macos", "aarch64"));
        assert_eq!(asset_name("0.2.0", "linux", "aarch64"), None);
        assert_eq!(asset_name("0.2.0", "freebsd", "x86_64"), None);
        assert_eq!((exe_name("windows"), exe_name("linux"), exe_name("macos")), ("giana-app.exe", "giana-app", "giana-app"));
    }

    #[test]
    fn a_github_answer_is_read() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert!(r.page.ends_with("/tag/v0.2.0"));
        assert_eq!(r.assets.len(), 2, "the entry without a URL is skipped");
        assert!(r.asset(SUMS_NAME).unwrap().url.starts_with(DOWNLOAD_PREFIX));
        assert!(r.asset(&asset_name("0.2.0", "linux", "x86_64").unwrap()).is_some());
        // not a release: drafts, pre-releases, garbage, the error answer of a missing repository
        assert_eq!(parse_release(&SAMPLE.replace("\"draft\": false", "\"draft\": true")), None);
        assert_eq!(parse_release(&SAMPLE.replace("\"prerelease\": false", "\"prerelease\": true")), None);
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release(r#"{"message":"Not Found","status":"404"}"#), None);
        assert_eq!(parse_release(&SAMPLE.replace("v0.2.0\", \"name", "latest\", \"name")), None);
    }

    #[test]
    fn a_release_is_offered_only_where_its_package_is_uploaded() {
        // the sample has the Linux package and the checksums, the others are still being built
        let r = parse_release(SAMPLE).unwrap();
        assert!(r.has_package_for("linux", "x86_64"));
        assert!(!r.has_package_for("windows", "x86_64"));
        assert!(!r.has_package_for("macos", "aarch64"));
        assert!(!r.has_package_for("linux", "aarch64"), "no package for that system at all");
        // without the checksum file nothing can be installed
        let no_sums = parse_release(&SAMPLE.replace("\"name\": \"SHA256SUMS.txt\"", "\"name\": \"other.txt\"")).unwrap();
        assert!(!no_sums.has_package_for("linux", "x86_64"));
    }

    #[test]
    fn checksum_files_are_read() {
        let h1 = "a".repeat(64);
        let h2 = "B".repeat(64);
        let text = format!("{h1}  one.tar.gz\n{h2} *two.zip\n\nnonsense\n{} short.zip\n", "c".repeat(10));
        assert_eq!(parse_sums(&text, "one.tar.gz").unwrap(), h1);
        assert_eq!(parse_sums(&text, "two.zip").unwrap(), "b".repeat(64), "binary mode marker and upper case");
        assert_eq!(parse_sums(&text, "three.zip"), None);
        assert_eq!(parse_sums(&text, "short.zip"), None, "a hash of the wrong length is no hash");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_check_runs_once_a_day() {
        assert!(check_due(0, 1000));
        assert!(!check_due(1000, 1000 + CHECK_INTERVAL_SECS - 1));
        assert!(check_due(1000, 1000 + CHECK_INTERVAL_SECS));
        assert!(check_due(5000, 100), "a clock that went back");
    }

    #[test]
    fn the_view_has_texts() {
        assert_eq!(View::None.line("U"), None);
        assert_eq!(View::Available("0.2.0".into()).line("U").unwrap(), "NEW VERSION 0.2.0 - PRESS U TO UPDATE");
        assert!(View::Done.line("U").unwrap().contains("RESTART"));
        assert!(View::Failed("X".into()).line("U").unwrap().len() <= 46);
    }

    #[test]
    fn unpacking_finds_the_program() {
        let tgz = tar_gz(&[("pkg/README.txt", b"hi"), ("pkg/giana-app", b"LINUX-BIN")]);
        assert_eq!(extract_exe("x-linux-x86_64.tar.gz", &tgz, "giana-app").unwrap(), b"LINUX-BIN");
        let mac = tar_gz(&[("Giana Sisters.app/Contents/Info.plist", b"plist"), ("Giana Sisters.app/Contents/MacOS/giana-app", b"MAC-BIN")]);
        assert_eq!(extract_exe("x-macos-universal.tar.gz", &mac, "giana-app").unwrap(), b"MAC-BIN");
        let z = zip_file(&[("giana-app.exe", b"WIN-BIN"), ("data/x.bin", b"d")]);
        assert_eq!(extract_exe("x-windows-x86_64.zip", &z, "giana-app.exe").unwrap(), b"WIN-BIN");
        assert!(extract_exe("x.tar.gz", &tgz, "giana-app.exe").is_err());
        assert!(extract_exe("x.zip", b"garbage", "giana-app.exe").is_err());
        assert!(extract_exe("x.tar.gz", b"garbage", "giana-app").is_err());
    }

    #[test]
    fn app_bundles_are_found() {
        let exe = Path::new("/Applications/Giana Sisters.app/Contents/MacOS/giana-app");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/Giana Sisters.app")));
        assert_eq!(bundle_of(Path::new("/home/me/giana-app")), None);
        assert_eq!(bundle_of(Path::new("/x/Thing/Contents/MacOS/giana-app")), None, "needs the .app");
        assert_eq!(bundle_of(Path::new("/x/Y.app/Contents/Other/giana-app")), None);
        assert!(is_managed_path(Path::new("/usr/bin/giana-app")) && is_managed_path(Path::new("/nix/store/abc/bin/giana-app")));
        assert!(!is_managed_path(Path::new("/usr/local/bin/giana-app")) && !is_managed_path(Path::new("/home/me/giana-app")));
    }

    #[test]
    fn replacing_on_linux_keeps_the_neighbours() {
        let dir = temp_dir("linux");
        let exe = dir.join("giana-app");
        std::fs::write(&exe, b"OLD").unwrap();
        std::fs::write(dir.join("settings.conf"), b"keep").unwrap();
        replace_exe(&exe, b"NEW", false).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"NEW");
        assert_eq!(std::fs::read(dir.join("settings.conf")).unwrap(), b"keep");
        assert!(!new_name(&exe).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0, "still executable");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replacing_on_windows_renames_the_old_one() {
        let dir = temp_dir("windows");
        let exe = dir.join("giana-app.exe");
        std::fs::write(&exe, b"OLD").unwrap();
        std::fs::write(old_name(&exe), b"OLDER").unwrap();
        replace_exe(&exe, b"NEW", true).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"NEW");
        assert_eq!(std::fs::read(old_name(&exe)).unwrap(), b"OLD", "the running program is kept as .old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn replacing_inside_an_app_bundle() {
        let dir = temp_dir("mac");
        let macos = dir.join("Giana Sisters.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        let exe = macos.join("giana-app");
        std::fs::write(&exe, b"OLD").unwrap();
        std::fs::write(macos.parent().unwrap().join("Info.plist"), b"plist").unwrap();
        assert_eq!(bundle_of(&exe), Some(dir.join("Giana Sisters.app")));
        let pkg = tar_gz(&[("Giana Sisters.app/Contents/MacOS/giana-app", b"NEW-MAC")]);
        let name = asset_name("0.2.0", "macos", "aarch64").unwrap();
        let sums = format!("{}  {name}\n", sha256_hex(&pkg));
        install_from_bytes(&name, &pkg, &sums, &exe, "macos").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"NEW-MAC");
        assert_eq!(std::fs::read(macos.parent().unwrap().join("Info.plist")).unwrap(), b"plist");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_folder_or_no_write_access_means_not_writable_or_error() {
        let dir = temp_dir("nowrite");
        let exe = dir.join("missing-dir/giana-app");
        assert!(replace_exe(&exe, b"NEW", false).is_err());
        assert!(!exe.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exe = dir.join("giana-app");
            std::fs::write(&exe, b"OLD").unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            let r = replace_exe(&exe, b"NEW", false);
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            // running as root can write anyway
            if r.is_err() {
                assert_eq!(r, Err(InstallError::NotWritable));
                assert_eq!(std::fs::read(&exe).unwrap(), b"OLD");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wrong_checksum_installs_nothing() {
        let dir = temp_dir("mismatch");
        let exe = dir.join("giana-app");
        std::fs::write(&exe, b"OLD").unwrap();
        let pkg = tar_gz(&[("giana-app", b"EVIL")]);
        let name = "giana-sisters-native-0.2.0-linux-x86_64.tar.gz";
        // wrong hash
        let bad = format!("{}  {name}\n", "0".repeat(64));
        let r = install_from_bytes(name, &pkg, &bad, &exe, "linux");
        assert!(matches!(r, Err(InstallError::Checksum(_))));
        // no line for the file
        let other = format!("{}  other.tar.gz\n", sha256_hex(&pkg));
        assert!(matches!(install_from_bytes(name, &pkg, &other, &exe, "linux"), Err(InstallError::Checksum(_))));
        // a package changed after the sums were made
        let mut tampered = pkg.clone();
        tampered.push(0);
        let good = format!("{}  {name}\n", sha256_hex(&pkg));
        assert!(matches!(install_from_bytes(name, &tampered, &good, &exe, "linux"), Err(InstallError::Checksum(_))));
        assert_eq!(std::fs::read(&exe).unwrap(), b"OLD");
        assert!(!new_name(&exe).exists());
        // the right one works
        install_from_bytes(name, &pkg, &good, &exe, "linux").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"EVIL", "(the test package's content)");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn programs_of_a_package_manager_are_not_overwritten() {
        let pkg = tar_gz(&[("giana-app", b"NEW")]);
        let name = "giana-sisters-native-0.2.0-linux-x86_64.tar.gz";
        let sums = format!("{}  {name}\n", sha256_hex(&pkg));
        assert_eq!(install_from_bytes(name, &pkg, &sums, Path::new("/usr/bin/giana-app"), "linux"), Err(InstallError::NotWritable));
    }
}
