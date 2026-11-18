# Player's guide

Details that the [README](../README.md) leaves out.

## First start

You need your own copy of the original game as a `.g64` or `.d64` disk image. Any dump of the original disk works, as
long as the game files on it are unmodified: the game compares the program and title files on the disk (not the whole
image file) with the original, so different dumps and both formats are fine. Cracked or modified versions are refused
with a clear message.

The game shows a page that asks for your disk image. Do one of these:

- click **CHOOSE FILE...** (or press Enter) and pick the `.g64` or `.d64`,
- drag the disk image onto the window, or onto the app icon,
- type or paste its path, or let the game search Downloads, Documents and your home folder (only if you choose that).

After that the game starts straight into the menu. From a terminal: `giana-app --import /path/to/disk.g64` (or `.d64`).
On Linux the file dialog needs the desktop portal or zenity; without one, use drag and drop.

**Windows:** the program is not signed; in SmartScreen click "More info" → "Run anyway". **macOS:** the app is not
notarized; open it once, then System Settings → Privacy & Security → "Open Anyway" (older macOS: right-click → Open).

## Two players

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

## Options

OPTIONS is a hub with three pages: VIDEO, SOUND and GENERAL. Every line shows a short note about what it does.

**VIDEO**

- **Fullscreen**, and **picture size**: *no border* (the default: sharp and as large as possible – the game 5× and the
  menus 4× on a 1920×1080 screen), *with border* (sharp, the whole C64 frame, menus 3× on 1920×1080) or *fill screen*.
- **Widescreen: ON / OFF** (off by default; never in the AUTHENTIC style). *On* turns the game into a wide game: the
  picture shows 48 more pixels of the level on each side (24 with *no border* / *fill screen*): 480×272 with the border,
  352×200 without, both about 16:9. The game works from the new edges: enemies, shots and items are created beyond the
  new right edge and removed beyond the new edges, so nothing pops in or out in view, and they walk, fall and turn in the
  extra columns. Giana can walk 24 pixels into each extra area too (the part that every picture size shows), and the
  extra areas are part of the level like the middle: she stands on their ground, hits the blocks and takes the diamonds
  there, bridges crumble under her, and enemies there hit her and are hit by her shots. **This changes how the game plays**, and it can make a stage easier or harder: you see enemies and
  gaps earlier, but enemies also stay around longer, so in the busiest places (stages 27 to 32 of the original) a few
  more of them are lost for lack of a free sprite, and the bosses start 48 pixels further out. The levels, the scroll and
  the rooms (statue and bonus rooms play like on the C64, with blank sides) stay as they are. *Off* is the 4:3 picture
  and the game exactly as on the C64. Beyond the ends of a level the border colour shows. Switching the setting during a
  game takes effect at once; in a network game the host's setting counts on both computers, and a saved game keeps the
  setting it was started with. (Up to 0.5.0 there was a third setting, VIEW ONLY; it is ON now.)
- **Screen filter: sharp or CRT TV**: CRT TV makes the picture look like on a tube television (scanlines, glow, soft pixels).
- **50 Hz mode**: the game runs at 50 frames per second like a European C64. In fullscreen it picks a 50, 100,
  150 or 200 Hz mode if your monitor has one, so every frame is shown evenly.
- **Smooth motion: ON / OFF** (off by default, the original is the default). On 60/120/144 Hz monitors the picture is
  moved smoothly between the C64's frames. The game itself is not changed.

**SOUND**: master volume, separate music and effects volume, and menu sounds (soft, classic or off). With both
sliders at full, the sound is exactly the original mix.

**GENERAL**: the *winner rule* (score or furthest), the *style* (modern or authentic, see below) the *update check*
(on start or off) and *check for updates* (ask now, and install what it finds).

## Authentic mode

**Style: MODERN / AUTHENTIC** (OPTIONS → GENERAL, independent of *Smooth motion*). AUTHENTIC shows exactly what the C64
showed: you start in the Time Warp screen and the title picture of the original (fire continues), then the game's own
title screen (`1` or `2` or fire), and after a game over the original high score table and the original name entry, with
the C64 border at 50 Hz and no menus, hints or messages on top. Everything runs natively in the app, no emulator: the title
sequence, the game's title and the high score screen are ports of the original routines that reproduce the raster effects
of the C64 line by line, and the game itself is the port with the C64's picture geometry. Two players taking turns work as in the original; network play is not offered, and the
smooth motion does not apply.

- `F1` (or `F10`, or the controller's Back button) opens a small settings overlay over the paused picture; closing it
  returns to the pure picture. `Esc` does nothing, as on the C64. On the high score screen the original asks for the C64's
  `F1` key ("PRESS F1 TO SAVE HIGHSCORES"): there `F1` is that key, and `F10` or Back still open the overlay.
- Type your name with the keyboard, `Backspace` deletes, `Enter` ends. The table is saved to `authentic_scores.prg` next to
  `settings.conf` when you press `F1` on that question, as the original saved it to disk; it is kept apart from the modern
  high score table.
- AUTHENTIC needs one more file from your disk image (`atg.prg`). If your game data was made by an earlier version, the item
  is greyed out; select it and the game asks for your disk image (`.g64` or `.d64`) again once.
- Not like the C64: the loading of the game (about 40 seconds on the real drive) is replaced by a second of black screen.
 

## Stopping in the middle of a game

Close the program (window close, `Alt+F4`, or the quit key of your system) while a game runs or is paused, and the game is
kept: next time the main menu starts with **CONTINUE 1 PLAYER** and/or **CONTINUE 2 PLAYERS** (the note under each says
stage and score, or both scores of the two players), and the game comes back exactly where you left it, paused, with the
music where it was. One player and two players taking turns have a kept game each, so a game with company is not lost
by playing alone in between; the original and every ROM module have their own. The pause menu says so under RESUME.

- A kept game is a suspend point, not a save game: it is gone once you continue it, and when you start a new game with
  the same number of players (1 PLAYER and 2 PLAYERS say *replaces the saved game* only if that one exists; the other
  kept game stays). The points of a game that you stop this way count when you finish it.
- **RESTART STAGE** costs a life: Giana is hit as by an enemy and the stage starts again, exactly as after any lost life
  (on the last life the game is over). In a network game it is there too and reaches the other computer like a button
  press; taking turns, only the player whose turn it is can use it.
- **RESTART GAME** starts a new game of the same kind (stage 1, all lives); the game that ran is not kept and its points do
  not count. Not in network games.
- **QUIT TO TITLE** gives the game up as before, nothing is kept. Network games and the AUTHENTIC style (it stays like the
  C64) are not kept either.
- A kept game belongs to the game data (module) it was made with and survives program updates. It is discarded, with a
  short notice, if the module was replaced or if an update changed what a saved game contains (rare).

## Where things are stored

Settings, controls, high scores and the kept game (`suspend-1p.bin` and `suspend-2p.bin`, or `suspend-<module>-1p.bin` and `-2p.bin`) live in `~/.config/giana-native/` (Linux and macOS) or `%APPDATA%\giana-native\`
(Windows). The extracted game data lives in `~/.local/share/giana-native/data/` (Linux),
`%APPDATA%\giana-native\data\` (Windows) or `~/Library/Application Support/giana-native/data/` (macOS); a `data`
folder next to the program works too, and `--data DIR` picks any other place.


## ROM modules

Fan-made versions of the game ("ROM hacks": other levels, graphics, colours, music, sometimes a changed engine) can be
played as **ROM modules**, files with the extension `.grom`. Add the hack's own disk image (`.d64`, `.g64`) and the game
turns it into a module; a module is a patch against *your own* extracted game data, so for most hacks it contains none of
the original.

- **Add one:** main menu, *ROM MODULES*, *ADD ROM MODULE...* and choose the hack's disk image or a `.grom` file (or drop
  it on the window, open it with the game, or run `giana-app FILE`). Pick it in the list and choose *PLAY*: the game
  switches to it right away (same window, back at the title; no restart). *ORIGINAL* at the top of the list is the unchanged game. Each module has its own high scores.
- **Which hacks work:** hacks that change levels, graphics and colours work as they are. Hacks that bring their own
  engine run on a native port of that engine:
  - the 1989 *Gianna Sisters Construction Kit* hacks (Gitty's Dreams, Power Sisters, Frankie's Horror Trip, The Furry
    Knibble Girls, The Gitty's News),
  - *The Great Giana Sisters 2* by The New Wave,
  - hacks on the 7A3 *Construction-Set* engine (Ridley X, Power Edition 2023, Giana Sisters Junior, Super Marioahner
    Sisters, games made with the construction set) and *Giana Sisters II (Gem Nightmare)* on its older variant.

  For an unknown hack with changed code the module page warns that it may not play as intended.
- **30th Anniversary preview** (*Giana Sisters 30th Anniversary Extended Preview*, demo 2.0, CSDb release 168405): a
  fan sequel with its own engine, ported natively like the original. *ROM MODULES*, *ADD 30A PREVIEW...* and choose its
  `.d64` (or add it like a module). The disk is checked and kept in the modules folder of this computer only: it is
  never sent to another player. Works: the intro, the title (keys *1*, *2*, *H*), the stages with their enemies,
  boss and power-ups, the treasure rooms, deaths, stage clear, the stage select and saved-game menu of the title, the
  end of the game, game over and the high-score table with name entry (saved in `c30a_scores.bin` next to the
  settings). *P* opens the preview's own pause menu (save and quit); Escape, F1, F10 or the controller's Back/Start open
  the app's menu (leave the preview there). Volume, picture size, CRT, fullscreen and SMOOTH MOTION apply; widescreen
  and network play do not.
- **Network games:** both computers have to play the same module. If you join a host that plays a module you do not have, the host sends it (a `.grom` holds no original data; it is checked against your own game data) and your game stores it. Pick it under ROM MODULES, then join again - a host never switches your game by itself.
- **Make one:** from a modified game image with `giana-app --make-module INPUT -o OUT.grom --name "NAME"` (optional
  `--author`, `--version`, `--description`, `--title F_2.PRG`). `INPUT` is the hack's disk image, a 64K image or RAM dump, a VICE snapshot
  (`.vsf`), a PRG of disk file `1`, or a `.bps` patch against `ingame.bin`. It needs your original game data and
  prints which game code the module changes. `giana-app --list-modules` shows what is installed.

## Command line

```
USAGE:
    giana-app [OPTIONS] [FILE]

FILE is your disk image (.g64, .d64) or a ROM module (.grom). Dropping it on the program works too.

FIRST START:
    --import FILE      import the game data from your disk image and exit
    --data DIR         keep the game data in DIR

PLAY:
    --stage N          start in stage N (1 to 33), without the menu
    --module FILE|ID   play this ROM module this time only
    --debug            F1 lives, F2 invulnerable, F3/F4 previous/next stage

ROM MODULES (these exit when done):
    --list-modules     list the installed modules
    --add-module FILE  add a module or a hack's disk image
    --make-module INPUT -o OUT.grom --name NAME [--author A] [--version V] [--description D] [--title F_2.PRG]
                       make a module from a modified game: a hack's disk image, a 64K image or RAM dump,
                       a PRG of disk file 1, a VICE snapshot (.vsf) or a .bps patch. Needs your game data.

UPDATES:
    --check-update     check for updates and exit
    --no-update-check  never check for updates (also OPTIONS > GENERAL > UPDATE CHECK)

OTHER:
    -h, --help         show this text
    -V, --version      show the version
```

## Linux install details


The package works on current and older distributions (needs glibc 2.34 or newer: Ubuntu 22.04, Debian 12, Fedora 35, RHEL 9
and newer) and needs no extra packages: SDL2 is built in and picks X11, Wayland, PipeWire, PulseAudio or ALSA at run time.
Unpack it, open a terminal in the folder and run:

```sh
./install.sh
```

No root needed. This puts the game in `~/.local/opt/giana-sisters`, adds the command `giana-sisters` and an entry
"Giana Sisters" in your applications menu, and registers `.g64` and `.grom` files so "Open with" works. Running it again
updates in place; the in-game updater works too. Optional: `./install.sh --g64 /path/to/disk.g64` imports your disk image
right away, `./install.sh --modules DIR` copies ROM modules (`*.grom`).

To uninstall, run `~/.local/opt/giana-sisters/uninstall.sh` (or `./uninstall.sh` from the unpacked folder). It removes exactly
what the installer created (listed in `~/.local/share/giana-native/install-manifest`) and keeps your settings, high scores,
modules and game data; add `--purge` to delete those too (it asks first, `--yes` skips the question).
You can also skip the installer and just start `./giana-app`.


## Updates

The game asks GitHub for a new version at every start (in the background, the game does not wait) and shows a notice in
the main menu; press `U` (or `X` on a controller) to update. OPTIONS → GENERAL → CHECK FOR UPDATES asks right away, also
when the automatic check is off, and says what came out; when it found a new version, select it again to install it.
Downloads are verified against the release's `SHA256SUMS.txt` before anything is replaced. The new version is used from
the next start. You can switch the automatic check off under OPTIONS → GENERAL → UPDATE CHECK, or start with
`--no-update-check`.

