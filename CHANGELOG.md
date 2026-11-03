# Changelog

## [0.2.1]

- Giana Sisters II "Gem Nightmare" (The Seven-A-Three, 1993) plays: an older version of the construction-set engine,
  its stages from the image and its own stage files.
- ROM MODULES: favorites (F / controller Y) stay on top; sort by POPULAR (default, by how often the hacks were
  downloaded on CSDb), NAME or LAST PLAYED (O / controller X); the highest stage reached is shown per module.
- Linux: the package runs on nearly every current distribution (glibc 2.34 or newer, SDL built in, nothing to install)
  and comes with `install.sh` (start menu entry, no root needed, `--modules DIR`, `--g64 FILE`) and `uninstall.sh`.
- `giana-app --add-module FILE` adds a module or a hack's disk image without playing it.
- The sound pauses while the file dialog of ADD ROM MODULE is open (the music looped).
- The update check offers a new version only once its package for your system is uploaded; releases upload each
  system's package as soon as it is built.

## [0.2.0]

Supported fan hacks (load your own disk image under ROM MODULES; a hack that needs an engine of its own runs on a native
port of that engine):

- the seven 1989 "Gianna Sisters Construction Kit" hacks (Gitty's Dreams 1 and 2, Power Sisters, Frankie's Horror Trip 1
  and 2, The Furry Knibble Girls, The Gitty's News): per stage time limits, warps back, boss and final bonus
- The Great Giana Sisters 2 by The New Wave (1989): its start menu with the three ways through 64 stages, stages loaded
  from the disk while you play (the old stages are rebuilt from your own game data, the module does not carry them)
- hacks on the 7A3 Great Giana Sisters Construction-Set engine (Ridley X, Power Edition 2023, Giana Sisters Junior, Super
  Marioahner Sisters, games made with the construction set): its stage files, swimming, moving platforms and elevators,
  ladders, its enemies, chains and bosses, the homing shot
- not yet: Giana Sisters II "Gem Nightmare" (an older construction-set engine) and the 30th Anniversary preview

- OPTIONS → SCREEN: SHARP / CRT. The CRT look gives soft pixels, scanlines that follow the C64 rows, an aperture
  grille, a little glow and darker edges; it needs no shaders and gets subtler in small windows.
- ROM modules: play fan hacks of the game as `.grom` files (patches against your own game data, no original data inside).
  Main menu *ROM MODULES* to add, pick and remove them; `--module`, `--make-module`, `--list-modules`; a `.grom` file
  can be opened with the game or dropped on the window. Each module has its own high scores, and a network game needs
  the same module on both computers (the network protocol changed, so both computers need this version).
- Network games: a guest that lacks the host's ROM module receives it from the host (size and SHA-256 checked, at most
  1 MB, validated against the guest's own game data) and stores it; the player picks it under ROM MODULES. Handshake is now `GIANANT7`.

- **Presentation: MODERN / AUTHENTIC** on the options page (independent of *Mode*). AUTHENTIC shows what the C64 showed, all
  of it running natively in the app (no emulator): the Time Warp screen with its raster bars, the title picture and its
  music, the game's own title screen (`1`, `2` or fire), the game with the C64's picture geometry, and after a game over the
  original high score table with the raster colour bars and the name entry. Two players take turns as in the original;
  C64 border, 50 Hz, no menus or hints. `F1`/`F10`/Back opens a settings overlay, `Esc` does nothing. High scores of
  AUTHENTIC go to `authentic_scores.prg` (saved with the C64's F1 like the original saved to disk). Needs the new optional
  data file `atg.prg`: existing data folders stay valid, the item is greyed out until the disk image is read again
  (offered on the options page). The cycle exact C64 model (crate `giana-authentic`) is only a reference for the tests.
  See `docs/authentic-mode.md`.

## [0.1.2]

- Fixed: the very first start (no game data yet) crashed before showing anything, on every system. The
  first-start page drew its text with the game's own font, which is part of the data it asks for; it now uses a
  font of its own until the data is there. A CI check starts the game without data so this can't come back.
- First start: **CHOOSE FILE...** opens your system's file dialog (selected by default, just press Enter); dropping
  the file on the window, typing or pasting its path and the search still work. The search only runs when chosen.
- A `.g64` given to the program (Open with, dragging it onto the app or `giana-app.exe`) is imported right away.
- macOS: `.g64` is a document type of the app, the bundle is ad-hoc signed, and the install notes describe
  "Open Anyway" for macOS 15 and later. CMD+V pastes on the first-start page.
- Errors that stop the game are shown in a message box and saved to `last-error.txt` in the settings folder,
  instead of the game silently closing when started without a console.
- Screenshots in the README.

## [0.1.1]

- App icon (original pixel art, `assets/icon`): macOS app, Windows program, window and taskbar, Linux archive

## [0.1.0]

First release: a native Rust port of "The Great Giana Sisters" (C64, Rainbow Arts, 1987). Not an emulator: the game
logic was ported routine by routine from the original 6502 program, the renderer draws the picture the way the C64
video chip would, and the sound comes from a port of the original sound driver feeding a SID model (resid-rs).
No game data is included; you need your own `.g64` disk image.

Added on top of the original game:

- main menu, pause menu and quit confirmation
- high scores (the ten best games of your computer, with names)
- two players on one computer (Giana and Maria take turns, as in the original)
- network play for two computers: at the same time, or taking turns
- options: fullscreen, volume, scaling, 50 Hz display mode, winner rule of 2-player games
- rebindable keyboard and game controller controls, stage select (`--stage N`)
- prebuilt packages for Linux x86_64, Windows x86_64 and macOS (Apple Silicon and Intel)
