# Changelog

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
