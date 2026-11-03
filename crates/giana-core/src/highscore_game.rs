//! The high score screen as the game uses it: the two tables, what the game over code hands over (`GameOver`), and what the
//! screen hands back (the tables, the sound driver). The screen itself is `highscore::Hs`.

use crate::highscore::{Hs, VicState};
use crate::title::LineState;

/// "ALL TIME GREATEST" (loaded from and saved to disk) and "TODAYS GREATEST" (RAM only) in the game program.
pub const ALL_TIME_AT: usize = 0x36F7;
pub const TODAY_AT: usize = 0x376F;
/// five records of 24 bytes
pub const TABLE_LEN: usize = 120;

/// Is this a high score file as the game writes it (`.prg`, load address `$36F7`, 120 bytes)?
pub fn plausible_scores(prg: &[u8]) -> bool {
    prg.len() == TABLE_LEN + 2 && prg[0] == 0xF7 && prg[1] == 0x36
}

/// The two high score tables. They live as long as the game program does: "today's" table fills up over the games of one
/// session, the "all time" table is what `authentic_scores.prg` (or the disk file `ATG`) holds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tables {
    pub all_time: [u8; TABLE_LEN],
    pub today: [u8; TABLE_LEN],
}

impl Tables {
    /// The tables as the program image has them (an older copy of `ATG` and the five fixed entries of today's table).
    pub fn from_image(image: &[u8]) -> Tables {
        let mut t = Tables { all_time: [0; TABLE_LEN], today: [0; TABLE_LEN] };
        t.all_time.copy_from_slice(&image[ALL_TIME_AT..ALL_TIME_AT + TABLE_LEN]);
        t.today.copy_from_slice(&image[TODAY_AT..TODAY_AT + TABLE_LEN]);
        t
    }

    /// The game's `LOAD "ATG"` at start: a file as `plausible_scores` describes replaces the "all time" table (anything else
    /// is ignored).
    pub fn with_disk_file(mut self, prg: &[u8]) -> Tables {
        if plausible_scores(prg) {
            self.all_time.copy_from_slice(&prg[2..]);
        }
        self
    }

    /// The file `SAVE "ATG"` writes: the "all time" table with its load address.
    pub fn all_time_prg(&self) -> Vec<u8> {
        let mut v = vec![ALL_TIME_AT as u8, (ALL_TIME_AT >> 8) as u8];
        v.extend_from_slice(&self.all_time);
        v
    }

    fn put(&self, ram: &mut [u8]) {
        ram[ALL_TIME_AT..ALL_TIME_AT + TABLE_LEN].copy_from_slice(&self.all_time);
        ram[TODAY_AT..TODAY_AT + TABLE_LEN].copy_from_slice(&self.today);
    }
}

/// What the game over code hands to the high score routine (the state of the game program at `$0C4E`).
pub struct GameOver<'a> {
    /// `$0435..$0437`: the score of the player who is out, BCD, lowest pair first
    pub score_bcd: [u8; 3],
    /// `$20`: the stage the player was in, and the BCD stage number `$9FAE[$20]` the table shows
    pub stage_index: u8,
    pub stage_bcd: u8,
    /// `$1568`: 0 Giana, 1 Maria
    pub player: u8,
    /// screen RAM `$C800` (1000 bytes), colour RAM (1000 bytes) and the character set `$C000` (2048 bytes) as the video chip
    /// shows them now
    pub screen: &'a [u8],
    pub color: &'a [u8],
    pub charset: &'a [u8],
    /// the 64K of the game program with the sound driver's variables as they are now (`Driver::mem`)
    pub driver_mem: &'a [u8],
    /// fine scroll `$D016 & 7` the game left, border and background colours
    pub xscroll: u8,
    pub border: u8,
    pub bg: [u8; 3],
}

impl<'a> GameOver<'a> {
    /// From the game state of the port, when the last life of a player is gone (`game_over`).
    pub fn from_game(game: &'a crate::state::GameState) -> GameOver<'a> {
        GameOver {
            score_bcd: game.score_bcd,
            stage_index: game.stage.clamp(0, 255) as u8,
            stage_bcd: game.bcd_table(game.stage as usize),
            player: game.player,
            screen: &game.screen_ram,
            color: &game.color_ram,
            charset: &game.sprite_mem[..0x800],
            driver_mem: &game.sound.mem,
            xscroll: game.scroll_fine,
            border: 0,
            bg: [game.en.vic_bg0, game.en.vic_bg1, game.vic_bg2],
        }
    }
}

impl Hs {
    /// Game over: the game over code has reached `$325D`. Starts at the top of the next frame (the beam at raster line 0).
    /// `draw` gets the raster lines (see `Hs::enter`). The raster position the original reaches `$325D` at depends on its
    /// interrupt chain and is not known to the port; the screen is the same from any position, only the exact place of the
    /// colour bar edges in the first frames moves.
    pub fn begin(over: &GameOver, tables: &Tables, draw: Box<dyn FnMut(usize, &LineState)>) -> Hs {
        let mut ram = over.driver_mem.to_vec();
        ram.resize(65536, 0);
        tables.put(&mut ram);
        let n = over.charset.len().min(0x800);
        ram[0xC000..0xC000 + n].copy_from_slice(&over.charset[..n]);
        let n = over.screen.len().min(1000);
        ram[0xC800..0xC800 + n].copy_from_slice(&over.screen[..n]);
        ram[0x435..0x438].copy_from_slice(&over.score_bcd);
        ram[0x20] = over.stage_index;
        ram[0x9FAE + over.stage_index as usize] = over.stage_bcd;
        ram[0x1568] = over.player;
        ram[0x401] = 0;
        ram[0xC6] = 0;
        ram[0x00] = 0x2F;
        ram[0x01] = 0x37;
        let mut regs = [0u8; 64];
        regs[0x11] = 0x10;
        regs[0x12] = 0x41;
        regs[0x16] = 0x10 | (over.xscroll & 7);
        regs[0x18] = 0x20;
        regs[0x1A] = 1;
        regs[0x20] = over.border & 15;
        for (i, c) in over.bg.iter().enumerate() {
            regs[0x21 + i] = c & 15;
        }
        let mut color = over.color.to_vec();
        color.resize(1024, 0);
        Hs::enter(ram, color, VicState::at_frame_start(regs), 0x00, draw)
    }

    /// The tables as they are now (the new record is in both, if the score made them).
    pub fn tables(&self) -> Tables {
        let mut t = Tables { all_time: [0; TABLE_LEN], today: [0; TABLE_LEN] };
        t.all_time.copy_from_slice(&self.ram[ALL_TIME_AT..ALL_TIME_AT + TABLE_LEN]);
        t.today.copy_from_slice(&self.ram[TODAY_AT..TODAY_AT + TABLE_LEN]);
        t
    }

    /// Hands the sound driver back to the game: its variables and SID registers as the high score tune left them.
    pub fn give_back_driver(&self, driver: &mut crate::sound_driver::Driver) {
        for r in [0x00A4..0x00BA, 0x41C0..0x4A80] {
            driver.mem[r.clone()].copy_from_slice(&self.ram[r]);
        }
        driver.sid = self.sid;
    }
}
