## What and why

<!-- What does this change, and why? Link the issue if there is one: "Fixes #123". -->

## How it was tested

<!-- Which stage, which settings, which system. Screenshots or a short video help for anything visible. -->

## Checklist

- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets` pass without warnings
- [ ] ORIGINAL play is unchanged, or the change is a fix that makes the port match the C64 more closely
- [ ] Anything that changes how the game plays is behind an option that is off by default
- [ ] No game data, disk images, ROMs or extracted graphics/music are added
- [ ] Player-visible changes have a line under `[Unreleased]` in `CHANGELOG.md`
