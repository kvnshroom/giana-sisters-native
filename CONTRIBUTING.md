# Contributing

Thanks for wanting to help. This is a hobby project with little maintainer time, so bug reports with clear steps,
small pull requests and forks that take the game further are all welcome.

## Reporting a bug

Open a [bug report](https://github.com/kvnshroom/giana-sisters-native/issues/new?template=bug_report.yml) and include:

- the version (shown in the main menu, or `giana-app --version`) and your system,
- which disk image you used (`.g64` or `.d64`) and whether a ROM module was active,
- the steps to make it happen, and what you expected instead.

**Something plays differently from the C64?** That is a bug too, and the most interesting kind. Say where it happens
(stage, position, what Giana was doing) and, if you can, check it in an emulator such as VICE. A short video of both helps a lot.

If the game stopped with an error, attach `last-error.txt` from the settings folder
(`~/.config/giana-native/` on Linux and macOS, `%APPDATA%\giana-native\` on Windows).

**Never attach the disk image or extracted game data** (anything from the `data` folder), not even in a zip. The
original game is not ours to share.

## Ideas and feature requests

Use the [feature request](https://github.com/kvnshroom/giana-sisters-native/issues/new?template=feature_request.yml)
form. Things that would help: support for more versions of the original disk, signed Windows and macOS builds, a
Flatpak, more controller layouts, translations, more ROM hacks.

## The one rule: ORIGINAL stays original

By default the game plays exactly like the C64 version – the same timing, the same collisions, the same quirks.
What looks like a bug of the original is often part of how the game feels, and players remember it that way.

- A change that fixes a difference **between this port and the C64** is a fix.
- A change to **how the original plays** (physics, enemies, rules, difficulty) goes behind an option that is off by
  default, the way WIDESCREEN does it.
- Changes to presentation (menus, picture, sound settings) are fine as long as the default stays close to the original.

## Building and testing

You need Rust stable ([rustup](https://rustup.rs/)), a C compiler and the SDL2 development files; see
[docs/building.md](docs/building.md).

```bash
cargo build --release -p giana-app
cargo test --workspace
cargo clippy --workspace --all-targets
```

Most tests need the extracted game data: start the game once with your disk image, or run
`tools/data/setup.sh /path/to/disk.g64` to put it in `data/` (never commit that folder; `.gitignore` keeps it out).
The network tests use the fixed ports 47111 and 47112.

## Pull requests

- Keep them small and about one thing; open an issue first for anything big.
- Code, comments and commit messages in English. Commit messages in the conventional style, for example
  `fix(audio): ...` or `feat(menu): ...`.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets` should pass without warnings.
- A change to the network protocol needs a new handshake version, so different builds refuse to play together
  instead of drifting apart.
- Player-visible changes get a line in the `[Unreleased]` section of [CHANGELOG.md](CHANGELOG.md).
- No game data, ROMs, disk images, extracted graphics or music in the repository, and no tables copied out of the game
  data. Code that reads them from the player's own copy is fine.

How changes get in: this repository is published from the maintainer's development tree, which also holds reference
tools that need the original game and so cannot be public. An accepted pull request is applied there, checked against
those tools, and comes back here with the next update, with your name as co-author of the commit.

## License

By contributing you agree that your contribution is licensed under the
[GNU General Public License v3.0 or later](LICENSE), like the rest of the project.
