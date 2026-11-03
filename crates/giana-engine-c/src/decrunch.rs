//! Native port of the cruncher that packs the one PRG of the 30A preview disk.
//!
//! The PRG loads at `$0801` and starts with `SYS 2064`. The stub at `$0810` copies two small routines to `$00F9`..`$01B8`
//! (a decruncher that runs at `$0100`) and `$0334`..`$03F3` (its helpers), then the decruncher
//!
//! 1. moves the plain tail of the file (a BASIC line `SYS 7440`) from the end of the stream down to `$0801`,
//! 2. unpacks the stream **backwards**: it reads packed bytes downwards from the end of the stream and writes unpacked
//!    bytes downwards from the top of RAM, until the read pointer reaches `$0A00`,
//! 3. maps in BASIC and `RUN`s the line at `$0801`, which starts the game at `$1D10`.
//!
//! There is no second layer: what the stream unpacks to is the game, complete in RAM (code, graphics, music, all 16
//! stages). Control bytes of the stream (`ctrl`, the names are the zero page cells of the original routine in brackets):
//!
//! | ctrl | meaning |
//! |---|---|
//! | `00` | 256 literal bytes follow below the read pointer (`$FB` = 1, `$FA` = 0) |
//! | `00nnnnnn` | copy 2 bytes from `dst + nnnnnn` (offset byte = the low 6 bits), no literals |
//! | `01nnnnnn` | one offset byte, copy 3 bytes, then `nnnnnn` literal bytes |
//! | `10nnnnnn` | copy 4 bytes, offset as below, then `nnnnnn` literals (`3F` = a count byte follows instead) |
//! | `11nnnnnn` | length: `nnnnnn` if at least 5, else a 16 bit length `nnnnnn << 8 | next byte`; then a literal-count byte and an offset |
//!
//! An offset is one byte `b` (value `b - F6`, where `F6` becomes 1 once the destination pointer has crossed a page
//! boundary) or, when `b <= F6`, two bytes (`b` is the high byte). The copy always runs from the high end downwards, so
//! overlapping copies repeat data exactly like the original routine. Every cell of the original (`F5`..`FF`) is a field
//! below so quirks (like the `F6` page flag) stay identical; the test in `tests/loader.rs` runs the real stub on a 6502
//! and compares all of RAM with this port.

/// Address the decrunched BASIC stub lives at (it is also where the program was loaded).
pub const LOAD_ADDR: u16 = 0x0801;
/// The stream is read down to this address (exclusive): the first stream byte.
const STREAM_END: u16 = 0x0A00;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Not the expected PRG (load address, BASIC stub, size).
    NotThePreview(&'static str),
    /// The stream does not unpack as expected (read pointer ran past the start, copy outside RAM ...).
    Corrupt(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotThePreview(s) => write!(f, "not the 30A preview program: {s}"),
            Error::Corrupt(s) => write!(f, "corrupt packed data: {s}"),
        }
    }
}
impl std::error::Error for Error {}

/// Where the parameters and routines are inside the loaded PRG (addresses of the loaded file image).
mod stub {
    /// Header bytes `$F9`..`$FF` of the decruncher: `$F9` (first literal run), `$FA`, `$FB`, `$FC/$FD` (end of the
    /// stream), `$FE/$FF` (end of the output).
    pub const HEADER: usize = 0x082D;
    /// `LDA src,Y / STA dst,Y` pairs of the tail move at `$0100` (copied from `$0834`): operands of the opcodes.
    pub const TAIL_MOVE: usize = 0x0834;
}

/// Unpacked state: all of RAM (`$0000..$FFFF`) as the game sees it right after the unpack, before it starts running.
/// Below `$0801` nothing is set (the real run leaves the cruncher's own variables there).
pub struct Decrunched {
    pub ram: Vec<u8>,
    /// Address of the first output byte the decruncher wrote (lowest address of the unpacked block).
    pub low: u16,
    /// `SYS` target of the BASIC stub at `$0801` (the game's start address).
    pub entry: u16,
}

struct State<'a> {
    m: &'a mut [u8],
    // the zero page cells of the original
    f5: u8,
    f6: u8,
    f7: u8,
    f8: u8,
    f9: u8,
    fa: u8,
    fb: u8,
    fe: u8,
    ff: u8,
    /// Read pointer (`$0379` high byte and X register of the original).
    pos: u16,
}

enum Flow {
    Done,
}

impl State<'_> {
    /// `$0365`: the destination crossed into the next lower page.
    fn next_page(&mut self) {
        if self.f6 < 1 {
            self.f6 += 1;
        }
        self.ff = self.ff.wrapping_sub(1);
    }

    /// `$0334`: copy `fb * 256 + fa` bytes from the end pointer `f9:f8` to the end pointer `ff:fe`, highest byte first.
    /// Both pointers move down by the length.
    fn copy(&mut self) {
        let (v, borrow) = self.fe.overflowing_sub(self.fa);
        self.fe = v;
        if borrow {
            self.next_page();
        }
        let (v, borrow) = self.f8.overflowing_sub(self.fa);
        self.f8 = v;
        if borrow {
            self.f9 = self.f9.wrapping_sub(1);
        }
        let mut y = self.fa;
        loop {
            if y | self.fb == 0 {
                return;
            }
            if y == 0 {
                self.fb = self.fb.wrapping_sub(1);
                self.f9 = self.f9.wrapping_sub(1);
                self.next_page();
            }
            y = y.wrapping_sub(1);
            let s = u16::from_le_bytes([self.f8, self.f9]).wrapping_add(y as u16);
            let d = u16::from_le_bytes([self.fe, self.ff]).wrapping_add(y as u16);
            self.m[d as usize] = self.m[s as usize];
        }
    }

    /// `$0370`: next packed byte (downwards). Reaching `$0A00` ends the unpack.
    fn byte(&mut self) -> Result<u8, Flow> {
        self.pos = self.pos.wrapping_sub(1);
        let b = self.m[self.pos as usize];
        if self.pos == STREAM_END {
            return Err(Flow::Done);
        }
        Ok(b)
    }

    /// Copies `fb*256+fa` literal bytes from below the read pointer (`$011E`..`$0128`).
    fn literal(&mut self) {
        let [lo, hi] = self.pos.to_le_bytes();
        self.f8 = lo;
        self.f9 = hi;
        self.copy();
        self.pos = u16::from_le_bytes([self.f8, self.f9]);
    }

    /// `$015A`: back reference `fa` bytes (and `fb` pages) from `dst + offset`, where `offset = f9:f8`.
    fn back_ref(&mut self) {
        let (v, c) = self.f8.overflowing_add(self.fe);
        self.f8 = v;
        self.f9 = self.f9.wrapping_add(self.ff).wrapping_add(c as u8);
        self.copy();
    }

    /// The decode loop (`$011A`..`$01B5`). Returns when the stream ends.
    fn run(&mut self) -> Result<(), Error> {
        match self.run_inner() {
            Err(Flow::Done) => Ok(()),
            Ok(()) => unreachable!(),
        }
    }

    fn run_inner(&mut self) -> Result<(), Flow> {
        // $011A: the first thing in the stream is a literal run of `$F9` bytes
        self.fa = self.f9;
        loop {
            self.literal();
            'control: loop {
                // $012F
                let a = self.byte()?;
                if a == 0 {
                    // 256 literal bytes
                    self.fb = 1;
                    self.fa = 0;
                    break 'control;
                }
                self.f5 = a;
                self.f8 = a & 0x3F;
                self.f7 = self.f8;
                self.f9 = 0;
                if a & 0x80 == 0 {
                    if a & 0x40 == 0 {
                        // 00nnnnnn: 2 bytes, offset in the control byte, no literals
                        self.f7 = 0;
                        self.fa = 2;
                    } else {
                        // 01nnnnnn: 3 bytes, offset byte follows
                        self.f8 = self.byte()?;
                        self.fa = 3;
                    }
                } else if a & 0x40 == 0 {
                    // 10nnnnnn: 4 bytes
                    self.fa = 4;
                    if self.f7 == 0x3F {
                        self.f7 = self.byte()?;
                    }
                    self.offset()?;
                } else {
                    // 11nnnnnn: length from the control byte or a following byte
                    let a = self.f7;
                    if a < 5 {
                        self.fb = a;
                        self.fa = self.byte()?;
                    } else {
                        self.fa = a;
                    }
                    self.f7 = self.byte()?;
                    self.offset()?;
                }
                self.back_ref();
                // $016A: literals after the copy, if any
                self.fa = self.f7;
                if self.f7 == 0 {
                    continue 'control;
                }
                break 'control;
            }
        }
    }

    /// `$018E`: reads an offset into `f9:f8` (one byte adjusted by `f6`, or high byte + low byte).
    fn offset(&mut self) -> Result<(), Flow> {
        let b = self.byte()?;
        let (d, borrow) = b.overflowing_sub(self.f6);
        if b == self.f6 || borrow {
            // b <= f6: b is the high byte
            self.f9 = b;
            self.f8 = self.byte()?;
        } else {
            self.f8 = d;
        }
        Ok(())
    }
}

/// Parses `SYS nnnn` out of a BASIC line at `addr` of `ram`.
fn sys_target(ram: &[u8], addr: usize) -> Option<u16> {
    // link (2), line number (2), token $9E (SYS), digits ...
    if ram.get(addr + 4) != Some(&0x9E) {
        return None;
    }
    let mut n: u32 = 0;
    let mut digits = 0;
    for &c in &ram[addr + 5..addr + 12] {
        match c {
            b'0'..=b'9' => {
                n = n * 10 + (c - b'0') as u32;
                digits += 1;
            }
            b' ' => {}
            _ => break,
        }
    }
    if digits == 0 || n > 0xFFFF {
        None
    } else {
        Some(n as u16)
    }
}

/// Unpacks the PRG (with its two byte load address) into RAM.
pub fn decrunch(prg: &[u8]) -> Result<Decrunched, Error> {
    if prg.len() < 0x1000 || prg[0] != 0x01 || prg[1] != 0x08 {
        return Err(Error::NotThePreview("load address is not $0801"));
    }
    let mut ram = vec![0u8; 0x1_0000];
    let body = &prg[2..];
    if LOAD_ADDR as usize + body.len() > 0x1_0000 {
        return Err(Error::NotThePreview("file too long"));
    }
    ram[LOAD_ADDR as usize..LOAD_ADDR as usize + body.len()].copy_from_slice(body);
    if sys_target(&ram, 0x0801) != Some(2064) {
        return Err(Error::NotThePreview("no SYS 2064"));
    }
    // $0810: SEI LDX #$FB TXS CLD LDA #$38 STA $01, jump into the copy loop that ends with JMP $0100
    if ram[0x0810..0x0819] != [0x78, 0xA2, 0xFB, 0x9A, 0xD8, 0xA9, 0x38, 0x85, 0x01] || ram[0x082A] != 0x4C {
        return Err(Error::NotThePreview("unexpected loader stub"));
    }
    let h = &ram[stub::HEADER..stub::HEADER + 7];
    let (f9, _fa, _fb, fc, fd, fe, ff) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6]);
    let stream_top = u16::from_le_bytes([fc, fd]);
    if stream_top <= STREAM_END || stream_top as usize > LOAD_ADDR as usize + body.len() {
        return Err(Error::Corrupt("stream end outside the file"));
    }
    // the tail move at $0100: LDA a,Y / STA b,Y twice, Y = 0..255 (the two ranges overlap on purpose)
    let t = stub::TAIL_MOVE;
    let w = |i: usize| u16::from_le_bytes([ram[t + i], ram[t + i + 1]]) as usize;
    if ram[t] != 0xB9 || ram[t + 3] != 0x99 || ram[t + 6] != 0xB9 || ram[t + 9] != 0x99 {
        return Err(Error::NotThePreview("unexpected tail move"));
    }
    let (s1, d1, s2, d2) = (w(1), w(4), w(7), w(10));
    for y in 0..256 {
        ram[d1 + y] = ram[s1 + y];
        ram[d2 + y] = ram[s2 + y];
    }
    let mut st = State { m: &mut ram, f5: 0, f6: 0, f7: 0, f8: 0, f9, fa: 0, fb: 0, fe, ff, pos: stream_top };
    st.run()?;
    let low = u16::from_le_bytes([st.fe, st.ff]);
    let entry = sys_target(&ram, 0x0801).ok_or(Error::Corrupt("no SYS line after the unpack"))?;
    Ok(Decrunched { ram, low, entry })
}
