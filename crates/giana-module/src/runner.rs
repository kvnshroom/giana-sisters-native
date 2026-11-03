//! High level boot of a fan hack disk: turns a `.d64`/`.g64` image or a single `.prg` into the 64K RAM the game would
//! run from, with no VICE and no C64 ROMs.
//!
//! The first PRG of the disk is loaded as `LOAD"*",8,1` would, started like `RUN` (the `SYS` address of its BASIC stub)
//! or at its load address, and run on the 6502 core of [`crate::cpu`]. The KERNAL is not there: a jump to one of its
//! entry points is trapped and answered by Rust code (`LOAD` reads the named file from the disk image, `SETLFS`,
//! `SETNAM`, `OPEN`/`CHRIN` for sequential reads, the rest does nothing). I/O reads return "idle" values (raster counter
//! runs, no key or joystick pressed). Banking writes to `$01` are recorded and decide whether `$D000..$DFFF` is I/O or
//! RAM, but the RAM itself is always RAM.
//!
//! The run ends when the program reaches the game entry (`$0820`), when it settles in an idle loop (everything is
//! unpacked and the program waits for input) or when it returns to BASIC. Disks with fast loaders or custom drive code
//! cannot work without a 1541 emulation; they are detected and refused with a clear message.

use crate::cpu::{Bus, Cpu};
use crate::disk::Disk;
use giana_extract::g64::DirEntry;
use std::collections::HashMap;

/// Result of a boot run.
#[derive(Debug, Clone)]
pub struct BootResult {
    /// The 64K RAM at the end of the run.
    pub ram: Vec<u8>,
    /// Log of the files that were loaded, e.g. `LOAD "G1" -> $0400..$07E8`.
    pub files: Vec<String>,
    /// Why the run stopped, in words.
    pub stop: String,
    /// True when the game entry was reached (as opposed to an idle loop or a return to BASIC).
    pub entry_reached: bool,
    /// The RAM at the moment of the last entry event (the unpacked program before its own start-up code ran), when
    /// there was one. `ram` is the state at the end of the run (the game waiting in its menu).
    pub entry_ram: Option<Vec<u8>>,
}

/// Knobs of the boot emulation.
#[derive(Debug, Clone)]
pub struct BootOptions {
    /// Addresses at which the run ends. `$0820` is the entry of the original game program; it only counts when it holds
    /// a `JMP`/`JSR` there (packer stubs of hacks use the same address for a small copy loop). Besides these, the run
    /// also ends when an unpacker running below `$0820` hands over to the program, or when a loader below `$0800`
    /// has loaded the big game file and jumps into it.
    pub entries: Vec<u16>,
    pub max_cycles: u64,
    pub max_loads: usize,
    /// Press keys and the joystick at wait loops to get past menus (before the game entry). Off to stop at the very
    /// first wait for input.
    pub inputs: bool,
}

impl Default for BootOptions {
    fn default() -> Self {
        BootOptions { entries: vec![0x0820], max_cycles: 200_000_000, max_loads: 64, inputs: true }
    }
}

const FAST_LOADER: &str = "THIS DISK USES A FAST LOADER OR CUSTOM DRIVE CODE - IT CANNOT BE LOADED WITHOUT A 1541 EMULATION. \
Use a cracked version that loads with the normal KERNAL, or convert it to a RAM dump / .grom module with VICE (tools/modules/capture_ram.sh).";

struct OpenFile {
    data: Vec<u8>,
    pos: usize,
}

struct Machine<'a> {
    ram: Vec<u8>,
    disk: Option<&'a Disk>,
    port: u8,
    ddr: u8,
    cycles: u64,
    dd00: u8,
    serial_toggles: u32,
    writes_hi: u64,
    io_reads: u64,
    key_reads: u64,
    vic: [u8; 64],
    dc00: u8,
    /// pressed keys (column, row) and joystick bits (0=up 1=down 2=left 3=right 4=fire) per port
    pressed: Vec<(u8, u8)>,
    joy: [u8; 2],
    // KERNAL state
    lfs: (u8, u8, u8),
    name: (u8, u16),
    open: HashMap<u8, OpenFile>,
    chan_in: Option<u8>,
    chan_out: Option<u8>,
    cmd_buf: Vec<u8>,
    iec_cmd: Vec<u8>,
    files: Vec<String>,
    loads: usize,
    fatal: Option<String>,
    /// a loader below $0800 loaded a big file (the game); the next jump into it is the entry
    big_load: bool,
    /// interrupt mask of CIA 1 (`$DC0D`): bit 0 is timer A, which the KERNAL leaves enabled
    cia1_mask: u8,
    /// GETIN calls without a key since the last check of the main loop: a program that only does that waits for a key
    getin_empty: u32,
    hooked_load_noted: bool,
    /// cycle of the next frame interrupt
    next_irq: u64,
    /// the key being held was already put into the keyboard buffer by the emulated IRQ keyboard scan
    key_queued: bool,
}

/// Cycles of a PAL frame (312 lines of 63 cycles): the rate of the interrupts the boot emulation delivers.
const FRAME_CYCLES: u64 = 312 * 63;

impl Machine<'_> {
    fn io_visible(&self) -> bool {
        self.port & 3 != 0 && self.port & 4 != 0
    }
    fn kernal_visible(&self) -> bool {
        self.port & 2 != 0
    }
    fn basic_visible(&self) -> bool {
        self.port & 3 == 3
    }
    fn io_read(&mut self, a: u16) -> u8 {
        self.io_reads += 1;
        let raster = ((self.cycles / 63) % 312) as u16;
        match a {
            0xD011 => 0x1B | ((raster >> 1) & 0x80) as u8,
            0xD012 => raster as u8,
            0xD019 => 0xF1,
            0xD01E | 0xD01F => 0,
            0xD020..=0xD02E => 0xF0 | self.vic[(a & 63) as usize],
            0xDC00 => 0xFF ^ self.joy[1],
            0xDC01 => {
                self.key_reads += 1;
                self.keyboard() & (0xFF ^ self.joy[0])
            }
            0xDC04 | 0xDC05 | 0xDC06 | 0xDC07 | 0xDD04 | 0xDD05 | 0xDD06 | 0xDD07 => (self.cycles >> 4) as u8,
            0xDC0D | 0xDD0D => 0x03,
            0xDD00 => (self.dd00 & 0x3F) | 0xC0,
            0xD000..=0xD3FF => self.vic[(a & 63) as usize],
            0xDC00..=0xDCFF | 0xDD00..=0xDDFF => 0,
            _ => 0xFF,
        }
    }
    /// Rows read from `$DC01` for the columns selected by the last write to `$DC00` (a 0 bit selects a column).
    fn keyboard(&self) -> u8 {
        let mut v = 0xFFu8;
        for &(col, row) in &self.pressed {
            if self.dc00 & (1 << col) == 0 {
                v &= !(1 << row);
            }
        }
        v
    }
    fn io_write(&mut self, a: u16, v: u8) {
        if (0xD000..=0xD3FF).contains(&a) {
            self.vic[(a & 63) as usize] = v;
        }
        if a == 0xDC00 {
            self.dc00 = v;
        }
        if a == 0xDC0D {
            // bit 7 set: the bits given are enabled, clear: disabled
            if v & 0x80 != 0 {
                self.cia1_mask |= v & 0x1F;
            } else {
                self.cia1_mask &= !(v & 0x1F);
            }
        }
        if a == 0xDD00 {
            // bits 3..5 are ATN/CLK/DATA out of the serial bus; the low two bits are the VIC bank
            if (self.dd00 ^ v) & 0x38 != 0 {
                self.serial_toggles += 1;
                if self.serial_toggles >= 48 && self.fatal.is_none() {
                    self.fatal = Some(FAST_LOADER.to_string());
                }
            }
            self.dd00 = v;
        }
    }

    fn name_bytes(&self) -> Vec<u8> {
        (0..self.name.0 as u16).map(|i| self.ram[self.name.1.wrapping_add(i) as usize]).collect()
    }

    fn note_cmd(&mut self, buf_is_iec: bool) {
        let buf = if buf_is_iec { &self.iec_cmd } else { &self.cmd_buf };
        if buf.windows(3).any(|w| w == b"M-W" || w == b"M-E" || w == b"M-R") && self.fatal.is_none() {
            self.fatal = Some(FAST_LOADER.to_string());
        }
    }

    fn do_load(&mut self, cpu: &mut Cpu) {
        self.loads += 1;
        if self.ram[0x330] != 0xA5 || self.ram[0x331] != 0xF4 {
            // The program replaced the KERNAL LOAD vector (a fast loader, or a hook of a cracked disk). The call is
            // answered here from the disk image all the same: a loader that does more than load shows in the result
            // (garbage in RAM), one that drives the serial bus itself is caught by the bus traffic check.
            if !self.hooked_load_noted {
                self.hooked_load_noted = true;
                self.files.push(format!("NOTE the program replaced the LOAD vector (now ${:02X}{:02X}); loads are done from the disk image", self.ram[0x331], self.ram[0x330]));
            }
        }
        let name = self.name_bytes();
        let shown = String::from_utf8_lossy(&name).to_string();
        let (_, dev, sa) = self.lfs;
        if !(8..=11).contains(&dev) {
            self.files.push(format!("LOAD \"{shown}\" from device {dev}: not a disk device"));
            cpu.c = true;
            cpu.a = 5;
            return;
        }
        let Some(disk) = self.disk else {
            self.files.push(format!("LOAD \"{shown}\": no disk (single PRG)"));
            cpu.c = true;
            cpu.a = 4;
            return;
        };
        let entry: Option<DirEntry> = if name.is_empty() || name == b"*" {
            disk.prgs().ok().and_then(|v| v.into_iter().next())
        } else if name == b"$" {
            None
        } else {
            disk.find(&name).ok().flatten()
        };
        let Some(e) = entry else {
            self.files.push(format!("LOAD \"{shown}\": file not found"));
            cpu.c = true;
            cpu.a = 4;
            return;
        };
        let data = match disk.read_entry(&e) {
            Ok(d) if d.len() >= 2 => d,
            Ok(_) => {
                cpu.c = true;
                cpu.a = 4;
                return;
            }
            Err(err) => {
                self.files.push(format!("LOAD \"{shown}\": {err}"));
                cpu.c = true;
                cpu.a = 4;
                return;
            }
        };
        let own = data[0] as usize | (data[1] as usize) << 8;
        let addr = if sa == 0 { cpu.x as usize | (cpu.y as usize) << 8 } else { own };
        let body = &data[2..];
        let n = body.len().min(0x1_0000 - addr);
        if cpu.a == 0 {
            self.ram[addr..addr + n].copy_from_slice(&body[..n]);
        }
        let end = addr + n;
        self.files.push(format!("LOAD \"{}\" -> ${:04X}..${:04X}", e.name, addr, end.wrapping_sub(1) & 0xFFFF));
        let ret = (self.ram[0x100 + cpu.sp.wrapping_add(1) as usize] as usize | (self.ram[0x100 + cpu.sp.wrapping_add(2) as usize] as usize) << 8) + 1;
        if n >= 0x8000 && ret < 0x0800 {
            self.big_load = true;
        }
        cpu.x = end as u8;
        cpu.y = (end >> 8) as u8;
        self.ram[0xAE] = end as u8;
        self.ram[0xAF] = (end >> 8) as u8;
        self.ram[0x90] = 0x40;
        cpu.c = false;
    }

    fn do_open(&mut self, cpu: &mut Cpu) {
        let (lfn, dev, sa) = self.lfs;
        let name = self.name_bytes();
        if sa == 15 || dev != 8 && (8..=11).contains(&dev) && sa >= 15 {
            self.cmd_buf.extend_from_slice(&name);
            self.note_cmd(false);
            self.open.insert(lfn, OpenFile { data: Vec::new(), pos: 0 });
            return;
        }
        if (8..=11).contains(&dev) {
            if name.first() == Some(&b'#') {
                self.fatal.get_or_insert_with(|| "THIS DISK USES DIRECT ACCESS (OPEN ...,\"#\") - NOT SUPPORTED WITHOUT A 1541 EMULATION".into());
                return;
            }
            let found = self.disk.and_then(|d| d.find(&name).ok().flatten());
            match found.and_then(|e| self.disk.unwrap().read_entry(&e).ok().map(|d| (e, d))) {
                Some((e, data)) => {
                    self.files.push(format!("OPEN \"{}\" ({} bytes)", e.name, data.len()));
                    self.open.insert(lfn, OpenFile { data, pos: 0 });
                }
                None => {
                    self.files.push(format!("OPEN \"{}\": file not found", String::from_utf8_lossy(&name)));
                    cpu.c = true;
                    cpu.a = 4;
                }
            }
        }
    }

    fn next_byte(&mut self) -> u8 {
        if let Some(f) = self.chan_in.and_then(|l| self.open.get_mut(&l)) {
            if f.pos < f.data.len() {
                let b = f.data[f.pos];
                f.pos += 1;
                if f.pos == f.data.len() {
                    self.ram[0x90] = 0x40;
                }
                return b;
            }
            self.ram[0x90] = 0x40;
        }
        0
    }

    /// A frame interrupt, when something asks for one: the raster interrupt of the VIC or a timer of CIA 1 is enabled.
    /// Like the hardware: PC and flags are pushed; with the KERNAL banked in its handler (`$FF48`) also saves A, X and Y
    /// and jumps through the RAM vector `$0314`, without it the vector at `$FFFE` is used. Programs with an IRQ driven
    /// depacker or intro (they `CLI` and then spin) only get on this way.
    fn interrupt(&mut self, cpu: &mut Cpu) {
        if self.vic[0x1A] & 1 == 0 && self.cia1_mask & 3 == 0 {
            return;
        }
        let (hi, lo) = ((cpu.pc >> 8) as u8, cpu.pc as u8);
        let flags = cpu.flags(false);
        cpu.push(self, hi);
        cpu.push(self, lo);
        cpu.push(self, flags);
        cpu.i = true;
        let vector = if self.kernal_visible() {
            let (a, x, y) = (cpu.a, cpu.x, cpu.y);
            cpu.push(self, a);
            cpu.push(self, x);
            cpu.push(self, y);
            0x0314
        } else {
            0xFFFE
        };
        cpu.pc = self.ram[vector] as u16 | (self.ram[vector + 1] as u16) << 8;
    }

    /// The keyboard scan of the KERNAL's IRQ handler: a key that goes down is put into the keyboard buffer (`$0277..`,
    /// the count in `$C6`), once per key press. Programs that wait for a key with `LDA $C6 / BEQ` (intros) need this.
    fn scan_keyboard(&mut self) {
        let key = self.pressed.iter().find_map(|&k| KEY_CHARS.iter().find(|(m, _)| *m == k).map(|&(_, c)| c));
        match key {
            Some(c) if !self.key_queued => {
                self.key_queued = true;
                let n = self.ram[0xC6] as usize;
                if n < 10 {
                    self.ram[0x0277 + n] = c;
                    self.ram[0xC6] = n as u8 + 1;
                }
            }
            None => self.key_queued = false,
            _ => {}
        }
    }

    /// A call into the KERNAL ROM below its jump table (`$E000..$FF80`) with the KERNAL banked in: programs call the
    /// screen and I/O initialisation of the ROM directly (`JSR $E544`, `$FDA3`, `$FD15`, ...). The few that matter for
    /// RAM are done here, every other one just returns. Only for calls (`JSR`): the address is where a `JSR` landed,
    /// so the return address is on the stack. Returns false when the ROM is not entered by a subroutine call.
    fn rom_routine(&mut self, cpu: &mut Cpu) -> bool {
        match cpu.pc {
            // the ways out of the KERNAL's interrupt handler: restore Y, X, A, then RTI
            0xEA31 | 0xEA7E | 0xEA81 => {
                if cpu.pc == 0xEA31 {
                    self.scan_keyboard();
                }
                cpu.y = cpu.pull(self);
                cpu.x = cpu.pull(self);
                cpu.a = cpu.pull(self);
                let f = cpu.pull(self);
                cpu.set_flags(f);
                let lo = cpu.pull(self) as u16;
                let hi = cpu.pull(self) as u16;
                cpu.pc = hi << 8 | lo;
                return true;
            }
            // CLRSCR and the screen editor initialisation: blank screen
            0xE544 | 0xE518 | 0xE51B | 0xE55F => self.ram[0x0400..0x07E8].fill(0x20),
            // IOINIT: keyboard column select
            0xFDA3 => self.dc00 = 0x7F,
            // RESTOR: the default vectors
            0xFD15 => {
                for (a, v) in [(0x0314usize, 0xEA31u16), (0x0316, 0xFE66), (0x0318, 0xFE47), (0x0330, 0xF4A5), (0x0332, 0xF6ED)] {
                    self.ram[a] = v as u8;
                    self.ram[a + 1] = (v >> 8) as u8;
                }
            }
            _ => {}
        }
        cpu.rts(self);
        true
    }

    /// Answers a call to a KERNAL entry point; returns false when `pc` is not one.
    fn kernal_trap(&mut self, cpu: &mut Cpu) -> bool {
        let pc = cpu.pc;
        if !(0xFF81..=0xFFF3).contains(&pc) || (pc - 0xFF81) % 3 != 0 {
            return false;
        }
        match pc {
            0xFF99 => {
                cpu.x = 0x00;
                cpu.y = 0xA0;
            }
            0xFF9C => {
                cpu.x = 0x00;
                cpu.y = 0x08;
            }
            0xFFB1 | 0xFFB4 => self.iec_cmd.clear(), // LISTEN / TALK
            0xFFA8 => {
                // IECOUT
                self.iec_cmd.push(cpu.a);
                self.note_cmd(true);
            }
            0xFFA5 => {
                cpu.a = 0;
            }
            0xFFB7 => cpu.a = self.ram[0x90],
            0xFFBA => self.lfs = (cpu.a, cpu.x, cpu.y),
            0xFFBD => self.name = (cpu.a, cpu.x as u16 | (cpu.y as u16) << 8),
            0xFFC0 => {
                cpu.c = false;
                self.do_open(cpu)
            }
            0xFFC3 => {
                self.open.remove(&cpu.a);
                if self.chan_in == Some(cpu.a) {
                    self.chan_in = None;
                }
                if self.chan_out == Some(cpu.a) {
                    self.chan_out = None;
                }
                cpu.c = false;
            }
            0xFFC6 => {
                self.chan_in = Some(cpu.x);
                cpu.c = false;
            }
            0xFFC9 => {
                self.chan_out = Some(cpu.x);
                cpu.c = false;
            }
            0xFFCC | 0xFFE7 => {
                self.chan_in = None;
                self.chan_out = None;
            }
            0xFFE4 if self.chan_in.is_none() => {
                // GETIN from the keyboard: the key being pressed, 0 if none
                cpu.a = self.pressed.iter().find_map(|&k| KEY_CHARS.iter().find(|(m, _)| *m == k).map(|&(_, c)| c)).unwrap_or(0);
                self.getin_empty = if cpu.a == 0 { self.getin_empty + 1 } else { 0 };
                // `JSR GETIN` followed by `BEQ`: the flags follow A
                cpu.z = cpu.a == 0;
                cpu.n = cpu.a & 0x80 != 0;
                cpu.c = false;
            }
            0xFFCF | 0xFFE4 => {
                cpu.a = self.next_byte();
                cpu.c = false;
            }
            0xFFD2 => {
                if self.chan_out.is_some_and(|l| self.lfs.0 == l && self.lfs.2 == 15 || self.open.get(&l).is_some_and(|f| f.data.is_empty())) {
                    self.cmd_buf.push(cpu.a);
                    self.note_cmd(false);
                }
                cpu.c = false;
            }
            0xFFD5 => self.do_load(cpu),
            0xFFD8 => {
                // SAVE: pretend it worked
                cpu.c = false;
            }
            0xFFE1 => {
                cpu.z = false; // STOP not pressed
            }
            0xFFED => {
                cpu.x = 40;
                cpu.y = 25;
            }
            0xFFF0 => {
                cpu.x = 0;
                cpu.y = 0;
            }
            0xFFF3 => {
                cpu.x = 0x00;
                cpu.y = 0xDC;
            }
            _ => {}
        }
        cpu.rts(self);
        true
    }
}

impl Bus for Machine<'_> {
    fn read(&mut self, a: u16) -> u8 {
        match a {
            0 => self.ddr,
            1 => self.port,
            0xD000..=0xDFFF if self.io_visible() => self.io_read(a),
            // the BASIC ROM's cold and warm start vectors (`JMP ($A002)` is how intros hand over to BASIC)
            0xA000..=0xA003 if self.basic_visible() => [0x94, 0xE3, 0x7B, 0xE3][(a - 0xA000) as usize],
            _ => self.ram[a as usize],
        }
    }
    fn write(&mut self, a: u16, v: u8) {
        match a {
            0 => {
                self.ddr = v;
                self.ram[0] = v;
            }
            1 => {
                self.port = v;
                self.ram[1] = v;
            }
            0xD000..=0xDFFF if self.io_visible() => self.io_write(a, v),
            _ => {
                if a >= 0x200 {
                    self.writes_hi += 1;
                }
                self.ram[a as usize] = v;
            }
        }
    }
}

type Input = (&'static str, &'static [(u8, u8)], [u8; 2]);
/// Keys of `INPUTS` as `(column written to $DC00, row read from $DC01)` and what GETIN returns for them.
const KEY_CHARS: &[((u8, u8), u8)] = &[((4, 7), 0x20), ((0, 7), 0x31), ((3, 7), 0x32), ((0, 1), 0x0D), ((7, 4), 0x4E), ((1, 3), 0x59), ((6, 1), 0x45), ((2, 2), 0x44)];

/// Things to press when the program sits in a wait loop, one after the other: N (the "no trainer" answer and what
/// several intros wait for), fire on port 2, fire on port 1, the keys 1 and 2, return, space, Y. (`(column, row)` of
/// the key matrix; joystick bit 4 is fire.)
const INPUTS: &[Input] = &[
    ("key n", &[(7, 4)], [0; 2]),
    ("joystick 2 fire", &[], [0, 0x10]),
    ("joystick 1 fire", &[], [0x10, 0]),
    ("key 1", &[(0, 7)], [0; 2]),
    ("key 2", &[(3, 7)], [0; 2]),
    ("return", &[(0, 1)], [0; 2]),
    ("space", &[(4, 7)], [0; 2]),
    ("key y", &[(1, 3)], [0; 2]),
    ("key e", &[(6, 1)], [0; 2]),
    ("key d", &[(2, 2)], [0; 2]),
];

fn ram_hash(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in b.chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(0x100000001b3).rotate_left(5);
    }
    h
}

/// Parses `SYS nnnn` out of the BASIC stub at `$0801`.
fn sys_address(ram: &[u8]) -> Option<u16> {
    let mut p = 0x0801usize;
    for _ in 0..16 {
        let link = ram[p] as usize | (ram[p + 1] as usize) << 8;
        let mut q = p + 4;
        while q < 0x10000 && ram[q] != 0 {
            if ram[q] == 0x9E {
                q += 1;
                while ram[q] == b' ' || ram[q] == b'(' {
                    q += 1;
                }
                let mut v: u32 = 0;
                let mut any = false;
                while ram[q].is_ascii_digit() {
                    v = v * 10 + (ram[q] - b'0') as u32;
                    any = true;
                    q += 1;
                }
                return (any && v < 0x10000).then_some(v as u16);
            }
            q += 1;
        }
        if link == 0 || link <= p {
            break;
        }
        p = link;
    }
    None
}

/// `GIANA_BOOT_DEBUG`: the busiest addresses, the files loaded so far and, with `GIANA_BOOT_DUMP`, the RAM.
fn debug_report(hist: &[u32], m: &Machine, pc: u16) {
    let mut v: Vec<(u32, usize)> = hist.iter().enumerate().map(|(a, &c)| (c, a)).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    eprintln!("  stopped at pc ${pc:04X}, $01 = ${:02X}, I/O visible: {}", m.port, m.io_visible());
    for (c, a) in v.iter().take(std::env::var("GIANA_BOOT_TOP").ok().and_then(|v| v.parse().ok()).unwrap_or(12)) {
        eprintln!("  pc ${a:04X}: {c}");
    }
    eprintln!("  files: {:?}", m.files);
    if let Some(p) = std::env::var_os("GIANA_BOOT_DUMP") {
        let _ = std::fs::write(p, &m.ram);
    }
}

fn boot(disk: Option<&Disk>, prg: Vec<u8>, name: &str, opts: &BootOptions) -> Result<BootResult, String> {
    if prg.len() < 3 {
        return Err(format!("{name}: the boot file is empty"));
    }
    let load = prg[0] as usize | (prg[1] as usize) << 8;
    let body = &prg[2..];
    if load + body.len() > 0x1_0000 {
        return Err(format!("{name}: the boot file does not fit into memory"));
    }
    let mut m = Machine {
        ram: vec![0; 0x1_0000],
        disk,
        port: 0x37,
        ddr: 0x2F,
        cycles: 0,
        dd00: 0x97,
        serial_toggles: 0,
        writes_hi: 0,
        io_reads: 0,
        key_reads: 0,
        vic: [0; 64],
        dc00: 0x7F, // the KERNAL leaves column 7 selected
        pressed: Vec::new(),
        joy: [0; 2],
        lfs: (0, 8, 0),
        name: (0, 0),
        open: HashMap::new(),
        chan_in: None,
        chan_out: None,
        cmd_buf: Vec::new(),
        iec_cmd: Vec::new(),
        files: Vec::new(),
        loads: 0,
        fatal: None,
        big_load: false,
        cia1_mask: 1,
        getin_empty: 0,
        hooked_load_noted: false,
        next_irq: FRAME_CYCLES,
        key_queued: false,
    };
    m.ram[0] = 0x2F;
    m.ram[1] = 0x37;
    m.ram[load..load + body.len()].copy_from_slice(body);
    // state that BASIC / the KERNAL leave behind and that crunchers read
    let end = load + body.len();
    for (a, v) in [(0x2B, 0x0801u16), (0x2D, end as u16), (0x2F, end as u16), (0x31, end as u16), (0xAE, end as u16), (0x33, 0xA000), (0x37, 0xA000)] {
        m.ram[a] = v as u8;
        m.ram[a + 1] = (v >> 8) as u8;
    }
    for (a, v) in [(0x0314, 0xEA31u16), (0x0316, 0xFE66), (0x0318, 0xFE47), (0x0330, 0xF4A5), (0x0332, 0xF6ED)] {
        m.ram[a] = v as u8;
        m.ram[a + 1] = (v >> 8) as u8;
    }
    m.ram[0x0400..0x0800].fill(0x20);
    m.files.push(format!("BOOT \"{name}\" -> ${:04X}..${:04X}", load, end.wrapping_sub(1)));

    let mut cpu = Cpu { sp: 0xF6, ..Default::default() };
    // A file that loads over the stack page ($0100..) is the "stack autostart" trick: the KERNAL's RTS after the LOAD
    // returns into the address the file wrote there.
    let stack_autostart = load < 0x0200 && end > 0x01F0;
    cpu.pc = if stack_autostart {
        cpu.sp = 0xF4;
        cpu.rts(&mut m);
        cpu.pc
    } else if load == 0x0801 {
        sys_address(&m.ram).ok_or_else(|| format!("{name}: BASIC program without a SYS call (cannot be started without BASIC)"))?
    } else {
        load as u16
    };
    m.files.push(format!("START at ${:04X}", cpu.pc));

    let debug = std::env::var_os("GIANA_BOOT_DEBUG").is_some();
    let mut hist = vec![0u32; if debug { 0x1_0000 } else { 0 }];
    let mut trail = [0u16; 32];
    let (mut last_pc, mut low_run) = (0u16, 0u64);
    let mut insn: u64 = 0;
    let mut seen = vec![false; 0x1_0000];
    let (mut distinct, mut idle_blocks, mut last_hash, mut blk_keys) = (0usize, 0u32, 0u64, 0u64);
    let (mut next_input, mut release_at) = (0usize, None::<u64>);
    let stop: String;
    let mut snapshot: Option<(String, Vec<u8>)> = None;
    // After the game entry a menu may be an intro that unpacks the rest of the game after a key press. A key is pressed
    // there too; when nothing new was unpacked or loaded afterwards (no new entry event) the game only moved on from its
    // menu, and the RAM from before the key press is the result (it has less run-time state).
    let mut before_press: Option<Vec<u8>> = None;
    let mut force_idle = false;
    let mut loads_seen = 0usize;
    let mut deadline = u64::MAX;
    // Emulated KERNAL/ROM calls and RUN redirects do not step the CPU; they are charged cycles here so that a disk that
    // loops through them (a hostile or broken one) still hits the cycle limit instead of hanging the import.
    const TRAP_CYCLES: u64 = 64;
    const MAX_RUN_REDIRECTS: u32 = 16;
    let mut run_redirects = 0u32;
    loop {
        if cpu.cycles > opts.max_cycles {
            return Err(format!("{name}: no result after {} million cycles (stuck at ${:04X})", opts.max_cycles / 1_000_000, cpu.pc));
        }
        if !cpu.i && cpu.cycles >= m.next_irq {
            m.next_irq = cpu.cycles + FRAME_CYCLES;
            m.interrupt(&mut cpu);
        }
        let pc = cpu.pc;
        if m.kernal_visible() && pc >= 0xFF81 && m.kernal_trap(&mut cpu) {
            cpu.cycles += TRAP_CYCLES;
            continue;
        }
        // `JMP ($A002)` into BASIC's warm start with "RUN" in the keyboard buffer (what intros type for the player)
        let typed_run = pc == 0xE37B && m.ram[0xC6] >= 4 && m.ram[0x0277..0x027B] == *b"RUN\r";
        if m.basic_visible() && (pc == 0xA871 || typed_run) {
            // `JMP $A871` is BASIC's RUN: the program in memory is a BASIC stub (`0 SYS nnnn`, the packers leave it at
            // `$0801`), and that is what really starts the game. A SYS into ROM, or RUN over and over, is refused.
            if let Some(addr) = sys_address(&m.ram) {
                run_redirects += 1;
                if run_redirects > MAX_RUN_REDIRECTS || addr >= 0xA000 {
                    return Err(format!("{name}: the BASIC stub does not start a program (SYS {addr})"));
                }
                m.files.push(format!("RUN (BASIC stub: SYS {addr})"));
                cpu.pc = addr;
                cpu.cycles += TRAP_CYCLES;
                continue;
            }
        }
        if m.kernal_visible() && (0xE000..0xFF81).contains(&pc) && m.rom_routine(&mut cpu) {
            cpu.cycles += TRAP_CYCLES;
            continue;
        }
        if m.basic_visible() && matches!(pc, 0xA474 | 0xA480 | 0xA483 | 0xA659 | 0xA68E | 0xA7AE | 0xA7E1 | 0xE37B | 0xE394 | 0xFCE2 | 0xFE66) {
            stop = format!("the program returned to BASIC / reset (${pc:04X})");
            break;
        }
        // Entry events. The RAM at the last one is the result: multi-stage packers hand over several times, and the
        // game's own start-up (which changes RAM) runs after the last one.
        let event = if opts.entries.contains(&pc) && insn > 0 && pc != last_pc && (pc != 0x0820 || matches!(m.ram[0x0820], 0x4C | 0x20)) {
            Some(format!("game entry ${pc:04X} reached"))
        } else if pc >= 0x0820 && pc < 0xFF81 && last_pc < 0x0820 && low_run >= 50_000 {
            Some(format!("unpacker finished (ran {low_run} instructions below $0820), entered ${pc:04X}"))
        } else if m.big_load && (0x0820..0xFF81).contains(&pc) {
            m.big_load = false;
            Some(format!("main program loaded, entered at ${pc:04X}"))
        } else {
            None
        };
        if let Some(ev) = event {
            m.files.push(format!("EVENT {ev}"));
            snapshot = Some((ev, m.ram.clone()));
            before_press = None;
            next_input = 0;
            deadline = cpu.cycles + 40_000_000;
        }
        if cpu.cycles > deadline && opts.inputs && next_input < INPUTS.len() && release_at.is_none() {
            // Still running and still something to try: an intro with a scroller and music never looks idle. Treat
            // the time limit like an idle loop once and press the next key.
            force_idle = true;
            deadline = cpu.cycles + 3_000_000;
        }
        if cpu.cycles > deadline {
            stop = "time limit after the entry".to_string();
            if debug {
                debug_report(&hist, &m, cpu.pc);
            }
            break;
        }
        if let Some(f) = m.fatal.take() {
            if debug {
                debug_report(&hist, &m, pc);
            }
            return Err(f);
        }
        if pc >= 0x0820 && last_pc < 0x0820 && low_run >= 1000 && debug {
            eprintln!("  jump {last_pc:04X} -> {pc:04X} after low run {low_run} at insn {insn}, bytes there {:02X?}", &m.ram[pc as usize..pc as usize + 6]);
        }
        if pc >= 0x0820 {
            low_run = 0;
        } else {
            low_run += 1;
        }
        last_pc = pc;
        if debug {
            hist[pc as usize] += 1;
        }
        if !seen[pc as usize] {
            seen[pc as usize] = true;
            distinct += 1;
        }
        if debug {
            trail[(insn % 32) as usize] = pc;
        }
        if let Err(e) = cpu.step(&mut m) {
            if debug {
                let last: Vec<String> = (insn + 1..insn + 33).map(|i| format!("{:04X}", trail[(i % 32) as usize])).collect();
                eprintln!("  last addresses before the error: {}", last.join(" "));
            }
            if snapshot.is_some() {
                stop = e;
                break;
            }
            return Err(format!("{name}: {e}"));
        }
        m.cycles = cpu.cycles;
        insn += 1;
        if insn % 100_000 == 0 {
            // idle = the program only runs a small loop and either nothing in RAM changes or it keeps polling the keys
            let hash = ram_hash(&m.ram[0x0800..]);
            let polling = m.key_reads - blk_keys > if snapshot.is_some() { 8 } else { 1000 };
            if std::mem::take(&mut force_idle) || m.getin_empty > 200 {
                // nothing but GETIN calls that find no key: waiting for a key
                idle_blocks = idle_blocks.max(100);
            } else if distinct <= 1024 && (hash == last_hash || polling) {
                idle_blocks += 1;
            } else {
                idle_blocks = 0;
            }
            if release_at.is_some_and(|t| cpu.cycles >= t) {
                m.pressed.clear();
                m.joy = [0; 2];
                release_at = None;
            }
            if release_at.is_some() {
                idle_blocks = 0;
            }
            if idle_blocks >= if snapshot.is_some() { 12 } else { 3 } {
                if m.loads != loads_seen {
                    // the last key made the program load something: the next prompt starts from the first key again
                    loads_seen = m.loads;
                    next_input = 0;
                }
                if let Some((label, keys, joy)) = INPUTS.get(next_input).filter(|_| opts.inputs) {
                    // maybe a menu or a "press any key" screen: press something and see if the game starts
                    next_input += 1;
                    if snapshot.is_some() && before_press.is_none() {
                        before_press = Some(m.ram.clone());
                    }
                    m.files.push(format!("INPUT {label} (program waits at ${:04X})", cpu.pc));
                    m.pressed = keys.to_vec();
                    m.joy = *joy;
                    release_at = Some(cpu.cycles + 1_500_000);
                    idle_blocks = 0;
                    if snapshot.is_some() {
                        deadline = cpu.cycles + 40_000_000;
                    }
                } else {
                    stop = format!("idle loop around ${:04X} (waiting for input)", cpu.pc);
                    break;
                }
            }
            last_hash = hash;
            m.getin_empty = 0;
            blk_keys = m.key_reads;
            seen.fill(false);
            distinct = 0;
        }
        if cpu.cycles > opts.max_cycles {
            if debug {
                debug_report(&hist, &m, cpu.pc);
            }
            if snapshot.is_some() {
                stop = "cycle limit after the entry".to_string();
                break;
            }
            return Err(format!("{name}: no result after {} million cycles (stuck at ${:04X})", opts.max_cycles / 1_000_000, cpu.pc));
        }
        if m.loads > opts.max_loads {
            return Err(format!("{name}: more than {} file loads, giving up", opts.max_loads));
        }
    }
    if let Some(f) = m.fatal.take() {
        return Err(f);
    }
    if let Some(p) = std::env::var_os("GIANA_BOOT_DUMP") {
        let _ = std::fs::write(p, &m.ram);
    }
    if m.hooked_load_noted && snapshot.is_none() {
        // the loads went to a loader of the program that was not run, and nothing was unpacked or started afterwards
        return Err(FAST_LOADER.to_string());
    }
    let ram = before_press.unwrap_or_else(|| std::mem::take(&mut m.ram));
    Ok(match snapshot {
        Some((ev, entry)) => BootResult { ram, entry_ram: Some(entry), files: m.files, stop: format!("{ev}; then {stop}"), entry_reached: true },
        None => BootResult { ram, entry_ram: None, files: m.files, stop, entry_reached: false },
    })
}

/// Boots a disk image (`.d64` or `.g64`): see the module documentation.
pub fn boot_disk(image: &[u8], name: &str) -> Result<BootResult, String> {
    boot_disk_with(image, name, &BootOptions::default())
}

pub fn boot_disk_with(image: &[u8], name: &str, opts: &BootOptions) -> Result<BootResult, String> {
    boot_disk_nth(image, name, 0, opts)
}

/// The programs of a disk that can be started like `LOAD"*",8,1` / `RUN` would: PRG files with a BASIC start
/// (load address `$0801`), in directory order. The first one is what a disk boots; construction kits and multi-program
/// disks keep their game in a later one.
pub fn boot_candidates(image: &[u8]) -> Result<Vec<String>, String> {
    let disk = Disk::parse(image)?;
    let mut out = Vec::new();
    for (i, e) in disk.prgs()?.into_iter().enumerate() {
        let data = disk.read_entry(&e)?;
        if i == 0 || (data.starts_with(&[0x01, 0x08]) || data.starts_with(&[0x10, 0x08])) {
            out.push(e.name);
        }
    }
    Ok(out)
}

/// Boots the `n`-th program of the list [`boot_candidates`] gives (0 is the program a disk boots by itself).
pub fn boot_disk_nth(image: &[u8], name: &str, n: usize, opts: &BootOptions) -> Result<BootResult, String> {
    let disk = Disk::parse(image).map_err(|e| format!("{name}: {e}"))?;
    let mut prgs = Vec::new();
    for (i, e) in disk.prgs()?.into_iter().enumerate() {
        let data = disk.read_entry(&e).map_err(|err| format!("{name}: {err}"))?;
        if i == 0 || (data.starts_with(&[0x01, 0x08]) || data.starts_with(&[0x10, 0x08])) {
            prgs.push((e.name, data));
        }
    }
    if prgs.is_empty() {
        return Err(format!("{name}: the disk has no program file (PRG)"));
    }
    let (file, prg) = prgs.into_iter().nth(n).ok_or_else(|| format!("{name}: the disk has no program number {n}"))?;
    boot(Some(&disk), prg, &file, opts)
}

/// Boots a single `.prg` (load address `$0801` with a BASIC `SYS` stub, or any address to start at). Files it tries
/// to load are not found.
pub fn boot_prg(prg: &[u8], name: &str) -> Result<BootResult, String> {
    boot_prg_with(prg, name, &BootOptions::default())
}

pub fn boot_prg_with(prg: &[u8], name: &str, opts: &BootOptions) -> Result<BootResult, String> {
    boot(None, prg.to_vec(), name, opts)
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
