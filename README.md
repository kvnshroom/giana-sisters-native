<p align="center"><img src="assets/icon/icon.png" width="128" height="128" alt=""></p>

# The Great Giana Sisters – native port

A native PC version of the 1987 C64 classic *The Great Giana Sisters*, written in Rust.

This is **not an emulator**. The game logic was ported routine by routine from the original 6502 program and checked,
frame by frame, against the real thing running in a C64 emulator. The result plays like the C64 version – same jumps,
same enemies, same music from the original sound driver – but runs as a normal program with a proper menu, controller
support, fullscreen and network play.

<p align="center">
  <img src="screenshots/menu.png" width="32%" alt="The main menu">
  <img src="screenshots/stage1.png" width="32%" alt="Giana jumping in stage 1 with enemies around">
  <img src="screenshots/stage4.png" width="32%" alt="The castle of stage 4">
</p>

## Why this exists

I built this for my dad. He played Giana Sisters back in the day, and I wanted us to be able to play it together
again – on our own PCs, over the network, without fiddling with emulator settings. That's also why there are things
the original never had: high scores with names, a two-player network mode where you both play at the same time, and a
"who got further wins" rule he asked for.

It's a hobby project and I probably won't have much time to keep working on it. **Forks, pull requests and
contributions are very welcome** – if you want to fix something, add a platform or take it further, go for it.
See [Contributing](#contributing) for a few starting points.

## What you need

- **Your own copy of the original game as a `.g64` disk image.** This repository and the downloads contain no game
  data at all – no graphics, music, levels or ROMs. On first start the game asks for your disk image and extracts
  what it needs.
- Currently one version of the disk is supported (SHA-256 of the `.g64`):
  `5ce29ce04786eca6518fb08dfe659abb3eee079b4135a3f7606f9d17a501ec77`.
  Other versions are refused with a clear message. Supporting more versions mostly means adding their checksums and
  checking the extraction – a nice first contribution.

## Install

Download the package for your system from the [Releases](../../releases) page:

| System | File | Notes |
|---|---|---|
| Linux (x86_64) | `giana-sisters-native-<version>-linux-x86_64.tar.gz` | needs SDL2: `sudo apt install libsdl2-2.0-0` or `sudo dnf install SDL2` |
| Windows (x86_64) | `giana-sisters-native-<version>-windows-x86_64.zip` | unsigned: in SmartScreen click "More info" → "Run anyway" |
| macOS (Apple Silicon and Intel) | `giana-sisters-native-<version>-macos-universal.tar.gz` | not notarized: open it once, then System Settings → Privacy & Security → "Open Anyway" (macOS 15 and later; older: right-click → Open), or `xattr -dr com.apple.quarantine "Giana Sisters.app"` |

Unpack it and start the game. On the first start it asks for your disk image: click **CHOOSE FILE...** (or just press Enter) and pick the `.g64`, or
drag it onto the window (or onto the app icon), type or paste its path, or let it search the usual folders (Downloads,
Documents, your home folder; only when you choose that). On Linux the file dialog needs the desktop portal or zenity,
without one use drag and drop. That's it – after that it starts straight into the
menu. (From the command line: `giana-app --import /path/to/disk.g64`.)

The game checks GitHub once a day for a new version and shows a notice in the main menu; press `U` (or `X` on a
controller) to update. Downloads are verified against the release's `SHA256SUMS.txt` before anything is replaced.
You can switch the check off under OPTIONS → UPDATE CHECK, or start with `--no-update-check`.

## Playing

| Action | Keyboard | Controller |
|---|---|---|
| Move | ← → or A D | D-pad or left stick |
| Duck | ↓ or S | D-pad down |
| Jump | Space, ↑ or W | B / ○ |
| Fire | X, Left Ctrl or Z | A / ✕, right trigger |
| Item (clock or bomb) | C | Y / △ |
| Pause menu | Esc | Start |

Everything can be rebound under KEYBOARD and CONTROLLER in the menu; the hints at the bottom of the screen always show
your current keys, and switch between keyboard and controller icons depending on what you last used. Xbox,
PlayStation and Switch controllers get their own button icons.

Other keys: `F11` or `Alt+Enter` for fullscreen, `M` to mute.

As on a real C64, the item key also fires: on the original the space bar shares a wire with the joystick's fire
button.

### Two players

- **2 PLAYERS** – Giana and Maria take turns on one computer, like in the original.
- **NETWORK → HOST SAME TIME** – both of you play your own game at the same time on two computers. A copy of the
  other game runs alongside, and whoever is out can watch the other one.
- **NETWORK → HOST TAKE TURNS** – one shared game with the original's turns; the other computer watches.

Whoever hosts picks the winner rule: **SCORE** (higher score wins) or **FURTHEST** (higher stage wins, score breaks
a tie). The original names no winner – that's our addition.

For network play the host has to allow **port 47064 (TCP and UDP)** – on Fedora for example
`sudo firewall-cmd --add-port=47064/tcp --add-port=47064/udp`. Both computers need the **same version** of the game;
different builds refuse to play together instead of drifting apart. Computers in the same network find each other
automatically; otherwise type the host's address.

### Options

- **Fullscreen**, and **scaling**: *whole pixels* (sharp, with the C64 border), *no border* (sharp and as large as
  possible – 5× on a 1920×1080 screen) or *fill screen*.
- **Screen: sharp or CRT**: CRT makes the picture look like on a tube television (scanlines, glow, soft pixels).
- **Display 50 Hz**: the game runs at 50 frames per second like a European C64. In fullscreen it picks a 50, 100,
  150 or 200 Hz mode if your monitor has one, so every frame is shown evenly.
- **Sound**: master volume, separate music and effects volume, and menu sounds (soft, classic or off). With both
  sliders at full, the sound is exactly the original mix.
- **Mode: ORIGINAL / ENHANCED**. ORIGINAL is the game as it was. ENHANCED switches on changes a purist might not want,
  currently *smooth motion*: on 60/120/144 Hz monitors the picture is moved smoothly between the C64's frames.

### Authentic mode

**Presentation: MODERN / AUTHENTIC** (on the options page, independent of *Mode*). AUTHENTIC shows exactly what the C64
showed: you start in the Time Warp screen and the title picture of the original (fire continues), then the game's own
title screen (`1` or `2` or fire), and after a game over the original high score table and the original name entry, with
the C64 border at 50 Hz and no menus, hints or messages on top. Everything runs natively in the app, no emulator: the title
sequence, the game's title and the high score screen are ports of the original routines that reproduce the raster effects
of the C64 line by line, and the game itself is the port with the C64's picture geometry. Two players taking turns work as in the original; network play is not offered, and the
enhancements of *Mode* do not apply.

- `F1` (or `F10`, or the controller's Back button) opens a small settings overlay over the paused picture; closing it
  returns to the pure picture. `Esc` does nothing, as on the C64. On the high score screen the original asks for the C64's
  `F1` key ("PRESS F1 TO SAVE HIGHSCORES"): there `F1` is that key, and `F10` or Back still open the overlay.
- Type your name with the keyboard, `Backspace` deletes, `Enter` ends. The table is saved to `authentic_scores.prg` next to
  `settings.conf` when you press `F1` on that question, as the original saved it to disk; it is kept apart from the modern
  high score table.
- AUTHENTIC needs one more file from your disk image (`atg.prg`). If your game data was made by an earlier version, the item
  is greyed out; select it and the game asks for the `.g64` again once.
- Not like the C64: the loading of the game (about 40 seconds on the real drive) is replaced by a second of black screen.
  Details and measurements: [docs/authentic-mode.md](docs/authentic-mode.md).

Settings, controls and high scores live in `~/.config/giana-native/` (Linux and macOS) or `%APPDATA%\giana-native\`
(Windows). The extracted game data lives in `~/.local/share/giana-native/data/` (Linux),
`%APPDATA%\giana-native\data\` (Windows) or `~/Library/Application Support/giana-native/data/` (macOS); a `data`
folder next to the program works too, and `--data DIR` picks any other place.

### ROM modules

Fan-made versions of the game ("ROM hacks": other levels, graphics, colours, music, sometimes a changed engine) can be
played as **ROM modules**, files with the extension `.grom`. Add the hack's own disk image (`.d64`, `.g64`) and the game
turns it into a module; a module is a patch against *your own* extracted game data, so for most hacks it contains none of
the original.

- **Add one:** main menu, *ROM MODULES*, *ADD ROM MODULE...* and choose the hack's disk image or a `.grom` file (or drop
  it on the window, open it with the game, or run `giana-app FILE`). Pick it in the list and choose *PLAY THIS*: the game
  restarts with it. *ORIGINAL* at the top of the list is the unchanged game. Each module has its own high scores.
- **Which hacks work:** hacks that change levels, graphics and colours work as they are. Hacks that bring their own
  engine run on a native port of that engine:
  - the 1989 *Gianna Sisters Construction Kit* hacks (Gitty's Dreams, Power Sisters, Frankie's Horror Trip, The Furry
    Knibble Girls, The Gitty's News),
  - *The Great Giana Sisters 2* by The New Wave,
  - hacks on the 7A3 *Construction-Set* engine (Ridley X, Power Edition 2023, Giana Sisters Junior, Super Marioahner
    Sisters, games made with the construction set).

  Not yet: *Giana Sisters II (Gem Nightmare)* and the *30th Anniversary* preview. For an unknown hack with changed code the
  module page warns that it may not play as intended.
- **Network games:** both computers have to play the same module. If you join a host that plays a module you do not have, the host sends it (a `.grom` holds no original data; it is checked against your own game data) and your game stores it. Pick it under ROM MODULES, then join again - a host never switches your game by itself.
- **Make one:** from a modified game image with `giana-app --make-module INPUT -o OUT.grom --name "NAME"` (optional
  `--author`, `--version`, `--description`, `--title F_2.PRG`). `INPUT` is the hack's disk image, a 64K image or RAM dump, a VICE snapshot
  (`.vsf`), a PRG of disk file `1`, or a `.bps` patch against `ingame.bin`. It needs your original game data and
  prints which game code the module changes. `giana-app --list-modules` shows what is installed.

### Command line

```
giana-app [OPTIONS] [FILE.g64 | FILE.grom]

    FILE.g64           on the first start, use this disk image ("Open with", or drag it onto giana-app.exe)
    FILE.grom          add this ROM module and play it from now on
    --module FILE|ID   play this ROM module in this run only
    --list-modules     list the installed ROM modules and exit
    --make-module INPUT -o OUT.grom --name NAME [--author A] [--version V] [--description D] [--title F_2.PRG]
                       make a ROM module from a modified game and exit
    --import FILE.g64  extract the game data from your original disk image and exit
    --data DIR         use the original game data in DIR (extract into it on first start)
    --stage N          skip the menu and start in stage N (1 to 33)
    --debug            debug keys: F1 lives, F2 invulnerable, F3/F4 previous/next stage
    --check-update     look for a new version now, print the result and exit
    --no-update-check  never look for a new version (also OPTIONS > UPDATE CHECK)
    -h, --help         show this text
    -V, --version      show the version
```

## Building from source

You need Rust (stable, via [rustup](https://rustup.rs/)), a C compiler and the SDL2 development files
(`sudo dnf install SDL2-devel`, `sudo apt install libsdl2-dev` or `brew install sdl2`).

```bash
cargo build --release -p giana-app
./target/release/giana-app          # asks for your .g64 on the first start
cargo test --workspace
```

`./run.sh` builds on first use and starts the game. `cargo build --release -p giana-app --features bundled-sdl`
builds SDL2 from source and links it in (that's how the Windows and macOS releases are made).

### How it's put together

| Crate | What it does |
|---|---|
| `giana-core` | the game logic – the actual port, in fixed 50 Hz steps |
| `giana-render` | draws the picture the way the C64's video chip would |
| `giana-audio` | runs the ported sound driver on a SID chip model |
| `giana-app` | window, input, menus, high scores, network play, options, updates |
| `giana-extract` | reads the game out of your `.g64` (no ROMs needed) |
| `giana-authentic` | test reference only: a cycle exact 6502 and VIC-II model that runs your own copy of the original, against which the native AUTHENTIC screens are compared (not part of the game) |

## How faithful is it?

During development every part of the game logic was compared with the original running in an emulator: walking,
jumping, every enemy type and boss, power-ups, the bonus room, warps, the status bar, the ending and the sound driver's
output, across all 33 stages. Known differences are tiny: after some deaths the restart can be one frame off, and a
few waits are measured rather than computed cycle by cycle. You shouldn't be able to feel any of it while playing.
If you do notice something that behaves differently from the C64 version, please open an issue.

## Contributing

Bug reports, fixes and forks are all welcome. Some ideas if you're looking for something to do:

- support more versions of the original disk (other `.g64` or `.d64` releases)
- a widescreen mode (needs care: enemies appear when the screen edge reaches them)
- signed Windows/macOS builds, Flatpak or other packages
- more controller layouts, Steam Deck testing
- translations of the menus

Please keep the ORIGINAL mode faithful – changes to how the game plays belong behind the ENHANCED switch.

## Legal

This repository and its releases contain **no game data**. You need your own copy of the original game. The code is
a port – a translation – of the original program into Rust; it contains no disk images, ROMs, graphics, music or level
files of the original. "The Great Giana Sisters" and all related names and characters belong to their respective
owners. This project is not affiliated with or endorsed by them.

The screenshots in this README show the original game's graphics and are included for illustration only; the rights remain
with their owners.

## Credits

- *The Great Giana Sisters* (1987) by Rainbow Arts – game design by Armin Gessert, graphics by Manfred Trenz,
  music by Chris Hülsbeck. All the good parts are theirs.
- [resid-rs](https://crates.io/crates/resid-rs), a Rust port of reSID, for the SID sound
- [SDL2](https://www.libsdl.org/) for window, input and audio
- [ureq](https://crates.io/crates/ureq), rustls and friends for the update check

## License

GPL-3.0-or-later, see [LICENSE](LICENSE). The license covers the code of this port, not the original game, whose
rights stay with their owners.
