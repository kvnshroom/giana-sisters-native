//! The raster IRQs of the game flow's screens (package F, `docs/rom-hack-family-c.md` 7.9): the menus and text screens
//! (`$2F54`), the title (`$2FC8` with its three phases), and the high-score screen (`$5098`, through the KERNAL's
//! `$0314` with the KERNAL ROM in). They draw raster bars (`$D021` changed line by line from small colour tables), run
//! the tile animation, the music and the joystick read, and on the title two scrolling text rows and the play logic.
//!
//! The bars go to [`Core::bars`] as `(line, colour)` for the renderer; the lines are those of the original's cycle-timed
//! loops (`$1890..$18B7`: one table entry per raster line).

use crate::frame::{Core, Original};
use crate::player;
use crate::scroll;
use crate::sprites::Io;

/// Raster bars: `n` entries of the table at `table`, from entry `n - 1` down to 0, one per line from `line` on
/// (`$1894..$18B7`, also entered at `$1890` with 9 entries and `$1892` with 8). `$2A/$2B` point at the table afterwards;
/// `$D021` keeps the last colour.
fn bars(core: &mut Core, table: u16, n: u8, line: u16) {
    let l = core.l;
    core.poke(l.bar_table, table as u8);
    core.poke(l.bar_table + 1, (table >> 8) as u8);
    for k in 0..n {
        let c = core.peek(table + (n - 1 - k) as u16);
        // each write lands at the end of its line (cycle 54 of the 63-cycle loop): it shows from the next line
        set_at(core, line + k as u16 + 1, 0x21, c);
        core.bars.push((line + k as u16, c & 15));
    }
}

/// A VIC register write of a screen's IRQ that shows from raster line `line` on (for the renderer,
/// [`Core::vic_writes`]).
fn set_at(core: &mut Core, line: u16, reg: u8, v: u8) {
    core.vic[reg as usize] = v;
    core.vic_writes.push((line % crate::frame::LINES, reg, v));
}

/// Lines after the last bar at which the title's register writes show (calibrated against VICE, `tests/screens.rs`).
const TITLE_LATE: u16 = 0;
/// The same for the high-score screen's.
const HISCORE_LATE: u16 = 0;

/// The handler of `vector` if it is one of the game flow's; `false` = not one of them.
pub fn irq(core: &mut Core, vector: u16, orig: &mut dyn Original) -> bool {
    let l = core.l;
    if core.peek(1) & 2 != 0 {
        // KERNAL in: the `$0314` handler, then the KERNAL's own IRQ (`$EA31`)
        if vector != l.irq_hiscore {
            return false;
        }
        if hiscore_irq(core) {
            kernal_irq(core);
        }
        return true;
    }
    match vector {
        v if v == l.irq_menu => menu_irq(core),
        v if v == l.irq_title => title_irq(core, orig),
        _ => return false,
    }
    true
}

/// Estimated CPU cycles of the handlers of [`irq`].
pub fn irq_cycles(core: &Core, vector: u16) -> i64 {
    let l = core.l;
    if vector == l.irq_menu {
        let n: i64 = (0..3).map(|i| core.peek(l.menu_bands + i) as i64).sum();
        return (8 * n + 2) * 63 + 1300;
    }
    if vector == l.irq_title {
        return match core.peek(l.title_phase) {
            // the phase that runs now is the one before (the handler counts down first)
            2 => 9 * 63 + 300,
            0 => 9 * 63 + 300,
            _ => 8 * 63 + 1800,
        };
    }
    if vector == l.irq_hiscore {
        return 8 * 63 + 900;
    }
    500
}

/// `$2F54` (line `$69`): three bands of raster bars (`$1878`, `$1880`, `$1888`; 8 lines per repeat, the repeats from
/// `$CBF4..$CBF6` counted down in `$E1..$E3`), the tile animation, the music, the joystick, `$D021 = 0`, `$D011 <- $D7`.
fn menu_irq(core: &mut Core) {
    let l = core.l;
    let mut line = core.line + 1;
    for (i, table) in [l.bars_menu, l.bars_menu + 8, l.bars_menu + 16].into_iter().enumerate() {
        let n = core.peek(l.menu_bands + i as u16);
        core.poke(l.cloud_columns + i as u16, n);
        let mut k = n;
        loop {
            bars(core, table, 8, line);
            line += 8;
            k = k.wrapping_sub(1);
            core.poke(l.cloud_columns + i as u16, k);
            if k == 0 {
                break;
            }
        }
    }
    scroll::tile_anim(core.ram(), l);
    core.music.tick();
    let joy = core.joy;
    if player::read_input(core.ram(), l, joy, 0).is_some() {
        core.cia_pra = 0xFF;
    }
    let d7 = core.peek(l.screen_mode);
    // after the tile animation, the music and the joystick (about 1,300 cycles)
    set_at(core, line + 20, 0x21, 0);
    set_at(core, line + 20, 0x11, d7);
}

/// `$2FC8` (lines `$31`, `$41`, `$51` of the title): the next line and phase from the tables at `$309B` (the phase
/// counter `$28` counts 2, 1, 0; the target is written into the `JMP` at `$2FEE`), then the phase:
/// `$2FF1` bars `$185E` and the colours of the header, `$D016 = $02D7` (the upper text row's scroll);
/// `$3011` bars `$1867`, `$D016 = $02D8` (the lower one's);
/// `$3025` bars `$1870`, the playfield's `$D016`, the two text scrollers (row 2 one pixel a frame, row 4 two), then the
/// play logic like the `$5A` handler, with the `$FB` handler next.
fn title_irq(core: &mut Core, orig: &mut dyn Original) {
    let l = core.l;
    let x = core.peek(l.title_phase) as u16;
    core.vic[0x12] = core.peek(l.title_lines + x);
    let lo = core.peek(l.title_lines + 3 + x);
    let hi = core.peek(l.title_lines + 6 + x);
    core.poke(l.title_jump_op, lo);
    core.poke(l.title_jump_op + 1, hi);
    let nx = if x == 0 { 2 } else { x - 1 };
    core.poke(l.title_phase, nx as u8);
    let line = core.line + 1;
    match x {
        2 => {
            bars(core, l.bars_title, 9, line);
            let (c2, c3, d016) = (core.peek(l.band_colours), core.peek(l.band_colours + 1), core.peek(l.title_scroll));
            set_at(core, line + 9 + TITLE_LATE, 0x22, c2);
            set_at(core, line + 9 + TITLE_LATE, 0x23, c3);
            set_at(core, line + 9 + TITLE_LATE, 0x16, d016);
        }
        1 => {
            bars(core, l.bars_title + 9, 9, line);
            let d016 = core.peek(l.title_scroll + 1);
            set_at(core, line + 9 + TITLE_LATE, 0x16, d016);
        }
        _ => {
            bars(core, l.bars_title + 18, 8, line);
            let d016 = core.peek(l.fine_scroll) | 0x10;
            // after a delay loop (`$3038`): a line later
            set_at(core, line + 8 + TITLE_LATE + 1, 0x21, 0);
            set_at(core, line + 8 + TITLE_LATE + 1, 0x16, d016);
            title_scrollers(core);
            {
                let mut io = Io::new(&mut core.music.driver.mem[..], &mut core.vic[..], l);
                io.w(l.irq_vector, l.irq_fb as u8);
                io.w(l.irq_vector + 1, (l.irq_fb >> 8) as u8);
            }
            core.logic_and_step(orig);
        }
    }
}

/// `$3043..$3097`: the title's two text rows: row 2 moves one pixel a frame (`$02D7`), row 4 two (`$02D8`); when one
/// has moved a character, its row shifts left and the next character of its text comes in at the right (`$2E25` from
/// index `$29`, ending with blanks and a restart; `$2E74` from `$2C`).
fn title_scrollers(core: &mut Core) {
    let l = core.l;
    let s = l.screen as usize;
    let a = core.peek(l.title_scroll).wrapping_sub(1);
    core.poke(l.title_scroll, a);
    if a & 0x80 != 0 {
        core.poke(l.title_scroll, 7);
        for y in 0x59..=0x7Fusize {
            let v = core.music.driver.mem[s - 8 + y];
            core.music.driver.mem[s - 9 + y] = v;
        }
        let y = core.peek(l.title_text_index);
        let mut c = core.peek(l.title_text + y as u16);
        let mut write = true;
        if y >= 0x4F {
            c = 0;
            if y >= 0x5B {
                core.poke(l.title_text_index, 0);
                write = false;
            }
        }
        if write {
            core.poke(l.screen + 0x77, c);
            core.poke(l.title_text_index, y.wrapping_add(1));
        }
    }
    let b = core.peek(l.title_scroll + 1).wrapping_sub(2);
    core.poke(l.title_scroll + 1, b);
    if b & 0x80 != 0 {
        core.poke(l.title_scroll + 1, 6);
        for y in 0x59..=0x7Fusize {
            let v = core.music.driver.mem[s + 0x48 + y];
            core.music.driver.mem[s + 0x47 + y] = v;
        }
        let y = core.peek(l.title_text_index2);
        let c = if y >= 0xE0 { 0 } else { core.peek(l.title_text + 0x4F + y as u16) };
        core.poke(l.screen + 0xC7, c);
        core.poke(l.title_text_index2, y.wrapping_add(1));
    }
}

/// `$5098` (high-score screen, KERNAL in, lines `$31`/`$F1`/`$91`): one band of 8 bars per phase (tables `$1870`,
/// `$1868`, `$185F`), `$D022` and a flashing `$D021` from `$4C5B`; in the last phase the flash counters, the tile
/// animation, the music, the entry's cursor colour in row 24 and the big letter that scrolls in (`$44/$45`, 8 bytes to
/// the character at `$C178`).
fn hiscore_irq(core: &mut Core) -> bool {
    let l = core.l;
    let x = core.peek(l.title_phase) as u16;
    core.vic[0x12] = core.peek(l.hiscore_lines + x);
    let lo = core.peek(l.hiscore_lines + 3 + x);
    let hi = core.peek(l.hiscore_lines + 6 + x);
    let line = core.line + 1;
    bars(core, u16::from_le_bytes([lo, hi]), 8, line);
    if x > 0 {
        let x = x - 1;
        let c2 = core.peek(l.hiscore_lines + 11 + x);
        set_at(core, line + 8 + HISCORE_LATE, 0x22, c2);
        core.poke(l.title_phase, x as u8);
        let i = core.peek(l.hiscore_flash).wrapping_add(core.peek(l.hiscore_lines + 9 + x));
        let c = core.peek(l.flash_colours + i as u16);
        set_at(core, line + 8 + HISCORE_LATE, 0x21, c);
        return false;
    }
    core.poke(l.title_phase, 2);
    let t = core.peek(l.hiscore_flash + 1).wrapping_sub(1);
    core.poke(l.hiscore_flash + 1, t);
    if t & 0x80 != 0 {
        core.poke(l.hiscore_flash + 1, 4);
        let f = core.peek(l.hiscore_flash).wrapping_sub(1);
        core.poke(l.hiscore_flash, if f & 0x80 != 0 { 0x0D } else { f });
    }
    scroll::tile_anim(core.ram(), l);
    core.music.tick();
    let d = core.peek(l.entry_delay);
    if d & 0x80 == 0 {
        core.poke(l.entry_delay, d.wrapping_sub(1));
    }
    let cur = core.peek(l.entry_cursor);
    for y in 0..12u16 {
        core.color[(0x3D7 + y) as usize] = if y as u8 == cur { 6 } else { 0 };
    }
    // `$5103`: the letter scroll: the old letter (`$3F/$40`) moves out and the new one (`$42/$43`, read below its
    // pointer while the index is negative) moves in, two rows a frame, up (`$44` positive) or down
    let dir = core.peek(l.entry_letter + 5);
    if dir != 0 {
        let mut y = core.peek(l.entry_letter + 6);
        y = if dir & 0x80 != 0 { y.wrapping_sub(2) } else { y.wrapping_add(2) };
        core.poke(l.entry_letter + 6, y);
        let ptr = |core: &Core, a: u16| u16::from_le_bytes([core.peek(a), core.peek(a + 1)]);
        let (old, new) = (ptr(core, l.entry_letter), ptr(core, l.entry_letter + 3));
        for x in (0..8u16).rev() {
            y = y.wrapping_sub(1);
            let v = if y & 0x80 != 0 { core.peek(new.wrapping_add(y as u16)) } else { core.peek(old.wrapping_add(y as u16)) };
            core.poke(l.charset + 0x178 + x, v);
        }
        let cx = core.peek(l.entry_cursor) as u16;
        core.poke(l.entry_name + cx, 0x2F);
        if y & 7 == 0 {
            core.poke(l.entry_letter + 5, 0);
        }
    }
    true
}

/// The KERNAL's IRQ after the game's (`$EA31`): the jiffy clock `$A0..$A2` counts, and the keyboard: `$CB` = the
/// key held (matrix code, `$40` none: [`Core::held_key`]) and `$C5`; the keyboard buffer is [`Core::typed`].
fn kernal_irq(core: &mut Core) {
    let l = core.l;
    // the jiffy clock is `$A0..$A2` (the tile animation's pointer `$A0/$A1` outside play)
    let mut a = l.tile_anim_ptr + 2;
    loop {
        let v = core.peek(a).wrapping_add(1);
        core.poke(a, v);
        if v != 0 || a == l.tile_anim_ptr {
            break;
        }
        a -= 1;
    }
    let held = core.held_key;
    core.poke(l.kernal_key, held);
    core.poke(l.kernal_key - 6, held);
}
