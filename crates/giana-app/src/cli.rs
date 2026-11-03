//! The command line of the game.

pub const HELP: &str = "The Great Giana Sisters (Rust port)

USAGE:
    giana-app [OPTIONS] [FILE.g64 | FILE.grom]

ARGUMENT:
    FILE.g64           on the first start, use this disk image (also: Open with, or drag it onto the program)
    FILE.grom          add this ROM module and play it from now on (also: Open with, or drag it onto the program)

OPTIONS:
    --import FILE.g64  extract the game data from your original disk image and exit
    --data DIR         use the original game data in DIR (extract into it on first start)
    --module FILE|ID   play this ROM module in this run only (a .grom file is added first; an ID is the start of
                       the id that --list-modules shows)
    --list-modules     list the installed ROM modules and exit
    --make-module INPUT -o OUT.grom --name NAME [--author A] [--version V] [--description D] [--title F_2.PRG]
                       make a ROM module from a modified game: a disk image of a hack (.d64/.g64; a hack of the
                       7A3 construction-set engine brings its stage files along), a 64K image or RAM dump, a PRG
                       of disk file 1, a VICE snapshot (.vsf) or a .bps patch against ingame.bin. Needs your
                       original game data; the module contains none of it. Prints what it changes and exits
    --stage N          skip the menu and start in stage N (1 to 33)
    --debug            debug keys: F1 lives, F2 invulnerable, F3/F4 previous/next stage
    --check-update     look for a new version now, print the result and exit
    --no-update-check  never look for a new version (also OPTIONS > UPDATE CHECK)
    -h, --help         show this text
    -V, --version      show the version

The game contains no original data. On first start it asks for your own The Great Giana Sisters disk image (.g64) and
extracts what it needs into its data folder ($XDG_DATA_HOME/giana-native/data, ~/.local/share/giana-native/data,
%APPDATA%\\giana-native\\data); a data folder next to the program is used too.

The game needs the SDL2 library (Fedora: sdl2-compat or SDL2, Debian/Ubuntu: libsdl2-2.0-0, macOS: brew install sdl2).
Settings, controls and high scores live in the giana-native folder of your configuration directory.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `--data DIR`: the data folder to use (and to extract into)
    pub data: Option<std::path::PathBuf>,
    /// the debug keys F1 to F4 work
    pub debug: bool,
    pub stage: Option<u32>,
    /// no automatic check for a new version
    pub no_update_check: bool,
    /// a lone `FILE.g64` argument (Open with, drag onto the program): imported at once if the game data is still missing
    pub open: Option<std::path::PathBuf>,
    /// `--module FILE|ID`: the ROM module for this run only
    pub module: Option<String>,
    /// a lone `FILE.grom` argument: added to the modules and chosen for good
    pub open_module: Option<std::path::PathBuf>,
}

/// `--make-module INPUT -o OUT ...`
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MakeModule {
    pub input: std::path::PathBuf,
    pub out: std::path::PathBuf,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    /// a modified title program (`f_2.prg`)
    pub title: Option<std::path::PathBuf>,
    pub data: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    /// `--import FILE` (with the data folder of `--data`): extract and exit
    Import(std::path::PathBuf, Option<std::path::PathBuf>),
    Help,
    Version,
    /// `--check-update`
    CheckUpdate,
    MakeModule(MakeModule),
    /// `--list-modules` (with the data folder of `--data`)
    ListModules(Option<std::path::PathBuf>),
}

/// The text of `--version`.
pub fn version_text() -> String {
    format!("giana-app {} (build {})", env!("CARGO_PKG_VERSION"), crate::net::BUILD)
}

fn stage_number(value: Option<&str>) -> Result<u32, String> {
    value
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|n| (1..=33).contains(n))
        .ok_or_else(|| format!("--stage needs a number from 1 to 33, not '{}'", value.unwrap_or("")))
}

fn is_g64(arg: &str) -> bool {
    std::path::Path::new(arg).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("g64"))
}

fn is_grom(arg: &str) -> bool {
    std::path::Path::new(arg).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("grom"))
}

/// The arguments for the program that replaces this one when the ROM module is switched: those of this run without what
/// only applied to it (the module of this run, the stage, the files to open).
pub fn restart_args<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--module" | "--stage" => {
                args.next();
            }
            a if a.starts_with("--module=") || a.starts_with("--stage=") => {}
            a if !a.starts_with('-') && (is_g64(a) || is_grom(a)) => {}
            _ => out.push(arg),
        }
    }
    out
}

fn value(args: &mut impl Iterator<Item = String>, what: &str) -> Result<String, String> {
    args.next().filter(|v| !v.is_empty()).ok_or_else(|| format!("{what} needs a value"))
}

/// Arguments without the program name. The error is the message for the player.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut opts = Options::default();
    let mut args = args.into_iter();
    let mut import = None;
    let mut open = None;
    let mut make: Option<std::path::PathBuf> = None;
    let mut mk = MakeModule::default();
    let mut list = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            // after --make-module (or one of its options) --version is the version of the module
            "--version" if make.is_some() || mk != MakeModule::default() => mk.version = value(&mut args, &arg)?,
            "-V" | "--version" => return Ok(Command::Version),
            "--check-update" => return Ok(Command::CheckUpdate),
            "--no-update-check" => opts.no_update_check = true,
            "--debug" => opts.debug = true,
            "--data" => opts.data = Some(args.next().filter(|v| !v.is_empty()).ok_or("--data needs a folder")?.into()),
            "--import" => import = Some(args.next().filter(|v| !v.is_empty()).ok_or("--import needs the path of a .g64 file")?.into()),
            "--module" => opts.module = Some(value(&mut args, &arg)?),
            "--list-modules" => list = true,
            "--make-module" => make = Some(value(&mut args, &arg)?.into()),
            "-o" | "--output" => mk.out = value(&mut args, &arg)?.into(),
            "--name" => mk.name = value(&mut args, &arg)?,
            "--author" => mk.author = value(&mut args, &arg)?,
            "--module-version" => mk.version = value(&mut args, &arg)?,
            "--description" => mk.description = value(&mut args, &arg)?,
            "--title" => mk.title = Some(value(&mut args, &arg)?.into()),
            "--stage" => opts.stage = Some(stage_number(args.next().as_deref())?),
            // old macOS versions pass a process serial number when launched from Finder
            other if other.starts_with("-psn_") => {}
            other if !other.starts_with('-') && open.is_none() && is_g64(other) => open = Some(other.into()),
            other if !other.starts_with('-') && opts.open_module.is_none() && is_grom(other) => opts.open_module = Some(other.into()),
            other => match other.strip_prefix("--stage=") {
                Some(v) => opts.stage = Some(stage_number(Some(v))?),
                None => return Err(format!("unknown argument '{other}'")),
            },
        }
    }
    opts.open = open;
    if let Some(input) = make {
        if mk.out.as_os_str().is_empty() || mk.name.trim().is_empty() {
            return Err("--make-module needs -o OUT.grom and --name NAME".into());
        }
        return Ok(Command::MakeModule(MakeModule { input, data: opts.data, ..mk }));
    }
    if mk != MakeModule::default() {
        return Err("-o, --name, --author, --version, --description and --title belong to --make-module".into());
    }
    if list {
        return Ok(Command::ListModules(opts.data));
    }
    Ok(match import {
        Some(file) => Command::Import(file, opts.data),
        None => Command::Run(opts),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_run_the_game() {
        assert_eq!(p(&[]), Ok(Command::Run(Options::default())));
    }

    #[test]
    fn help_version_stage_and_debug() {
        assert_eq!(p(&["--help"]), Ok(Command::Help));
        assert_eq!(p(&["-V"]), Ok(Command::Version));
        assert_eq!(p(&["--debug", "--stage", "5"]), Ok(Command::Run(Options { debug: true, stage: Some(5), ..Options::default() })));
        assert_eq!(p(&["--stage=33"]), Ok(Command::Run(Options { debug: false, stage: Some(33), ..Options::default() })));
    }

    #[test]
    fn the_update_flags() {
        assert_eq!(p(&["--check-update"]), Ok(Command::CheckUpdate));
        assert_eq!(p(&["--no-update-check"]), Ok(Command::Run(Options { no_update_check: true, ..Options::default() })));
    }

    #[test]
    fn data_and_import() {
        let Ok(Command::Run(o)) = p(&["--data", "/x/y"]) else { panic!() };
        assert_eq!(o.data, Some("/x/y".into()));
        assert_eq!(p(&["--import", "a.g64", "--data", "d"]), Ok(Command::Import("a.g64".into(), Some("d".into()))));
        assert_eq!(p(&["--import", "a.g64"]), Ok(Command::Import("a.g64".into(), None)));
        assert!(p(&["--import"]).is_err());
        assert!(p(&["--data"]).is_err());
    }

    #[test]
    fn a_lone_g64_path_is_accepted() {
        let Ok(Command::Run(o)) = p(&["/x/My Games/Giana.G64"]) else { panic!() };
        assert_eq!(o.open, Some("/x/My Games/Giana.G64".into()));
        let Ok(Command::Run(o)) = p(&["-psn_0_12345", "--debug", "a.g64"]) else { panic!() };
        assert!(o.debug && o.open == Some("a.g64".into()));
        assert!(p(&["notes.txt"]).is_err());
        assert!(p(&["a.g64", "b.g64"]).is_err());
    }

    #[test]
    fn rom_module_options() {
        let Ok(Command::Run(o)) = p(&["--module", "ab12cd", "--stage", "2"]) else { panic!() };
        assert_eq!((o.module.as_deref(), o.stage, o.open_module), (Some("ab12cd"), Some(2), None));
        let Ok(Command::Run(o)) = p(&["/x/My Hack.GROM", "--debug"]) else { panic!() };
        assert_eq!(o.open_module, Some("/x/My Hack.GROM".into()));
        assert!(o.open.is_none() && o.module.is_none());
        // a disk image and a module can come together
        let Ok(Command::Run(o)) = p(&["a.g64", "b.grom"]) else { panic!() };
        assert_eq!((o.open, o.open_module), (Some("a.g64".into()), Some("b.grom".into())));
        assert!(p(&["--module"]).is_err());
        assert!(p(&["a.grom", "b.grom"]).is_err());
        assert_eq!(p(&["--list-modules", "--data", "d"]), Ok(Command::ListModules(Some("d".into()))));
        assert_eq!(p(&["--list-modules"]), Ok(Command::ListModules(None)));
    }

    #[test]
    fn making_a_module() {
        let args = ["--make-module", "hack.bin", "-o", "out.grom", "--name", "MORE LEVELS", "--author", "ME", "--version", "1.2", "--description", "33 NEW", "--title", "t.prg", "--data", "d"];
        let Ok(Command::MakeModule(m)) = p(&args) else { panic!() };
        assert_eq!(m, MakeModule {
            input: "hack.bin".into(),
            out: "out.grom".into(),
            name: "MORE LEVELS".into(),
            author: "ME".into(),
            version: "1.2".into(),
            description: "33 NEW".into(),
            title: Some("t.prg".into()),
            data: Some("d".into()),
        });
        // the order does not matter, author and the rest are optional
        let Ok(Command::MakeModule(m)) = p(&["-o", "o.grom", "--name", "X", "--make-module", "in.vsf"]) else { panic!() };
        assert_eq!((m.input, m.author, m.title), ("in.vsf".into(), String::new(), None));
        assert!(p(&["--make-module", "hack.bin", "--name", "X"]).unwrap_err().contains("-o"));
        assert!(p(&["--make-module", "hack.bin", "-o", "o.grom"]).is_err());
        assert!(p(&["--make-module"]).is_err());
        assert!(p(&["--name", "X"]).is_err(), "module fields without --make-module");
        // --version still prints the version of the game, unless it comes with a module to make
        assert_eq!(p(&["--version"]), Ok(Command::Version));
        let Ok(Command::MakeModule(m)) = p(&["--make-module", "a.bin", "--version", "2.0", "-o", "o.grom", "--name", "N"]) else { panic!() };
        assert_eq!(m.version, "2.0");
        let Ok(Command::MakeModule(m)) = p(&["--module-version", "3", "--make-module", "a.bin", "-o", "o.grom", "--name", "N"]) else { panic!() };
        assert_eq!(m.version, "3");
    }

    #[test]
    fn a_restart_keeps_the_options_that_stay() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let kept = restart_args(args(&["--data", "/d", "--module", "abcd", "--stage", "3", "--debug", "x.g64", "y.grom", "--stage=4", "--module=ff"]));
        assert_eq!(kept, args(&["--data", "/d", "--debug"]));
        assert!(restart_args(Vec::new()).is_empty());
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(p(&["--stage"]).is_err());
        assert!(p(&["--stage", "0"]).is_err());
        assert!(p(&["--stage", "34"]).is_err());
        assert!(p(&["--stage", "x"]).is_err());
        assert!(p(&["--bogus"]).unwrap_err().contains("--bogus"));
    }
}
