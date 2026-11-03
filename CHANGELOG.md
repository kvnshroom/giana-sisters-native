# Changelog

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
