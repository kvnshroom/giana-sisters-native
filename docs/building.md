# Building and internals

## Building from source

You need Rust (stable, via [rustup](https://rustup.rs/)), a C and C++ compiler (C++17; the SID model libresidfp in
`crates/residfp-sys` is C++) and the SDL2 development files (`sudo dnf install SDL2-devel gcc-c++`,
`sudo apt install libsdl2-dev g++` or `brew install sdl2`). `--no-default-features` builds without C++, with the older
SID model resid-rs; `GIANA_SID=resid` picks that model at run time in a normal build.

```bash
cargo build --release -p giana-app
./target/release/giana-app          # asks for your .g64 on the first start
cargo test --workspace
```

`./run.sh` builds on first use and starts the game. `cargo build --release -p giana-app --features bundled-sdl`
builds SDL2 from source and links it in (that's how the Windows and macOS releases are made).

The screenshots in the README are the port's own output, drawn without a window by the examples `menu_shot`, `shot`
and `boss_shot` of `giana-app` (they write PPM files; the README uses them scaled 3× as PNG).

## How it's put together

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

