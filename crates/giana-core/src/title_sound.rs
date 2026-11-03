//! The sound of the title picture, native: the music player of the title program and its 4 bit sample.
//!
//! `title_music::Player6502` is the static translation of the code (the Huelsbeck player whose tick `$C478` the CIA 1 timer
//! interrupt calls, and the NMI routine `$9B19` that plays the sample through the SID volume register). This module is what the
//! original's hardware did around it: the two CIA timers (their latches are written by the code itself: the player sets the
//! tempo in CIA 1, each sample start sets the rate in CIA 2) and the frame grid. The tables and the music come from the
//! player's own `f_2.prg`: the memory is the title program after its set-up code, with the player copied up to `$C000` like the
//! program's loop at `$0B37` does it; nothing of that is part of the repository.

use crate::title_music::Player6502;

/// Cycles of one PAL frame (63 cycles on 312 lines), the unit of `TitleSound::run_frame`.
pub const FRAME_CYCLES: u32 = 63 * 312;

/// Cycles from the music start (`JSR $C000` at `$0B56`) to the first CIA 1 interrupt. The CIA 1 timer runs freely since the
/// computer was switched on (the KERNAL's jiffy interrupt), so this depends on how long the computer has been running;
/// the value is the one VICE shows when the program is autostarted (`tools/authentic/vice_sid_dump.py`).
pub const FIRST_TICK_DELAY: u64 = 0;

/// CIA 1 timer A latch of the KERNAL (the PAL value) when the player has not yet set its own.
pub const KERNAL_JIFFY_LATCH: u16 = 0x4295;

/// Cycles from the timer underflow to the first instruction of the player tick: the interrupt sequence (7), the KERNAL's
/// IRQ entry up to `JMP ($0314)` and the player's own entry code (`SEI`, bank switch, `JSR $C478`).
pub const TICK_ENTRY_CYCLES: u32 = 40;
/// The same for the NMI: the interrupt sequence (7) up to the routine at `$9B19` (the KERNAL's NMI entry `JMP ($0318)`).
pub const NMI_ENTRY_CYCLES: u32 = 14;

/// The first interrupt of the original comes later than the timer's rhythm says (it is taken only when the program clears
/// its interrupt flag, while the second one is already due on the timer's own grid); VICE shows the second interrupt this many
/// cycles after `latch + 1` from the first. A measured constant, the same in every run.
pub const FIRST_GAP_EXTRA: u64 = 870;


/// The video chip takes the bus on bad lines (the picture is a bitmap), the code runs slower than its cycle count says:
/// measured against VICE, a tick of the player takes about 7 % longer.
pub const STRETCH_PERCENT: u32 = 107;

/// A SID write with its cycle in the frame: (cycle, register, value).
pub type TimedWrite = (u32, u8, u8);

pub struct TitleSound {
    pub player: Player6502,
    /// cycles since the start of the music: the beginning of the current frame
    frame_start: u64,
    next_tick: u64,
    next_nmi: u64,
    /// the SID registers `$D400..$D418` after the last write
    pub regs: [u8; 25],
    silenced: bool,
    /// cycles added once to the interval after the first tick (see `FIRST_GAP_EXTRA`)
    first_gap_extra: u64,
    /// writes of a call that ran past the end of its frame: they belong to the next one
    carry: Vec<TimedWrite>,
}

/// `from` and `to` of the program's copy loop at `$0B37`: `$3D` pages from `$4C00` to `$9000`.
const RELOCATE_FROM: usize = 0x4C00;
const RELOCATE_TO: usize = 0x9000;
const RELOCATE_LEN: usize = 0x3D00;

impl TitleSound {
    /// `setup_memory` is the memory of the title program after its set-up (`giana_extract::derive::title_setup_memory`).
    pub fn new(setup_memory: &[u8]) -> Self {
        Self::with_timing(setup_memory, FIRST_TICK_DELAY, KERNAL_JIFFY_LATCH)
    }

    pub fn with_timing(setup_memory: &[u8], first_tick: u64, cia1_latch: u16) -> Self {
        let mut mem = setup_memory.to_vec();
        mem.copy_within(RELOCATE_FROM..RELOCATE_FROM + RELOCATE_LEN, RELOCATE_TO);
        Self::from_relocated(mem, first_tick, cia1_latch)
    }

    /// Like `with_timing` for a memory in which the player is already at `$C000` (`mem` is changed: the start of the music
    /// is what `JSR $C000` does).
    pub fn from_relocated(mut mem: Vec<u8>, first_tick: u64, cia1_latch: u16) -> Self {
        // `JSR $C000`: the interrupt vector goes to the player, `JSR $9B00` sets the NMI vector and the sample pointer
        // (high bytes of start and end equal: no sample), and CIA 2 timer A is started (it counts down from $FFFF)
        mem[0x314] = 0x22;
        mem[0x315] = 0xC0;
        mem[0x318] = 0x19;
        mem[0x319] = 0x9B;
        mem[0x04] = 1;
        mem[0x02] = 1;
        let player = Player6502::new(mem, cia1_latch, 0xFFFF);
        Self { player, frame_start: 0, next_tick: first_tick, next_nmi: 0x1_0000, regs: [0; 25], silenced: false, first_gap_extra: FIRST_GAP_EXTRA, carry: Vec::new() }
    }

    /// Runs one frame of 19656 cycles and returns the SID writes of the music code and of the sample, in order, each with
    /// its cycle in the frame.
    pub fn run_frame(&mut self) -> Vec<TimedWrite> {
        let mut out = std::mem::take(&mut self.carry);
        if self.silenced {
            return out;
        }
        let end = self.frame_start + FRAME_CYCLES as u64;
        loop {
            let t = self.next_tick.min(self.next_nmi);
            if t >= end {
                break;
            }
            let at = t.saturating_sub(self.frame_start) as u32;
            let is_tick = self.next_tick <= self.next_nmi;
            if is_tick {
                // the timer reloads from its latch, then the interrupt routine runs and may change the latch
                self.next_tick = t + self.player.cia1_latch as u64 + 1 + std::mem::take(&mut self.first_gap_extra);
                self.player.tick();
            } else {
                self.next_nmi = t + self.player.cia2_latch as u64 + 1;
                self.player.nmi();
            }
            let entry = if is_tick { TICK_ENTRY_CYCLES } else { NMI_ENTRY_CYCLES };
            for (cycles, reg, val) in self.player.take_writes() {
                if (reg as usize) < 25 {
                    self.regs[reg as usize] = val;
                }
                out.push((at + entry + cycles * STRETCH_PERCENT / 100, reg, val));
            }
        }
        self.frame_start = end;
        // the interrupt routines of one timer ran one after the other, in the original they are interleaved by their time
        out.sort_by_key(|w| w.0);
        if let Some(i) = out.iter().position(|w| w.0 >= FRAME_CYCLES) {
            self.carry = out.split_off(i);
            for w in &mut self.carry {
                w.0 -= FRAME_CYCLES;
            }
        }
        out
    }

    /// For tests: no NMI at all (the sample never plays; the original run with an RTI at `$9B19` does not see another one either).
    pub fn disable_sample(&mut self) {
        self.next_nmi = u64::MAX;
    }

    /// The program leaves the picture: it stops CIA 2 (the sample), takes the interrupt away and writes `$D418` = 0.
    pub fn silence(&mut self) -> Vec<TimedWrite> {
        if self.silenced {
            return Vec::new();
        }
        self.silenced = true;
        self.regs[0x18] = 0;
        vec![(0, 0x18, 0)]
    }

    pub fn is_silenced(&self) -> bool {
        self.silenced
    }
}
