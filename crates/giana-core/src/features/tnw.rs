//! Family T: "The Great Giana Sisters 2" by The New Wave (1989). See `docs/rom-hack-families.md` and
//! `giana-module/src/tnw.rs` (the file format).
//!
//! The hack keeps the original program and replaces the stage tables by one stage file that its loader reads from the
//! disk at every stage start (`$9936: JSR $6F80`). The port keeps the file in [`TnwState::mem`] (the memory from `$6182`) and
//! reads the places of the original that used the tables from there:
//!
//! | Original | Family T |
//! |---|---|
//! | stream pointer `$6000,Y` (`sub_9936`) | stream at `$618D` of the file |
//! | colours `$604E`, `$6075`, `$609C`, `$60C3` (`sub_0D39`) | `$6182..$6185` |
//! | music `$1529,Y` (`$0AA5`) | `$6186` |
//! | start position `$60EA,Y` (`sub_13F0`) | `$6187 + variant` (x), `$6188 + variant` (y) |
//! | enemy list `$3D20,Y` (`sub_2276`) | pointer at `$618B` |
//! | stage numbers `$9FAE` | `$6F00` (identity), bonus rooms `$41..$47` instead of `$21..$27` |
//!
//! `$7ABF` holds the mode chosen in the start menu (`$7AC0`), the loader turns it into the file number:
//! mode 0 = stage, mode 1 = `(stage >> 1) + 1` for odd and `(stage >> 1) + 100` for even stages, mode 2 = stage + 100;
//! the file name is that number plus `$40`. The copy protection (`$33F2`) is not reproduced.
//!
//! Every byte read from the file is bounds-checked: a module is data from other people and must not be able to panic.

use crate::state::{ControllerInput, GameState};
use giana_engine_b::files::ModFile;
use std::sync::{Arc, RwLock};

/// Address the loader puts every stage file at.
pub const LOAD: usize = 0x6182;
/// Size of the memory the stage files are loaded into (from [`LOAD`]).
const MEM: usize = 0x2000;
/// The menu text in the image: 256 bytes of screen codes copied to `$C800` by `$7AC0` (three rows, F1, F3, F5).
const MENU_TEXT: usize = 0x7B13;
/// Menu rows: the key of the original and the mode it chooses.
const MENU_MODES: [u8; 3] = [1, 0, 2];

static TNW: RwLock<Option<Arc<Vec<ModFile>>>> = RwLock::new(None);

/// Switches family T on for the whole process (or off with `None`): the stage files of the module, as the game loads them
/// (`giana_module::Applied::files` for a module of engine "t": whole content without the load address, original stages
/// already restored). Call it together with `data::install` of the module's image, before the first `GameState`.
pub fn set_tnw(files: Option<Vec<ModFile>>) {
    *TNW.write().unwrap() = files.map(Arc::new);
}

/// The stage files of the installed family T module, `None` for the original game.
pub fn tnw_files() -> Option<Arc<Vec<ModFile>>> {
    TNW.read().unwrap().clone()
}

/// The state of family T in a [`GameState`].
#[derive(Clone)]
pub struct TnwState {
    files: Arc<Vec<ModFile>>,
    /// `$7ABF`: 0 = the new stages, 1 = grand loop, 2 = the old stages
    pub mode: u8,
    /// The memory from `$6182`: the stage file that was loaded last.
    mem: Vec<u8>,
    /// The start menu is up; the highlighted row.
    pub menu: Option<usize>,
    menu_held: bool,
    /// Screen and colour memory from before the menu drew over it (the stage start continues with it).
    backup: Option<(Vec<u8>, Vec<u8>)>,
}

/// File number the loader (`$6F80`) asks for.
pub fn file_number(mode: u8, stage: u8) -> u8 {
    match mode {
        0 => stage,
        1 if stage & 1 == 1 => (stage >> 1).wrapping_add(1),
        _ => (if mode == 1 { stage >> 1 } else { stage }).wrapping_add(100),
    }
}

impl TnwState {
    pub fn from_installed() -> Option<Box<TnwState>> {
        tnw_files().map(|files| Box::new(TnwState { files, mode: 0, mem: vec![0; MEM], menu: None, menu_held: true, backup: None }))
    }

    /// `$6070`: loads the file for the stage. A file that does not exist leaves the memory as it is (the KERNAL load fails).
    pub fn load(&mut self, stage: u32) {
        let name = [file_number(self.mode, stage as u8).wrapping_add(0x40)];
        if let Some(f) = self.files.iter().find(|f| f.name == name) {
            let n = f.data.len().min(MEM);
            self.mem[..n].copy_from_slice(&f.data[..n]);
        }
    }

    /// The byte at a C64 address of the loaded file; `$FF` outside it (it ends every list, so bad data ends them).
    pub fn byte(&self, addr: usize) -> u8 {
        addr.checked_sub(LOAD).and_then(|i| self.mem.get(i)).copied().unwrap_or(0xFF)
    }

    pub fn word(&self, addr: usize) -> usize {
        self.byte(addr) as usize | (self.byte(addr + 1) as usize) << 8
    }
}

impl GameState {
    /// Byte of the stage data at `addr`: the loaded stage file for family T, else the image.
    #[inline]
    pub(crate) fn stage_byte(&self, addr: usize) -> u8 {
        match &self.tnw {
            Some(t) => t.byte(addr),
            None => crate::image::img(addr),
        }
    }

    /// `$6F80` (called from `sub_9936`): loads the stage file for `$20`.
    pub(crate) fn tnw_load_stage(&mut self) {
        let stage = self.stage;
        if let Some(t) = &mut self.tnw {
            t.load(stage);
        }
    }

    /// `$0D39`: the colours from the loaded file. Returns false for the original.
    pub(crate) fn tnw_colours(&mut self) -> bool {
        let Some(t) = &self.tnw else { return false };
        let c = [t.byte(LOAD), t.byte(LOAD + 1), t.byte(LOAD + 2), t.byte(LOAD + 3)];
        self.en.vic_bg0 = c[0];
        self.en.vic_bg1 = c[1];
        self.vic_bg2 = c[2];
        self.zp_1e = c[3];
        true
    }

    /// `$0AA5`: the music track of the loaded file.
    pub(crate) fn tnw_music(&self) -> Option<u8> {
        self.tnw.as_ref().map(|t| t.byte(LOAD + 4))
    }

    /// `$13F8`: start position (x, y) of the loaded file for the spawn variant.
    pub(crate) fn tnw_start_position(&self) -> Option<(u8, u8)> {
        let t = self.tnw.as_ref()?;
        let v = (self.spawn_variant as usize) & 3;
        Some((t.byte(LOAD + 5 + v), t.byte(LOAD + 6 + v)))
    }

    /// Pointer to the enemy list of the loaded file (`$618B`).
    pub(crate) fn tnw_enemy_list(&self) -> Option<usize> {
        self.tnw.as_ref().map(|t| t.word(LOAD + 9))
    }

    /// Stream address of the loaded file (`$618D`).
    pub(crate) fn tnw_stream(&self) -> Option<usize> {
        self.tnw.as_ref().map(|_| LOAD + 11)
    }

    /// The decimal-to-BCD table of the hack (`$6F00`, identity) at `n`; `None` for the original.
    pub(crate) fn tnw_bcd(&self, n: usize) -> Option<u8> {
        self.tnw.as_ref().map(|_| crate::image::img(0x6F00 + n))
    }

    /// Family T has no last stage number: the hack's `INC $20` always goes on (mode 0 even asks for a file that does not
    /// exist at stage 33 and plays the stage that is in memory again), the game ends when the boss of the last stage dies
    /// (`$157E`, `ending.rs`). Only the byte of `$20` is the end of the line.
    pub(crate) fn tnw_is_last_stage(&self) -> bool {
        self.stage >= 0xFF
    }

    /// The numbers of the statue room logic: (last stage, room after it, one past the last cycled room, first cycled room).
    pub(crate) fn tnw_room_numbers(&self) -> (u32, u32, u8, u8) {
        if self.tnw.is_some() {
            (0x40, 0x41, 0x47, 0x42)
        } else {
            (0x20, 0x21, 0x27, 0x22)
        }
    }

    /// `$0996`: the start menu. Called where the title ends and the first stage would start; true when the menu is up (the
    /// stage starts after the choice).
    pub(crate) fn tnw_menu_begin(&mut self) -> bool {
        if self.tnw.is_none() {
            return false;
        }
        let backup = (self.screen_ram.to_vec(), self.color_ram.to_vec());
        if let Some(t) = &mut self.tnw {
            t.backup = Some(backup);
        }
        self.tnw_draw_menu(0);
        true
    }

    /// Draws the menu as `$7AC0` does (`$C800` rows 0..2 from the image, colour 7), the highlighted row in white.
    fn tnw_draw_menu(&mut self, sel: usize) {
        for i in 0..1000 {
            self.screen_ram[i] = if i < 256 { crate::image::img(MENU_TEXT + i) } else { 0 };
        }
        for i in 0..1000 {
            self.color_ram[i] = if i < 256 { 7 } else { 0 };
        }
        for i in 0..40 {
            self.color_ram[sel * 40 + i] = 1;
        }
        self.en.vic_bg0 = 0;
        self.en.vic_me = 0;
        self.en.vic_frame.me = 0;
        if let Some(t) = &mut self.tnw {
            t.menu = Some(sel);
        }
        self.vic_capture_draw_frame();
    }

    /// Chooses mode `m` (0, 1, 2) in the menu and starts the first stage. Nothing happens outside the menu.
    pub fn tnw_choose_mode(&mut self, m: u8) {
        if !self.tnw.as_ref().is_some_and(|t| t.menu.is_some()) {
            return;
        }
        let backup = self.tnw.as_mut().and_then(|t| {
            t.mode = m.min(2);
            t.menu = None;
            t.backup.take()
        });
        if let Some((screen, colour)) = backup {
            self.screen_ram.copy_from_slice(&screen);
            self.color_ram.copy_from_slice(&colour);
        }
        let stage = self.stage;
        self.start_stage(stage);
    }

    /// One frame of the menu: up and down move the highlight, fire, jump or start choose. True while the menu is up.
    pub(crate) fn tnw_menu_update(&mut self, input: &ControllerInput) -> bool {
        let Some(sel) = self.tnw.as_ref().and_then(|t| t.menu) else { return false };
        let pressed = input.fire || input.jump || input.start;
        let moved = input.up || input.down;
        let held = pressed || moved;
        let was = self.tnw.as_ref().is_some_and(|t| t.menu_held);
        if let Some(t) = &mut self.tnw {
            t.menu_held = held;
        }
        if was || !held {
            self.vic_capture_draw_frame();
            return true;
        }
        if pressed {
            self.tnw_choose_mode(MENU_MODES[sel.min(2)]);
        } else {
            let n = if input.down { (sel + 1) % 3 } else { (sel + 2) % 3 };
            self.tnw_draw_menu(n);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_numbers_follow_the_loader() {
        // mode 0: the stage; mode 2: stage + 100
        assert_eq!((file_number(0, 5), file_number(2, 5)), (5, 105));
        // mode 1: odd stages count the new set, even ones the old set
        assert_eq!([1, 2, 3, 4, 5, 6].map(|s| file_number(1, s)), [1, 101, 2, 102, 3, 103]);
        // bonus rooms $42..$47
        assert_eq!([0x42, 0x43, 0x44, 0x45, 0x46, 0x47].map(|s| file_number(1, s)), [133, 34, 134, 35, 135, 36]);
        assert_eq!(file_number(2, 0x47), 171);
        assert_eq!(file_number(2, 200), 44);
    }

    #[test]
    fn reads_are_bounds_checked() {
        let files = Arc::new(vec![ModFile::new(b"A", vec![1, 2, 3])]);
        let mut t = TnwState { files, mode: 0, mem: vec![0; MEM], menu: None, menu_held: false, backup: None };
        t.load(1);
        assert_eq!((t.byte(LOAD), t.byte(LOAD + 2), t.byte(LOAD + 3)), (1, 3, 0));
        assert_eq!((t.byte(0), t.byte(LOAD - 1), t.byte(LOAD + MEM), t.byte(usize::MAX)), (0xFF, 0xFF, 0xFF, 0xFF));
        assert_eq!(t.word(LOAD + MEM - 1), 0xFFFF & (t.byte(LOAD + MEM - 1) as usize | 0xFF00));
        t.load(2); // no such file: the memory stays
        assert_eq!(t.byte(LOAD), 1);
    }
}
