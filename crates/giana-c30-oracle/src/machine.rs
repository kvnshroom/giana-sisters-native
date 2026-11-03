//! The machine: CPU + [`Hw`], frame stepping, hooks, calls.

use crate::hw::Hw;
use crate::snapshot::{Picture, Snapshot};
use giana_engine_c::Image;
use giana_module::cpu::Cpu;

/// Entry of the first raster IRQ of a frame (line `$41`): where [`Machine::frame`] stops. VICE traces are sampled here.
pub const FRAME_PC: u16 = 0x3488;
/// Entry of the `$FB` IRQ (line 251): the picture of the frame is complete here ([`Machine::picture`]).
pub const PICTURE_PC: u16 = 0x32DC;
/// Return address trick of [`Machine::call`]: the routine's `RTS` lands here.
const SENTINEL: u16 = 0xFFF0;
/// Patches that skip the intro and the title (what `tools/c30/snapshot.py` pokes into VICE): `(address, bytes)`.
const BOOT_PATCHES: [(u16, &[u8]); 3] = [(0x21FA, &[0xA9, 0xEF, 0xEA]), (0x2207, &[0xA9, 0xFF, 0xEA]), (0x1D9F, &[0xFF])];
/// Cycles per opcode (NMOS 6502, undocumented ones as the common tables have them) before page-crossing and branch penalties.
const CYCLES: [u8; 256] = [
    7, 6, 2, 8, 3, 3, 5, 5, 3, 2, 2, 2, 4, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    6, 6, 2, 8, 3, 3, 5, 5, 4, 2, 2, 2, 4, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    6, 6, 2, 8, 3, 3, 5, 5, 3, 2, 2, 2, 3, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    6, 6, 2, 8, 3, 3, 5, 5, 4, 2, 2, 2, 5, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4,
    2, 6, 2, 6, 4, 4, 4, 4, 2, 5, 2, 5, 5, 5, 5, 5,
    2, 6, 2, 6, 3, 3, 3, 3, 2, 2, 2, 2, 4, 4, 4, 4,
    2, 5, 2, 5, 4, 4, 4, 4, 2, 4, 2, 4, 4, 4, 4, 4,
    2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
    2, 6, 2, 8, 3, 3, 5, 5, 2, 2, 2, 2, 4, 4, 6, 6,
    2, 5, 2, 8, 4, 4, 6, 6, 2, 4, 2, 7, 4, 4, 7, 7,
];

/// Addressing mode per opcode: `i` implied/accumulator, `m` immediate, `z` zp, `x`/`y` zp,X / zp,Y, `a` absolute,
/// `X`/`Y` absolute,X / absolute,Y, `I` (zp,X), `J` (zp),Y, `r` relative, `n` indirect (JMP).
const MODE: &[u8; 256] = b"iIiIzzzzimimaaaarJiJxxxxiYiYXXXXaIiIzzzzimimaaaarJiJxxxxiYiYXXXXiIiIzzzzimimaaaarJiJxxxxiYiYXXXXiIiIzzzzimimnaaarJiJxxxxiYiYXXXXmImIzzzzimimaaaarJiJxxyyiYiYXXYYmImIzzzzimimaaaarJiJxxyyiYiYXXYYmImIzzzzimimaaaarJiJxxxxiYiYXXXXmImIzzzzimimaaaarJiJxxxxiYiYXXXX";

/// Guard for [`Machine::frame`]: the game runs long delay loops with the raster IRQ switched off (stage clear: 2 x 100 frames
/// in one go), so a "frame" can take several million instructions.
const MAX_INSTR_PER_FRAME: u64 = 100_000_000;

/// A hook runs when the CPU is about to execute the instruction at the hooked address. If it leaves `cpu.pc` at the
/// hooked address, the original instruction then executes (an *observer*); if it moves `pc` (typically with
/// [`Machine::rts`]), the original code at that address is skipped (a *replacement*).
pub type Hook = Box<dyn FnMut(&mut Machine)>;

pub struct Machine {
    pub cpu: Cpu,
    pub hw: Hw,
    hooks: Vec<Option<Hook>>,
    hook_at: Vec<u16>,
    /// the PC at which [`Machine::frame`] stops (default [`FRAME_PC`])
    pub sync_pc: u16,
    /// completed calls of [`Machine::frame`]
    pub frame_no: u64,
    /// executed instructions (IRQ entries not counted)
    pub instrs: u64,
    charged: u32,
    pub(crate) kernal_name: (u8, u16),
}

impl Machine {
    /// A machine over a 64 KB RAM image, CPU at `pc` with IRQs masked, `$01 = $35`.
    pub fn new(ram: &[u8], pc: u16) -> Machine {
        let mut hw = Hw::new(ram);
        hw.ram[0] = 0x2F;
        hw.ram[1] = 0x35;
        let mut cpu = Cpu::default();
        cpu.pc = pc;
        Machine { cpu, hw, hooks: Vec::new(), hook_at: vec![0; 65536], sync_pc: FRAME_PC, frame_no: 0, instrs: 0, charged: 0, kernal_name: (0, 0) }
    }

    /// Starts the game from the unpacked disk image like VICE does in `tools/c30/snapshot.py`: the intro and title are
    /// skipped (three patches), stage 1 starts, and the machine stops at the first `$3488` (frame 0).
    ///
    /// `d64`: the disk, for the high-score file the game loads at start-up through the KERNAL (see [`crate::kernal`]);
    /// without it that load fails like on a disk without the file.
    pub fn boot(image: &Image, d64: Option<&[u8]>) -> Result<Machine, String> {
        let mut m = Machine::new(&image.ram, image.entry);
        crate::kernal::install(&mut m, d64.map(|d| d.to_vec()));
        for (a, b) in BOOT_PATCHES {
            m.hw.ram[a as usize..a as usize + b.len()].copy_from_slice(b);
        }
        m.cpu.sp = 0xFF;
        m.run_to_pc(FRAME_PC, 50_000_000)?;
        m.frame_no = 0;
        m.hw.vic_log.clear();
        m.hw.sid_log.clear();
        m.hw.coll_reads.clear();
        Ok(m)
    }

    /// Restarts stage `n` (0..15, 100..104 for the special rooms) the way `tools/c30/vice_stages.py` does (`$FC = n`,
    /// jump to `$1E56`) and runs 60 frames, ending at a `$3488` again. The VICE reference for this is `v<n>.ram`.
    pub fn start_stage(&mut self, n: u8) -> Result<(), String> {
        self.hw.ram[0xFC] = n;
        self.cpu.pc = 0x1E56;
        for _ in 0..60 {
            self.frame()?;
        }
        Ok(())
    }

    /// A copy of the machine's state without hooks (use it to run the same state hooked and unhooked).
    pub fn fork(&self) -> Machine {
        Machine {
            cpu: self.cpu.clone(),
            hw: self.hw.clone(),
            hooks: Vec::new(),
            hook_at: vec![0; 65536],
            sync_pc: self.sync_pc,
            frame_no: self.frame_no,
            instrs: self.instrs,
            charged: 0,
            kernal_name: self.kernal_name,
        }
    }

    /// Models the cycles the VIC takes from the CPU (40 on a bad line, 2 per sprite with DMA per line).
    pub fn steal_cycles(&mut self, on: bool) {
        self.hw.steal = on;
    }

    // ---- hooks ------------------------------------------------------------------------------------------------

    /// Installs `f` at `addr` (see [`Hook`]). Replaces an earlier hook at the same address.
    pub fn hook(&mut self, addr: u16, f: impl FnMut(&mut Machine) + 'static) {
        let b: Hook = Box::new(f);
        match self.hook_at[addr as usize] {
            0 => {
                self.hooks.push(Some(b));
                self.hook_at[addr as usize] = self.hooks.len() as u16;
            }
            i => self.hooks[i as usize - 1] = Some(b),
        }
    }

    /// Replaces the subroutine at `addr` by `f`: `f` does the routine's work on `m.hw.ram` / `m.cpu` (registers and flags
    /// as the routine leaves them), then the machine executes the `RTS`. `cycles` is what the original would take, charged
    /// to the raster so that IRQs stay where they were (0 is fine for logic that does not depend on raster timing).
    pub fn replace(&mut self, addr: u16, cycles: u32, mut f: impl FnMut(&mut Machine) + 'static) {
        self.hook(addr, move |m| {
            f(m);
            m.rts();
            m.charge(cycles);
        });
    }

    pub fn unhook(&mut self, addr: u16) {
        if let i @ 1.. = self.hook_at[addr as usize] {
            self.hooks[i as usize - 1] = None;
            self.hook_at[addr as usize] = 0;
        }
    }

    /// `RTS`: pops the return address and continues after it.
    pub fn rts(&mut self) {
        let lo = self.cpu.pull(&mut self.hw) as u16;
        let hi = self.cpu.pull(&mut self.hw) as u16;
        self.cpu.pc = (hi << 8 | lo).wrapping_add(1);
    }

    /// Adds CPU cycles to the current instruction (for hooks that replace code).
    pub fn charge(&mut self, cycles: u32) {
        self.charged += cycles;
    }

    // ---- memory -----------------------------------------------------------------------------------------------

    /// RAM byte without side effects (never the I/O window).
    #[inline]
    pub fn peek(&self, a: u16) -> u8 {
        self.hw.ram[a as usize]
    }

    #[inline]
    pub fn peek16(&self, a: u16) -> u16 {
        self.hw.ram[a as usize] as u16 | (self.hw.ram[a.wrapping_add(1) as usize] as u16) << 8
    }

    #[inline]
    pub fn poke(&mut self, a: u16, v: u8) {
        self.hw.ram[a as usize] = v;
    }

    pub fn peek_slice(&self, a: u16, n: usize) -> &[u8] {
        &self.hw.ram[a as usize..a as usize + n]
    }

    // ---- input ------------------------------------------------------------------------------------------------

    /// Joystick port 2, `joy::*` bits that are *pressed*.
    pub fn set_joy(&mut self, pressed: u8) {
        self.hw.joy = !pressed | 0xE0;
    }

    /// Presses or releases the key at matrix `(column, row)` (`$DC00` bit = column, `$DC01` bit = row).
    pub fn set_key(&mut self, col: usize, row: usize, down: bool) {
        if down {
            self.hw.keys[col] &= !(1 << row);
        } else {
            self.hw.keys[col] |= 1 << row;
        }
    }

    // ---- running ----------------------------------------------------------------------------------------------

    /// One CPU step: a pending raster IRQ is taken instead of an instruction, a hook at `pc` runs first.
    pub fn step(&mut self) -> Result<(), String> {
        if !self.cpu.i && self.hw.irq_pending() {
            let pc = self.cpu.pc;
            self.cpu.push(&mut self.hw, (pc >> 8) as u8);
            self.cpu.push(&mut self.hw, pc as u8);
            let f = self.cpu.flags(false);
            self.cpu.push(&mut self.hw, f);
            self.cpu.i = true;
            self.cpu.pc = self.peek16(0xFFFE);
            self.cpu.cycles += 7;
            self.hw.advance(7);
            return Ok(());
        }
        self.step_no_irq()
    }

    fn step_no_irq(&mut self) -> Result<(), String> {
        let pc = self.cpu.pc;
        let h = self.hook_at[pc as usize];
        if h != 0 {
            if let Some(mut f) = self.hooks[h as usize - 1].take() {
                f(self);
                if self.hooks[h as usize - 1].is_none() && self.hook_at[pc as usize] == h {
                    self.hooks[h as usize - 1] = Some(f);
                }
                if self.cpu.pc != pc {
                    let c = std::mem::take(&mut self.charged);
                    self.cpu.cycles += c as u64;
                    self.hw.advance(c.max(1));
                    return Ok(());
                }
            }
        }
        let exact = self.exact_cycles();
        self.hw.cur_pc = pc;
        self.cpu.step(&mut self.hw)?;
        self.instrs += 1;
        let c = exact + std::mem::take(&mut self.charged);
        self.hw.advance(c);
        Ok(())
    }

    /// Cycles of the instruction at `pc` including page-crossing and taken-branch penalties (`Cpu::step`'s own count is
    /// only approximate, and the game's frame is nearly all cycles, so the raster position matters).
    fn exact_cycles(&self) -> u32 {
        let r = &self.hw.ram;
        let pc = self.cpu.pc as usize;
        let op = r[pc] as usize;
        let mut c = CYCLES[op] as u32;
        let (b1, b2) = (r[(pc + 1) & 0xFFFF] as usize, r[(pc + 2) & 0xFFFF] as usize);
        let crossed = |base: usize, idx: u8| (base & 0xFF00) != ((base + idx as usize) & 0xFF00);
        match MODE[op] {
            b'X' if c == 4 && crossed(b1 | b2 << 8, self.cpu.x) => c += 1,
            b'Y' if c == 4 && crossed(b1 | b2 << 8, self.cpu.y) => c += 1,
            b'J' if c == 5 && crossed(r[b1] as usize | (r[(b1 + 1) & 255] as usize) << 8, self.cpu.y) => c += 1,
            b'r' => {
                let cpu = &self.cpu;
                let taken = match op >> 5 {
                    0 => !cpu.n,
                    1 => cpu.n,
                    2 => !cpu.v,
                    3 => cpu.v,
                    4 => !cpu.c,
                    5 => cpu.c,
                    6 => !cpu.z,
                    _ => cpu.z,
                };
                if taken {
                    let next = (pc + 2) & 0xFFFF;
                    let dest = (next as i32 + b1 as u8 as i8 as i32) as usize & 0xFFFF;
                    c += 1 + ((next & 0xFF00) != (dest & 0xFF00)) as u32;
                }
            }
            _ => {}
        }
        c
    }

    /// Runs until the CPU is about to execute `pc` (after at least one step). `max` bounds the instructions.
    pub fn run_to_pc(&mut self, pc: u16, max: u64) -> Result<(), String> {
        let mut n = 0;
        loop {
            self.step()?;
            if self.cpu.pc == pc {
                return Ok(());
            }
            n += 1;
            if n > max {
                return Err(format!("${pc:04X} not reached in {max} steps (pc = ${:04X}, line {})", self.cpu.pc, self.hw.line));
            }
        }
    }

    /// Runs one frame: until the CPU is at [`Machine::sync_pc`] again. The logs of [`Hw`] hold exactly this frame's
    /// VIC writes, SID writes and `$D01E` reads afterwards.
    pub fn frame(&mut self) -> Result<(), String> {
        self.hw.vic_log.clear();
        self.hw.sid_log.clear();
        self.hw.coll_reads.clear();
        self.run_to_pc(self.sync_pc, MAX_INSTR_PER_FRAME)?;
        self.frame_no += 1;
        Ok(())
    }

    /// `n` frames with the same joystick state.
    pub fn frames(&mut self, n: u64) -> Result<(), String> {
        for _ in 0..n {
            self.frame()?;
        }
        Ok(())
    }

    /// Calls the subroutine at `addr` and runs until it returns (IRQs are masked meanwhile, the raster keeps counting).
    /// The registers are whatever the caller set in `cpu`; they are the routine's results afterwards. Returns the cycles.
    pub fn call(&mut self, addr: u16) -> Result<u64, String> {
        let (pc0, i0, c0) = (self.cpu.pc, self.cpu.i, self.cpu.cycles);
        let ret = SENTINEL - 1;
        self.cpu.push(&mut self.hw, (ret >> 8) as u8);
        self.cpu.push(&mut self.hw, ret as u8);
        self.cpu.pc = addr;
        self.cpu.i = true;
        let mut n = 0u64;
        while self.cpu.pc != SENTINEL {
            self.step_no_irq()?;
            n += 1;
            if n > MAX_INSTR_PER_FRAME {
                return Err(format!("call ${addr:04X} did not return (pc = ${:04X})", self.cpu.pc));
            }
        }
        self.cpu.pc = pc0;
        self.cpu.i = i0;
        Ok(self.cpu.cycles - c0)
    }

    // ---- snapshots --------------------------------------------------------------------------------------------

    /// The machine state now, with this frame's logs.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::of(self)
    }

    /// The finished picture of the current frame, for `giana-engine-c`'s renderer: call it right after
    /// [`Machine::frame`] (at the start of the frame, raster line `$41`). It takes the VIC registers, sprite pointers and
    /// fine scrolls of the frame start, runs on to the `$FB` IRQ ([`PICTURE_PC`], by then every text row has had its bad
    /// line) and replaces screen RAM and colour RAM by what the VIC *fetched* for each row at its bad line
    /// ([`Hw::fetched_screen`]), because the game rewrites rows below the beam before it arrives and rows above it after
    /// it has passed: the picture shows what was on screen at the moment. The machine is then in the middle of the frame;
    /// the next [`Machine::frame`] completes it.
    pub fn picture(&mut self) -> Result<Picture, String> {
        let mut vic = [0u8; 47];
        vic.copy_from_slice(&self.hw.vic[..47]);
        let mut ram = self.hw.ram.to_vec();
        self.run_to_pc(PICTURE_PC, MAX_INSTR_PER_FRAME)?;
        let mut color = self.hw.color.to_vec();
        for r in 0..25.min(self.hw.fetched_rows as usize) {
            ram[0xC800 + r * 40..0xC800 + r * 40 + 40].copy_from_slice(&self.hw.fetched_screen[r]);
            color[r * 40..r * 40 + 40].copy_from_slice(&self.hw.fetched_color[r]);
        }
        Ok(Picture { ram, vic, color })
    }
}
