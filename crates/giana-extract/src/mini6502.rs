//! Minimal NMOS 6502 interpreter (documented opcodes, no decimal mode, no interrupts, no I/O).
//!
//! Only used to run the unpacker that sits at the start of the title program (disk file `2`) so that the title picture
//! can be taken out of the crunched file. It is not an emulator of the C64.

pub struct Cpu {
    pub m: Vec<u8>,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    c: bool,
    z: bool,
    i: bool,
    d: bool,
    v: bool,
    n: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Inx,
    Iny,
    Acc,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Cpu { m: vec![0; 65536], a: 0, x: 0, y: 0, sp: 0xFF, pc: 0, c: false, z: false, i: false, d: false, v: false, n: false }
    }

    fn rd(&self, a: u16) -> u8 {
        self.m[a as usize]
    }
    fn wr(&mut self, a: u16, v: u8) {
        self.m[a as usize] = v;
    }
    fn rd16(&self, a: u16) -> u16 {
        // keeps the 6502 page-wrap bug of JMP ($xxFF)
        self.rd(a) as u16 | (self.rd((a & 0xFF00) | (a.wrapping_add(1) & 0xFF)) as u16) << 8
    }
    fn push(&mut self, v: u8) {
        self.m[0x100 + self.sp as usize] = v;
        self.sp = self.sp.wrapping_sub(1);
    }
    fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.m[0x100 + self.sp as usize]
    }
    fn flags(&self, brk: bool) -> u8 {
        (self.n as u8) << 7
            | (self.v as u8) << 6
            | 0x20
            | (brk as u8) << 4
            | (self.d as u8) << 3
            | (self.i as u8) << 2
            | (self.z as u8) << 1
            | self.c as u8
    }
    fn set_flags(&mut self, v: u8) {
        self.n = v & 0x80 != 0;
        self.v = v & 0x40 != 0;
        self.d = v & 0x08 != 0;
        self.i = v & 0x04 != 0;
        self.z = v & 0x02 != 0;
        self.c = v & 0x01 != 0;
    }
    fn nz(&mut self, v: u8) -> u8 {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
        v
    }
    fn adc(&mut self, m: u8) {
        let s = self.a as u16 + m as u16 + self.c as u16;
        self.v = (!(self.a ^ m) & (self.a ^ s as u8) & 0x80) != 0;
        self.c = s > 0xFF;
        self.a = self.nz(s as u8);
    }
    fn cmp(&mut self, r: u8, m: u8) {
        self.c = r >= m;
        self.nz(r.wrapping_sub(m));
    }
    fn fetch(&mut self) -> u8 {
        let v = self.m[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        v
    }
    fn fetch16(&mut self) -> u16 {
        let lo = self.fetch() as u16;
        lo | (self.fetch() as u16) << 8
    }
    fn branch(&mut self, cond: bool) {
        let o = self.fetch() as i8;
        if cond {
            self.pc = self.pc.wrapping_add(o as i16 as u16);
        }
    }
    fn ea(&mut self, mode: Mode) -> Result<u16, String> {
        Ok(match mode {
            Mode::Zp => self.fetch() as u16,
            Mode::Zpx => self.fetch().wrapping_add(self.x) as u16,
            Mode::Zpy => self.fetch().wrapping_add(self.y) as u16,
            Mode::Abs => self.fetch16(),
            Mode::Abx => self.fetch16().wrapping_add(self.x as u16),
            Mode::Aby => self.fetch16().wrapping_add(self.y as u16),
            Mode::Inx => {
                let z = self.fetch().wrapping_add(self.x);
                self.m[z as usize] as u16 | (self.m[z.wrapping_add(1) as usize] as u16) << 8
            }
            Mode::Iny => {
                let z = self.fetch();
                (self.m[z as usize] as u16 | (self.m[z.wrapping_add(1) as usize] as u16) << 8).wrapping_add(self.y as u16)
            }
            Mode::Imm | Mode::Acc => return Err(format!("bad addressing mode at {:04X}", self.pc)),
        })
    }
    fn operand(&mut self, mode: Mode) -> Result<u8, String> {
        if mode == Mode::Imm {
            Ok(self.fetch())
        } else {
            let a = self.ea(mode)?;
            Ok(self.rd(a))
        }
    }

    pub fn step(&mut self) -> Result<(), String> {
        let op = self.fetch();
        let (hi, lo) = (op >> 4, op & 15);
        match op {
            0x00 => {
                self.fetch();
                self.push((self.pc >> 8) as u8);
                self.push(self.pc as u8);
                let p = self.flags(true);
                self.push(p);
                self.i = true;
                self.pc = self.rd16(0xFFFE);
                return Ok(());
            }
            0x20 => {
                let a = self.fetch16();
                let r = self.pc.wrapping_sub(1);
                self.push((r >> 8) as u8);
                self.push(r as u8);
                self.pc = a;
                return Ok(());
            }
            0x40 => {
                let p = self.pull();
                self.set_flags(p);
                self.pc = self.pull() as u16 | (self.pull() as u16) << 8;
                return Ok(());
            }
            0x60 => {
                self.pc = (self.pull() as u16 | (self.pull() as u16) << 8).wrapping_add(1);
                return Ok(());
            }
            0x4C => {
                self.pc = self.fetch16();
                return Ok(());
            }
            0x6C => {
                let a = self.fetch16();
                self.pc = self.rd16(a);
                return Ok(());
            }
            _ => {}
        }
        if lo == 0 && hi & 1 == 1 {
            let cond = match hi >> 1 {
                0 => !self.n,
                1 => self.n,
                2 => !self.v,
                3 => self.v,
                4 => !self.c,
                5 => self.c,
                6 => !self.z,
                _ => self.z,
            };
            self.branch(cond);
            return Ok(());
        }
        match op {
            0x08 => {
                let p = self.flags(true);
                self.push(p)
            }
            0x28 => {
                let p = self.pull();
                self.set_flags(p)
            }
            0x48 => self.push(self.a),
            0x68 => {
                let v = self.pull();
                self.a = self.nz(v)
            }
            0x88 => self.y = self.nz(self.y.wrapping_sub(1)),
            0xC8 => self.y = self.nz(self.y.wrapping_add(1)),
            0xCA => self.x = self.nz(self.x.wrapping_sub(1)),
            0xE8 => self.x = self.nz(self.x.wrapping_add(1)),
            0x18 => self.c = false,
            0x38 => self.c = true,
            0x58 => self.i = false,
            0x78 => self.i = true,
            0xB8 => self.v = false,
            0xD8 => self.d = false,
            0xF8 => self.d = true,
            0x8A => self.a = self.nz(self.x),
            0x98 => self.a = self.nz(self.y),
            0xAA => self.x = self.nz(self.a),
            0xA8 => self.y = self.nz(self.a),
            0xBA => self.x = self.nz(self.sp),
            0x9A => self.sp = self.x,
            0xEA => {}
            _ => return self.step_group(op),
        }
        Ok(())
    }

    fn step_group(&mut self, op: u8) -> Result<(), String> {
        let grp = op >> 5;
        let col = (op >> 2) & 7;
        match op & 3 {
            1 => {
                let mode = [Mode::Inx, Mode::Zp, Mode::Imm, Mode::Abs, Mode::Iny, Mode::Zpx, Mode::Aby, Mode::Abx][col as usize];
                if grp == 4 {
                    let a = self.ea(mode)?;
                    self.wr(a, self.a);
                    return Ok(());
                }
                let m = self.operand(mode)?;
                match grp {
                    0 => self.a = self.nz(self.a | m),
                    1 => self.a = self.nz(self.a & m),
                    2 => self.a = self.nz(self.a ^ m),
                    3 => self.adc(m),
                    5 => self.a = self.nz(m),
                    6 => self.cmp(self.a, m),
                    _ => self.adc(m ^ 0xFF),
                }
            }
            2 => {
                let mut mode = match col {
                    0 => Mode::Imm,
                    1 => Mode::Zp,
                    2 => Mode::Acc,
                    3 => Mode::Abs,
                    5 => Mode::Zpx,
                    7 => Mode::Abx,
                    _ => return Err(format!("unsupported opcode {op:02X}")),
                };
                if grp == 4 || grp == 5 {
                    mode = match mode {
                        Mode::Zpx => Mode::Zpy,
                        Mode::Abx => Mode::Aby,
                        m => m,
                    };
                    if grp == 4 {
                        let a = self.ea(mode)?;
                        self.wr(a, self.x);
                    } else {
                        let m = self.operand(mode)?;
                        self.x = self.nz(m);
                    }
                    return Ok(());
                }
                if mode == Mode::Imm {
                    return Err(format!("bad opcode {op:02X}"));
                }
                let (mut v, addr) = if mode == Mode::Acc {
                    (self.a, None)
                } else {
                    let a = self.ea(mode)?;
                    (self.rd(a), Some(a))
                };
                match grp {
                    0 => {
                        self.c = v & 0x80 != 0;
                        v = self.nz(v << 1)
                    }
                    1 => {
                        let c = self.c as u8;
                        self.c = v & 0x80 != 0;
                        v = self.nz(v << 1 | c)
                    }
                    2 => {
                        self.c = v & 1 != 0;
                        v = self.nz(v >> 1)
                    }
                    3 => {
                        let c = self.c as u8;
                        self.c = v & 1 != 0;
                        v = self.nz(v >> 1 | c << 7)
                    }
                    6 => v = self.nz(v.wrapping_sub(1)),
                    _ => v = self.nz(v.wrapping_add(1)),
                }
                match addr {
                    None => self.a = v,
                    Some(a) => self.wr(a, v),
                }
            }
            0 => {
                let mode = match col {
                    0 => Mode::Imm,
                    1 => Mode::Zp,
                    3 => Mode::Abs,
                    5 => Mode::Zpx,
                    7 => Mode::Abx,
                    _ => return Err(format!("unsupported opcode {op:02X}")),
                };
                match grp {
                    1 if mode == Mode::Zp || mode == Mode::Abs => {
                        let m = self.operand(mode)?;
                        self.z = self.a & m == 0;
                        self.n = m & 0x80 != 0;
                        self.v = m & 0x40 != 0;
                    }
                    4 => {
                        let a = self.ea(mode)?;
                        self.wr(a, self.y);
                    }
                    5 => {
                        let m = self.operand(mode)?;
                        self.y = self.nz(m);
                    }
                    6 | 7 => {
                        let m = self.operand(mode)?;
                        self.cmp(if grp == 6 { self.y } else { self.x }, m);
                    }
                    _ => return Err(format!("unsupported opcode {op:02X} at {:04X}", self.pc.wrapping_sub(1))),
                }
            }
            _ => return Err(format!("unsupported opcode {op:02X} at {:04X}", self.pc.wrapping_sub(1))),
        }
        Ok(())
    }

    /// Runs until `pc == stop`, or fails after `limit` instructions.
    pub fn run_until(&mut self, stop: u16, limit: u64) -> Result<(), String> {
        for _ in 0..limit {
            if self.pc == stop {
                return Ok(());
            }
            self.step()?;
        }
        Err(format!("instruction limit reached at {:04X}", self.pc))
    }
}
