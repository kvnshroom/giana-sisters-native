# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each section is also the text of its GitHub release.

## [Unreleased]

## [0.6.1] - 2026-11-18

### Changed

- Linux install: `.d64` disk images (most ROM hacks and the 30th Anniversary preview come as one) open with the game,
  like `.g64` and `.grom` already did: double-click one, or *Open with*, to add it.
- README: adding a ROM hack is one step (*ROM MODULES*, *ADD ROM MODULE...*, or drop its disk image on the window).

## [0.6.0] - 2026-11-18

Network play needs the new version on both computers. Suspended games from 0.5.0 cannot be continued (the saved game
layout changed).

### Added

- *ROM MODULES*, *ADD 30A PREVIEW...*: plays the *30th Anniversary* preview (demo 2.0) from your own `.d64`, on a native
  port of its engine, checked frame by frame against the original: intro, title, all 16 stages with their enemies and
  boss, treasure rooms, its own pause menu (*P*, with save and quit), stage select, the saved-game menu, the ending and
  the high scores. Smooth scrolling on displays that are not a multiple of 50 Hz (SMOOTH MOTION). The disk stays on
  your computer; widescreen and network play are off for it. It leads *SORT: POPULAR* (the most downloaded hack).

### Changed

- WIDESCREEN is *ON* or *OFF* now. *ON* plays like a real widescreen game: Giana can walk into the extra areas on both
  sides, enemies come and go at the edges you see, and the menu notes that this changes how the game plays (stages can
  get easier or harder). *OFF* stays the default and plays exactly like the C64.
- Shorter menus: a note only where a choice has a consequence you cannot see (it costs a life, replaces the saved game,
  goes online), one line each. Shorter first start page, plainer error and update messages, a shorter `--help`.
- The SID chip model is now reSIDfp (libresidfp 1.2.3, the model of sidplayfp and of current VICE) instead of
  resid-rs (reSID 0.16 from 2004). Measured against VICE on the same SID writes, the music and effects come out clearly
  closer: the filter (resonance and cutoff sweeps) and the pulse+triangle sounds above all. The sound is about 3 dB
  quieter than before. `GIANA_SID=resid` brings back the old model; building needs a C++ compiler now
  (`--no-default-features` builds without one, with the old model).

### Removed

- WIDESCREEN *VIEW ONLY* (the wide picture with the C64's rules); a saved *VIEW ONLY* setting reads as *ON*.

### Fixed

- Widescreen ON: the extra areas are part of the level now: Giana hits the blocks and takes the diamonds there, bridges
  crumble, debris falls and items rise through them like in the middle, and what changed there stays changed when the
  level scrolls it on. Enemies left of the picture's old edge are seen in the right direction by Giana's homing shot and
  by the enemies of the 7A3 hacks that turn towards her, and a moving platform of those hacks carries her into the left
  extra area like up to the old edge.
- Widescreen: the left extra area showed old columns (stage 5), the right one wrong columns at the end of a stage (the
  castle of stage 1), stage 33 was treated as a bonus room (enemies and the crystal popped in and out), the dragon flame
  of Gem Nightmare and Power Edition vanished while still visible, and the extra areas stayed black for a moment after a
  bonus room.
- After the death jump, a stage restart could take one step too many.
- Windows: switching to fullscreen could end the game. A frame that cannot be drawn while the display changes is now
  skipped, and the game uses Direct3D 11.
- Windows: the sound chip model did not link with the MinGW toolchain.

## [0.5.0] - 2026-11-08

Network play needs 0.5.0 on both computers (0.4.x refuses to play with it).

### Added

- OPTIONS → GENERAL → *CHECK FOR UPDATES* asks right away (also with the automatic check off), says what came out, and
  installs a new version when selected again.
- Network games start only when both players are ready: after connecting, each presses jump or fire (*PRESS JUMP OR
  FIRE WHEN READY*), and the game runs once both did. A button still held from the menu does not count.
- Pause menu: *RESTART STAGE* (costs a life: Giana is hit as by an enemy, so the stage starts again like after any lost
  life; also in network games, where it travels with the input) and *RESTART GAME* (a new game of the same kind, not in
  network games).
- A Linux package for 64-bit ARM (`linux-aarch64`, e.g. Raspberry Pi 4/5 with Raspberry Pi OS or Batocera); the update
  check offers it on those systems too.

### Changed

- The update check runs at every start instead of once a day (UPDATE CHECK: *ON START* / *OFF*).
- `update_check = false` (or `no`, `0`) in the settings file now means off, like `off`.
- PICTURE SIZE *NO BORDER* is the default for new installs (existing settings are kept). It only cuts empty border, the
  game itself is unchanged.
- *NO BORDER* and *FILL SCREEN* also make the menus and the title bigger: they cut the empty rows at the top and bottom of
  the border, so on a 1920×1080 screen the menus are 4× instead of 3× (8× instead of 7× on 4K), still sharp.

### Removed

- Network games no longer show a line about the other player (score, lives, stage, whose turn) over the game: with
  *NO BORDER* it covered the last rows of the picture. Only *OTHER PLAYER PAUSED* is still shown while it holds the game.

## [0.4.1] - 2026-11-07

### Changed

- The first-start import takes a `.g64` or a `.d64` of the original disk, and any dump of it: the game now compares the
  program and title files on the disk with the original instead of the hash of one particular image file. Other dumps
  (for example the C64 Preservation Project one) and `.d64` files work; the copy protection tracks do not matter.
- Cracked or modified versions are refused with "This is a modified or cracked version. Please use a dump of the
  original disk."; a file that is not a disk image, or a disk without the game, gets its own message.
- SEARCH THIS COMPUTER looks at `.g64` and `.d64` files, tells you when it only found a modified version, and never runs
  anything from a disk it has not recognised.
- Sound closer to VICE: the SID model now band-limits its output (reSID "resampling", like VICE) instead of picking
  single samples, which folded the chip's high overtones back into the audible range and made the top end harsh.
- MENU SOUNDS *SOFT*: the ticks use a triangle wave (softer; they stay well below *CLASSIC*).

### Fixed

- The sound chip now runs on the C64's own frame (19656 cycles, 50.125 Hz), the rate the game already ran at. Before,
  in the AUTHENTIC style the queued sound frames piled up: after a few minutes the sound lagged up to half a second
  behind and a frame of sound was dropped every 8 seconds.

## [0.4.0] - 2026-11-06

### Added

- WIDESCREEN (OPTIONS > VIDEO, off by default): about 16:9, with 48 more pixels of the level on each side (24 with
  *no border* / *fill screen*); the status bar spreads over the full width.
  - *ON*: enemies, shots and items come and go at the new edges, so nothing pops in or out in view. Giana, the levels
    and the bosses stay as on the C64; in the busiest places a few more enemies are lost for lack of a free sprite.
  - *VIEW ONLY*: the wider picture, and the game plays exactly like on the C64.
  - Not available in the AUTHENTIC style.

## [0.3.0] - 2026-11-05

### Added

- Stop in the middle of a game: closing the program saves it, and CONTINUE 1 PLAYER / CONTINUE 2 PLAYERS in the main
  menu bring it back paused, exactly where it was (one saved game per player count and ROM module; QUIT TO TITLE ends a
  game without saving). Saved games survive updates.

### Changed

- Choosing a ROM module switches the game at once; the program no longer restarts.
- Menus: OPTIONS is split into VIDEO, SOUND and GENERAL; CONTROLS holds KEYBOARD and CONTROLLER; plainer names
  (PICTURE SIZE, SCREEN FILTER, 50 HZ MODE, SMOOTH MOTION as one switch, WINNER RULE, STYLE, UPDATE CHECK) and a short
  note for every line. The pause menu is RESUME, CONTROLS, OPTIONS, QUIT TO TITLE.
- First start: shorter, clearer messages when the disk image cannot be used; a shorter `--help`; a lean README with
  the details in docs/guide.md.

### Fixed

- ROM hacks closer to the real ones (checked against the hacks' own code in an emulator):
  - Construction Kit hacks: after a death Giana restarts at the hack's own half-way point (she often appeared five
    columns off, in a wall or on an enemy, and died again at once); lives and stage numbers come from the hack.
  - Construction-set hacks: the statue and the dragon stage are the hack's own; Super Marioahner Sisters jumps with fire;
    the secret key no longer skips a stage.
  - The construction-set game has its music and sound again.
  - Gem Nightmare: the importer keeps 56 bytes of the hack's code it used to overwrite.
- The original game: the stage exit after a warp and the restart banner follow the C64 more closely.

## [0.2.1] - 2026-11-03

### Added

- Giana Sisters II "Gem Nightmare" (The Seven-A-Three, 1993) plays: an older version of the construction-set engine,
  its stages from the image and its own stage files.
- ROM MODULES: favorites (F / controller Y) stay on top; sort by POPULAR (default, by how often the hacks were
  downloaded on CSDb), NAME or LAST PLAYED (O / controller X); the highest stage reached is shown per module.
- `giana-app --add-module FILE` adds a module or a hack's disk image without playing it.

### Changed

- Linux: the package runs on nearly every current distribution (glibc 2.34 or newer, SDL built in, nothing to install)
  and comes with `install.sh` (start menu entry, no root needed, `--modules DIR`, `--g64 FILE`) and `uninstall.sh`.
- The update check offers a new version only once its package for your system is uploaded; releases upload each
  system's package as soon as it is built.

### Fixed

- The sound pauses while the file dialog of ADD ROM MODULE is open (the music looped).

## [0.2.0] - 2026-11-03

The network protocol changed, so both computers need this version.

### Added

- Supported fan hacks (load your own disk image under ROM MODULES; a hack that needs an engine of its own runs on a
  native port of that engine):
  - the seven 1989 "Gianna Sisters Construction Kit" hacks (Gitty's Dreams 1 and 2, Power Sisters, Frankie's Horror
    Trip 1 and 2, The Furry Knibble Girls, The Gitty's News): per stage time limits, warps back, boss and final bonus
  - The Great Giana Sisters 2 by The New Wave (1989): its start menu with the three ways through 64 stages, stages
    loaded from the disk while you play (the old stages are rebuilt from your own game data, the module does not carry
    them)
  - hacks on the 7A3 Great Giana Sisters Construction-Set engine (Ridley X, Power Edition 2023, Giana Sisters Junior,
    Super Marioahner Sisters, games made with the construction set): its stage files, swimming, moving platforms and
    elevators, ladders, its enemies, chains and bosses, the homing shot
  - not yet: Giana Sisters II "Gem Nightmare" (an older construction-set engine) and the 30th Anniversary preview
- ROM modules: play fan hacks of the game as `.grom` files (patches against your own game data, no original data
  inside). Main menu *ROM MODULES* to add, pick and remove them; `--module`, `--make-module`, `--list-modules`; a
  `.grom` file can be opened with the game or dropped on the window. Each module has its own high scores, and a network
  game needs the same module on both computers.
- Network games: a guest that lacks the host's ROM module receives it from the host (size and SHA-256 checked, at most
  1 MB, validated against the guest's own game data) and stores it; the player picks it under ROM MODULES. Handshake is
  now `GIANANT7`.
- OPTIONS → SCREEN: SHARP / CRT. The CRT look gives soft pixels, scanlines that follow the C64 rows, an aperture
  grille, a little glow and darker edges; it needs no shaders and gets subtler in small windows.
- **Presentation: MODERN / AUTHENTIC** on the options page (independent of *Mode*). AUTHENTIC shows what the C64 showed,
  all of it running natively in the app (no emulator): the Time Warp screen with its raster bars, the title picture and
  its music, the game's own title screen (`1`, `2` or fire), the game with the C64's picture geometry, and after a game
  over the original high score table with the raster colour bars and the name entry. Two players take turns as in the
  original; C64 border, 50 Hz, no menus or hints. `F1`/`F10`/Back opens a settings overlay, `Esc` does nothing. High
  scores of AUTHENTIC go to `authentic_scores.prg` (saved with the C64's F1 like the original saved to disk). Needs the
  new optional data file `atg.prg`: existing data folders stay valid, the item is greyed out until the disk image is
  read again (offered on the options page). The cycle exact C64 model (crate `giana-authentic`) is only a reference for
  the tests. See `docs/authentic-mode.md`.

## [0.1.2] - 2026-11-03

### Added

- First start: **CHOOSE FILE...** opens your system's file dialog (selected by default, just press Enter); dropping
  the file on the window, typing or pasting its path and the search still work. The search only runs when chosen.
- A `.g64` given to the program (Open with, dragging it onto the app or `giana-app.exe`) is imported right away.
- macOS: `.g64` is a document type of the app, the bundle is ad-hoc signed, and the install notes describe
  "Open Anyway" for macOS 15 and later. CMD+V pastes on the first-start page.
- Screenshots in the README.

### Changed

- Errors that stop the game are shown in a message box and saved to `last-error.txt` in the settings folder,
  instead of the game silently closing when started without a console.

### Fixed

- The very first start (no game data yet) crashed before showing anything, on every system. The first-start page drew
  its text with the game's own font, which is part of the data it asks for; it now uses a font of its own until the
  data is there. A CI check starts the game without data so this can't come back.

## [0.1.1] - 2026-11-03

### Added

- App icon (original pixel art, `assets/icon`): macOS app, Windows program, window and taskbar, Linux archive.

## [0.1.0] - 2026-11-03

First release: a native Rust port of "The Great Giana Sisters" (C64, Rainbow Arts, 1987). Not an emulator: the game
logic was ported routine by routine from the original 6502 program, the renderer draws the picture the way the C64
video chip would, and the sound comes from a port of the original sound driver feeding a SID model (resid-rs).
No game data is included; you need your own `.g64` disk image.

### Added

On top of the original game:

- main menu, pause menu and quit confirmation
- high scores (the ten best games of your computer, with names)
- two players on one computer (Giana and Maria take turns, as in the original)
- network play for two computers: at the same time, or taking turns
- options: fullscreen, volume, scaling, 50 Hz display mode, winner rule of 2-player games
- rebindable keyboard and game controller controls, stage select (`--stage N`)
- prebuilt packages for Linux x86_64, Windows x86_64 and macOS (Apple Silicon and Intel)

[Unreleased]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.6.1...HEAD
[0.6.1]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/kvnshroom/giana-sisters-native/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kvnshroom/giana-sisters-native/releases/tag/v0.1.0
