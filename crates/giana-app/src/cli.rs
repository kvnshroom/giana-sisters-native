//! The command line of the game.

pub const HELP: &str = "The Great Giana Sisters (Rust port)

USAGE:
    giana-app [OPTIONS]

OPTIONS:
    --import FILE.g64  extract the game data from your original disk image and exit
    --data DIR         use the original game data in DIR (extract into it on first start)
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

/// Arguments without the program name. The error is the message for the player.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut opts = Options::default();
    let mut args = args.into_iter();
    let mut import = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--check-update" => return Ok(Command::CheckUpdate),
            "--no-update-check" => opts.no_update_check = true,
            "--debug" => opts.debug = true,
            "--data" => opts.data = Some(args.next().filter(|v| !v.is_empty()).ok_or("--data needs a folder")?.into()),
            "--import" => import = Some(args.next().filter(|v| !v.is_empty()).ok_or("--import needs the path of a .g64 file")?.into()),
            "--stage" => opts.stage = Some(stage_number(args.next().as_deref())?),
            other => match other.strip_prefix("--stage=") {
                Some(v) => opts.stage = Some(stage_number(Some(v))?),
                None => return Err(format!("unknown argument '{other}'")),
            },
        }
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
    fn bad_input_is_an_error() {
        assert!(p(&["--stage"]).is_err());
        assert!(p(&["--stage", "0"]).is_err());
        assert!(p(&["--stage", "34"]).is_err());
        assert!(p(&["--stage", "x"]).is_err());
        assert!(p(&["--bogus"]).unwrap_err().contains("--bogus"));
    }
}
