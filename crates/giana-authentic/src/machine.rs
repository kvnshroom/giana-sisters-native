//! The C64 slice: 64K RAM, the I/O the original screens use (VIC-II, SID registers, CIA 1 and 2, colour RAM), the 6502
//! and a Rust stand-in for the few KERNAL routines the original code calls. There is no ROM and no 1541: disk accesses
//! are answered by Rust code (`DiskEvent`).

use crate::cpu::{Bus, Cpu};
use crate::vic::{Vic, CYCLES};

/// What the program asked the disk drive to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiskEvent {
    /// `SAVE` of RAM `start..end` under `name` (the program scratches the old file first).
    Save { name: Vec<u8>, start: u16, data: Vec<u8> },
}

pub struct C64Bus {
    pub ram: Vec<u8>,
    pub color: Vec<u8>,
    pub vic: Vic,
    pub sid: [u8; 32],
    /// CIA 1/2 port latches and direction registers
    pub cia1: [u8; 16],
    pub cia2: [u8; 16],
    /// joystick states, bit 0 up, 1 down, 2 left, 3 right, 4 fire (1 = pressed)
    pub joy1: u8,
    pub joy2: u8,
    /// keyboard matrix: bit `row` of entry `col` is set while the key is down
    pub keys: [u8; 8],
    /// total cycles since the start
    pub clock: u64,
    /// a write to a SID register happened: (register, value, frame cycle)
    pub sid_writes: Vec<(u8, u8)>,
    pub log_writes: bool,
    /// IRQ line history: bit 0 after the last cycle, bit 1 after the one before (the CPU polls one cycle early)
    pub irq_hist: u8,
    /// (raster line, cycle, address, value) of every write while `log_writes` is on
    pub write_log: Vec<(u16, u16, u16, u8)>,
}

impl C64Bus {
    fn port(&self) -> u8 {
        // CPU port: bits with DDR 0 read as 1 (pull-ups)
        (self.ram[1] & self.ram[0]) | !self.ram[0]
    }

    pub fn kernal_visible(&self) -> bool {
        self.port() & 2 != 0
    }

    fn io_visible(&self) -> bool {
        let p = self.port();
        p & 3 != 0 && p & 4 != 0
    }

    pub fn vic_bank(&self) -> usize {
        ((!self.cia2[0]) & 3) as usize * 0x4000
    }

    /// Reads without side effects or timing (for the renderer, tests and the host).
    pub fn peek(&self, a: u16) -> u8 {
        if (0xD000..0xE000).contains(&a) && self.io_visible() {
            return match a {
                0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] | 0xF0,
                0xD400..=0xD7FF => self.sid[(a & 0x1F) as usize],
                0xD000..=0xD3FF => self.vic.regs[(a & 0x3F) as usize],
                0xDC00..=0xDCFF => self.cia1[(a & 0xF) as usize],
                0xDD00..=0xDDFF => self.cia2[(a & 0xF) as usize],
                _ => 0xFF,
            };
        }
        self.ram[a as usize]
    }

    fn tick(&mut self) {
        self.clock += 1;
        self.vic.cycle += 1;
        if self.vic.cycle > CYCLES {
            let ram = std::mem::take(&mut self.ram);
            let color = std::mem::take(&mut self.color);
            let bank = self.vic_bank();
            self.vic.end_line(&ram, &color, bank);
            self.ram = ram;
            self.color = color;
        }
        self.vic.start_cycle();
        self.irq_hist = self.irq_hist << 1 | self.vic.irq_line() as u8;
        if self.vic.cycle == 15 && self.vic.badline {
            let bank = self.vic_bank();
            let (ram, color) = (std::mem::take(&mut self.ram), std::mem::take(&mut self.color));
            self.vic.c_access(&ram, &color, bank);
            self.ram = ram;
            self.color = color;
        }
    }

    /// Bad line stall: the CPU cannot read while the VIC fetches the video matrix (cycles 12..=54), and writes are not
    /// possible once the VIC owns the bus (cycles 15..=54).
    fn stall(&mut self, write: bool) {
        if self.vic.badline {
            let c = self.vic.cycle;
            let lo = if write { 15 } else { 12 };
            if (lo..=54).contains(&c) {
                while self.vic.cycle <= 54 {
                    self.tick();
                }
            }
        }
        self.sprite_stall();
    }

    /// Sprite DMA: each enabled sprite that is displayed on this line takes the bus for 2 cycles (3 with the BA lead).
    fn sprite_stall(&mut self) {
        // not needed by the screens handled so far; kept as the single hook
    }

    fn io_read(&mut self, a: u16) -> u8 {
        match a {
            0xD000..=0xD3FF => self.vic.read((a & 0x3F) as u8),
            0xD400..=0xD7FF => match a & 0x1F {
                0x1B | 0x1C => 0,
                r => self.sid[r as usize],
            },
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] | 0xF0,
            0xDC00..=0xDCFF => match a & 0xF {
                0 => {
                    (self.cia1[0] | !self.cia1[2]) & !(self.joy2 & 0x1F)
                }
                1 => {
                    let mut v = 0xFFu8;
                    let sel = self.cia1[0] | !self.cia1[2];
                    for col in 0..8 {
                        if sel & (1 << col) == 0 {
                            v &= !self.keys[col];
                        }
                    }
                    (v | self.cia1[3]) & !(self.joy1 & 0x1F)
                }
                0xD => 0,
                r => self.cia1[r as usize],
            },
            0xDD00..=0xDDFF => match a & 0xF {
                0 => (self.cia2[0] | !self.cia2[2]) | 0,
                0xD => 0,
                r => self.cia2[r as usize],
            },
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, a: u16, v: u8) {
        match a {
            0xD000..=0xD3FF => self.vic.write((a & 0x3F) as u8, v),
            0xD400..=0xD7FF => {
                self.sid[(a & 0x1F) as usize] = v;
                self.sid_writes.push(((a & 0x1F) as u8, v));
            }
            0xD800..=0xDBFF => self.color[(a - 0xD800) as usize] = v & 0x0F,
            0xDC00..=0xDCFF => self.cia1[(a & 0xF) as usize] = v,
            0xDD00..=0xDDFF => self.cia2[(a & 0xF) as usize] = v,
            _ => {}
        }
    }
}

impl Bus for C64Bus {
    fn read(&mut self, a: u16) -> u8 {
        self.stall(false);
        let v = if (0xD000..0xE000).contains(&a) && self.io_visible() {
            self.io_read(a)
        } else if a == 0 {
            self.ram[0]
        } else if a == 1 {
            self.port()
        } else {
            self.ram[a as usize]
        };
        self.tick();
        v
    }

    fn write(&mut self, a: u16, v: u8) {
        self.stall(true);
        if self.log_writes {
            self.write_log.push((self.vic.line, self.vic.cycle, a, v));
        }
        if (0xD000..0xE000).contains(&a) && self.io_visible() {
            self.io_write(a, v);
        } else {
            self.ram[a as usize] = v;
        }
        self.tick();
    }

    fn idle(&mut self) {
        self.stall(false);
        self.tick();
    }
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: C64Bus,
    /// disk requests since the last `take_disk`
    pub disk: Vec<DiskEvent>,
    /// the last error (undefined opcode, unknown KERNAL entry): the run stops
    pub error: Option<String>,
    /// `run_frame` stops when the program counter reaches this address (and sets `stopped`)
    pub stop_at: Option<u16>,
    pub stopped: bool,
    /// scratch for the LISTEN/CIOUT command channel
    cmd: Vec<u8>,
    name_ptr: (u16, u8),
    /// keyboard buffer state of the KERNAL stand-in
    kbd_prev: u8,
}

impl Machine {
    /// A machine with the given 64K RAM contents.
    pub fn new(ram: Vec<u8>) -> Self {
        assert_eq!(ram.len(), 65536);
        let bus = C64Bus {
            ram,
            color: vec![0; 1024],
            vic: Vic::new(),
            sid: [0; 32],
            cia1: [0; 16],
            cia2: [0; 16],
            joy1: 0,
            joy2: 0,
            keys: [0; 8],
            clock: 0,
            sid_writes: Vec::new(),
            log_writes: false,
            irq_hist: 0,
            write_log: Vec::new(),
        };
        let mut m = Machine { cpu: Cpu::new(), bus, disk: Vec::new(), error: None, stop_at: None, stopped: false, cmd: Vec::new(), name_ptr: (0, 0), kbd_prev: 64 };
        m.bus.ram[0] = 0x2F;
        m.bus.ram[1] = 0x37;
        m.bus.cia1[2] = 0xFF;
        m.bus.cia2[2] = 0x3F;
        m.bus.cia2[0] = 0x03;
        m
    }

    pub fn take_disk(&mut self) -> Vec<DiskEvent> {
        std::mem::take(&mut self.disk)
    }

    fn burn(&mut self, n: u32) {
        for _ in 0..n {
            self.bus.idle();
        }
    }

    /// Takes the pending IRQ if the CPU allows it. With the KERNAL ROM visible the IRQ goes through the ROM prologue and
    /// `($0314)`, otherwise through the RAM vector at `$FFFE`.
    fn maybe_irq(&mut self) {
        if self.cpu.i || self.bus.irq_hist & 2 == 0 {
            return;
        }
        if self.bus.kernal_visible() {
            // hardware part (7 cycles), then $FF48: PHA TXA PHA TYA PHA TSX LDA $0104,X AND #$10 BEQ JMP ($0314)
            self.cpu.interrupt_push(&mut self.bus, false);
            self.burn(2);
            for v in [self.cpu.a, self.cpu.x, self.cpu.y] {
                let s = self.cpu.sp;
                self.bus.ram[0x100 + s as usize] = v;
                self.cpu.sp = s.wrapping_sub(1);
            }
            self.cpu.pc = self.bus.ram[0x314] as u16 | (self.bus.ram[0x315] as u16) << 8;
            self.burn(29);
        } else {
            self.cpu.interrupt(&mut self.bus, 0xFFFE, false);
        }
    }

    /// Runs one instruction (or one KERNAL stand-in). Returns false when stopped by an error.
    pub fn step(&mut self) -> bool {
        if self.error.is_some() {
            return false;
        }
        self.maybe_irq();
        let pc = self.cpu.pc;
        if pc >= 0xE000 && self.bus.kernal_visible() {
            if let Err(e) = self.kernal(pc) {
                self.error = Some(e);
                return false;
            }
            return true;
        }
        if let Err(e) = self.cpu.step(&mut self.bus) {
            self.error = Some(e);
            return false;
        }
        true
    }

    /// Runs until the VIC has finished the next frame (or an error occurs).
    pub fn run_frame(&mut self) -> bool {
        self.bus.vic.frame_done = false;
        self.bus.sid_writes.clear();
        while !self.bus.vic.frame_done {
            if !self.step() {
                return false;
            }
            if self.stop_at == Some(self.cpu.pc) {
                self.stopped = true;
                break;
            }
        }
        true
    }

    fn rts(&mut self) {
        let lo = {
            self.cpu.sp = self.cpu.sp.wrapping_add(1);
            self.bus.ram[0x100 + self.cpu.sp as usize] as u16
        };
        self.cpu.sp = self.cpu.sp.wrapping_add(1);
        let hi = self.bus.ram[0x100 + self.cpu.sp as usize] as u16;
        self.cpu.pc = (lo | hi << 8).wrapping_add(1);
        self.burn(6);
    }

    fn pull(&mut self) -> u8 {
        self.cpu.sp = self.cpu.sp.wrapping_add(1);
        self.bus.ram[0x100 + self.cpu.sp as usize]
    }

    /// Stand-ins for the KERNAL entries the original code uses.
    fn kernal(&mut self, pc: u16) -> Result<(), String> {
        match pc {
            // IRQ: increment the jiffy clock, scan the keyboard, then return from the interrupt
            0xEA31 => {
                self.scan_keyboard();
                self.burn(100);
                self.cpu.pc = 0xEA81;
            }
            0xEA81 => {
                self.cpu.y = self.pull();
                self.cpu.x = self.pull();
                self.cpu.a = self.pull();
                let f = self.pull();
                self.cpu.set_flags(f);
                let lo = self.pull() as u16;
                let hi = self.pull() as u16;
                self.cpu.pc = lo | hi << 8;
                self.burn(6 + 3 + 3 + 3 + 4);
            }
            // GETIN from the keyboard queue
            0xE124 => {
                let n = self.bus.ram[0xC6];
                if n == 0 {
                    self.cpu.a = 0;
                    self.cpu.z = true;
                    self.cpu.n = false;
                } else {
                    let ch = self.bus.ram[0x277];
                    for i in 0..9 {
                        self.bus.ram[0x277 + i] = self.bus.ram[0x278 + i];
                    }
                    self.bus.ram[0xC6] = n - 1;
                    self.cpu.a = ch;
                    self.cpu.z = ch == 0;
                    self.cpu.n = ch & 0x80 != 0;
                    self.cpu.i = false;
                }
                self.cpu.c = false;
                self.rts();
            }
            // IOINIT: the registers it leaves behind
            0xFDA3 => {
                self.bus.cia1[0xD] = 0x7F;
                self.bus.cia2[0xD] = 0x7F;
                self.bus.cia1[0] = 0x7F;
                self.bus.cia1[2] = 0xFF;
                self.bus.cia1[3] = 0;
                self.bus.cia2[0] = 0x03;
                self.bus.cia2[2] = 0x3F;
                self.bus.sid = [0; 32];
                self.bus.ram[1] = 0x37;
                self.burn(120);
                self.rts();
            }
            // LISTEN, SECOND, UNLSN: open the command channel
            0xFFB1 => {
                self.cmd.clear();
                self.rts();
            }
            0xFF93 | 0xFFAE => self.rts(),
            // CIOUT
            0xFFA8 => {
                self.cmd.push(self.cpu.a);
                self.cpu.c = false;
                self.rts();
            }
            // SETLFS
            0xFFBA => self.rts(),
            // SETNAM: A = length, X/Y = address
            0xFFBD => {
                self.name_ptr = (self.cpu.x as u16 | (self.cpu.y as u16) << 8, self.cpu.a);
                self.rts();
            }
            // SAVE: A = zero page pointer to the start address, X/Y = end address
            0xFFD8 => {
                let zp = self.cpu.a as usize;
                let start = self.bus.ram[zp] as u16 | (self.bus.ram[zp + 1] as u16) << 8;
                let end = self.cpu.x as u16 | (self.cpu.y as u16) << 8;
                let (p, n) = self.name_ptr;
                let name: Vec<u8> = (0..n as u16).map(|i| self.bus.ram[p.wrapping_add(i) as usize]).collect();
                let data = self.bus.ram[start as usize..end as usize].to_vec();
                self.disk.push(DiskEvent::Save { name, start, data });
                self.cpu.c = false;
                self.rts();
            }
            _ => return Err(format!("unhandled KERNAL entry ${pc:04X}")),
        }
        Ok(())
    }

    /// SCNKEY: matrix to PETSCII, one new key per interrupt into the queue at `$0277`.
    fn scan_keyboard(&mut self) {
        const TABLE: [u8; 64] = [
            0x14, 0x0D, 0x1D, 0x88, 0x85, 0x86, 0x87, 0x11, 0x33, 0x57, 0x41, 0x34, 0x5A, 0x53, 0x45, 0xFF, 0x35, 0x52, 0x44, 0x36, 0x43,
            0x46, 0x54, 0x58, 0x37, 0x59, 0x47, 0x38, 0x42, 0x48, 0x55, 0x56, 0x39, 0x49, 0x4A, 0x30, 0x4D, 0x4B, 0x4F, 0x4E, 0x2B, 0x50,
            0x4C, 0x2D, 0x2E, 0x3A, 0x40, 0x2C, 0x5C, 0x2A, 0x3B, 0x13, 0xFF, 0x3D, 0x5E, 0x2F, 0x31, 0x5F, 0xFF, 0x32, 0x20, 0xFF, 0x51,
            0x03,
        ];
        let mut code = 64u8;
        'outer: for col in 0..8usize {
            for row in 0..8usize {
                if self.bus.keys[col] & (1 << row) != 0 {
                    let c = (col * 8 + row) as u8;
                    // shift keys (left 15, right 52) are modifiers, not keys
                    if c == 15 || c == 52 || c == 58 || c == 61 {
                        continue;
                    }
                    code = c;
                    break 'outer;
                }
            }
        }
        self.bus.ram[0xCB] = code;
        if code != 64 && code != self.kbd_prev {
            let n = self.bus.ram[0xC6] as usize;
            if n < 10 {
                self.bus.ram[0x277 + n] = TABLE[code as usize];
                self.bus.ram[0xC6] = (n + 1) as u8;
            }
        }
        self.kbd_prev = code;
        self.bus.ram[0xC5] = code;
    }
}
