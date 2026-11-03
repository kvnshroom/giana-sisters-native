//! Cycle exact NMOS 6502 core (documented opcodes plus the usual undocumented NOPs, decimal mode, dummy reads and
//! read-modify-write double writes). Every bus access is one clock cycle: the [`Bus`] counts them and may stall the CPU
//! (VIC bad lines), so the timing of a write inside an instruction is right.

pub trait Bus {
    /// One cycle with a read.
    fn read(&mut self, a: u16) -> u8;
    /// One cycle with a write.
    fn write(&mut self, a: u16, v: u8);
    /// One cycle without a useful bus access (internal operation).
    fn idle(&mut self);
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub c: bool,
    pub z: bool,
    pub i: bool,
    pub d: bool,
    pub v: bool,
    pub n: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Am {
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Inx,
    Iny,
}

impl Cpu {
    pub fn new() -> Self {
        Cpu { sp: 0xFD, i: true, ..Default::default() }
    }

    pub fn flags(&self, brk: bool) -> u8 {
        (self.n as u8) << 7
            | (self.v as u8) << 6
            | 0x20
            | (brk as u8) << 4
            | (self.d as u8) << 3
            | (self.i as u8) << 2
            | (self.z as u8) << 1
            | self.c as u8
    }

    pub fn set_flags(&mut self, v: u8) {
        self.n = v & 0x80 != 0;
        self.v = v & 0x40 != 0;
        self.d = v & 0x08 != 0;
        self.i = v & 0x04 != 0;
        self.z = v & 0x02 != 0;
        self.c = v & 0x01 != 0;
    }

    fn push<B: Bus>(&mut self, b: &mut B, v: u8) {
        b.write(0x100 + self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull<B: Bus>(&mut self, b: &mut B) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        b.read(0x100 + self.sp as u16)
    }

    fn fetch<B: Bus>(&mut self, b: &mut B) -> u8 {
        let v = b.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn nz(&mut self, v: u8) -> u8 {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
        v
    }

    /// Hardware interrupt entry (IRQ or NMI): 7 cycles.
    pub fn interrupt<B: Bus>(&mut self, b: &mut B, vector: u16, brk: bool) {
        self.interrupt_push(b, brk);
        let lo = b.read(vector) as u16;
        let hi = b.read(vector.wrapping_add(1)) as u16;
        self.pc = lo | hi << 8;
    }

    /// The first five cycles of an interrupt: pushes PC and the flags and sets I. The caller fetches the vector.
    pub fn interrupt_push<B: Bus>(&mut self, b: &mut B, brk: bool) {
        if !brk {
            b.read(self.pc);
            b.read(self.pc);
        }
        let pc = self.pc;
        self.push(b, (pc >> 8) as u8);
        self.push(b, pc as u8);
        let f = self.flags(brk);
        self.push(b, f);
        self.i = true;
    }

    /// Effective address of the operand; spends the cycles of the addressing mode. `write` is true for stores and
    /// read-modify-write (fixed length, no page crossing shortcut).
    fn ea<B: Bus>(&mut self, b: &mut B, am: Am, write: bool) -> u16 {
        match am {
            Am::Zp => self.fetch(b) as u16,
            Am::Zpx => {
                let z = self.fetch(b);
                b.read(z as u16);
                z.wrapping_add(self.x) as u16
            }
            Am::Zpy => {
                let z = self.fetch(b);
                b.read(z as u16);
                z.wrapping_add(self.y) as u16
            }
            Am::Abs => {
                let lo = self.fetch(b) as u16;
                lo | (self.fetch(b) as u16) << 8
            }
            Am::Abx | Am::Aby => {
                let lo = self.fetch(b) as u16;
                let hi = self.fetch(b) as u16;
                let idx = if am == Am::Abx { self.x } else { self.y } as u16;
                let base = lo | hi << 8;
                let a = base.wrapping_add(idx);
                if write || (base & 0xFF00) != (a & 0xFF00) {
                    b.read((base & 0xFF00) | (a & 0x00FF));
                }
                a
            }
            Am::Inx => {
                let z = self.fetch(b);
                b.read(z as u16);
                let z = z.wrapping_add(self.x);
                let lo = b.read(z as u16) as u16;
                let hi = b.read(z.wrapping_add(1) as u16) as u16;
                lo | hi << 8
            }
            Am::Iny => {
                let z = self.fetch(b);
                let lo = b.read(z as u16) as u16;
                let hi = b.read(z.wrapping_add(1) as u16) as u16;
                let base = lo | hi << 8;
                let a = base.wrapping_add(self.y as u16);
                if write || (base & 0xFF00) != (a & 0xFF00) {
                    b.read((base & 0xFF00) | (a & 0x00FF));
                }
                a
            }
            _ => unreachable!(),
        }
    }

    fn adc(&mut self, m: u8) {
        if self.d {
            let a = self.a as u32;
            let v = m as u32;
            let c = self.c as u32;
            let mut t = (a & 0xF) + (v & 0xF) + c;
            if t > 9 {
                t += 6;
            }
            t = if t <= 0x0F { (t & 0xF) + (a & 0xF0) + (v & 0xF0) } else { (t & 0xF) + (a & 0xF0) + (v & 0xF0) + 0x10 };
            self.z = (a + v + c) & 0xFF == 0;
            self.n = t & 0x80 != 0;
            self.v = ((a ^ t) & 0x80) != 0 && ((a ^ v) & 0x80) == 0;
            if (t & 0x1F0) > 0x90 {
                t += 0x60;
            }
            self.c = (t & 0xFF0) > 0xF0;
            self.a = t as u8;
        } else {
            let s = self.a as u16 + m as u16 + self.c as u16;
            self.v = (!(self.a ^ m) & (self.a ^ s as u8) & 0x80) != 0;
            self.c = s > 0xFF;
            self.a = self.nz(s as u8);
        }
    }

    fn sbc(&mut self, m: u8) {
        if self.d {
            let a = self.a as i32;
            let v = m as i32;
            let borrow = (!self.c) as i32;
            let t = a - v - borrow;
            let mut ta = (a & 0xF) - (v & 0xF) - borrow;
            ta = if ta & 0x10 != 0 { ((ta - 6) & 0xF) | ((a & 0xF0) - (v & 0xF0) - 0x10) } else { (ta & 0xF) | ((a & 0xF0) - (v & 0xF0)) };
            if ta & 0x100 != 0 {
                ta -= 0x60;
            }
            self.c = t >= 0;
            self.nz(t as u8);
            self.v = ((a ^ t) & 0x80) != 0 && ((a ^ v) & 0x80) != 0;
            self.a = ta as u8;
        } else {
            self.adc(!m);
        }
    }

    fn cmp(&mut self, r: u8, m: u8) {
        self.c = r >= m;
        self.nz(r.wrapping_sub(m));
    }

    fn branch<B: Bus>(&mut self, b: &mut B, cond: bool) {
        let o = self.fetch(b) as i8;
        if cond {
            b.read(self.pc);
            let to = self.pc.wrapping_add(o as i16 as u16);
            if (to & 0xFF00) != (self.pc & 0xFF00) {
                b.read((self.pc & 0xFF00) | (to & 0x00FF));
            }
            self.pc = to;
        }
    }

    fn asl(&mut self, v: u8) -> u8 {
        self.c = v & 0x80 != 0;
        self.nz(v << 1)
    }
    fn lsr(&mut self, v: u8) -> u8 {
        self.c = v & 1 != 0;
        self.nz(v >> 1)
    }
    fn rol(&mut self, v: u8) -> u8 {
        let c = self.c as u8;
        self.c = v & 0x80 != 0;
        self.nz(v << 1 | c)
    }
    fn ror(&mut self, v: u8) -> u8 {
        let c = (self.c as u8) << 7;
        self.c = v & 1 != 0;
        self.nz(v >> 1 | c)
    }

    /// Executes one instruction. Returns an error for an undefined opcode.
    pub fn step<B: Bus>(&mut self, b: &mut B) -> Result<(), String> {
        let at = self.pc;
        let op = self.fetch(b);
        use Am::*;
        // (addressing mode, group handler)
        match op {
            // ---- loads / stores
            0xA9 => self.ld(b, Imm, 'a'),
            0xA5 => self.ld(b, Zp, 'a'),
            0xB5 => self.ld(b, Zpx, 'a'),
            0xAD => self.ld(b, Abs, 'a'),
            0xBD => self.ld(b, Abx, 'a'),
            0xB9 => self.ld(b, Aby, 'a'),
            0xA1 => self.ld(b, Inx, 'a'),
            0xB1 => self.ld(b, Iny, 'a'),
            0xA2 => self.ld(b, Imm, 'x'),
            0xA6 => self.ld(b, Zp, 'x'),
            0xB6 => self.ld(b, Zpy, 'x'),
            0xAE => self.ld(b, Abs, 'x'),
            0xBE => self.ld(b, Aby, 'x'),
            0xA0 => self.ld(b, Imm, 'y'),
            0xA4 => self.ld(b, Zp, 'y'),
            0xB4 => self.ld(b, Zpx, 'y'),
            0xAC => self.ld(b, Abs, 'y'),
            0xBC => self.ld(b, Abx, 'y'),
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => {
                let am = match op {
                    0x85 => Zp,
                    0x95 => Zpx,
                    0x8D => Abs,
                    0x9D => Abx,
                    0x99 => Aby,
                    0x81 => Inx,
                    _ => Iny,
                };
                let a = self.ea(b, am, true);
                b.write(a, self.a);
            }
            0x86 | 0x96 | 0x8E => {
                let am = match op {
                    0x86 => Zp,
                    0x96 => Zpy,
                    _ => Abs,
                };
                let a = self.ea(b, am, true);
                b.write(a, self.x);
            }
            0x84 | 0x94 | 0x8C => {
                let am = match op {
                    0x84 => Zp,
                    0x94 => Zpx,
                    _ => Abs,
                };
                let a = self.ea(b, am, true);
                b.write(a, self.y);
            }
            // ---- ALU on A
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 | 0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31
            | 0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 | 0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71
            | 0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 | 0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                let am = match op & 0x1C {
                    0x08 => Imm,
                    0x04 => Zp,
                    0x14 => Zpx,
                    0x0C => Abs,
                    0x1C => Abx,
                    0x18 => Aby,
                    0x00 => Inx,
                    _ => Iny,
                };
                let m = if am == Imm { self.fetch(b) } else { let a = self.ea(b, am, false); b.read(a) };
                match op & 0xE0 {
                    0x00 => {
                        let v = self.a | m;
                        self.a = self.nz(v);
                    }
                    0x20 => {
                        let v = self.a & m;
                        self.a = self.nz(v);
                    }
                    0x40 => {
                        let v = self.a ^ m;
                        self.a = self.nz(v);
                    }
                    0x60 => self.adc(m),
                    0xC0 => self.cmp(self.a, m),
                    _ => self.sbc(m),
                }
            }
            // ---- CPX / CPY / BIT
            0xE0 | 0xE4 | 0xEC | 0xC0 | 0xC4 | 0xCC => {
                let am = match op & 0x0C {
                    0x00 => Imm,
                    0x04 => Zp,
                    _ => Abs,
                };
                let m = if am == Imm { self.fetch(b) } else { let a = self.ea(b, am, false); b.read(a) };
                let r = if op >= 0xE0 { self.x } else { self.y };
                self.cmp(r, m);
            }
            0x24 | 0x2C => {
                let a = self.ea(b, if op == 0x24 { Zp } else { Abs }, false);
                let m = b.read(a);
                self.z = self.a & m == 0;
                self.n = m & 0x80 != 0;
                self.v = m & 0x40 != 0;
            }
            // ---- shifts and inc/dec
            0x0A | 0x2A | 0x4A | 0x6A => {
                b.read(self.pc);
                self.a = match op {
                    0x0A => self.asl(self.a),
                    0x2A => self.rol(self.a),
                    0x4A => self.lsr(self.a),
                    _ => self.ror(self.a),
                };
            }
            0x06 | 0x16 | 0x0E | 0x1E | 0x26 | 0x36 | 0x2E | 0x3E | 0x46 | 0x56 | 0x4E | 0x5E | 0x66 | 0x76 | 0x6E | 0x7E | 0xC6
            | 0xD6 | 0xCE | 0xDE | 0xE6 | 0xF6 | 0xEE | 0xFE => {
                let am = match op & 0x1C {
                    0x04 => Zp,
                    0x14 => Zpx,
                    0x0C => Abs,
                    _ => Abx,
                };
                let a = self.ea(b, am, true);
                let m = b.read(a);
                b.write(a, m);
                let r = match op & 0xE0 {
                    0x00 => self.asl(m),
                    0x20 => self.rol(m),
                    0x40 => self.lsr(m),
                    0x60 => self.ror(m),
                    0xC0 => {
                        let v = m.wrapping_sub(1);
                        self.nz(v)
                    }
                    _ => {
                        let v = m.wrapping_add(1);
                        self.nz(v)
                    }
                };
                b.write(a, r);
            }
            // ---- register ops
            0xAA => {
                b.read(self.pc);
                self.x = self.nz(self.a);
            }
            0xA8 => {
                b.read(self.pc);
                self.y = self.nz(self.a);
            }
            0x8A => {
                b.read(self.pc);
                self.a = self.nz(self.x);
            }
            0x98 => {
                b.read(self.pc);
                self.a = self.nz(self.y);
            }
            0xBA => {
                b.read(self.pc);
                self.x = self.nz(self.sp);
            }
            0x9A => {
                b.read(self.pc);
                self.sp = self.x;
            }
            0xE8 => {
                b.read(self.pc);
                let v = self.x.wrapping_add(1);
                self.x = self.nz(v);
            }
            0xCA => {
                b.read(self.pc);
                let v = self.x.wrapping_sub(1);
                self.x = self.nz(v);
            }
            0xC8 => {
                b.read(self.pc);
                let v = self.y.wrapping_add(1);
                self.y = self.nz(v);
            }
            0x88 => {
                b.read(self.pc);
                let v = self.y.wrapping_sub(1);
                self.y = self.nz(v);
            }
            0x18 | 0x38 | 0x58 | 0x78 | 0xB8 | 0xD8 | 0xF8 => {
                b.read(self.pc);
                match op {
                    0x18 => self.c = false,
                    0x38 => self.c = true,
                    0x58 => self.i = false,
                    0x78 => self.i = true,
                    0xB8 => self.v = false,
                    0xD8 => self.d = false,
                    _ => self.d = true,
                }
            }
            0xEA | 0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => {
                b.read(self.pc);
            }
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                self.fetch(b);
            }
            0x04 | 0x44 | 0x64 => {
                let a = self.ea(b, Zp, false);
                b.read(a);
            }
            0x0C => {
                let a = self.ea(b, Abs, false);
                b.read(a);
            }
            // ---- stack
            0x48 => {
                b.read(self.pc);
                let a = self.a;
                self.push(b, a);
            }
            0x08 => {
                b.read(self.pc);
                let f = self.flags(true);
                self.push(b, f);
            }
            0x68 => {
                b.read(self.pc);
                b.idle();
                let v = self.pull(b);
                self.a = self.nz(v);
            }
            0x28 => {
                b.read(self.pc);
                b.idle();
                let v = self.pull(b);
                self.set_flags(v);
            }
            // ---- jumps
            0x4C => {
                let lo = self.fetch(b) as u16;
                let hi = self.fetch(b) as u16;
                self.pc = lo | hi << 8;
            }
            0x6C => {
                let lo = self.fetch(b) as u16;
                let hi = self.fetch(b) as u16;
                let p = lo | hi << 8;
                let l = b.read(p) as u16;
                let h = b.read((p & 0xFF00) | (p.wrapping_add(1) & 0x00FF)) as u16;
                self.pc = l | h << 8;
            }
            0x20 => {
                let lo = self.fetch(b) as u16;
                b.idle();
                let pc = self.pc;
                self.push(b, (pc >> 8) as u8);
                self.push(b, pc as u8);
                let hi = self.fetch(b) as u16;
                self.pc = lo | hi << 8;
            }
            0x60 => {
                b.read(self.pc);
                b.idle();
                let lo = self.pull(b) as u16;
                let hi = self.pull(b) as u16;
                self.pc = (lo | hi << 8).wrapping_add(1);
                b.idle();
            }
            0x40 => {
                b.read(self.pc);
                b.idle();
                let f = self.pull(b);
                self.set_flags(f);
                let lo = self.pull(b) as u16;
                let hi = self.pull(b) as u16;
                self.pc = lo | hi << 8;
            }
            0x00 => {
                self.fetch(b);
                self.interrupt(b, 0xFFFE, true);
            }
            // ---- branches
            0x10 => self.branch(b, !self.n),
            0x30 => self.branch(b, self.n),
            0x50 => self.branch(b, !self.v),
            0x70 => self.branch(b, self.v),
            0x90 => self.branch(b, !self.c),
            0xB0 => self.branch(b, self.c),
            0xD0 => self.branch(b, !self.z),
            0xF0 => self.branch(b, self.z),
            _ => return Err(format!("undefined opcode {op:02X} at ${at:04X}")),
        }
        Ok(())
    }

    fn ld<B: Bus>(&mut self, b: &mut B, am: Am, r: char) {
        let m = if am == Am::Imm { self.fetch(b) } else { let a = self.ea(b, am, false); b.read(a) };
        let v = self.nz(m);
        match r {
            'a' => self.a = v,
            'x' => self.x = v,
            _ => self.y = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram {
        m: Vec<u8>,
        cycles: u64,
    }
    impl Bus for Ram {
        fn read(&mut self, a: u16) -> u8 {
            self.cycles += 1;
            self.m[a as usize]
        }
        fn write(&mut self, a: u16, v: u8) {
            self.cycles += 1;
            self.m[a as usize] = v;
        }
        fn idle(&mut self) {
            self.cycles += 1;
        }
    }

    fn run(code: &[u8], steps: usize) -> (Cpu, Ram) {
        let mut r = Ram { m: vec![0; 65536], cycles: 0 };
        r.m[0x200..0x200 + code.len()].copy_from_slice(code);
        let mut c = Cpu::new();
        c.pc = 0x200;
        for _ in 0..steps {
            c.step(&mut r).unwrap();
        }
        (c, r)
    }

    #[test]
    fn cycle_counts() {
        // lda #1 (2) sta $d021 (4) ldx #10 (2) dex (2) bpl taken (3)
        let (_, r) = run(&[0xA9, 1, 0x8D, 0x21, 0xD0, 0xA2, 10, 0xCA, 0x10, 0xFD], 5);
        assert_eq!(r.cycles, 2 + 4 + 2 + 2 + 3);
        // lda abs,x with page cross: 5; without: 4
        let (_, r) = run(&[0xA2, 0xFF, 0xBD, 0x80, 0x10], 2);
        assert_eq!(r.cycles, 2 + 5);
        let (_, r) = run(&[0xA2, 0x01, 0xBD, 0x80, 0x10], 2);
        assert_eq!(r.cycles, 2 + 4);
        // inc abs: 6, jsr 6, rts 6
        let (_, r) = run(&[0xEE, 0x00, 0x30], 1);
        assert_eq!(r.cycles, 6);
        let (_, r) = run(&[0x20, 0x10, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x60], 2);
        assert_eq!(r.cycles, 12);
    }

    #[test]
    fn decimal_mode() {
        // sed clc lda #$19 adc #$28 -> $47 ; sec lda #$47 sbc #$28 -> $19
        let (c, _) = run(&[0xF8, 0x18, 0xA9, 0x19, 0x69, 0x28], 4);
        assert_eq!(c.a, 0x47);
        let (c, _) = run(&[0xF8, 0x38, 0xA9, 0x47, 0xE9, 0x28], 4);
        assert_eq!(c.a, 0x19);
        let (c, _) = run(&[0xF8, 0x18, 0xA9, 0x99, 0x69, 0x01], 4);
        assert_eq!((c.a, c.c), (0x00, true));
    }
}
