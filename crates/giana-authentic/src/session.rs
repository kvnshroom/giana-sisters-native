//! The AUTHENTIC session: the whole original, from the title program on the disk to the game and its high score
//! screens, run frame by frame. It is the player's own program on the slice of `machine`; the picture and the SID
//! registers are what the C64 would have shown and played.
//!
//! Flow of the original: title program (disk file `2`: the Time Warp screen, the title picture, both wait for fire on
//! joystick port 1) -> loader (the disk is not read here: a short black interval stands in for the loading time) ->
//! game program (disk file `1`) entered at `$084F`, with the high score table taken from disk file `ATG`.

use crate::machine::{DiskEvent, Machine};

/// Frames of black screen between the title program and the game (the real loading takes about 40 seconds).
pub const LOADING_FRAMES: u32 = 50;

/// What the session needs from the player's data folder.
pub struct AuthenticData {
    /// disk file `2` (the title program), raw `.prg`
    pub title_prg: Vec<u8>,
    /// the 64K program image of disk file `1`
    pub ingame: Vec<u8>,
    /// disk file `ATG` (the high score table the game loads at start), raw `.prg` with its load address
    pub atg: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// the title program (Time Warp screen, title picture)
    Title,
    /// black screen while the original would load the game
    Loading(u32),
    /// the game program, including its title, high score and game over screens
    Game,
}

/// A key of the C64 keyboard matrix: column (port A line) and row (port B line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub col: u8,
    pub row: u8,
}

impl Key {
    pub const fn new(col: u8, row: u8) -> Self {
        Key { col, row }
    }
    pub const RETURN: Key = Key::new(0, 1);
    pub const DELETE: Key = Key::new(0, 0);
    pub const F1: Key = Key::new(0, 4);
    pub const SPACE: Key = Key::new(7, 4);
    pub const LSHIFT: Key = Key::new(1, 7);
    pub const N1: Key = Key::new(7, 0);
    pub const N2: Key = Key::new(7, 3);
}

/// The matrix position of a letter or digit (`a`..`z`, `0`..`9`), for typing a name.
pub fn key_for_char(c: char) -> Option<Key> {
    // (column, row) of the matrix, from the C64 keyboard layout
    Some(match c.to_ascii_lowercase() {
        'a' => Key::new(1, 2),
        'b' => Key::new(3, 4),
        'c' => Key::new(2, 4),
        'd' => Key::new(2, 2),
        'e' => Key::new(1, 6),
        'f' => Key::new(2, 5),
        'g' => Key::new(3, 2),
        'h' => Key::new(3, 5),
        'i' => Key::new(4, 1),
        'j' => Key::new(4, 2),
        'k' => Key::new(4, 5),
        'l' => Key::new(5, 2),
        'm' => Key::new(4, 4),
        'n' => Key::new(4, 7),
        'o' => Key::new(4, 6),
        'p' => Key::new(5, 1),
        'q' => Key::new(7, 6),
        'r' => Key::new(2, 1),
        's' => Key::new(1, 5),
        't' => Key::new(2, 6),
        'u' => Key::new(3, 6),
        'v' => Key::new(3, 7),
        'w' => Key::new(1, 1),
        'x' => Key::new(2, 7),
        'y' => Key::new(3, 1),
        'z' => Key::new(1, 4),
        '0' => Key::new(4, 3),
        '1' => Key::new(7, 0),
        '2' => Key::new(7, 3),
        '3' => Key::new(1, 0),
        '4' => Key::new(1, 3),
        '5' => Key::new(2, 0),
        '6' => Key::new(2, 3),
        '7' => Key::new(3, 0),
        '8' => Key::new(3, 3),
        '9' => Key::new(4, 0),
        _ => return None,
    })
}

pub struct Authentic {
    data: AuthenticData,
    pub machine: Machine,
    pub phase: Phase,
    /// frames run since the start
    pub frames: u64,
    /// when set, the next game start loads the high score table from here instead of `ATG` of the disk
    scores: Option<Vec<u8>>,
    /// SID registers `$D400..$D418` at the end of the last frame
    pub sid: [u8; 25],
    /// disk writes of the game since `take_saves` (the high score table)
    saves: Vec<DiskEvent>,
}

impl Authentic {
    /// Starts the title program. `scores` is a high score table saved earlier (a `.prg` as the game writes it).
    pub fn new(data: AuthenticData, scores: Option<Vec<u8>>) -> Result<Self, String> {
        let machine = Self::title_machine(&data)?;
        Ok(Authentic { data, machine, phase: Phase::Title, frames: 0, scores, sid: [0; 25], saves: Vec::new() })
    }

    /// Starts with the game program (disk file `1`) at its own title, as after the loader: the title program has been shown by
    /// the native title sequence (`giana_core::title`) in this case.
    pub fn new_in_game(data: AuthenticData, scores: Option<Vec<u8>>) -> Self {
        let machine = Self::game_machine(&data, scores.as_deref());
        Authentic { data, machine, phase: Phase::Game, frames: 0, scores, sid: [0; 25], saves: Vec::new() }
    }

    fn title_machine(data: &AuthenticData) -> Result<Machine, String> {
        let ram = giana_extract::derive::title_memory(&data.title_prg)?;
        let mut m = Machine::new(ram);
        m.cpu.pc = 0x080E;
        m.cpu.i = false;
        m.stop_at = Some(0x0334);
        Ok(m)
    }

    fn game_machine(data: &AuthenticData, scores: Option<&[u8]>) -> Machine {
        let mut ram = data.ingame.clone();
        // the game loads `ATG` over its own copy of the table before it starts
        for t in [Some(data.atg.as_slice()), scores].into_iter().flatten() {
            if t.len() > 2 {
                let at = t[0] as usize | (t[1] as usize) << 8;
                let n = (t.len() - 2).min(65536 - at);
                ram[at..at + n].copy_from_slice(&t[2..2 + n]);
            }
        }
        // the image of the data folder carries the oracle's cloud scroll patch (`ORA $0404`, docs/oracle-notes.md section 23);
        // the original has `ORA $0405`
        ram[0x1277] = 0x05;
        let mut m = Machine::new(ram);
        m.cpu.pc = 0x084F;
        m.cpu.i = true;
        // the screen as the loader leaves it: 25 rows, vertical scroll 3, display off; the game sets the display bit (`$08C2`)
        m.bus.vic.regs[0x11] = 0x0B;
        m.bus.ram[0x314] = 0x31;
        m.bus.ram[0x315] = 0xEA;
        m.bus.ram[0x91] = 0xFF;
        m
    }

    /// Joystick in port 2 (the game's) and port 1 (the title program's), bit 0 up, 1 down, 2 left, 3 right, 4 fire.
    pub fn set_joystick(&mut self, bits: u8) {
        self.machine.bus.joy1 = bits & 0x10;
        self.machine.bus.joy2 = bits & 0x1F;
    }

    /// The keys that are held down now.
    pub fn set_keys(&mut self, keys: &[Key]) {
        let mut m = [0u8; 8];
        for k in keys {
            m[k.col as usize & 7] |= 1 << (k.row & 7);
        }
        self.machine.bus.keys = m;
    }

    /// Runs one frame (20 ms). Returns false when the program stopped on something the slice cannot do, with the reason
    /// in `error()`.
    pub fn run_frame(&mut self) -> bool {
        self.frames += 1;
        if !matches!(self.phase, Phase::Loading(_)) {
            let ok = self.machine.run_frame();
            self.sid.copy_from_slice(&self.machine.bus.sid[..25]);
            if !ok {
                return false;
            }
            self.saves.extend(self.machine.take_disk());
        }
        match self.phase {
            Phase::Title => {
                if self.machine.stopped {
                    // the loader of the title program is reached: the game would be loaded now
                    self.phase = Phase::Loading(0);
                    self.blank();
                }
            }
            Phase::Loading(n) => {
                if n + 1 >= LOADING_FRAMES {
                    self.machine = Self::game_machine(&self.data, self.scores.as_deref());
                    self.phase = Phase::Game;
                } else {
                    self.phase = Phase::Loading(n + 1);
                    self.blank();
                }
            }
            Phase::Game => {}
        }
        true
    }

    /// The black picture of the loading interval and silence.
    fn blank(&mut self) {
        self.machine.bus.vic.pic.fill(0);
        self.sid = [0; 25];
    }

    /// The program waits at "PRESS F1 TO SAVE HIGHSCORES" (the loop at `$33EA`, which spends most of its time in the joystick
    /// routine `$9F77` it calls).
    pub fn waiting_for_save_key(&self) -> bool {
        if self.phase != Phase::Game {
            return false;
        }
        let pc = self.machine.cpu.pc;
        if (0x33EA..0x3400).contains(&pc) {
            return true;
        }
        let sp = self.machine.cpu.sp as usize;
        let ram = &self.machine.bus.ram;
        (0x9F77..0x9FAE).contains(&pc) && sp < 0xFD && ram[0x101 + sp] == 0xEC && ram[0x102 + sp] == 0x33
    }

    pub fn error(&self) -> Option<&str> {
        self.machine.error.as_deref()
    }

    /// The picture of the last frame: 384 x 272 C64 colour indices.
    pub fn picture(&self) -> &[u8] {
        &self.machine.bus.vic.pic
    }

    /// Disk saves since the last call: the program writes its high score table. Each is a complete `.prg`.
    pub fn take_saves(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.saves)
            .into_iter()
            .map(|e| {
                let DiskEvent::Save { start, data, .. } = e;
                let mut prg = vec![start as u8, (start >> 8) as u8];
                prg.extend_from_slice(&data);
                prg
            })
            .collect()
    }

    /// The table a later start should load (what `take_saves` returned last), remembered for restarts in this run.
    pub fn use_scores(&mut self, prg: Vec<u8>) {
        self.scores = Some(prg);
    }

    /// Back to the beginning (the title program), as after switching the computer off and on.
    pub fn reset(&mut self) -> Result<(), String> {
        self.machine = Self::title_machine(&self.data)?;
        self.phase = Phase::Title;
        Ok(())
    }
}
