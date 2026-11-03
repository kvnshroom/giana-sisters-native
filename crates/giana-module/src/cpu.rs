//! A 6502 core for the high level boot emulation: all documented opcodes, decimal mode, the undocumented opcodes
//! that crunchers and depackers use (LAX, SAX, DCP, ISC, SLO, SRE, RLA, RRA, ANC, ALR, ARR, AXS, NOPs, ...) and a
//! rough cycle count. Memory goes through the [`Bus`] trait so the caller decides what I/O and banking do.
//! (`giana-extract::mini6502` stays as it is; it only runs the title unpacker.)

pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, v: u8);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Imp,
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ix,
    Iy,
    Ind,
    Rel,
}
use Mode::*;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Op {
    Ora, And, Eor, Adc, Sta, Lda, Cmp, Sbc,
    Asl, Rol, Lsr, Ror, Stx, Ldx, Dec, Inc,
    Bit, Jmp, Sty, Ldy, Cpy, Cpx,
    Brk, Jsr, Rti, Rts, Php, Plp, Pha, Pla, Dey, Tay, Iny, Inx,
    Clc, Sec, Cli, Sei, Tya, Clv, Cld, Sed,
    Txa, Txs, Tax, Tsx, Dex, Nop,
    Branch,
    // undocumented
    Slo, Rla, Sre, Rra, Sax, Lax, Dcp, Isc,
    Anc, Alr, Arr, Xaa, Lxa, Axs, Las, Sha, Shx, Shy, Tas, Kil,
}
use Op::*;

fn decode(op: u8) -> (Op, Mode) {
    let aaa = (op >> 5) as usize;
    let bbb = (op >> 2) & 7;
    match op & 3 {
        1 => {
            let m = [Ix, Zp, Imm, Abs, Iy, Zpx, Aby, Abx][bbb as usize];
            if op == 0x89 {
                return (Nop, Imm);
            }
            ([Ora, And, Eor, Adc, Sta, Lda, Cmp, Sbc][aaa], m)
        }
        2 => match op {
            0x8A => (Txa, Imp),
            0x9A => (Txs, Imp),
            0xAA => (Tax, Imp),
            0xBA => (Tsx, Imp),
            0xCA => (Dex, Imp),
            0xEA | 0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => (Nop, Imp),
            0x82 | 0xC2 | 0xE2 => (Nop, Imm),
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => (Kil, Imp),
            0x9E => (Shx, Aby),
            0x96 => (Stx, Zpy),
            0xB6 => (Ldx, Zpy),
            0xBE => (Ldx, Aby),
            0xA2 => (Ldx, Imm),
            _ => {
                let m = [Imm, Zp, Acc, Abs, Imp, Zpx, Imp, Abx][bbb as usize];
                ([Asl, Rol, Lsr, Ror, Stx, Ldx, Dec, Inc][aaa], m)
            }
        },
        0 => match bbb {
            0 => match aaa {
                0 => (Brk, Imp),
                1 => (Jsr, Abs),
                2 => (Rti, Imp),
                3 => (Rts, Imp),
                4 => (Nop, Imm),
                5 => (Ldy, Imm),
                6 => (Cpy, Imm),
                _ => (Cpx, Imm),
            },
            1 => match aaa {
                1 => (Bit, Zp),
                4 => (Sty, Zp),
                5 => (Ldy, Zp),
                6 => (Cpy, Zp),
                7 => (Cpx, Zp),
                _ => (Nop, Zp),
            },
            2 => ([Php, Plp, Pha, Pla, Dey, Tay, Iny, Inx][aaa], Imp),
            3 => match aaa {
                0 => (Nop, Abs),
                1 => (Bit, Abs),
                2 => (Jmp, Abs),
                3 => (Jmp, Ind),
                4 => (Sty, Abs),
                5 => (Ldy, Abs),
                6 => (Cpy, Abs),
                _ => (Cpx, Abs),
            },
            4 => (Branch, Rel),
            5 => match aaa {
                4 => (Sty, Zpx),
                5 => (Ldy, Zpx),
                _ => (Nop, Zpx),
            },
            6 => ([Clc, Sec, Cli, Sei, Tya, Clv, Cld, Sed][aaa], Imp),
            _ => match aaa {
                4 => (Shy, Abx),
                5 => (Ldy, Abx),
                _ => (Nop, Abx),
            },
        },
        _ => match op {
            0x0B | 0x2B => (Anc, Imm),
            0x4B => (Alr, Imm),
            0x6B => (Arr, Imm),
            0x8B => (Xaa, Imm),
            0xAB => (Lxa, Imm),
            0xCB => (Axs, Imm),
            0xEB => (Sbc, Imm),
            0x93 => (Sha, Iy),
            0x9F => (Sha, Aby),
            0x9B => (Tas, Aby),
            0xBB => (Las, Aby),
            0x97 => (Sax, Zpy),
            0xB7 => (Lax, Zpy),
            0xBF => (Lax, Aby),
            _ => {
                let m = [Ix, Zp, Imm, Abs, Iy, Zpx, Aby, Abx][bbb as usize];
                ([Slo, Rla, Sre, Rra, Sax, Lax, Dcp, Isc][aaa], m)
            }
        },
    }
}

#[derive(Clone, Debug)]
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
    pub cycles: u64,
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu { a: 0, x: 0, y: 0, sp: 0xFF, pc: 0, c: false, z: false, i: true, d: false, v: false, n: false, cycles: 0 }
    }
}

impl Cpu {
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
    pub fn push<B: Bus>(&mut self, b: &mut B, v: u8) {
        b.write(0x100 + self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }
    pub fn pull<B: Bus>(&mut self, b: &mut B) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        b.read(0x100 + self.sp as u16)
    }
    /// Does what `RTS` does (used by the KERNAL traps).
    pub fn rts<B: Bus>(&mut self, b: &mut B) {
        let lo = self.pull(b) as u16;
        let hi = self.pull(b) as u16;
        self.pc = (hi << 8 | lo).wrapping_add(1);
    }
    fn nz(&mut self, v: u8) -> u8 {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
        v
    }
    fn fetch<B: Bus>(&mut self, b: &mut B) -> u8 {
        let v = b.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }
    fn fetch16<B: Bus>(&mut self, b: &mut B) -> u16 {
        let lo = self.fetch(b) as u16;
        lo | (self.fetch(b) as u16) << 8
    }
    fn adc(&mut self, m: u8) {
        if self.d {
            let mut lo = (self.a & 15) as u16 + (m & 15) as u16 + self.c as u16;
            let bin = self.a as u16 + m as u16 + self.c as u16;
            self.z = bin as u8 == 0;
            if lo > 9 {
                lo += 6;
            }
            let mut hi = (self.a >> 4) as u16 + (m >> 4) as u16 + (lo > 15) as u16;
            self.n = hi & 8 != 0;
            self.v = (!(self.a ^ m) & (self.a ^ (hi << 4) as u8) & 0x80) != 0;
            if hi > 9 {
                hi += 6;
            }
            self.c = hi > 15;
            self.a = ((hi << 4) | (lo & 15)) as u8;
        } else {
            let s = self.a as u16 + m as u16 + self.c as u16;
            self.v = (!(self.a ^ m) & (self.a ^ s as u8) & 0x80) != 0;
            self.c = s > 0xFF;
            self.a = self.nz(s as u8);
        }
    }
    fn sbc(&mut self, m: u8) {
        if self.d {
            let bin = self.a as i16 - m as i16 - (!self.c) as i16;
            let mut lo = (self.a & 15) as i16 - (m & 15) as i16 - (!self.c) as i16;
            let mut hi = (self.a >> 4) as i16 - (m >> 4) as i16;
            if lo < 0 {
                lo -= 6;
                hi -= 1;
            }
            if hi < 0 {
                hi -= 6;
            }
            let r = bin as u8;
            self.v = ((self.a ^ m) & (self.a ^ r) & 0x80) != 0;
            self.c = bin >= 0;
            self.nz(r);
            self.a = ((hi << 4) | (lo & 15)) as u8;
        } else {
            self.adc(m ^ 0xFF);
        }
    }
    fn cmp(&mut self, r: u8, m: u8) {
        self.c = r >= m;
        self.nz(r.wrapping_sub(m));
    }

    /// Executes one instruction. Errors on KIL opcodes.
    pub fn step<B: Bus>(&mut self, b: &mut B) -> Result<(), String> {
        let at = self.pc;
        let opc = self.fetch(b);
        let (op, mode) = decode(opc);
        let mut cyc: u64 = match mode {
            Imp | Acc | Imm | Rel => 2,
            Zp => 3,
            Zpx | Zpy | Abs | Abx | Aby => 4,
            Ix => 6,
            Iy => 5,
            Ind => 5,
        };
        // effective address (None for implied/accumulator/immediate)
        let ea: Option<u16> = match mode {
            Imp | Acc | Rel => None,
            Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                Some(a)
            }
            Zp => Some(self.fetch(b) as u16),
            Zpx => Some(self.fetch(b).wrapping_add(self.x) as u16),
            Zpy => Some(self.fetch(b).wrapping_add(self.y) as u16),
            Abs => Some(self.fetch16(b)),
            Abx => Some(self.fetch16(b).wrapping_add(self.x as u16)),
            Aby => Some(self.fetch16(b).wrapping_add(self.y as u16)),
            Ix => {
                let z = self.fetch(b).wrapping_add(self.x);
                let lo = b.read(z as u16) as u16;
                Some(lo | (b.read(z.wrapping_add(1) as u16) as u16) << 8)
            }
            Iy => {
                let z = self.fetch(b);
                let lo = b.read(z as u16) as u16;
                let base = lo | (b.read(z.wrapping_add(1) as u16) as u16) << 8;
                Some(base.wrapping_add(self.y as u16))
            }
            Ind => {
                let p = self.fetch16(b);
                let lo = b.read(p) as u16;
                // JMP ($xxFF) wraps inside the page
                let hi = b.read((p & 0xFF00) | (p.wrapping_add(1) & 0xFF)) as u16;
                Some(lo | hi << 8)
            }
        };
        macro_rules! rd {
            () => {
                b.read(ea.unwrap())
            };
        }
        // read-modify-write helper: applies `f` to the operand (memory or accumulator), returns the new value
        macro_rules! rmw {
            ($f:expr) => {{
                cyc += 2;
                if mode == Acc {
                    let v = self.a;
                    let r = $f(self, v);
                    self.a = r;
                    r
                } else {
                    let ad = ea.unwrap();
                    let v = b.read(ad);
                    b.write(ad, v); // dummy write like the real chip
                    let r = $f(self, v);
                    b.write(ad, r);
                    r
                }
            }};
        }
        let asl = |s: &mut Cpu, v: u8| {
            s.c = v & 0x80 != 0;
            s.nz(v << 1)
        };
        let rol = |s: &mut Cpu, v: u8| {
            let c = s.c as u8;
            s.c = v & 0x80 != 0;
            s.nz(v << 1 | c)
        };
        let lsr = |s: &mut Cpu, v: u8| {
            s.c = v & 1 != 0;
            s.nz(v >> 1)
        };
        let ror = |s: &mut Cpu, v: u8| {
            let c = s.c as u8;
            s.c = v & 1 != 0;
            s.nz(v >> 1 | c << 7)
        };
        match op {
            Ora => {
                let m = rd!();
                self.a = self.nz(self.a | m)
            }
            And => {
                let m = rd!();
                self.a = self.nz(self.a & m)
            }
            Eor => {
                let m = rd!();
                self.a = self.nz(self.a ^ m)
            }
            Adc => {
                let m = rd!();
                self.adc(m)
            }
            Sbc => {
                let m = rd!();
                self.sbc(m)
            }
            Cmp => {
                let m = rd!();
                self.cmp(self.a, m)
            }
            Cpx => {
                let m = rd!();
                self.cmp(self.x, m)
            }
            Cpy => {
                let m = rd!();
                self.cmp(self.y, m)
            }
            Lda => {
                let m = rd!();
                self.a = self.nz(m)
            }
            Ldx => {
                let m = rd!();
                self.x = self.nz(m)
            }
            Ldy => {
                let m = rd!();
                self.y = self.nz(m)
            }
            Sta => b.write(ea.unwrap(), self.a),
            Stx => b.write(ea.unwrap(), self.x),
            Sty => b.write(ea.unwrap(), self.y),
            Bit => {
                let m = rd!();
                self.z = self.a & m == 0;
                self.n = m & 0x80 != 0;
                self.v = m & 0x40 != 0;
            }
            Asl => {
                rmw!(asl);
            }
            Rol => {
                rmw!(rol);
            }
            Lsr => {
                rmw!(lsr);
            }
            Ror => {
                rmw!(ror);
            }
            Dec => {
                rmw!(|s: &mut Cpu, v: u8| s.nz(v.wrapping_sub(1)));
            }
            Inc => {
                rmw!(|s: &mut Cpu, v: u8| s.nz(v.wrapping_add(1)));
            }
            Jmp => {
                self.pc = ea.unwrap();
                cyc = if mode == Ind { 5 } else { 3 };
            }
            Jsr => {
                let a = ea.unwrap();
                let r = self.pc.wrapping_sub(1);
                self.push(b, (r >> 8) as u8);
                self.push(b, r as u8);
                self.pc = a;
                cyc = 6;
            }
            Rts => {
                self.rts(b);
                cyc = 6;
            }
            Rti => {
                let p = self.pull(b);
                self.set_flags(p);
                let lo = self.pull(b) as u16;
                self.pc = lo | (self.pull(b) as u16) << 8;
                cyc = 6;
            }
            Brk => {
                self.fetch(b);
                self.push(b, (self.pc >> 8) as u8);
                self.push(b, self.pc as u8);
                let p = self.flags(true);
                self.push(b, p);
                self.i = true;
                let lo = b.read(0xFFFE) as u16;
                self.pc = lo | (b.read(0xFFFF) as u16) << 8;
                cyc = 7;
            }
            Php => {
                let p = self.flags(true);
                self.push(b, p);
                cyc = 3;
            }
            Plp => {
                let p = self.pull(b);
                self.set_flags(p);
                cyc = 4;
            }
            Pha => {
                self.push(b, self.a);
                cyc = 3;
            }
            Pla => {
                let v = self.pull(b);
                self.a = self.nz(v);
                cyc = 4;
            }
            Dey => self.y = self.nz(self.y.wrapping_sub(1)),
            Iny => self.y = self.nz(self.y.wrapping_add(1)),
            Dex => self.x = self.nz(self.x.wrapping_sub(1)),
            Inx => self.x = self.nz(self.x.wrapping_add(1)),
            Tay => self.y = self.nz(self.a),
            Tax => self.x = self.nz(self.a),
            Tya => self.a = self.nz(self.y),
            Txa => self.a = self.nz(self.x),
            Tsx => self.x = self.nz(self.sp),
            Txs => self.sp = self.x,
            Clc => self.c = false,
            Sec => self.c = true,
            Cli => self.i = false,
            Sei => self.i = true,
            Clv => self.v = false,
            Cld => self.d = false,
            Sed => self.d = true,
            Nop => {
                if ea.is_some() && mode != Imm {
                    let _ = rd!();
                }
            }
            Branch => {
                let o = self.fetch(b) as i8;
                let cond = match opc >> 6 {
                    0 => self.n,
                    1 => self.v,
                    2 => self.c,
                    _ => self.z,
                } == (opc & 0x20 != 0);
                if cond {
                    let dst = self.pc.wrapping_add(o as i16 as u16);
                    cyc += 1 + ((dst ^ self.pc) & 0xFF00 != 0) as u64;
                    self.pc = dst;
                }
            }
            // undocumented
            Slo => {
                let r = rmw!(asl);
                self.a = self.nz(self.a | r);
            }
            Rla => {
                let r = rmw!(rol);
                self.a = self.nz(self.a & r);
            }
            Sre => {
                let r = rmw!(lsr);
                self.a = self.nz(self.a ^ r);
            }
            Rra => {
                let r = rmw!(ror);
                self.adc(r);
            }
            Dcp => {
                let r = rmw!(|_s: &mut Cpu, v: u8| v.wrapping_sub(1));
                self.cmp(self.a, r);
            }
            Isc => {
                let r = rmw!(|_s: &mut Cpu, v: u8| v.wrapping_add(1));
                self.sbc(r);
            }
            Sax => b.write(ea.unwrap(), self.a & self.x),
            Lax => {
                let m = rd!();
                self.a = m;
                self.x = self.nz(m);
            }
            Lxa => {
                let m = rd!();
                self.a = m;
                self.x = self.nz(m);
            }
            Xaa => {
                let m = rd!();
                self.a = self.nz(self.x & m);
            }
            Anc => {
                let m = rd!();
                self.a = self.nz(self.a & m);
                self.c = self.n;
            }
            Alr => {
                let m = rd!();
                let v = self.a & m;
                self.c = v & 1 != 0;
                self.a = self.nz(v >> 1);
            }
            Arr => {
                let m = rd!();
                let v = self.a & m;
                let r = (v >> 1) | (self.c as u8) << 7;
                self.a = self.nz(r);
                self.c = r & 0x40 != 0;
                self.v = ((r >> 6) ^ (r >> 5)) & 1 != 0;
            }
            Axs => {
                let m = rd!();
                let t = self.a & self.x;
                self.c = t >= m;
                self.x = self.nz(t.wrapping_sub(m));
            }
            Las => {
                let m = rd!() & self.sp;
                self.a = m;
                self.x = m;
                self.sp = m;
                self.nz(m);
            }
            Sha => {
                let a = ea.unwrap();
                b.write(a, self.a & self.x & ((a >> 8) as u8).wrapping_add(1));
            }
            Shx => {
                let a = ea.unwrap();
                b.write(a, self.x & ((a >> 8) as u8).wrapping_add(1));
            }
            Shy => {
                let a = ea.unwrap();
                b.write(a, self.y & ((a >> 8) as u8).wrapping_add(1));
            }
            Tas => {
                self.sp = self.a & self.x;
                let a = ea.unwrap();
                b.write(a, self.sp & ((a >> 8) as u8).wrapping_add(1));
            }
            Kil => return Err(format!("CPU jam (opcode {opc:02X}) at ${at:04X}")),
        }
        self.cycles += cyc;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);
    impl Bus for Ram {
        fn read(&mut self, a: u16) -> u8 {
            self.0[a as usize]
        }
        fn write(&mut self, a: u16, v: u8) {
            self.0[a as usize] = v;
        }
    }

    fn run(code: &[u8], steps: usize, setup: impl FnOnce(&mut Cpu, &mut Ram)) -> (Cpu, Ram) {
        let mut ram = Ram(vec![0; 65536]);
        ram.0[0x1000..0x1000 + code.len()].copy_from_slice(code);
        let mut cpu = Cpu { pc: 0x1000, ..Default::default() };
        setup(&mut cpu, &mut ram);
        for _ in 0..steps {
            cpu.step(&mut ram).unwrap();
        }
        (cpu, ram)
    }

    #[test]
    fn documented_basics() {
        // LDA #$80 ; ASL ; (C=1, A=0) ; ADC #$05 ; STA $20 ; LDX #$FF ; INX ; BEQ +2 ; (skipped LDA #1) ; LDA #2
        let (c, r) = run(&[0xA9, 0x80, 0x0A, 0x69, 0x05, 0x85, 0x20, 0xA2, 0xFF, 0xE8, 0xF0, 0x02, 0xA9, 0x01, 0xA9, 0x02], 9, |_, _| {});
        assert_eq!(c.a, 2);
        assert_eq!(r.0[0x20], 6); // 0 + 5 + carry
        assert!(c.z == false);
    }

    #[test]
    fn jsr_rts_and_stack() {
        let (c, _) = run(&[0x20, 0x10, 0x10, 0xEA], 3, |_, r| {
            r.0[0x1010] = 0xA9;
            r.0[0x1011] = 7;
            r.0[0x1012] = 0x60;
        });
        assert_eq!((c.a, c.pc, c.sp), (7, 0x1003, 0xFF));
    }

    #[test]
    fn bcd_add_sub() {
        let (c, _) = run(&[0xF8, 0x18, 0xA9, 0x19, 0x69, 0x28], 4, |_, _| {});
        assert_eq!(c.a, 0x47);
        let (c, _) = run(&[0xF8, 0x38, 0xA9, 0x50, 0xE9, 0x13], 4, |_, _| {});
        assert_eq!(c.a, 0x37);
        assert!(c.c);
    }

    #[test]
    fn undocumented_lax_sax_dcp_isc() {
        // LAX $30 ; SAX $31 ; DCP $32 ; ISC $33
        let (c, r) = run(&[0xA7, 0x30, 0x87, 0x31, 0xC7, 0x32, 0xE7, 0x33], 4, |_, r| {
            r.0[0x30] = 0xF0;
            r.0[0x32] = 0xF0;
            r.0[0x33] = 0x01;
        });
        assert_eq!(c.x, 0xF0);
        assert_eq!(r.0[0x31], 0xF0);
        assert_eq!(r.0[0x32], 0xEF);
        assert_eq!(r.0[0x33], 0x02);
    }

    #[test]
    fn slo_and_nops() {
        let (c, r) = run(&[0xA9, 0x01, 0x07, 0x40, 0x04, 0x40, 0x0C, 0x00, 0x20, 0x1A, 0x80, 0xFF], 5, |_, r| r.0[0x40] = 0x81);
        assert_eq!(r.0[0x40], 0x02);
        assert_eq!(c.a, 0x03);
        assert_eq!(c.pc, 0x100A);
    }

    #[test]
    fn kil_errors() {
        let mut ram = Ram(vec![0; 65536]);
        ram.0[0] = 0x02;
        let mut cpu = Cpu::default();
        assert!(cpu.step(&mut ram).is_err());
    }
}
