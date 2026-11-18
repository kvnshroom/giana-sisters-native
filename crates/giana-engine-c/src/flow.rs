//! The game flow of the 30A preview (work package F of `docs/rom-hack-family-c.md`, 7.9): the main program of the
//! original as a native state machine. It runs in the time the raster IRQs leave ([`crate::frame::Core::run_frame`]).
//!
//! The original's main program is straight-line 6502 code that waits in a few kinds of loop: for the joystick's fire
//! button (`$21FA`), for the screen to go off or on (`$23A6`/`$23AE`: until an IRQ has copied `$D7` to `$D011`), for
//! Giana's sprite to be shown at her new y (`$4194`, one frame of the death jump), in delay loops that count CPU cycles
//! (`$371E`, and `$3706`, which fire cuts short), and in polling loops (the main loop `$1F68`, the title, the menus).
//! [`Main`] keeps where the program is ([`Pc`]) and what it waits for ([`Wait`]); everything between two waits runs at
//! once. The delay loops get the CPU time that the scheduler estimates for the gaps between the IRQ handlers.

use crate::frame::{Core, Original, FRAME_LINE, LINES};
use crate::layout::C30aLayout;
use crate::player::{self, DeathPhase, DeathStart, DeathStep};
use crate::rooms::{self, RoomPath};
use crate::{blocks, hiscore, screens, text};

/// Where the main program is (each value is a point of the original where it waits or loops).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pc {
    /// the main loop `$1F68` (one pass per time slice)
    MainLoop,
    /// `$20F3`: the main loop found `$99`
    DeathStart,
    /// `$2114`: the death jump, one step per frame (waits at `$4194`)
    DeathJump(DeathPhase),
    /// `$2138`: after the jump: the delay, then the reset
    DeathDelayed,
    /// `$214D..$2163`: "time up" text after a death by time
    DeathLife,
    /// `$2164`: a life less
    DeathLoseLife,
    /// `$27AF` (subroutine): screen off, cleared, the menu IRQ, screen on; then the return stack
    ScreenReset(u8),
    /// `$4D40` (subroutine): the font switch, loop by loop
    Swap(u8),
    SwapEnd,
    /// `$1E56`: the stage restarts after a lost life (package F, stage start)
    StageRestart,
    /// `$2172..$21CC`: game over and the high-score screen (`GameOver(10)`: from the title's `H`)
    GameOver(u8),
    /// `$1126..$12EA`: the intro (once)
    Intro(u8),
    /// `$1D13..$1DD6`: the title
    Title(u8),
    /// `$4E67..$4F93` (subroutine): the high-score name entry
    Entry(u8),
    /// `$1E47`: the next stage (from the stage clear)
    StageNext,
    /// `$1E5B..$1F65`: the stage start, step by step (between its waits)
    StageStart(u8),
    /// `$360A` (subroutine): Giana's sprites for the power level
    PowerSprites(u8),
    /// `$20E9`: after a power-up change in the main loop
    MainLoopPower,
    /// `$1F6D..$202D`: the stage is over, step by step
    StageClear(u8),
    /// `$2221` (subroutine, `Bonus(10)` = `$228C`): the bonus counts
    Bonus(u8),
    /// `$3FB9..$4053` (entering, `Room(0..)`), `$409E..$40B2` (the lift rises, `Room(20)`), `$40C3..$4151` (back into
    /// the stage, `Room(30..)`): the treasure rooms' code of the main loop between its waits ([`crate::rooms`])
    Room(u8),
    /// `$21CF..$21F9` (with `$4CBE`): the pause menu's save and quit, step by step
    SaveQuit(u8),
    /// `$492B..$4973` (subroutine): the stage select of a new game
    StageSelect(u8),
    /// `$8C0E` (subroutine): the level buffer, in two parts ([`Main::level`])
    Level(u8),
    /// `$1DD8..$1E44`: the title's saved-game menu
    SavedGame(u8),
    /// `$47B9..$48E9`: the end of the game (bonus counts, the end text), then the game over
    GameEnd(u8),
    /// the main program is in code that has no native port yet: it stays here (tests stop at it)
    Unported(u16),
}

/// What the main program waits for before it goes on at its [`Pc`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    None,
    /// a polling loop: the rest of the time slice is used up, the next slice runs the [`Pc`] again
    Yield,
    /// `$4194`: until `$D001` (Giana's sprite y as the `$FB` IRQ wrote it) equals `$78`
    Sprite0Y,
    /// `$21FA`: fire pressed (then [`Wait::FireUp`])
    Fire,
    /// `$2207`: fire released
    FireUp,
    /// `$23A6` (`false`) / `$23AE` (`true`): until `$D011` bit 4 is as wanted
    Screen(bool),
    /// `$4F8A`: until the IRQ has scrolled the high-score entry's letter in (`$44 = 0`)
    Letter,
    /// `$23B6` entered with IRQs masked: its `CLI` lets a pending IRQ in before it writes `$D7` (then [`Wait::Screen`])
    Cli(bool),
    /// a polling loop whose passes take this many CPU cycles each: the next pass runs in the same time slice when
    /// there is time left (the treasure rooms' main loop, which reads `$D01E` once a pass)
    Pass(i64),
}

/// The main program.
#[derive(Clone, Debug)]
pub struct Main {
    pub pc: Pc,
    pub wait: Wait,
    /// IRQs masked by the main program (`SEI` .. `CLI`)
    pub i: bool,
    /// CPU cycles of a delay loop still to run
    pub busy: i64,
    /// the delay ends when fire is pressed (`$3706`)
    pub busy_fire: bool,
    /// the delay loop reads the joystick itself (`$1052`), else `$32` comes from an IRQ
    pub busy_reads_input: bool,
    /// return points of the subroutines that wait (`JSR` of code with a wait inside)
    pub ret: Vec<Pc>,
    /// where the original's main program is when the busy time ends (the `RTS` of a delay loop, or the instruction
    /// after a long piece of work); see [`Main::sync`]
    pub busy_end: u16,
    /// the busy time is work (screen clears, copies), not a delay loop: the state is in between
    pub working: bool,
    /// for the tests against the original: the moments (`(pc, line)`) at which the original's main program reached the
    /// [`BUSY_ENDS`] in this frame; with it a busy time ends where the original's did, instead of after the estimated
    /// cycles (the estimate is measured on its own)
    pub sync: Option<Vec<(u16, u16)>>,
    /// the high-score file for the intro's load (the app's [`hiscore::ScoreStore`]); taken when loaded
    pub scores: Option<Vec<u8>>,
    /// the high-score file to save (the high-score screen's up): the app takes it and gives it to its store
    pub save: Option<Vec<u8>>,
    /// the raster time (cycles from line `$41`) at which the code of the current step runs (estimated from the time
    /// slice), for the main program's reads of `$D01E`
    pub now: i64,
    /// CPU cycles the main program has run (all time slices before the current one; `cpu_in_slice` in it)
    pub cpu: i64,
    pub cpu_in_slice: i64,
    /// the screen clear `$8F5C` in progress: its start ([`Main::cpu`]) and the iterations of its two loops done
    /// (0..512): an IRQ that writes into the screen during it keeps only what comes after the loop has passed
    pub clearing: Option<(i64, u16)>,
    /// where the original is when the level buffer of [`Main::level`] is done
    pub level_end: u16,
    /// a screen wait (`$23A6`/`$23AE`) that follows when the busy time ends
    pub then_screen: Option<bool>,
}

/// Where the original's main program is when a busy time of [`Main`] ends: the `RTS` of `$371E`, the ends of `$3706`,
/// and the instructions after long pieces of work.
pub const BUSY_ENDS: &[u16] = &[
    DELAY_END, UNITS_END, UNITS_FIRE, DEBOUNCE_END, 0x27B2, 0x4D4D, 0x4D5A, 0x4D67, 0x4D79, 0x4D8B, 0x1E67, 0x1F24, 0x1F4D, 0x1F62, 0x35E2, 0x3676, 0x36B5, 0x2658, 0x2845, 0x1EFA, 0x119A, 0x11AE, 0x11C2, 0x11D0, 0x12DC, 0x106E, 0x12EA, 0x1D21, 0x1D3B, 0x1D5B, 0x4DF7, 0x2187, 0x3FD9, 0x3FE7, 0x40D4, 0x40DD, 0x412C, 0x4135, 0x21F3, 0x4CD8, 0x4973, 0x26F2, 0x27E6, 0x1FF7, 0x8C23, 0x2343, 0x1DDE, 0x47BC, 0x48CC,
];
/// `$2211`: the `RTS` of the fire release wait `$2201`.
pub const DEBOUNCE_END: u16 = 0x2211;
/// Cycles of the 255 reads of `$2207`.
pub const DEBOUNCE_CYCLES: i64 = 255 * 14 + 10;
/// `$3724`: the `RTS` of the delay loop `$371E`.
pub const DELAY_END: u16 = 0x3724;
/// `$371D`/`$371A`: the `RTS` of `$3706`, and its `JSR $2201` (the fire release wait) where both ways out of its loop
/// (the count, fire) end.
pub const UNITS_END: u16 = 0x371D;
pub const UNITS_FIRE: u16 = 0x371A;

impl Main {
    /// The main program at power-on (`$1D10`: the intro, then the title), with the high-score file `scores`.
    pub fn power_on(scores: Option<Vec<u8>>) -> Main {
        Main { pc: Pc::Intro(0), scores, ..Main::in_main_loop() }
    }

    /// The main program in the main loop of a running stage.
    pub fn in_main_loop() -> Main {
        Main { pc: Pc::MainLoop, wait: Wait::None, i: false, busy: 0, busy_fire: false, busy_reads_input: false, ret: Vec::new(), busy_end: 0, working: false, sync: None, scores: None, save: None, now: 0, cpu: 0, cpu_in_slice: 0, clearing: None, level_end: 0, then_screen: None }
    }

    /// Runs the main program for `budget` CPU cycles (or until it changed the IRQ setup: the return value is then
    /// `true` and `budget` holds what is left).
    pub fn slice(&mut self, core: &mut Core, orig: &mut dyn Original, budget: &mut i64, now: i64, gap_end: i64) -> bool {
        let start = *budget;
        let changed = self.slice_inner(core, orig, budget, now, gap_end);
        self.cpu += start - *budget;
        if self.clearing.is_some() {
            self.advance_clear(core, self.cpu);
        }
        changed
    }

    /// The screen clear's iterations up to the main program's CPU time `cpu` (`$8F60`: 25 cycles per `Y`, the four
    /// screen pages at once from offset 0, then 255 down to 1; then the colour RAM the same way).
    fn advance_clear(&mut self, core: &mut Core, cpu: i64) {
        let Some((t0, done)) = self.clearing else { return };
        let target = ((cpu - t0).max(0) / 25).min(512) as u16;
        let s = core.l.screen as usize;
        for i in done..target {
            let y = (256 - (i as usize % 256)) % 256;
            for page in [0usize, 0x100, 0x200, 0x2E8] {
                if i < 256 {
                    core.music.driver.mem[s + page + y] = 0;
                } else {
                    core.color[page + y] = 0;
                }
            }
        }
        self.clearing = Some((t0, target));
    }

    /// The end of the screen clear: the rest of its loops, then the item icon's characters (`$8F85`).
    fn finish_clear(&mut self, core: &mut Core) {
        if self.clearing.is_some() {
            self.advance_clear(core, i64::MAX / 2);
            self.clearing = None;
            if core.peek(core.l.charset) == 0 {
                let l = core.l;
                item_chars(core.ram(), l, l.item_icon_none);
            }
        }
    }

    fn slice_inner(&mut self, core: &mut Core, orig: &mut dyn Original, budget: &mut i64, now: i64, gap_end: i64) -> bool {
        let start = *budget;
        let before = irq_setup(core, self);
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 10_000, "the main program does not wait (pc {:?})", self.pc);
            match self.wait {
                Wait::None => {}
                Wait::Yield => {
                    self.wait = Wait::None;
                    *budget = 0;
                    return false;
                }
                Wait::Pass(n) => {
                    self.wait = Wait::None;
                    if *budget <= n {
                        *budget = 0;
                        return false;
                    }
                    *budget -= n;
                }
                Wait::Sprite0Y => {
                    if core.vic[1] != core.peek(core.l.player_y) {
                        *budget = 0;
                        return false;
                    }
                    self.wait = Wait::None;
                }
                Wait::Fire => {
                    if core.joy & 0x10 != 0 {
                        *budget = 0;
                        return false;
                    }
                    self.wait = Wait::FireUp;
                    continue;
                }
                Wait::FireUp => {
                    if core.joy & 0x10 == 0 {
                        *budget = 0;
                        return false;
                    }
                    self.wait = Wait::None;
                    // `$2207`: 255 reads of the released button
                    self.work(DEBOUNCE_CYCLES, DEBOUNCE_END);
                    continue;
                }
                Wait::Letter => {
                    if core.peek(core.l.entry_letter + 5) != 0 {
                        *budget = 0;
                        return false;
                    }
                    self.wait = Wait::None;
                }
                Wait::Cli(on) => {
                    core.poke(core.l.screen_mode, if on { 0x1B } else { 0x0B });
                    self.wait = Wait::Screen(on);
                    continue;
                }
                Wait::Screen(on) => {
                    if core.vic[0x1A] & 1 == 0 {
                        let d7 = core.peek(core.l.screen_mode);
                        core.vic[0x11] = d7;
                    }
                    if (core.vic[0x11] & 0x10 != 0) != on {
                        *budget = 0;
                        return false;
                    }
                    self.wait = Wait::None;
                }
            }
            if self.busy > 0 {
                if self.busy_reads_input {
                    let (l, joy) = (core.l, core.joy);
                    let _ = player::read_input(core.ram(), l, joy, 0);
                }
                if self.busy_fire && core.peek(core.l.fire) != 0 {
                    self.busy = 0;
                    self.busy_fire = false;
                    self.busy_reads_input = false;
                    self.wait = Wait::FireUp;
                    continue;
                } else if let Some(events) = &mut self.sync {
                    let end = self.busy_end;
                    let found = events.iter().position(|&(pc, _)| pc == end);
                    match found {
                        Some(k) => {
                            let (_, line) = events.remove(k);
                            let e = (line as i64 + 312 - 0x41) % 312 * 63;
                            let elapsed = start - *budget;
                            if e <= gap_end {
                                // the gap's raster time includes the VIC's cycles: scale to the budget
                                let span = (gap_end - now).max(1);
                                *budget -= ((e - now).max(0) * start / span - elapsed).max(0);
                                self.busy = 0;
                                self.busy_fire = false;
                                self.busy_reads_input = false;
                                self.working = false;
                                if let Some(on) = self.then_screen.take() {
                                    self.screen(core, on);
                                    continue;
                                }
                                if end == UNITS_FIRE {
                                    // `$371A`: the end of `$3706` goes through the fire release wait `$2201` too
                                    self.wait = Wait::FireUp;
                                    continue;
                                }
                            } else {
                                events.insert(k, (end, line));
                                *budget = 0;
                                return false;
                            }
                        }
                        None => {
                            *budget = 0;
                            return false;
                        }
                    }
                } else {
                    let n = self.busy.min(*budget);
                    self.busy -= n;
                    *budget -= n;
                    if self.busy > 0 {
                        return false;
                    }
                    self.busy_fire = false;
                    self.busy_reads_input = false;
                    self.working = false;
                    if let Some(on) = self.then_screen.take() {
                        self.screen(core, on);
                        continue;
                    }
                    if self.busy_end == UNITS_FIRE {
                        self.wait = Wait::FireUp;
                        continue;
                    }
                }
            }
            if let Pc::Unported(_) = self.pc {
                *budget = 0;
                return false;
            }
            self.now = now + (start - *budget).max(0) * (gap_end - now) / start.max(1);
            self.cpu_in_slice = start - *budget;
            let vic = core.vic;
            self.step(core, orig);
            // the main program's writes to the picture's registers, at its raster time
            let line = ((self.now / 63) as u16 + FRAME_LINE + 1) % LINES;
            for reg in crate::frame::PICTURE_REGS {
                if core.vic[reg as usize] != vic[reg as usize] {
                    core.vic_writes.push((line, reg, core.vic[reg as usize]));
                }
            }
            if irq_setup(core, self) != before {
                return true;
            }
        }
    }

    /// Runs the code at [`Main::pc`] up to its next wait.
    fn step(&mut self, core: &mut Core, orig: &mut dyn Original) {
        let l = core.l;
        match self.pc {
            Pc::MainLoop => self.main_loop(core, orig),
            Pc::DeathStart => {
                // `$20F3`
                match player::death_start(core.ram(), l) {
                    DeathStart::Skip => {
                        self.main_loop_tail(core);
                    }
                    DeathStart::Dying { song } => {
                        core.music.start_song(song);
                        player::death_freeze(core.ram(), l);
                        self.pc = Pc::DeathJump(DeathPhase::Rise(0));
                    }
                }
            }
            Pc::DeathJump(phase) => match player::death_step(core.ram(), l, phase) {
                DeathStep::Moved { phase, .. } => {
                    self.wait = Wait::Sprite0Y;
                    self.pc = Pc::DeathJump(phase.next());
                }
                DeathStep::Gone { .. } => {
                    self.delay(0, 0);
                    self.pc = Pc::DeathDelayed;
                }
            },
            Pc::DeathDelayed => {
                // `$213D`
                player::death_reset(core.ram(), l);
                clear_animations(core.ram(), l);
                self.gosub(Pc::ScreenReset(0), Pc::DeathLife);
            }
            Pc::DeathLife => {
                if core.peek(l.dying) & 0x80 != 0 {
                    self.pc = Pc::DeathLoseLife;
                    return;
                }
                // `$2151`: time up
                core.poke(l.menu_bands, 1);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 7, 0x0F, 9);
                self.screen(core, true);
                self.delay_units(4);
                self.pc = Pc::DeathLoseLife;
            }
            Pc::DeathLoseLife => {
                if player::lose_life(core.ram(), l) {
                    self.pc = Pc::StageRestart;
                } else {
                    self.pc = Pc::GameOver(0);
                }
            }
            Pc::ScreenReset(0) => {
                self.screen(core, false);
                self.pc = Pc::ScreenReset(1);
            }
            Pc::ScreenReset(1) => {
                // `$8F5C` in its two loops over time (what an IRQ writes into the screen during them stays only where
                // the loop has passed)
                self.clearing = Some((self.cpu + self.cpu_in_slice, 0));
                self.work(CLEAR_CYCLES, 0x27B2);
                self.pc = Pc::ScreenReset(2);
            }
            Pc::ScreenReset(2) => {
                self.finish_clear(core);
                self.pc = Pc::ScreenReset(3);
                if core.peek(l.charset) == 0 {
                    self.gosub(Pc::Swap(0), Pc::ScreenReset(3));
                }
            }
            Pc::Swap(k) => {
                // `$4D40` in its five loops: each one's effect at its start, then its time (the IRQs that come in the
                // middle see the loops before them done and the later ones not yet)
                charset_swap_part(core.ram(), l, k);
                let (cycles, end) = SWAP_PARTS[k as usize];
                self.work(cycles, end);
                self.pc = if k == 4 { Pc::SwapEnd } else { Pc::Swap(k + 1) };
            }
            Pc::SwapEnd => self.ret(),
            Pc::ScreenReset(3) => {
                core.poke(l.menu_bands, 5);
                core.poke(l.menu_bands + 1, 4);
                core.poke(l.menu_bands + 2, 2);
                core.vic[0x12] = 0x69;
                core.poke(l.irq_vector, l.irq_menu as u8);
                core.poke(l.irq_vector + 1, (l.irq_menu >> 8) as u8);
                // `SEI`, enable, acknowledge: no IRQ from an old flag; the IRQs stay masked up to the `CLI` of the
                // screen-on wait (an IRQ whose line comes in between waits for it)
                core.vic[0x1A] = 1;
                core.latch = false;
                core.vic[0x16] = 8;
                core.vic[0x15] = 0;
                self.i = true;
                self.work(RESET_TAIL_CYCLES, 0x27E6);
                self.pc = Pc::ScreenReset(9);
            }
            Pc::ScreenReset(_) => {
                self.screen(core, true);
                self.ret();
            }
            Pc::GameOver(k) => self.game_over(core, k),
            Pc::Intro(k) => self.intro(core, k),
            Pc::Title(k) => self.title(core, k),
            Pc::Entry(k) => self.entry(core, k),
            Pc::StageNext => {
                core.poke(l.restart_column, 0);
                core.poke(l.stage_counter, 0xFF);
                let st = core.peek(l.stage).wrapping_add(1);
                core.poke(l.stage, st);
                core.poke(l.same_player, 1);
                self.pc = Pc::StageStart(0);
            }
            Pc::StageRestart => {
                core.poke(l.same_player, 0);
                self.pc = Pc::StageStart(0);
            }
            Pc::StageStart(k) => self.stage_start(core, k),
            Pc::StageClear(k) => self.stage_clear(core, k),
            Pc::Bonus(k) => self.bonus(core, k),
            Pc::PowerSprites(k) => self.power_sprites(core, k),
            Pc::MainLoopPower => {
                core.poke(l.game_state, 0);
                core.poke(l.hud_redraw, 0);
                self.pc = Pc::MainLoop;
            }
            Pc::Room(k) => self.room(core, k),
            Pc::SaveQuit(k) => self.save_quit(core, k),
            Pc::StageSelect(k) => self.stage_select(core, k),
            Pc::Level(k) => self.level_step(core, k),
            Pc::SavedGame(k) => self.saved_game(core, k),
            Pc::GameEnd(k) => self.game_end(core, k),
            Pc::Unported(_) => {}
        }
    }

    /// `JSR` into code with a wait: `sub` runs, then `back`.
    fn gosub(&mut self, sub: Pc, back: Pc) {
        self.ret.push(back);
        self.pc = sub;
    }

    /// The `RTS` of a [`Main::gosub`].
    fn ret(&mut self) {
        self.pc = self.ret.pop().expect("RTS without JSR in the main program");
    }

    /// `$23B6` with `$0B` (`$23A6`, screen off) or `$1B` (`$23AE`, screen on): `CLI`, `$D7`, `$D011` directly when the
    /// raster IRQ is off, then the wait until `$D011` shows it.
    fn screen(&mut self, core: &mut Core, on: bool) {
        if self.i {
            // the `CLI` first: the scheduler gives a pending IRQ its turn, then `$D7` is written
            self.i = false;
            self.wait = Wait::Cli(on);
            return;
        }
        core.poke(core.l.screen_mode, if on { 0x1B } else { 0x0B });
        self.wait = Wait::Screen(on);
    }

    /// `$22C5` with its screen-on wait: text `n` at the print position, the printer's time ([`text::print_cycles`]),
    /// then the wait.
    fn print_wait(&mut self, core: &mut Core, n: u8) {
        let l = core.l;
        let cycles = text::print_cycles(core.ram(), l, n);
        let (ram, color, _) = core.parts();
        text::print(ram, color, l, n);
        self.work(cycles, 0x2343);
        self.then_screen = Some(true);
    }

    /// `$8C0E` (`JSR` from the caller, which goes on at `back` when the original is at `end_pc`): the level buffer cleared
    /// first, the stage setup and map after that time ([`Pc::Level`]).
    fn level(&mut self, core: &mut Core, end_pc: u16, back: Pc) {
        let l = core.l;
        screens::level_clear(core.ram(), l);
        self.level_end = end_pc;
        self.work(screens::LEVEL_CLEAR_CYCLES, 0x8C23);
        self.ret.push(back);
        self.pc = Pc::Level(1);
    }

    /// The steps of [`Main::level`].
    fn level_step(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        match k {
            1 => {
                let (ram, color, _) = core.parts();
                self.pc = Pc::Level(2);
                if screens::level_build(ram, color, l) {
                    // the banner printer's screen-on wait
                    self.screen(core, true);
                }
            }
            2 => {
                self.work(LEVEL_CYCLES - screens::LEVEL_CLEAR_CYCLES, self.level_end);
                self.pc = Pc::Level(3);
            }
            _ => self.ret(),
        }
    }

    /// `$3706`: `a` units of delay that fire cuts short (then the wait for its release, `$2201`).
    fn delay_units(&mut self, a: u8) {
        self.busy = units_cycles(a);
        self.busy_fire = true;
        // the loop ends at `$371A` (`JSR $2201`, the fire release wait) with fire or without
        self.busy_end = UNITS_FIRE;
    }

    /// `$371E` with `X`, `Y`.
    fn delay(&mut self, x: u8, y: u8) {
        self.busy = delay_cycles(x, y);
        self.busy_end = DELAY_END;
    }

    /// A long piece of work of the original (copies, screen clears) that ends at `end_pc`: it takes `cycles`; its effect
    /// is already done (the native code runs it at once).
    fn work(&mut self, cycles: i64, end_pc: u16) {
        self.busy = cycles;
        self.busy_end = end_pc;
        self.working = true;
    }

    /// `$1F6D..$202D`: the stage is over (the end block, `$02D3`, or the stage-end tile / the next-stage keys, `$98`):
    /// the "stage clear" screen, the time bonus, the bonus for a stage without scrolling, then the next stage or the end.
    fn stage_clear(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::StageClear(k + 1);
        match k {
            0 => {
                core.music.restart_song(6);
                self.delay(0, 0);
            }
            1 | 2 => self.delay(0, 0),
            3 => {
                core.poke(l.end_bump, 0);
                core.poke(l.probe_tiles + 1, 0);
                self.pc = Pc::StageClear(10);
            }
            10 => {
                // `$1F82`
                let st = core.peek(l.stage);
                core.poke(l.bonus_stage, st);
                core.poke(l.game_state, 0xFF);
                clear_animations(core.ram(), l);
                self.gosub(Pc::ScreenReset(0), Pc::StageClear(11));
            }
            11 => {
                core.poke(l.menu_bands, 1);
                core.poke(l.menu_bands + 1, 1);
                core.poke(l.menu_bands + 2, 3);
                let n = core.peek(l.player_number);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, n, 0x0D, 9);
                text::print(ram, color, l, 5);
                let st = ram[l.stage as usize].wrapping_add(ram[(l.stage_header + 2) as usize]);
                ram[l.stage as usize] = st;
                if st >= 0x0D {
                    text::print(ram, color, l, 6);
                } else {
                    ram[l.print_col as usize] = 0x0F;
                }
                text::print(ram, color, l, 3);
                text::number(ram, color, l, st.wrapping_add(1));
                self.screen(core, true);
                self.delay_units(6);
            }
            12 => {
                core.poke(l.stage_end, 1);
                self.pc = Pc::StageClear(20);
            }
            20 => {
                // `$1FED`: the song start (`$5C93`) is long and an IRQ's music tick comes in the middle of it: its
                // effect at its end
                if core.peek(l.pickup_end) & 0x80 == 0 {
                    self.work(SONG_CYCLES, 0x1FF7);
                    self.pc = Pc::StageClear(24);
                    return;
                }
                self.gosub(Pc::Bonus(0), Pc::StageClear(21));
            }
            24 => {
                core.music.start_song(7);
                self.gosub(Pc::Bonus(0), Pc::StageClear(21));
            }
            21 => {
                if core.peek(l.stage_counter) == 0 && core.peek(l.scroll_column) == 0 {
                    core.poke(l.menu_bands + 1, 5);
                    let (ram, color, _) = core.parts();
                    text::print_at(ram, color, l, 0x10, 6, 0x0D);
                    self.screen(core, true);
                    self.gosub(Pc::Bonus(10), Pc::StageClear(22));
                }
            }
            22 => self.delay_units(6),
            23 => {
                let end = core.peek(l.pickup_end);
                if end != 0 {
                    if core.peek(l.select_cursor) != 0 {
                        self.pc = start_pc(core);
                        return;
                    }
                    if core.peek(l.stage) != 0 {
                        core.poke(l.stage, end);
                    }
                    self.pc = Pc::GameEnd(0);
                    return;
                }
                if core.peek(l.stage) == 0 {
                    core.poke(l.stage, 0xFF);
                }
                self.pc = Pc::StageNext;
            }
            _ => unreachable!(),
        }
    }

    /// `$2221..$22BE`: the bonus counts: the time left (`$CB/$CA`, 10 points a second), then (`$228C`, entered at
    /// `Bonus(10)`) the stage number as a bonus for a stage without scrolling (500 points each). Fire speeds them up.
    fn bonus(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::Bonus(k + 1);
        match k {
            0 => {
                clear_animations(core.ram(), l);
                self.gosub(Pc::ScreenReset(0), Pc::Bonus(1));
            }
            1 => {
                core.poke(l.game_state, 0xFF);
                core.vic[0x12] = 0x49;
                core.poke(l.menu_bands, 9);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 0x14, 0x0C, 3);
                let st = ram[l.bonus_stage as usize];
                ram[l.print_col as usize] = 0x12;
                text::number(ram, color, l, st);
                text::print_at(ram, color, l, 8, 6, 7);
                self.screen(core, true);
                self.pc = Pc::Bonus(2);
            }
            2 => {
                // `$226D`
                let (ram, color, _) = core.parts();
                let (hi, lo) = (ram[l.time as usize + 1], ram[l.time as usize]);
                text::bcd_at(ram, color, l, hi, 6, 9);
                text::bcd(ram, color, l, lo);
                text::score(ram, color, l);
                self.pc = Pc::Bonus(3);
                if core.peek(l.fire) == 0 {
                    self.delay(0, 0x0A);
                }
            }
            3 => {
                // `$2250`
                let t = l.time as usize;
                let ram = core.ram();
                if ram[t] == 0 {
                    let (hi, _) = blocks::sbc_decimal(ram[t + 1], 1, true);
                    ram[t + 1] = hi;
                    if hi & 0x80 != 0 {
                        return self.ret();
                    }
                }
                ram[t] = blocks::sbc_decimal(ram[t], 1, true).0;
                blocks::score_add(ram, l, 0x10);
                self.pc = Pc::Bonus(2);
            }
            10 => {
                // `$22A8`
                let (ram, color, _) = core.parts();
                let n = ram[l.bonus_stage as usize];
                text::number_at(ram, color, l, n, 7, 0x10);
                text::score(ram, color, l);
                self.pc = Pc::Bonus(11);
                if core.peek(l.fire) == 0 {
                    self.delay(0, 0x50);
                }
            }
            11 => {
                // `$2298`
                let n = core.peek(l.bonus_stage);
                if n == 0 {
                    return self.ret();
                }
                core.poke(l.bonus_stage, n - 1);
                score_add16(core.ram(), l, 0x00, 0x05);
                self.pc = Pc::Bonus(10);
            }
            _ => unreachable!(),
        }
    }

    /// `$1126..$12EA`: the intro, once after power-on (its end patches `$1D10` so that the title skips it): three
    /// pages that wait for fire, the page that loads the high-score file (here: [`Main::scores`]) and measures the
    /// machine's speed (PAL gives `$FE = $63`, the line "PAL" of `$1000`), then a delay that fire cuts short.
    fn intro(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::Intro(k + 1);
        match k {
            0 => {
                self.i = true;
                self.screen(core, false);
            }
            1 => {
                core.vic[0x20] = 0;
                core.vic[0x21] = 0;
                core.vic[0x22] = 5;
                core.vic[0x23] = 9;
                core.poke(l.intro_mode, 0);
                // the colour rows of the first page (`$1143..$1161`)
                core.poke(0x0291, 0x90);
                for x in 1..=0x90usize {
                    for (base, c) in [(0usize, 0x0F), (0x90, 0x0F), (0x120, 0x0F), (0x1B0, 0x0F), (0x268, 5), (0x2F8, 2), (0x36E, 1)] {
                        core.color[base + x - 1] = c;
                    }
                }
                core.vic[0x16] = 0x18;
                core.poke(1, 0x36);
                core.vic[0x18] = 0x20;
                self.screen(core, true);
            }
            2 => self.wait = Wait::Fire,
            3 => self.screen(core, false),
            4 => {
                clear_screen(core, false);
                for y in 1..=0xC0u16 {
                    for (src, dst) in [(0x14EEu16, 0xC3FFu16), (0x15AE, 0xC4BF), (0x166E, 0xC57F)] {
                        let v = core.peek(src + y);
                        core.poke(dst + y, v);
                    }
                }
                self.work(CLEAR_CYCLES + 0xC0 * 30, 0x119A);
            }
            5 => {
                if core.peek(l.charset) == 0 {
                    self.gosub(Pc::Swap(0), Pc::Intro(6));
                }
            }
            6 => self.screen(core, false),
            7 => {
                clear_screen(core, false);
                core.vic[0x21] = 0x0C;
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 0x0B, 0, 0);
                self.screen(core, true);
                self.work(CLEAR_CYCLES, 0x11AE);
            }
            8 => self.wait = Wait::Fire,
            9 => self.screen(core, false),
            10 => {
                clear_screen(core, false);
                core.vic[0x21] = 5;
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 0x0F, 0, 0);
                self.screen(core, true);
                self.work(CLEAR_CYCLES, 0x11C2);
            }
            11 => self.wait = Wait::Fire,
            12 => self.screen(core, false),
            13 => {
                clear_screen(core, false);
                core.vic[0x21] = 0x0A;
                self.screen(core, true);
                self.work(CLEAR_CYCLES, 0x11D0);
            }
            14 => {
                for y in 0..=0x12u16 {
                    let v = core.peek(l.intro_texts + y);
                    core.poke(l.screen + y, v);
                }
                // `$11DB`: the device number into `$CC3F`; `LOAD "HID2!G30A",8` to `$5192`: the score store
                let dev = core.peek(0xBA);
                core.poke(0xCC3F, dev);
                if let Some(bytes) = self.scores.take() {
                    hiscore::load_file(core.ram(), l, &bytes);
                }
                core.poke(1, 0x35);
                for y in 0..=0x14u16 {
                    let v = core.peek(l.intro_texts + 0x13 + y);
                    core.poke(l.screen + 0x50 + y, v);
                }
                // `$120F..$1266`: the speed test (cycles counted between two raster IRQs); a PAL machine gives these
                core.poke(l.power, 0x25);
                core.poke(l.diamonds, 0x63);
                core.poke(l.lives, 0);
                core.poke(0x02, 0);
                core.set_vector(l.nmi_vector, 0x1098);
                core.vic[0x11] = 0x1B;
                core.vic[0x12] = 0xC8;
                core.set_vector(l.irq_vector, 0x1088);
                core.vic[0x1A] = 0;
                let (ram, color, _) = core.parts();
                let fe = ram[l.diamonds as usize];
                text::bcd_at(ram, color, l, fe, 2, 4);
                for y in 0..=0x21u16 {
                    let v = ram[(l.intro_texts - 0x22 + y) as usize];
                    ram[(l.screen + 0xA5 + y) as usize] = v;
                }
                for y in 0..=0x0Fu16 {
                    let v = ram[(l.intro_pal + y) as usize];
                    ram[(l.screen + 0xF4 + y) as usize] = v;
                }
                self.i = false;
                self.work(INTRO_TEST_CYCLES, 0x12DC);
            }
            15 => {
                core.poke(l.intro_count, 0x0A);
                self.busy = INTRO_DELAY_CYCLES;
                self.busy_fire = true;
                self.busy_reads_input = true;
                self.busy_end = 0x106E;
            }
            16 => {
                core.poke(l.game_start, 0xAD);
                self.screen(core, false);
            }
            _ => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x12EA);
                self.pc = Pc::Title(0);
            }
        }
    }

    /// `$1D13..$1DD6`: the title: a fresh state, Giana's left-facing shapes, the attract stage 0 with its IRQ chain
    /// (`$2FC8` and the play handlers), the scrolling texts and the title song; then it waits for `1` or fire (one
    /// player), `2` (two) or `H` (the high-score table).
    fn title(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::Title(k + 1);
        match k {
            0 => {
                self.i = false;
                self.screen(core, false);
            }
            1 => {
                // `$4C08`, then `$8F5C`
                core.vic[0x1A] = 0;
                core.poke(1, 0x35);
                self.i = false;
                self.screen(core, false);
            }
            2 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x1D21);
            }
            3 => {
                screens::input_on(core.ram(), l);
                screens::reset_state(core);
                core.music.init();
                for y in 0..=0x1Fu16 {
                    core.poke(l.player_number + y, 0);
                    core.poke(l.hud_slots + y, 0);
                }
                if core.peek(l.charset) == 0 {
                    self.gosub(Pc::Swap(0), Pc::Title(4));
                }
            }
            4 => {
                let n = screens::sprite_mirror(core.ram(), l);
                self.work(n as i64 * 8700, 0x36B5);
                self.pc = Pc::Title(31);
            }
            31 => {
                // `$36B5..$3605`, the end of `$3676`
                power_sprites_end(core);
                self.pc = Pc::Title(5);
            }
            5 => {
                core.poke(l.power, 0);
                core.vic[0x18] = 0x20;
                core.poke(l.game_state, 0xFF);
                core.poke(l.auto_scroll, 0xFF);
                core.poke(l.probe, 0x60);
                core.poke(l.in_room, 1);
                core.poke(l.stage, 0);
                self.level(core, 0x1D5B, Pc::Title(6));
            }
            6 => {
                // `$27EA`: the title's chain: the `$FB` handler goes on at line `$31` with `$2FC8`
                self.i = true;
                core.poke(1, 0x35);
                core.poke(l.irq_fb_line_op, 0x31);
                core.poke(l.irq_fb_vector_lo_op, l.irq_title as u8);
                core.poke(l.irq_fb_vector_hi_op, (l.irq_title >> 8) as u8);
                core.poke(l.title_phase, 2);
                core.set_vector(l.nmi_vector, l.game_start);
                core.set_vector(l.irq_vector, l.irq_fb);
                core.vic[0x12] = 0xFC;
                self.screen(core, false);
            }
            7 => {
                core.vic[0x1A] = 1;
                self.i = false;
                // an old raster flag gives the IRQ now, before the wait below sets `$D7`
                self.pc = Pc::Title(30);
            }
            30 => {
                self.pc = Pc::Title(8);
                self.screen(core, true);
            }
            8 => {
                core.poke(l.scroll_column, 7);
                for x in 0..=0x23u16 {
                    let v = core.peek(l.title_text - 0x24 + x);
                    core.poke(l.screen + 1 + x, v);
                }
                core.music.restart_song(3);
                core.poke(l.probe, 0x60);
                core.poke(l.in_room, 1);
                core.poke(l.game_state, 0);
            }
            9 => {
                // `$1D82`: the keys
                if core.scan(0xF7) == 0xDF {
                    core.poke(l.game_over_flags + 1, 1);
                    self.pc = Pc::GameOver(10);
                    return;
                }
                let k = core.scan(0x7F);
                if k == 0xFE || (k != 0xF7 && core.peek(l.fire) != 0) {
                    core.poke(l.game_over_flags + 1, 1);
                } else if k != 0xF7 {
                    self.pc = Pc::Title(9);
                    self.wait = Wait::Yield;
                    return;
                }
                self.screen(core, false);
            }
            10 => {
                core.poke(l.probe, 0xA5);
                core.poke(l.game_state, 0xA5);
                core.poke(l.auto_scroll, 0);
                core.poke(l.in_room, 0);
                core.poke(l.stage, 0);
                core.poke(l.diamonds, 0);
                core.poke(l.select_cursor, 0);
                core.poke(l.player_slots + 0x0F, 1);
                core.poke(l.player_slots + 0x11, 5);
                core.poke(l.lives, 5);
                if core.peek(l.saved_game) != 0 {
                    self.pc = Pc::SavedGame(0);
                    return;
                }
                self.pc = Pc::StageNext;
            }
            _ => unreachable!(),
        }
    }

    /// `$2172..$21CC` (and `$4DF4..$4E50`): game over: the high-score screen (its own IRQ through the KERNAL), the
    /// player's rank in the two tables and the name entry (`$4E67`), the table until fire, up (save: [`Main::save`]),
    /// space or F1; then the title. `GameOver(10)` is the title's `H`.
    fn game_over(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::GameOver(k + 1);
        match k {
            0 => {
                let y = core.peek(l.player_number) as u16;
                core.poke(l.game_over_flags + y, 1);
                self.screen(core, false);
            }
            1 | 10 => {
                if k == 10 {
                    self.pc = Pc::GameOver(11);
                    self.screen(core, false);
                    return;
                }
                screens::reset_state(core);
                core.music.restart_song(0);
                self.screen(core, false);
            }
            11 => {
                screens::reset_state(core);
                core.music.restart_song(0);
                self.pc = Pc::GameOver(2);
                self.screen(core, false);
            }
            2 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x4DF7);
            }
            3 => {
                if core.peek(l.charset) == 0 {
                    self.gosub(Pc::Swap(0), Pc::GameOver(4));
                }
            }
            4 => {
                hiscore_screen(core);
                self.i = false;
                self.screen(core, true);
                self.work(HISCORE_CYCLES, 0x2187);
            }
            5 => {
                let ram = core.ram();
                hiscore::record(ram, l);
                hiscore::select_table(ram, l, false);
                let mut qualifies = hiscore::rank(ram, l).is_some();
                if !qualifies {
                    hiscore::select_table(ram, l, true);
                    qualifies = hiscore::rank(ram, l).is_some();
                }
                if qualifies {
                    self.gosub(Pc::Entry(0), Pc::GameOver(6));
                }
            }
            6 => {
                for x in 0..=0x1Fu16 {
                    let v = core.peek(l.hiscore_prompt + x);
                    core.poke(l.hud_slots - 0x1C + x, v);
                }
            }
            7 => {
                // `$21A8`
                let joy = core.joy;
                let _ = player::read_input(core.ram(), l, joy, 0);
                let up = core.peek(l.joy) & 1 != 0;
                let key = core.peek(l.kernal_key);
                if up {
                    self.save = Some(hiscore::file_bytes(core.ram(), l));
                } else if core.peek(l.fire) == 0 && key != 0x3C && key != 0x04 {
                    self.pc = Pc::GameOver(7);
                    self.wait = Wait::Yield;
                }
            }
            _ => {
                core.poke(l.kernal_key, 0);
                core.vic[0x1A] = 0;
                core.poke(1, 0x35);
                self.i = false;
                self.pc = Pc::StageRestart;
            }
        }
    }

    /// `$4E67..$4F93`: the name entry: keys (`GETIN`: letters, digits, a few signs, DEL, RETURN) or the joystick (up and
    /// down choose the letter, left and right move, fire on the last place ends); every new letter scrolls in big
    /// (the IRQ's `$5103`) and the program waits for it. RETURN or the end: the record into both tables (`$500F`).
    fn entry(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        match k {
            0 => {
                core.poke(l.spawn_list, 3);
                for x in 0..=0x1Bu16 {
                    let v = core.peek(l.hiscore_prompt + 0x20 + x);
                    core.poke(l.hud_slots - 0x1A + x, v);
                }
                core.poke(0x4E, 0);
                core.poke(l.kernal_key - 5, 0);
                core.poke(l.entry_cursor, 0);
                self.pc = Pc::Entry(1);
            }
            1 => {
                // `$4E7C`/`$4E82`
                core.poke(l.joy, 0);
                let y = core.peek(l.entry_cursor);
                match core.typed.pop_front() {
                    Some(0x0D) => {
                        self.pc = Pc::Entry(9);
                    }
                    Some(0x14) => {
                        core.poke(0x4E, 0);
                        if y != 0x0B {
                            core.poke(l.entry_name + y as u16, 0x2E);
                        }
                        if y == 0 {
                            return;
                        }
                        let y = y - 1;
                        core.poke(l.entry_cursor, y);
                        let c = core.peek(l.entry_name + y as u16);
                        core.poke(l.entry_letter, c);
                        core.poke(l.entry_letter + 2, 0);
                        self.letter(core, false);
                        self.pc = Pc::Entry(7);
                    }
                    Some(c) => {
                        core.poke(0x4E, 0);
                        let mapped = map_key(core, c);
                        if let Some(code) = mapped {
                            core.poke(l.entry_letter + 2, code);
                            core.poke(l.entry_letter, 0);
                            self.letter(core, true);
                            if y < 0x0A {
                                core.poke(l.entry_cursor, y + 1);
                            }
                        }
                    }
                    None => self.entry_joystick(core),
                }
            }
            8 => {
                // the letter has scrolled in: `$4F8E`
                let y = core.peek(l.entry_cursor) as u16;
                let c = core.peek(l.entry_letter + 2);
                core.poke(l.entry_name + y, c);
                self.pc = Pc::Entry(1);
            }
            7 => {
                // after DEL's letter: `$4EEE`
                let y = core.peek(l.entry_cursor) as u16;
                core.poke(l.entry_name + y, 0x2E);
                self.pc = Pc::Entry(1);
            }
            9 => {
                let ram = core.ram();
                hiscore::select_table(ram, l, false);
                hiscore::insert(ram, l);
                hiscore::select_table(ram, l, true);
                hiscore::insert(ram, l);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 0x15, 7, 0);
                text::print_at(ram, color, l, 0x16, 7, 0x0C);
                self.ret();
            }
            _ => self.pc = Pc::Entry(1),
        }
    }

    /// `$4EF5..$4F58`: the joystick in the name entry.
    fn entry_joystick(&mut self, core: &mut Core) {
        let l = core.l;
        let joy = core.joy;
        let _ = player::read_input(core.ram(), l, joy, 0);
        let mut y = core.peek(l.entry_cursor);
        let fire = core.peek(l.fire);
        if fire != 0 {
            if core.peek(0x4E) != 0 {
                if y == 0x0A {
                    self.pc = Pc::Entry(9);
                    self.wait = Wait::FireUp;
                    return;
                }
                y = 0x0A;
                core.poke(l.entry_cursor, y);
            }
            core.poke(0x4E, fire);
            self.wait = Wait::FireUp;
            return;
        }
        let dir = core.peek(l.joy) & 0x7F;
        if dir == 0 {
            self.wait = Wait::Yield;
            return;
        }
        core.poke(0x4E, dir);
        let mut x = core.peek(l.entry_name + y as u16);
        if x == 0x2E {
            x = 0;
        }
        core.poke(l.entry_letter, x);
        if dir & 3 != 0 {
            if dir & 1 != 0 {
                x += 1;
                if x >= 0x3A {
                    x = 1;
                }
            } else {
                x = if x == 0 { 0x39 } else { x - 1 };
            }
            if x == 0x27 {
                x = 0x30;
            }
            if x == 0x2F {
                x = 0x26;
            }
            core.poke(l.entry_letter + 2, x);
            self.letter(core, dir & 1 == 0);
            return;
        }
        if core.peek(l.entry_delay) & 0x80 == 0 {
            self.wait = Wait::Yield;
            return;
        }
        core.poke(l.entry_delay, 5);
        let ny = if dir & 4 != 0 { y.wrapping_sub(1) } else { y + 1 };
        if ny < 0x0B {
            core.poke(l.entry_cursor, ny);
        }
        self.wait = Wait::Yield;
    }

    /// `$4F60` (`down = true`: the new letter `$41` comes in from below) / `$4F6E`: start the letter scroll and wait
    /// for the IRQ to end it (`$44 = 0`).
    fn letter(&mut self, core: &mut Core, down: bool) {
        let l = core.l;
        let e = l.entry_letter;
        let (old, new) = (core.peek(e), core.peek(e + 2));
        let (dir, row) = if down { (1u8, 0u8) } else { (0xFF, 8) };
        core.poke(e + 3, if down { old } else { new });
        if down {
            core.poke(e, new);
        }
        core.poke(e + 6, row);
        // `$4F94`: the two codes as character addresses (`$C000 + 8 * code`), the second one row on
        for (zp, code) in [(e, core.peek(e)), (e + 3, core.peek(e + 3).wrapping_add(1))] {
            let a = l.charset.wrapping_add(code as u16 * 8);
            core.poke(zp, a as u8);
            core.poke(zp + 1, (a >> 8) as u8);
        }
        let hi = core.peek(e + 4).wrapping_sub(1);
        core.poke(e + 4, hi);
        core.poke(e + 5, dir);
        self.wait = Wait::Letter;
        self.pc = Pc::Entry(8);
    }

    /// `$492B..$4973`: the stage select of a new game (offered once the highest stage reached `$520F` is 2 or more):
    /// after fire is let go, the menu screen ([`Pc::ScreenReset`]) with the player's name and "select stage", the stage
    /// number `$0313` (from 1); up and down (the joystick alone) change it within 1..`$520F`, each change printed with a
    /// delay; fire takes it (`$FC`), `$0313 = 0` and the screen is cleared.
    fn stage_select(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::StageSelect(k + 1);
        match k {
            0 => core.poke(l.select_cursor, 1),
            1 => {
                // `$4930`: until fire is let go (the port read directly)
                if core.joy & 0x10 == 0 {
                    self.pc = Pc::StageSelect(1);
                    self.wait = Wait::Yield;
                    return;
                }
                self.gosub(Pc::ScreenReset(0), Pc::StageSelect(2));
            }
            2 => self.screen(core, true),
            3 => {
                let n = core.peek(l.player_number);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, n, 0x11, 9);
                for x in 0..0x12u16 {
                    ram[(l.screen + 0x1EB + x) as usize] = ram[(l.select_text + x) as usize];
                }
            }
            4 => {
                // `$4952`: the number, then `$371E` with `X` = its tens (left by `$234F`), `Y = $40`
                let n = core.peek(l.select_cursor);
                let (ram, color, _) = core.parts();
                text::number_at(ram, color, l, n, 0x13, 0x0E);
                self.delay(n / 10, 0x40);
            }
            5 => {
                // `$4961`
                let joy = core.joy;
                let n = core.peek(l.select_cursor);
                if joy & 0x10 == 0 {
                    core.poke(l.stage, n);
                    core.poke(l.select_cursor, 0);
                    clear_screen(core, false);
                    self.work(CLEAR_CYCLES, 0x4973);
                    return;
                }
                let highest = core.peek(l.highest_stage);
                match joy & 0x7F {
                    0x7E if n.wrapping_add(1) <= highest => {
                        core.poke(l.select_cursor, n + 1);
                        self.pc = Pc::StageSelect(4);
                    }
                    0x7D if n.wrapping_sub(1) != 0 => {
                        core.poke(l.select_cursor, n - 1);
                        self.pc = Pc::StageSelect(4);
                    }
                    _ => {
                        self.pc = Pc::StageSelect(5);
                        self.wait = Wait::Yield;
                    }
                }
            }
            _ => self.ret(),
        }
    }

    /// `$1E5B..$1F65`: the stage start.
    fn stage_start(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::StageStart(k + 1);
        match k {
            0 => {
                let v = core.peek(l.stage_counter).wrapping_add(1);
                core.poke(l.stage_counter, v);
                screens::input_on(core.ram(), l);
                clear_animations(core.ram(), l);
                self.screen(core, false);
            }
            1 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x1E67);
            }
            2 => {
                if core.peek(l.game_over_flags) & core.peek(l.game_over_flags + 1) != 0 {
                    self.pc = start_pc(core);
                    return;
                }
                players_swap(core);
                self.pc = Pc::StageStart(32);
            }
            32 => {
                // `$48EC`
                let first = core.peek(l.stage) | core.peek(l.score) | core.peek(l.score + 1);
                if first == 1 && core.peek(l.lives) == 5 {
                    if core.peek(l.highest_stage) >= 2 {
                        return self.gosub(Pc::StageSelect(0), Pc::StageStart(30));
                    }
                    stage_select_lives(core);
                }
                self.pc = Pc::StageStart(31);
            }
            30 => {
                // back from the stage select (`$4901`)
                stage_select_lives(core);
                self.pc = Pc::StageStart(31);
            }
            31 => {
                stage_select_end(core);
                // up to `$1EFA` (`JSR $419C`, which turns the raster IRQ off): an IRQ in between still comes
                self.work(SLOTS_CYCLES, 0x1EFA);
                self.pc = Pc::StageStart(3);
            }
            3 => {
                screens::reset_state(core);
                screens::clear_objects(core.ram(), l);
                self.gosub(Pc::ScreenReset(0), Pc::StageStart(4));
            }
            4 => {
                let n = core.peek(l.player_number);
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, n, 0x11, 7);
                text::print(ram, color, l, 2);
                text::print(ram, color, l, 3);
                let st = ram[l.stage as usize];
                text::number(ram, color, l, st);
                core.music.start_song(6);
                self.level(core, 0x1F24, Pc::StageStart(5));
            }
            5 => {
                core.poke(l.time_frames, 0x28);
                core.poke(l.time, core.peek(l.stage_header + 4));
                core.poke(l.time + 1, core.peek(l.stage_header + 5));
                self.gosub(Pc::PowerSprites(0), Pc::StageStart(6));
            }
            6 => {
                let (ram, color, _) = core.parts();
                text::print_at(ram, color, l, 0x17, 0x0A, 9);
                self.screen(core, true);
            }
            7 => {
                // `$21FA`: fire pressed and let go
                self.wait = Wait::Fire;
            }
            8 => {
                let song = core.peek(l.stage_header + 3);
                core.music.start_song(song);
                // the song start and `$2818` up to its IRQ setup (`$2845`): the moment decides whether the `$FB`
                // IRQ at line `$FC` still comes in this frame
                self.work(SONG_CYCLES + 60, 0x2845);
                self.pc = Pc::StageStart(20);
            }
            20 => {
                self.pc = Pc::StageStart(9);
                screens::play_irq(core);
                self.i = true;
                self.screen(core, false);
            }
            9 => {
                core.vic[0x1A] = 1;
                self.i = false;
                self.pc = Pc::StageStart(24);
            }
            24 => {
                self.pc = Pc::StageStart(10);
                self.screen(core, false);
            }
            10 => {
                clear_screen(core, true);
                self.work(CLEAR_CYCLES, 0x1F4D);
            }
            11 => {
                if core.peek(l.charset) != 0 {
                    self.gosub(Pc::Swap(0), Pc::StageStart(12));
                }
            }
            12 => {
                core.poke(l.cloud_columns, 5);
                core.poke(l.cloud_columns + 1, 0x13);
                core.poke(l.cloud_columns + 2, 0x1F);
                screens::cloud_band(core);
                let (ram, color, _) = core.parts();
                screens::draw_playfield(ram, color, l);
                self.work(PLAYFIELD_CYCLES, 0x1F62);
            }
            13 => {
                // `$2646`: the status bar rows, the full status bar, then its texts (their screen-on wait); the `$FB`
                // IRQ's operands for the status bar (`$26F2`) change after the first part (an IRQ can come in between)
                let ops = (core.peek(l.hud_d016_op), core.peek(l.hud_d023_op));
                top_rows_clear(core);
                core.poke(l.hud_d016_op, ops.0);
                core.poke(l.hud_d023_op, ops.1);
                self.work(TOP_ROWS_OPS_CYCLES, 0x26F2);
                self.pc = Pc::StageStart(25);
            }
            25 => {
                let (a, x) = blocks::hud_operands(core.ram(), l);
                core.poke(l.hud_d023_op, a);
                core.poke(l.hud_d016_op, x);
                self.work(TOP_ROWS_CYCLES - TOP_ROWS_OPS_CYCLES, 0x2658);
                self.pc = Pc::StageStart(14);
            }
            14 => {
                // `$2658..$2670`: each text of the printer ends with its screen-on wait (here the first one waits: the
                // screen is still off); the printing takes its time before it
                let n = core.peek(l.player_number);
                core.poke(l.print_col, 0);
                core.poke(l.print_col + 1, 0);
                self.print_wait(core, n);
                self.pc = Pc::StageStart(21);
            }
            21 => {
                self.print_wait(core, 4);
                self.pc = Pc::StageStart(22);
            }
            22 => {
                let (ram, color, _) = core.parts();
                let st = ram[l.stage as usize];
                text::number_at(ram, color, l, st, 0x1D, 1);
                self.pc = Pc::StageStart(15);
            }
            15 => {
                screens::giana_start(core);
                self.wait = Wait::Sprite0Y;
            }
            16 => {
                screens::giana_start_spawn(core);
                self.work(SPAWN_CYCLES, 0x35E2);
                self.pc = Pc::StageStart(23);
            }
            23 => {
                screens::giana_start_end(core);
                self.pc = Pc::MainLoop;
            }
            _ => unreachable!(),
        }
    }

    /// `$360A`: Giana's sprite set for the power level (`$FD`): when it changes (the shapes at `$ED04` show which set
    /// is in), Giana flashes for a delay, the sets are swapped and the left-facing shapes made again.
    fn power_sprites(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        match k {
            0 => {
                core.poke(l.pickup_taken, 0);
                let powered = core.peek(l.giana_shapes + 4) == 0x7C;
                if core.peek(l.power) != 0 {
                    if !powered {
                        screens::giana_colours(core);
                        return self.ret();
                    }
                    core.poke(l.effect, 4);
                } else if powered {
                    screens::giana_colours(core);
                    return self.ret();
                }
                core.poke(l.layer_hires, 0xFF);
                core.poke(l.game_state, 0xFF);
                player::set_frames(core.ram(), l, 0xF3);
                core.vic[0x1C] &= 0xF8;
                core.poke(l.layer0_colour, 1);
                core.poke(l.layer_offset_ops, 0xFE);
                core.poke(l.layer_offset_ops + 5, 3);
                self.delay(3, 0xC0);
                self.pc = Pc::PowerSprites(1);
            }
            1 => {
                let n = screens::sprite_swap(core.ram(), l);
                self.work(n as i64 * 1650, 0x3676);
                self.pc = Pc::PowerSprites(2);
            }
            2 => {
                let n = screens::sprite_mirror(core.ram(), l);
                self.work(n as i64 * 8700, 0x36B5);
                self.pc = Pc::PowerSprites(3);
            }
            _ => {
                power_sprites_end(core);
                self.ret();
            }
        }
    }

    /// One pass of the main loop `$1F68..$20F0`.
    fn main_loop(&mut self, core: &mut Core, orig: &mut dyn Original) {
        let l = core.l;
        if core.peek(l.end_bump) != 0 {
            self.pc = Pc::StageClear(0);
            return;
        }
        if core.peek(l.stage_end) != 0 {
            // `$1FDC`
            if core.peek(l.in_room) != 0 {
                core.poke(l.state_02e0, 0xFF);
            } else {
                let st = core.peek(l.stage);
                if st >= 0x0E {
                    self.pc = Pc::StageClear(10);
                } else {
                    core.poke(l.bonus_stage, st);
                    self.pc = Pc::StageClear(20);
                }
                return;
            }
        }
        if core.peek(l.state_95) != 0 {
            if !self.pause_menu(core) {
                return;
            }
        } else {
            // `$20C6`
            let redraw = core.peek(l.pause_state);
            if redraw != 0 && redraw & 0x80 == 0 {
                top_rows(core);
                core.poke(l.pause_state, 0);
            }
            let (ram, color, vic) = core.parts();
            blocks::hud(ram, color, vic, l);
            if core.peek(l.in_room) != 0 {
                // `$20DA`: the treasure rooms (`$3FA1`)
                let _ = orig;
                match rooms::room_logic(core.ram(), l) {
                    RoomPath::Inside => {
                        // `$4057`: its first instruction reads `$D01E` (at the main program's raster time)
                        let line = ((self.now / 63) as u16 + FRAME_LINE) % LINES;
                        let rel = |x: u16| (x + LINES - FRAME_LINE) % LINES;
                        if rel(line) > rel(core.line) {
                            core.raster_to(line);
                        }
                        let coll = core.unit.read_collisions();
                        rooms::room_exit_check(core.ram(), l, coll);
                        self.main_loop_tail(core);
                        if self.wait == Wait::Yield {
                            self.wait = Wait::Pass(ROOM_PASS_CYCLES);
                        }
                        return;
                    }
                    RoomPath::Entering => return self.gosub(Pc::Room(0), Pc::Room(99)),
                    RoomPath::Rising => {
                        // `$409E`: the wait `$4194` for the raster, then the lift
                        self.wait = Wait::Sprite0Y;
                        return self.gosub(Pc::Room(20), Pc::Room(99));
                    }
                    RoomPath::Leaving => return self.gosub(Pc::Room(30), Pc::Room(99)),
                }
            }
        }
        self.main_loop_tail(core);
    }

    /// `$20DD..$20F0`: the death check and the power-up change.
    fn main_loop_tail(&mut self, core: &mut Core) {
        let l = core.l;
        let dying = core.peek(l.dying);
        // (`$20F3`: in a treasure room `$99 = $FF` does not start the death sequence; [`player::death_start`])
        if dying != 0 && (core.peek(l.in_room) == 0 || dying != 0xFF) {
            self.pc = Pc::DeathStart;
            return;
        }
        if core.peek(l.pickup_taken) != 0 {
            self.gosub(Pc::PowerSprites(0), Pc::MainLoopPower);
            return;
        }
        self.pc = Pc::MainLoop;
        self.wait = Wait::Yield;
    }

    /// The treasure rooms' code of the main loop between its waits (the order of [`crate::rooms`]): `k` = 0.. going in
    /// (`$3FB9`), 20 the lift rising (`$40A1`), 30.. back into the stage (`$40C3`), 99 the rest of the main loop pass.
    fn room(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::Room(k + 1);
        match k {
            // `$3F8C`: until the block animations of the IRQs are over
            0 | 30 if !animations_idle(core.ram(), l) => {
                self.pc = Pc::Room(k);
                self.wait = Wait::Yield;
            }
            0 => {
                rooms::room_mark(core.ram(), l);
                self.screen(core, false);
            }
            1 => {
                rooms::room_select(core.ram(), l);
                self.level(core, 0x3FD9, Pc::Room(2));
            }
            2 => {
                screens::cloud_band(core);
                rooms::room_hide(core.ram(), l);
                let (ram, color, _) = core.parts();
                screens::screen_swap(ram, color, l);
                self.work(SWAP_SCREEN_CYCLES, 0x3FE7);
            }
            3 => {
                // `$35A1` with `Y = 0`, up to its wait `$4194`
                screens::giana_start_at(core, 0);
                self.wait = Wait::Sprite0Y;
            }
            4 => {
                // `$35C0`: in a room no spawn, `$35E2`
                screens::giana_start_spawn(core);
                screens::giana_start_end(core);
                let (ram, _, vic) = core.parts();
                rooms::room_place(ram, vic, l);
                self.screen(core, true);
            }
            5 | 21 | 39 => self.ret(),
            20 => {
                if rooms::room_rise(core.ram(), l) {
                    // at the top: `$40B4`, then the way back
                    self.pc = Pc::Room(30);
                } else {
                    self.ret();
                }
            }
            30 => self.screen(core, false),
            31 => {
                rooms::room_scroll_back(core.ram(), l);
                let (ram, color, _) = core.parts();
                let printed = screens::stage_setup(ram, color, l);
                screens::screen_swap(ram, color, l);
                if printed {
                    self.screen(core, true);
                }
                self.work(SETUP_CYCLES + SWAP_SCREEN_CYCLES, 0x40D4);
            }
            32 => {
                screens::cloud_band(core);
                self.screen(core, true);
            }
            33 => self.level(core, 0x40DD, Pc::Room(34)),
            34 => {
                let (ram, color, _) = core.parts();
                rooms::room_door(ram, color, l);
                self.work(DOOR_CYCLES, 0x412C);
            }
            // `$412C..$4148` in its two parts: the `$FB` IRQ can come in between
            35 => {
                let (ram, _, vic) = core.parts();
                rooms::room_back_restore(ram, vic, l);
                self.work(BACK_CYCLES, 0x4135);
            }
            36 => {
                let (x, y) = rooms::room_back_flags(core.ram(), l);
                self.delay(x, y);
            }
            37 => {
                // the delay leaves `Y = 0`
                rooms::room_done(core.ram(), l, 0);
                self.pc = Pc::Room(39);
            }
            99 => self.main_loop_tail(core),
            _ => unreachable!("room step {k}"),
        }
    }

    /// `$203C..$20C3`: a pass of the pause menu (`$95` set; the IRQs' key code `$7222` opens, blinks and closes it,
    /// [`keys`]). When it has just been opened (`$96` negative): the status rows cleared, the menu's texts (`$11`, and
    /// `$13` "restart here" when Giana is away from the restart column), its last row in `$DF`. Then up and down move
    /// the cursor `$97` (on a change of the joystick byte), fire chooses: 0 continue (`$97 = $FF`, the IRQ closes the
    /// menu), 1 lose a life (`$3F28` with `$FE`), 2 quit (`$1D10`, the title), 3 save and quit ([`Pc::SaveQuit`]).
    /// Returns `false` when the main program leaves the main loop.
    fn pause_menu(&mut self, core: &mut Core) -> bool {
        let l = core.l;
        if core.peek(l.pause_state) & 0x80 != 0 {
            // `$2040`
            item_chars(core.ram(), l, l.item_icon_none);
            let s = l.screen as usize;
            core.ram()[s..s + 0xC8].fill(0);
            core.poke(l.hud_d016_op, 7);
            let (ram, color, _) = core.parts();
            text::print_at(ram, color, l, 0x11, 6, 0);
            let mut last = 3;
            if ram[l.scroll_column as usize].wrapping_sub(0x27) != ram[l.restart_column as usize] {
                text::print_at(ram, color, l, 0x13, 6, 4);
                last = 2;
            }
            ram[l.menu_last as usize] = last;
            ram[l.pause_state as usize] = 0;
            return true;
        }
        // `$207A`
        let j = core.peek(l.joy);
        if j != core.peek(l.menu_joy) {
            core.poke(l.menu_joy, j);
            let x = core.peek(l.menu_cursor);
            if j & 1 != 0 {
                if x != 0 {
                    core.poke(l.menu_cursor, x.wrapping_sub(1));
                    menu_row_colour(core, x, 3);
                }
                return true;
            }
            if j & 2 != 0 {
                if x < core.peek(l.menu_last) {
                    core.poke(l.menu_cursor, x.wrapping_add(1));
                    menu_row_colour(core, x, 3);
                }
                return true;
            }
        }
        // `$20A2`: fire
        let x = core.peek(l.menu_cursor);
        if core.peek(l.fire) == 0 || x & 0x80 != 0 {
            return true;
        }
        match x {
            0 => core.poke(l.menu_cursor, 0xFF),
            1 => {
                let _ = player::die(core.ram(), l, 0xFE, 0);
            }
            2 => {
                self.pc = start_pc(core);
                return false;
            }
            _ => {
                self.pc = Pc::SaveQuit(0);
                return false;
            }
        }
        true
    }

    /// `$21CF..$21F9` and the KERNAL save `$4CBE`: the player state (`$02E1..$02F7`) into the saved game `$5211..`,
    /// `$5210 = $FF`, the play state reset (`$419C`), the KERNAL in (`$01 = $36`, the raster IRQ off, `$0314 = $EA31`), the
    /// screen cleared, the game font, the save screen ("saving", white background), the file to the app's store
    /// ([`Main::save`]); then the game starts again at `$1D10` (the title, which offers the saved game).
    fn save_quit(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::SaveQuit(k + 1);
        match k {
            0 => {
                for y in 0..=0x16u16 {
                    let v = core.peek(l.player_number + y);
                    core.poke(l.saved_game + 1 + y, v);
                }
                core.poke(l.saved_game, 0xFF);
                screens::reset_state(core);
                self.i = true;
                core.poke(1, 0x36);
                core.vic[0x1A] = 0;
                core.poke(l.kernal_key - 5, 0);
                core.set_vector(l.kernal_irq_vector, 0xEA31);
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x21F3);
            }
            1 => {
                if core.peek(l.charset) == 0 {
                    self.gosub(Pc::Swap(0), Pc::SaveQuit(2));
                }
            }
            2 => {
                // `$4CBE`
                clear_screen(core, false);
                core.poke(0x9D, 0);
                core.vic[0x21] = 1;
                for x in 0..=0x0Au16 {
                    let v = core.peek(l.saving_text + x);
                    core.poke(l.screen + 0x1C6 + x, v);
                }
                self.work(CLEAR_CYCLES, 0x4CD8);
            }
            3 => self.screen(core, true),
            10 => {
                // `$1E36`: the saved game is taken: `$21E2` with `$5210 = 0`, the same steps as above
                core.poke(l.saved_game, 0);
                screens::reset_state(core);
                self.i = true;
                core.poke(1, 0x36);
                core.vic[0x1A] = 0;
                core.poke(l.kernal_key - 5, 0);
                core.set_vector(l.kernal_irq_vector, 0xEA31);
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x21F3);
                self.pc = Pc::SaveQuit(1);
            }
            _ => {
                self.save = Some(hiscore::file_bytes(core.ram(), l));
                if core.peek(l.saved_game) != 0 {
                    self.pc = start_pc(core);
                } else {
                    // back in `$1E3B` (the saved game's start): `$4C08`, `$27B2`, the player's state, `$48EC`
                    core.vic[0x1A] = 0;
                    core.poke(1, 0x35);
                    self.i = false;
                    // `JSR $27B2` (the moment the original reaches it)
                    self.work(1, 0x27B2);
                    self.gosub(Pc::ScreenReset(2), Pc::SavedGame(20));
                }
            }
        }
    }

    /// `$47B9..$48E9`: the end of the game (the stage-end pickup of the last stage, `$02D6`): the bonus screen counts
    /// the diamonds (`$FE`, 75 points each), the power level (`$FD`, 1,000 each) and the lives (`$FB`, 7,500 each)
    /// into the score, one a step with a delay that fire skips; then the end text (the outro placeholder), the game
    /// over flag of the player, the player's name and text 9, and the game over (`$2172`, the high-score screen).
    fn game_end(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::GameEnd(k + 1);
        let at = |core: &mut Core, x: u8, y: u8| {
            core.poke(l.print_col, x);
            core.poke(l.print_col + 1, y);
        };
        // `$47A2` (the score at column `$1B` of the current row), then the delay `$371E` with `X` = the score's last
        // byte (left by the printer) and `y`, unless fire is held
        let score_line = |me: &mut Main, core: &mut Core, y: u8| {
            core.poke(l.print_col, 0x1B);
            let (ram, color, _) = core.parts();
            for i in (0..3u16).rev() {
                let v = ram[(l.score + i) as usize];
                text::bcd(ram, color, l, v);
            }
            if core.peek(l.fire) == 0 {
                let x = core.peek(l.score);
                me.delay(x, y);
            }
        };
        let put = |core: &mut Core, at: u16, v: &[u8]| {
            for (i, &c) in v.iter().enumerate() {
                core.poke(l.screen + at + i as u16, c);
            }
        };
        let copy = |core: &mut Core, from: u16, at: u16| {
            for x in 0..5u16 {
                let v = core.peek(from + x);
                core.poke(l.screen + at + x, v);
            }
        };
        match k {
            0 => self.screen(core, false),
            1 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x47BC);
            }
            2 => {
                core.vic[0x12] = 0x41;
                core.poke(l.menu_bands, 1);
                core.poke(l.menu_bands + 2, 1);
                core.poke(l.menu_bands + 1, 0x15);
                at(core, 9, 2);
                self.print_wait(core, 0x0C);
            }
            3 | 7 | 11 => {
                // `$479B`: text 8 at column 6 of row 6, `$0B`, `$10`
                let row = [6, 0x0B, 0x10][(k / 4) as usize];
                at(core, 6, row);
                self.print_wait(core, 8);
            }
            4 => {
                copy(core, l.end_word_bonus, 0xF6);
                put(core, 0x152, &[0x37, 0x35]);
            }
            5 => {
                let (ram, color, _) = core.parts();
                let n = ram[l.diamonds as usize];
                text::bcd_at(ram, color, l, n, 8, 8);
                score_line(self, core, 0x20);
            }
            6 => {
                let n = core.peek(l.diamonds);
                if n != 0 {
                    core.poke(l.diamonds, blocks::sbc_decimal(n, 1, true).0);
                    blocks::score_add(core.ram(), l, 0x75);
                    self.pc = Pc::GameEnd(5);
                }
            }
            8 => {
                copy(core, l.end_word_extra, 0x1BE);
                put(core, 0x219, &[0x31, 0x30]);
                put(core, 0x21C, &[0x30]);
            }
            9 => {
                let n = core.peek(l.power);
                core.poke(l.screen + 0x210, n.wrapping_add(0x30));
                core.poke(l.print_col + 1, 0x0D);
                score_line(self, core, 0x60);
            }
            10 => {
                let n = core.peek(l.power);
                if n != 0 {
                    core.poke(l.power, n - 1);
                    score_add16(core.ram(), l, 0, 0x10);
                    self.pc = Pc::GameEnd(9);
                }
            }
            12 => {
                copy(core, l.end_word_lives, 0x286);
                put(core, 0x2E1, &[0x37, 0x35]);
                put(core, 0x2E4, &[0x30]);
            }
            13 => {
                let (ram, color, _) = core.parts();
                let n = ram[l.lives as usize];
                text::bcd_at(ram, color, l, n, 8, 0x12);
                score_line(self, core, 0x60);
            }
            14 => {
                let n = core.peek(l.lives);
                if n != 0 {
                    core.poke(l.lives, blocks::sbc_decimal(n, 1, true).0);
                    score_add16(core.ram(), l, 0, 0x75);
                    self.pc = Pc::GameEnd(13);
                }
            }
            15 => {
                at(core, 3, 0x18);
                self.print_wait(core, 0x0D);
            }
            16 => self.delay_units(4),
            17 => {
                let y = core.peek(l.player_number) as u16;
                core.poke(l.game_over_flags + y, 0xFF);
                self.i = true;
                self.screen(core, false);
            }
            18 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x48CC);
            }
            19 => {
                core.poke(l.menu_bands + 1, 0x10);
                let n = core.peek(l.player_number);
                at(core, 0x0C, 9);
                self.print_wait(core, n);
            }
            20 => self.print_wait(core, 9),
            21 => self.delay_units(0x0A),
            _ => {
                core.poke(l.fire, 0);
                self.pc = Pc::GameOver(0);
            }
        }
    }

    /// `$1DD8..$1E44`: the title's menu when a saved game exists (`$5210`): the screen cleared, its text, the play
    /// IRQ chain, the game font, fire let go, then up and down choose between the two rows (colours 1 and 3 of rows 9
    /// and 12); fire takes the row: the first continues the saved game (its player state from `$5211..` into
    /// `$02E1..`, the saved game cleared and the file saved, [`Pc::SaveQuit`] from 10), the second starts a new game.
    fn saved_game(&mut self, core: &mut Core, k: u8) {
        let l = core.l;
        self.pc = Pc::SavedGame(k + 1);
        match k {
            0 => {
                screens::input_on(core.ram(), l);
                self.screen(core, false);
            }
            1 => {
                clear_screen(core, false);
                self.work(CLEAR_CYCLES, 0x1DDE);
            }
            2 => {
                core.poke(l.print_col, 5);
                core.poke(l.print_col + 1, 9);
                self.print_wait(core, 0x12);
            }
            3 => {
                screens::reset_state(core);
                screens::play_irq(core);
                self.i = true;
                self.screen(core, false);
            }
            4 => {
                core.vic[0x1A] = 1;
                self.i = false;
                if core.peek(l.charset) != 0 {
                    self.gosub(Pc::Swap(0), Pc::SavedGame(5));
                }
            }
            5 => self.wait = Wait::FireUp,
            6 => {
                core.poke(l.fire, 0);
                self.screen(core, true);
            }
            7 | 8 => {
                if k == 7 {
                    core.poke(l.print_ptr, 1);
                    core.poke(l.print_ptr + 1, 3);
                } else {
                    let j = core.peek(l.joy);
                    if j & 1 != 0 {
                        core.poke(l.print_ptr, 1);
                        core.poke(l.print_ptr + 1, 3);
                    } else if j & 2 != 0 {
                        core.poke(l.print_ptr, 3);
                        core.poke(l.print_ptr + 1, 1);
                    }
                }
                let (a, b) = (core.peek(l.print_ptr), core.peek(l.print_ptr + 1));
                for y in 0..=0x28usize {
                    core.color[0x168 + y] = a & 15;
                    core.color[0x1E0 + y] = b & 15;
                }
                if core.peek(l.fire) == 0 {
                    self.pc = Pc::SavedGame(8);
                    self.wait = Wait::Yield;
                }
            }
            9 => {
                if core.peek(l.print_ptr) != 1 {
                    self.pc = Pc::StageNext;
                    return;
                }
                for y in 0..=0x16u16 {
                    let v = core.peek(l.saved_game + 1 + y);
                    core.poke(l.player_number + y, v);
                }
                self.pc = Pc::SaveQuit(10);
            }
            _ => {
                // `$1E41`: `JMP $1EBF` with the saved player
                let y = core.peek(l.player_number);
                players_restore(core, y);
                self.pc = Pc::StageStart(32);
            }
        }
    }
}

/// The IRQ setup the scheduler depends on: `$D012`, `$D011` bit 7, `$D01A`, the vector, the main program's I flag.
fn irq_setup(core: &Core, main: &Main) -> (u8, u8, u8, u16, bool) {
    let v = core.l.irq_vector;
    (core.vic[0x12], core.vic[0x11] & 0x80, core.vic[0x1A] & 1, u16::from_le_bytes([core.peek(v), core.peek(v + 1)]), main.i)
}

/// Cycles of `$4D2E..$27E5` after the font switch (the menu IRQ's setup, masked) up to the screen-on wait's `CLI`.
pub const RESET_TAIL_CYCLES: i64 = 70;
/// Cycles of the screen clear `$8F60..$8F84` (with the colour fill).
pub const CLEAR_CYCLES: i64 = 2 * 256 * 25;

/// CPU cycles of a pass of the main loop in a treasure room (the original reads `$D01E` about every 13 raster lines).
pub const ROOM_PASS_CYCLES: i64 = 740;
/// Cycles of the door search and closing of the treasure rooms (`$40DD..$412B`).
pub const DOOR_CYCLES: i64 = 600;
/// Cycles of `$412C..$4134` (the sprite variables back, `$14..$1F` cleared, the input off).
pub const BACK_CYCLES: i64 = 500;
/// Cycles of the screen swap of the treasure rooms (`$4152`: 800 cells of about 44 cycles).
pub const SWAP_SCREEN_CYCLES: i64 = 800 * 44;
/// Cycles of the stage setup `$49B8` alone (a part of [`LEVEL_CYCLES`]).
pub const SETUP_CYCLES: i64 = 15_000;

/// `$3F8C`: the block animations of the IRQs are over (every byte of the four state tables 0).
fn animations_idle(ram: &[u8], l: &C30aLayout) -> bool {
    (0..8u16).all(|y| {
        [l.anim_3991_state, l.anim_38f8_state, l.anim_3b3e_state, l.anim_3b3e_state + 8]
            .iter()
            .fold(0u8, |a, &t| a.wrapping_add(ram[(t + y) as usize]))
            == 0
    })
}

/// Cycles of the level buffer (`$8C0E`: clear, setup, map).
pub const LEVEL_CYCLES: i64 = screens::LEVEL_CLEAR_CYCLES + 20_000;
/// Cycles of `$1E67..$1EF9` (the save slots and `$48EC`).
pub const SLOTS_CYCLES: i64 = 400;
/// Cycles of a song start (`$5C93`).
pub const SONG_CYCLES: i64 = 500;
/// Cycles of the playfield drawing `$260B`.
pub const PLAYFIELD_CYCLES: i64 = 20 * 40 * 22;
/// Cycles of the spawn loop of the stage start (`$35CC..$35E1`).
pub const SPAWN_CYCLES: i64 = 39 * 120;

/// The cells of a player's save slot (`$02E2,Y`, `$02E4,Y`, .. two bytes per cell, `Y` = the player): score (3),
/// power, restart column, stages started, diamonds, stage, lives, (`$02F4` = game over, not saved here), `$CFFF`.
fn slot_cells(l: &C30aLayout) -> [Option<u16>; 11] {
    [
        Some(l.score),
        Some(l.score + 1),
        Some(l.score + 2),
        Some(l.power),
        Some(l.restart_column),
        Some(l.stage_counter),
        Some(l.diamonds),
        Some(l.stage),
        Some(l.lives),
        None,
        Some(l.room_counter),
    ]
}

/// `$1E72..$1EF4`: the player's state into its save slot; in a two-player game after a lost life (`$02F8 = 0`) the
/// other player whose game is not over is next (`$02F4,Y`); then that player's state back.
pub fn players_swap(core: &mut Core) {
    let l = core.l;
    let s = l.player_slots;
    let mut y = core.peek(l.player_number) as u16;
    let cells = slot_cells(l);
    for (k, a) in cells.iter().enumerate() {
        if let Some(a) = *a {
            let v = core.peek(a);
            core.poke(s + 2 * k as u16 + y, v);
        }
    }
    if core.peek(l.same_player) == 0 {
        loop {
            y = if y == 1 { 0 } else { 1 };
            core.poke(l.player_number, y as u8);
            if core.peek(l.game_over_flags + y) == 0 {
                break;
            }
        }
    }
    players_restore(core, y as u8);
}

/// `$1EBF..$1EF4`: player `y`'s state back from its save slot.
pub fn players_restore(core: &mut Core, y: u8) {
    let l = core.l;
    let s = l.player_slots;
    for (k, a) in slot_cells(l).iter().enumerate() {
        if let Some(a) = *a {
            let v = core.peek(s + 2 * k as u16 + y as u16);
            core.poke(a, v);
        }
    }
}

/// `$4901..$490C`, after the stage select of the first start: the lives for the stage (`$0D - $FC`, when that is 0..4).
pub fn stage_select_lives(core: &mut Core) {
    let l = core.l;
    let st = core.peek(l.stage);
    if st <= 0x0D && 0x0D - st < 5 {
        core.poke(l.lives, 0x0D - st);
    }
    core.poke(l.room_counter, (st >> 2).wrapping_add(0x64));
}

/// `$4918..$4923`: a selected stage (`$0313`) starts with one life.
pub fn stage_select_end(core: &mut Core) {
    let l = core.l;
    let sel = core.peek(l.select_cursor);
    if sel != 0 {
        core.poke(l.stage, sel);
        core.poke(l.lives, 1);
    }
}

/// `$8BDD`: the BCD amount `y:a` added to the score (`$CBF0..$CBF2`); past 999999 it stays at 999999.
pub fn score_add16(ram: &mut [u8], l: &C30aLayout, a: u8, y: u8) {
    let s = l.score as usize;
    let (b0, c) = blocks::adc_decimal(a, ram[s], false);
    let (b1, c) = blocks::adc_decimal(y, ram[s + 1], c);
    let (b2, c) = blocks::adc_decimal(ram[s + 2], 0, c);
    let v = if c { [0x99; 3] } else { [b0, b1, b2] };
    ram[s..s + 3].copy_from_slice(&v);
}

/// Cycles of `$371E` with `X` and `Y` (0 = 256).
pub fn delay_cycles(x: u8, y: u8) -> i64 {
    let n = |v: u8| if v == 0 { 256i64 } else { v as i64 };
    let first = 5 * n(x) - 1 + 5;
    let rest = (n(y) - 1) * (5 * 256 - 1 + 5);
    first + rest - 1 + 12
}

/// Cycles of `$3706` with `A` units (without fire).
pub fn units_cycles(a: u8) -> i64 {
    let n = if a == 0 { 256i64 } else { a as i64 };
    n * 256 * (5 * 256 - 1 + 3 + 2 + 2 + 3) + n * 8 + 20
}

/// `$25F7`: the block animations stop (all slots of the four kinds).
pub fn clear_animations(ram: &mut [u8], l: &C30aLayout) {
    for y in 0..8u16 {
        for a in [l.anim_3991_state, l.anim_38f8_state, l.anim_3b3e_state, l.anim_3b3e_state + 8] {
            ram[(a + y) as usize] = 0;
        }
    }
}

/// `$8F5C` (`colour7 = false`, colour 0) / `$8F5A` (colour 7): screen RAM `$C800..$CBE7` and colour RAM cleared; with
/// the game font (`$C000 = 0`) also the item icon's characters from `$B410` (`$8F8A`, `$269D`). The screen-off wait
/// in front of it (`$23A6`) is the caller's.
pub fn clear_screen(core: &mut Core, colour7: bool) {
    let l = core.l;
    let s = l.screen as usize;
    for a in s..s + 1000 {
        core.music.driver.mem[a] = 0;
    }
    let c = if colour7 { 7 } else { 0 };
    for a in 0..1000 {
        core.color[a] = c;
    }
    if core.peek(l.charset) == 0 {
        item_chars(core.ram(), l, l.item_icon_none);
    }
}

/// `$269D`: the 48 bytes at `src` into the characters of the item icon (`$C018`, `$C0D0`, `$C040`, `$C050`, `$C058`,
/// `$C080`, 8 bytes each, interleaved as the original reads them backwards).
pub fn item_chars(ram: &mut [u8], l: &C30aLayout, src: u16) {
    let c = l.charset;
    let mut y = 0x2Fu16;
    for x in (0..8u16).rev() {
        for off in [0x80u16, 0x58, 0x50] {
            ram[(c + off + x) as usize] = ram[(src + y) as usize];
            y = y.wrapping_sub(1);
        }
    }
    for x in (0..8u16).rev() {
        for off in [0x40u16, 0xD0, 0x18] {
            ram[(c + off + x) as usize] = ram[(src + y) as usize];
            y = y.wrapping_sub(1);
        }
    }
}

/// `$4D2E`: the game's font (the menu font is the inverted one, `$C000 != 0`).
pub fn charset_game(ram: &mut [u8], l: &C30aLayout) -> bool {
    let swap = ram[l.charset as usize] == 0;
    if swap {
        charset_swap(ram, l);
    }
    swap
}

/// `$4D37`: the menu font.
pub fn charset_menu(ram: &mut [u8], l: &C30aLayout) -> bool {
    let swap = ram[l.charset as usize] != 0;
    if swap {
        charset_swap(ram, l);
    }
    swap
}


/// `$4D40`: inverts the characters `$C000..$C0DF`, `$C180..$C1CF` and `$1AE8..$1B17`, and swaps `$C0E0..$C137` with
/// `$4D8C` and `$C170..$C17F` with `$4DE4`.
pub fn charset_swap(ram: &mut [u8], l: &C30aLayout) {
    for k in 0..5 {
        charset_swap_part(ram, l, k);
    }
}

/// The five loops of `$4D40` (see [`charset_swap`]).
pub fn charset_swap_part(ram: &mut [u8], l: &C30aLayout, k: u8) {
    let c = l.charset as usize;
    match k {
        0 => (1..=0xE0usize).for_each(|x| ram[c - 1 + x] ^= 0xFF),
        1 => (1..=0x50usize).for_each(|x| ram[c + 0x17F + x] ^= 0xFF),
        2 => (1..=0x30usize).for_each(|x| ram[l.tile_anim_bits as usize + 0x0F + x] ^= 0xFF),
        3 => (0..=0x57usize).for_each(|x| ram.swap(c + 0xE0 + x, l.charset_save as usize + x)),
        _ => (0..=0x0Fusize).for_each(|x| ram.swap(c + 0x170 + x, l.charset_save as usize + 0x58 + x)),
    }
}

/// Cycles and end (the original's `PC` after the loop) of the five loops of `$4D40`.
pub const SWAP_PARTS: [(i64, u16); 5] = [(0xE0 * 16, 0x4D4D), (0x50 * 16, 0x4D5A), (0x30 * 16, 0x4D67), (0x58 * 25, 0x4D79), (16 * 25 + 6, 0x4D8B)];

/// `$2646`: the status bar rows 0-1 cleared (colour 7), the full status bar, its texts and the stage number.
pub fn top_rows(core: &mut Core) {
    top_rows_clear(core);
    top_rows_texts(core);
}

/// `$2646..$2657`: rows 0-1 cleared (colour 7) and the full status bar (`$26EF`).
pub fn top_rows_clear(core: &mut Core) {
    let l = core.l;
    let screen = l.screen as usize;
    for y in 0..0x50usize {
        core.ram()[screen + y] = 0;
        core.color[y] = 7;
    }
    core.poke(l.hud_redraw, 0);
    let (ram, color, vic) = core.parts();
    blocks::hud(ram, color, vic, l);
}

/// `$2658..$2670`: the player's name, text 4 and the stage number (`$FC`) in the status bar.
pub fn top_rows_texts(core: &mut Core) {
    let l = core.l;
    let (ram, color, _) = core.parts();
    let n = ram[l.player_number as usize];
    text::print_at(ram, color, l, n, 0, 0);
    text::print(ram, color, l, 4);
    let st = ram[l.stage as usize];
    text::number_at(ram, color, l, st, 0x1D, 1);
}

/// Cycles of `$2646..$26F1` (the rows cleared and the item's sprite shape, up to the status bar's operands).
pub const TOP_ROWS_OPS_CYCLES: i64 = 1700;
/// Cycles of `$2646..$2657` (the full status bar).
pub const TOP_ROWS_CYCLES: i64 = 3500;

/// `$7222`: the keys of a frame (object frame, `$FB` IRQ): pause (`P` or RUN/STOP), the hidden next-stage key
/// combination, the pause menu's state and the space bar that fires a stored power-up.
pub fn keys(core: &mut Core, orig: &mut dyn Original) {
    let _ = orig;
    let l = core.l;
    let k = core.scan(0xDF);
    if k == 0xFD {
        return pause_key(core);
    }
    let k = core.scan(0xE9);
    if k == 0x69 {
        // `$7274`: next stage, score 0
        let v = core.peek(l.stage_counter).wrapping_add(1);
        core.poke(l.stage_counter, v);
        core.poke(l.stage_end, 0xFF);
        if core.peek(l.in_room) != 0 {
            return;
        }
        core.poke(l.stage, 0);
        for i in 0..3 {
            core.poke(l.score + i, 0);
        }
        return;
    }
    let k = core.scan(0x7F);
    if k == 0x7F {
        return pause_key(core);
    }
    let y = core.peek(l.state_95);
    if y == 0 {
        // `$7259`: space with a stored power-up
        if k == 0xEF {
            match core.peek(l.power) {
                5 => {
                    let t = core.peek(l.freeze_timer).wrapping_sub(1);
                    core.poke(l.freeze_timer, t);
                    use_power(core);
                }
                6 => {
                    core.poke(l.effect, 0x0E);
                    let bg = core.vic[0x21];
                    core.poke(l.power_colours, bg);
                    core.poke(l.power_flash, 4);
                    use_power(core);
                }
                _ => {}
            }
        }
        return;
    }
    if y & 0x80 != 0 {
        // `$7268`: the menu's cursor row blinks
        let c = core.peek(l.flash_colours + core.peek(l.frame_counter) as u16);
        let x = core.peek(l.menu_cursor);
        if x & 0x80 == 0 {
            menu_row_colour(core, x, c);
        } else {
            pause_resume(core);
        }
        return;
    }
    // `$724C`: counting down to the menu
    let mut y = y - 1;
    if y != 0 {
        y = 0xFF;
    }
    if core.peek(l.fire) == 0 {
        core.poke(l.state_95, y);
        core.poke(l.game_state, y);
    }
}

/// `$72C4`: the power level becomes 4 and the status bar is drawn again.
fn use_power(core: &mut Core) {
    let l = core.l;
    core.poke(l.power, 4);
    core.poke(l.hud_redraw, 0);
}

/// `$7284`: the pause key.
fn pause_key(core: &mut Core) {
    let l = core.l;
    if (core.peek(l.auto_scroll) | core.peek(l.dying) | core.peek(l.in_room)) & 0x7F != 0 {
        return;
    }
    let s = core.peek(l.state_95);
    if s == 0 {
        core.poke(l.game_state, 0xFF);
        core.poke(l.pause_state, 0xFF);
        core.poke(l.state_95, 2);
        core.poke(l.menu_cursor, 0);
    } else if s & 0x80 != 0 {
        pause_resume(core);
    }
}

/// `$72A4`: the menu closes: `$95 = $96 = 1`, the cloud rows are drawn again (`$2866`).
fn pause_resume(core: &mut Core) {
    let l = core.l;
    core.poke(l.state_95, 1);
    core.poke(l.pause_state, 1);
    cloud_rows(core);
}

/// `$2866..$287F`: rows 2-4 cleared; with a cloud colour (`$0108`) their colour and the clouds (`$2A78`).
pub fn cloud_rows(core: &mut Core) {
    let l = core.l;
    let s = l.screen as usize;
    for y in 1..=0x78usize {
        core.ram()[s + 0x4F + y] = 0;
    }
    let c = core.peek(l.stage_header + 8);
    if c == 0 {
        return;
    }
    for y in 1..=0x78usize {
        core.color[0x4F + y] = c & 15;
    }
    crate::scroll::clouds_left(core.ram(), l);
    core.poke(l.scroll_px, 0);
}

/// `$2212`/`$2214`: a menu row's colour (`$D800 + $8DE4[x]`, 40 cells).
pub fn menu_row_colour(core: &mut Core, x: u8, c: u8) {
    let l = core.l;
    let start = core.peek(l.row_lo + 2 + x as u16) as usize;
    for i in 0..40usize {
        let a = start + i;
        if a < 1024 {
            core.color[a] = c & 15;
        }
    }
}


/// `$36B5..$3605`: the end of the sprite change: `$1A = $1D = 0`, Giana 2 pixels lower, the layer offsets back to 1,
/// the player's colours.
pub fn power_sprites_end(core: &mut Core) {
    let l = core.l;
    core.poke(l.anim_index, 0);
    core.poke(l.layer_hires, 0);
    let y = core.peek(l.player_y).wrapping_add(2);
    core.poke(l.player_y, y);
    core.poke(l.layer_offset_ops + 5, 1);
    core.poke(l.layer_offset_ops, 1);
    screens::giana_colours(core);
}

/// Where `$1D10` goes: the intro the first time, the title once the intro has patched its `JSR` into an `LDA`.
pub fn start_pc(core: &Core) -> Pc {
    if core.peek(core.l.game_start) == 0x20 {
        Pc::Intro(0)
    } else {
        Pc::Title(0)
    }
}

/// `$4EA5..$4EC3`: a typed PETSCII key as a name character: the eleven signs of `$4E51` through `$4E5C`, digits as they
/// are, letters `A..Z` as screen codes 1..26; `None` = not a name character.
fn map_key(core: &Core, c: u8) -> Option<u8> {
    let l = core.l;
    let mut a = c;
    let mut found = false;
    for x in (0..11u16).rev() {
        if core.peek(l.entry_keys + x) == c {
            a = core.peek(l.entry_keys + 11 + x);
            found = true;
            break;
        }
    }
    if found && a & 0x80 == 0 {
        return Some(a);
    }
    if !(0x30..0x5C).contains(&a) {
        return None;
    }
    if a < 0x3A {
        return Some(a);
    }
    if a < 0x40 {
        return None;
    }
    Some(a - 0x40)
}

/// `$4DFA..$4E50`: the high-score screen: the KERNAL's IRQ (`$504E`: `$01 = $36`, the handler `$5098` through `$0314`
/// at line `$2D`), the screen drawn from its map record (`$4C81`: the tables are its shapes) at column 0, colours, the
/// rows moved up, the two titles.
pub fn hiscore_screen(core: &mut Core) {
    let l = core.l;
    core.poke(1, 0x36);
    core.poke(l.kernal_key + 1, 0x36);
    core.poke(l.entry_cursor, 0x36);
    core.vic[0x1A] = 1;
    core.vic[0x12] = 0x2D;
    core.vic[0x11] = 0x1B;
    core.vic[0x16] = 0x18;
    core.set_vector(l.kernal_irq_vector, l.irq_hiscore);
    core.poke(l.kernal_key - 6, 0x40);
    core.poke(l.kernal_key, 0x40);
    core.vic[0x23] = 0;
    core.set_vector(l.pointer2, l.hiscore_map);
    screens::build_map(core.ram(), l);
    let (ram, color, _) = core.parts();
    screens::draw_playfield_at(ram, color, l, 0);
    for x in 0..3usize {
        for o in [0x208usize, 0x230, 0x22D, 0x255] {
            core.color[o + x] = 0x0D;
        }
    }
    for x in 1..=0xC8usize {
        let v = core.music.driver.mem[0xCA07 + x];
        core.music.driver.mem[0xC827 + x] = v;
        let mut c = core.color[0x207 + x] | 0xF0;
        if c == 0xFD {
            c = 0xFB;
        }
        core.color[0x27 + x] = c & 15;
    }
    let (ram, color, _) = core.parts();
    text::print_at(ram, color, l, 0x15, 7, 0);
    text::print_at(ram, color, l, 0x16, 7, 0x0C);
}

/// Cycles of the intro's speed test page.
pub const INTRO_TEST_CYCLES: i64 = 20_000;
/// Cycles of the intro's last delay (`$1052` with 10 units).
pub const INTRO_DELAY_CYCLES: i64 = 10 * 256 * 1325;
/// Cycles of the high-score screen's drawing.
pub const HISCORE_CYCLES: i64 = 30_000;
