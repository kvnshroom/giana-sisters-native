//! The text and number printer of the 30A preview (`$22C1..$23A5`): the screens of the game flow, the stage banner and
//! the status bar print through it. Ported for package F (`docs/rom-hack-family-c.md`, 7.9).
//!
//! A text is a byte string from the pointer tables `$23C9`/`$23E1`: `$00..$7F` a screen code (written at the print
//! position plus the byte's index, with the current colour when one is set), `$80..$EE` skips `n & $7F` cells (the
//! print position moves on), `$EF..$FE` sets the colour for the following characters, `$FF` ends it. At the end the
//! print column `$24` moves on by the length of the string (index of the `$FF`, wrapping into the next rows), and the
//! original waits until the screen is on (`$23AE`): [`print`] returns, the caller does the wait.

use crate::layout::C30aLayout;

/// The colour pointer of a screen pointer (`$C8xx` -> `$D8xx`): `| $10` on the high byte.
const COLOUR: u8 = 0x10;

fn rd(ram: &[u8], a: u16) -> u8 {
    ram[a as usize]
}

/// A write through the game's pointers: colour RAM for `$D800..$DBFF`, RAM elsewhere (other I/O is dropped).
pub fn put(ram: &mut [u8], color: &mut [u8], a: u16, v: u8) {
    match a {
        0xD800..=0xDBFF => color[(a - 0xD800) as usize] = v & 15,
        0xD000..=0xDFFF => {}
        _ => ram[a as usize] = v,
    }
}

/// `$22C1`: text `n` at column `x`, row `y` (`$24/$25`).
pub fn print_at(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, n: u8, x: u8, y: u8) {
    ram[l.print_col as usize] = x;
    ram[l.print_col as usize + 1] = y;
    print(ram, color, l, n);
}

/// Estimated CPU cycles of the printer `$22C1` for text `n` up to its screen-on wait (`$2343`): the setup, then each
/// byte of the text (a character, a colour code, a run of blanks).
pub fn print_cycles(ram: &[u8], l: &C30aLayout, n: u8) -> i64 {
    let src = u16::from_le_bytes([rd(ram, l.text_lo + n as u16), rd(ram, l.text_hi + n as u16)]);
    let mut t = 80i64;
    let mut coloured = false;
    for y in 0..=255u16 {
        let a = rd(ram, src.wrapping_add(y));
        match a {
            0xFF => return t + 40,
            0xEF..=0xFE => {
                coloured = a != 0;
                t += 12;
            }
            0x80..=0xEE => t += 15 + 22 * (a & 0x7F) as i64,
            _ => t += if coloured { 33 } else { 27 },
        }
    }
    t
}

/// `$22C5`: text `n` at the print position `$24/$25`.
pub fn print(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, n: u8) {
    let (src_lo, src_hi) = (l.print_ptr as usize, l.print_ptr as usize + 1);
    ram[src_lo] = rd(ram, l.text_lo + n as u16);
    ram[src_hi] = rd(ram, l.text_hi + n as u16);
    print_from_ptr(ram, color, l);
}

/// `$22CF`: the text at `$04/$05` at the print position. Returns the index of its end byte (the original's `Y`).
pub fn print_from_ptr(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) -> u8 {
    let pc = l.print_col as usize;
    let (src_lo, src_hi) = (l.print_ptr as usize, l.print_ptr as usize + 1);
    let colour_at = l.obj_probe_slot as usize; // `$0A`: the printer's colour
    ram[colour_at] = 0;
    let row = ram[pc + 1];
    let (lo, c) = rd(ram, l.row_lo + 1 + row as u16).overflowing_add(ram[pc]);
    let hi = rd(ram, l.row_hi + 1 + row as u16).wrapping_add(c as u8);
    let t = l.text_ptr as usize;
    ram[t] = lo;
    ram[t + 2] = lo;
    ram[t + 1] = hi;
    ram[t + 3] = hi | COLOUR;
    let mut y: u8 = 0;
    loop {
        let src = u16::from_le_bytes([ram[src_lo], ram[src_hi]]).wrapping_add(y as u16);
        let a = rd(ram, src);
        if a & 0x80 == 0 {
            let dst = u16::from_le_bytes([ram[t], ram[t + 1]]).wrapping_add(y as u16);
            put(ram, color, dst, a);
            let c = ram[colour_at];
            if c != 0 {
                let dst = u16::from_le_bytes([ram[t + 2], ram[t + 3]]).wrapping_add(y as u16);
                put(ram, color, dst, c);
            }
        } else if a == 0xFF {
            break;
        } else if a >= 0xEF {
            ram[colour_at] = a;
        } else {
            for _ in 0..(a & 0x7F) {
                let col = ram[pc].wrapping_add(1);
                ram[pc] = col;
                if col == 0x28 {
                    ram[pc] = 0;
                    ram[pc + 1] = ram[pc + 1].wrapping_add(1);
                }
                for p in [t, t + 2] {
                    ram[p] = ram[p].wrapping_add(1);
                    if ram[p] == 0 {
                        ram[p + 1] = ram[p + 1].wrapping_add(1);
                    }
                }
            }
        }
        y = y.wrapping_add(1);
        if y == 0 {
            ram[src_hi] = ram[src_hi].wrapping_add(1);
            ram[t + 1] = ram[t + 1].wrapping_add(1);
            ram[t + 3] = ram[t + 3].wrapping_add(1);
        }
    }
    // `$2333`: the column moves on by the string's length, wrapping into the next rows
    let mut a8 = y.wrapping_add(ram[pc]);
    while a8 >= 0x28 {
        ram[pc + 1] = ram[pc + 1].wrapping_add(1);
        a8 -= 0x28;
        if a8 == 0 {
            break;
        }
    }
    ram[pc] = a8;
    y
}

/// `$2392`: the print pointer `$04/$05` = the row `$25`; returns `$24` (the original's `Y`).
fn row_ptr(ram: &mut [u8], l: &C30aLayout) -> u8 {
    let row = ram[l.print_col as usize + 1] as u16;
    ram[l.print_ptr as usize] = rd(ram, l.row_lo + 1 + row);
    ram[l.print_ptr as usize + 1] = rd(ram, l.row_hi + 1 + row);
    ram[l.print_col as usize]
}

fn put_ptr(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, y: u8, v: u8) {
    let p = u16::from_le_bytes([ram[l.print_ptr as usize], ram[l.print_ptr as usize + 1]]).wrapping_add(y as u16);
    put(ram, color, p, v);
}

/// `$234F`: the binary number `a` (0..99) as two decimal digits at column `x`, row `y`; a tens digit of 5 or more is
/// the character `$18`.
pub fn number_at(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, a: u8, x: u8, y: u8) {
    ram[l.print_col as usize] = x;
    ram[l.print_col as usize + 1] = y;
    number(ram, color, l, a);
}

/// `$2352`: [`number_at`] at the print position.
pub fn number(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, a: u8) {
    let y = row_ptr(ram, l);
    number_digits(ram, color, l, a, y);
}

fn number_digits(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, a: u8, y: u8) {
    let (mut a, mut tens) = (a, 0u8);
    while a >= 10 {
        a -= 10;
        tens += 1;
    }
    let mut t = tens | 0x30;
    if t >= 0x35 {
        t = 0x18;
    }
    put_ptr(ram, color, l, y, t);
    let y = y.wrapping_add(1);
    put_ptr(ram, color, l, y, a | 0x30);
    ram[l.print_col as usize] = y.wrapping_add(1);
}

/// `$2377`: the BCD byte `a` as two digits at column `x`, row `y`.
pub fn bcd_at(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, a: u8, x: u8, y: u8) {
    ram[l.print_col as usize] = x;
    ram[l.print_col as usize + 1] = y;
    bcd(ram, color, l, a);
}

/// `$237A`: [`bcd_at`] at the print position.
pub fn bcd(ram: &mut [u8], color: &mut [u8], l: &C30aLayout, a: u8) {
    let y = row_ptr(ram, l);
    put_ptr(ram, color, l, y, a >> 4 | 0x30);
    let y = y.wrapping_add(1);
    put_ptr(ram, color, l, y, (a & 15) | 0x30);
    ram[l.print_col as usize] = y.wrapping_add(1);
}

/// `$47A2`: the score (`$CBF0..$CBF2`, high byte first) at column `$1B` of the print row.
pub fn score(ram: &mut [u8], color: &mut [u8], l: &C30aLayout) {
    ram[l.print_col as usize] = 0x1B;
    for i in [2u16, 1, 0] {
        let v = ram[(l.score + i) as usize];
        bcd(ram, color, l, v);
    }
}
