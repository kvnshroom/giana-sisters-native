//! The bosses and the multi-sprite objects (work package O4 of `docs/rom-hack-family-c.md`, 7.4 and 7.7): native
//! port of the group placement `$8521` that the object frame `$72EA` runs after the update loop, verified call by call,
//! frame by frame and on random states against the original running in the oracle machine
//! (`crates/giana-c30-oracle/tests/bosses.rs`).
//!
//! # What a boss is in this preview
//!
//! There is no boss code of its own in Demo 2.0 (the "different path at `$8Cxx`" of section 2.6 is the map builder
//! `$8C0E`, which every stage runs). A boss is an object of class 3 (a script, [`crate::classes::scripted`]) whose
//! descriptor kind `$0222` is **negative**: the kind counts the hits. The contact handler (`$7CD6`, package O3) adds one
//! to the leader's kind for every hit of Giana's shot (`INC $0222,X`; a positive kind such as `$7F` only flashes the
//! sprite, colour 2); at kind 0 the next hit kills the boss like any enemy (`$7D9E`: the hit frame, phase `$1800` = 5,
//! sound 5, the score of `$0227/$0254`: `$00/$30` for the stage-13 boss). The phase makes the update loop take the route
//! of the hit script `$4296` ([`crate::classes::scripted_phase`]: up 2, 1, a pause, then down 1, 2 and 3 pixels a frame
//! until the despawn takes it). Nothing else changes: the stage goes on, and its
//! end is the ordinary stage-end pickup further right (types `$10/$11`, value `$C0`, two stacked sprites).
//!
//! | type | descriptor | sprites | kind (hits) | script | where |
//! |---|---|---|---|---|---|
//! | `$12..$14` | `$448F` | 3 (group 2, side by side) | `$F1` (16 hits) | `$4223`: a slow wave over the floor, `$FD 01 $40` = a fire ball (class 4, type `$40`) once a lap, a fast dash `-3`/`+3` | stage 13, column `$28` (the only boss the spawn lists of Demo 2.0 contain) |
//! | `$15` | `$44A4` | 2 (group 1) | `$F5` (12 hits) | `$46E8`: left and right | unused in Demo 2.0 |
//!
//! Stages 8 and 15 (named as boss stages in 2.6) end with groups of 3 sprites too, but of kind `$7F` (cannot be hurt):
//! the stage-15 end has type `$16` (class 2, up and down, script `$46DB`) and stage 8 the invisible blockers of types
//! `$20..$23` (frame `$A9`, colour 0). Their placement is the same group code. Everything a boss fight runs is therefore
//! ported in other packages (O2: the scripts and the fire ball's class 4; O3: the shot and the hit counting in the
//! contact handler; F: the stage end), except this group placement.
//!
//! # Groups (`$8521`)
//!
//! A leader slot `n` (4..7) with members (`$02A4 + n` != 0, member slots in `$02A8/$02AC/$02B0 + n`, their number in
//! [`Field::Group`]) places its members every frame from its own position, by [`Field::Width`] (`$025E`):
//!
//! | width | layout |
//! |---|---|
//! | 0 | side by side: each member 24 pixels right of the one before, same y (the x high bit carried in `$88`) |
//! | 1..`$7F` | a 2x2 block of the fixed slots 4..7 from slot 7: 6 above 7, 4 right of 7, 5 above 4 |
//! | `$80` | one member 21 lines below (y `$FD` = off when it would wrap or lie above line `$20`); it is the multiplexed sprite: `$61/$62` (first/second sprite of the multiplexer, the lower one in `$62`), `$30`, `$63/$64` (the line of the switch, y + `$13`) |
//! | `$81..$FF` | stacked: each member 21 lines above the one before |
//!
//! A leader whose phase `$1800` is not 0 (hit, dying) gives phase 1 to all its members first; the members' sprite frames
//! follow the leader's (`frame + 1`, `+ 2`, `+ 3`). At the end `$30` and `$62` are shifted left (`ASL`): the multiplexer
//! (`$3183`, package W) reads them as "set" flags.

use crate::layout::C30aLayout;
use crate::objects::Field;

/// The 6502 registers that matter at the end of [`place_groups`] (the next routine of the object frame, `$81C6`, reads
/// `X`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regs {
    pub a: u8,
    pub x: u8,
    pub y: u8,
}

/// Memory of one call: zero-page indexing wraps in page 0 (`LDA $70,X`), absolute indexing does not (`STA $0070,Y`).
struct Mem<'a> {
    ram: &'a mut [u8],
    l: &'a C30aLayout,
}

impl Mem<'_> {
    fn r(&self, a: u16) -> u8 {
        self.ram[a as usize]
    }
    fn w(&mut self, a: u16, v: u8) {
        self.ram[a as usize] = v;
    }
    /// `base,X` absolute
    fn ra(&self, base: u16, x: u8) -> u8 {
        self.ram[base.wrapping_add(u16::from(x)) as usize]
    }
    fn wa(&mut self, base: u16, x: u8, v: u8) {
        self.ram[base.wrapping_add(u16::from(x)) as usize] = v;
    }
    /// `zp,X`
    fn rz(&self, zp: u16, x: u8) -> u8 {
        self.ram[(zp as u8).wrapping_add(x) as usize]
    }
    fn field(&self, f: Field, x: u8) -> u8 {
        self.ra(f.at(self.l, 0), x)
    }
    /// `$02A8 + 4k + X`: member `k` of the group led by slot `X` (`k` = 0..2); with `X` advanced by 4 per member the
    /// original reads it as `$02A8,X`
    fn member(&self, x: u8) -> u8 {
        self.ra(self.l.group_members + 4, x)
    }
    /// `LDA $88 : AND bit,X : BEQ clear : LDA $88 : ORA bit,Y : BNE store : clear: LDA $88 : AND mask,Y : store: STA $88`
    /// (`on` = the leader's bit; an `ORA` that gives 0 falls into the clearing path, as in the original)
    fn set_msb(&mut self, y: u8, on: bool) {
        let msb = self.r(self.l.sprite_x_msb);
        let mut v = 0;
        if on {
            v = msb | self.ra(self.l.bit_set, y);
        }
        if v == 0 {
            v = msb & self.ra(self.l.bit_clear, y);
        }
        self.w(self.l.sprite_x_msb, v);
    }
    fn dec_count(&mut self) -> u8 {
        let c = self.r(self.l.group_count).wrapping_sub(1);
        self.w(self.l.group_count, c);
        c
    }
}

/// `$8521`: places the members of every group (leaders 7 down to 4) and gives them the leader's phase and frames; see
/// the module documentation. `y` is the caller's `Y` (kept when no slot leads a group). Returns the registers as the
/// original's `RTS` leaves them.
///
/// Every value is read from memory where the original reads it: the stores `STA $0070,Y`/`$0078,Y`/`$0080,Y` of a
/// member slot beyond 7 (only on corrupt states) can reach the zero-page temporaries `$F1..$F5`.
pub fn place_groups(ram: &mut [u8], l: &C30aLayout, y: u8) -> Regs {
    let mut m = Mem { ram, l };
    m.w(l.mux_first, 0);
    m.w(l.mux_other, 0);
    let mut y = y;
    let mut x = 7u8;
    let mut a;
    loop {
        a = m.ra(l.group_members, x);
        if a != 0 {
            m.w(l.group_leader_tmp, x);
            m.w(l.group_count, m.field(Field::Group, x));
            y = place_members(&mut m, x).1;
            x = m.r(l.group_leader_tmp);
            m.w(l.group_count, m.field(Field::Group, x));
            if m.ra(l.obj_phase, x) != 0 {
                // `$8544`: phase 1 for every member (`X` steps by 4 through the member table)
                let mut xx = x;
                loop {
                    y = m.member(xx);
                    m.wa(l.obj_phase, y, 1);
                    xx = xx.wrapping_add(4);
                    if m.dec_count() == 0 {
                        break;
                    }
                }
                x = m.r(l.group_leader_tmp);
                m.w(l.group_count, m.field(Field::Group, x));
            }
            // `$855A`: frames leader + 1, + 2, + 3 (the carry of each `ADC #1` goes into the next)
            a = m.rz(l.player_frames, x);
            let mut carry = false;
            for k in 0..3u16 {
                y = m.ra(l.group_members + 4 + 4 * k, x);
                let sum = u16::from(a) + 1 + u16::from(carry);
                a = sum as u8;
                carry = sum > 0xFF;
                m.wa(l.player_frames, y, a);
                // the third member ends the loop without a `DEC $F4`
                if k == 2 || m.dec_count() == 0 {
                    break;
                }
            }
        }
        // `$857D`
        x = x.wrapping_sub(1);
        if x < 4 {
            break;
        }
    }
    // `$8582`
    let v = m.r(l.mux_slot) << 1;
    m.w(l.mux_slot, v);
    let v = m.r(l.mux_other) << 1;
    m.w(l.mux_other, v);
    Regs { a, x, y }
}

/// `$8587`: the members' positions for the leader `x` (count in `$F4`). Returns `A` and `Y` as it leaves them.
fn place_members(m: &mut Mem, x: u8) -> (u8, u8) {
    let l = m.l;
    let width = m.field(Field::Width, x);
    if width == 0 {
        // `$858E`: side by side
        let hi = u8::from(m.r(l.sprite_x_msb) & m.ra(l.bit_set, x) != 0);
        m.w(l.child_msb, hi);
        let v = m.rz(l.player_y, x);
        m.w(l.bump_limit, v);
        let mut a = m.rz(l.player_x, x);
        let mut xx = x;
        loop {
            let y = m.member(xx);
            let sum = u16::from(a) + 0x18;
            m.w(l.spawn_pick, sum as u8);
            m.wa(l.player_x, y, sum as u8);
            let mut hi = m.r(l.child_msb);
            if sum > 0xFF {
                hi ^= 1;
                m.w(l.child_msb, hi);
            }
            m.set_msb(y, hi != 0);
            let v = m.r(l.bump_limit);
            m.wa(l.player_y, y, v);
            xx = xx.wrapping_add(4);
            a = m.r(l.spawn_pick);
            if m.dec_count() == 0 {
                return (a, y);
            }
        }
    } else if width < 0x80 {
        // `$85D1`: the 2x2 block of slots 4..7
        let x7 = m.r(l.player_x + 7);
        m.w(l.player_x + 6, x7);
        let sum = u16::from(x7) + 0x18;
        m.w(l.player_x + 5, sum as u8);
        m.w(l.player_x + 4, sum as u8);
        let msb = m.r(l.sprite_x_msb);
        let mut v = if msb & 0x80 != 0 { msb | 0xF0 } else { msb & 0x0F };
        if sum > 0xFF {
            v ^= 0x30;
        }
        m.w(l.sprite_x_msb, v);
        let y7 = m.r(l.player_y + 7);
        m.w(l.player_y + 4, y7);
        let up = y7.wrapping_sub(0x15);
        m.w(l.player_y + 6, up);
        m.w(l.player_y + 5, up);
        (up, width)
    } else if width != 0x80 {
        // `$85FB`: stacked upwards (`$F2` keeps the raw bit, not 0/1)
        let hi = m.r(l.sprite_x_msb) & m.ra(l.bit_set, x);
        m.w(l.child_msb, hi);
        let v = m.rz(l.player_x, x);
        m.w(l.spawn_pick, v);
        let mut a = m.rz(l.player_y, x);
        let mut xx = x;
        loop {
            let y = m.member(xx);
            a = a.wrapping_sub(0x15);
            m.wa(l.player_y, y, a);
            m.w(l.bump_limit, a);
            let hi = m.r(l.child_msb);
            m.set_msb(y, hi != 0);
            let v = m.r(l.spawn_pick);
            m.wa(l.player_x, y, v);
            xx = xx.wrapping_add(4);
            a = m.r(l.bump_limit);
            if m.dec_count() == 0 {
                return (a, y);
            }
        }
    } else {
        // `$8634`: one member below, the multiplexed sprite
        let y = m.member(x);
        let hi = m.r(l.sprite_x_msb) & m.ra(l.bit_set, x) != 0;
        m.set_msb(y, hi);
        let v = m.rz(l.player_x, x);
        m.wa(l.player_x, y, v);
        let sum = u16::from(m.rz(l.player_y, x)) + 0x15;
        let a = if sum > 0xFF || (sum as u8) < 0x20 { 0xFD } else { sum as u8 };
        m.wa(l.player_y, y, a);
        let first = m.r(l.mux_first);
        if first != 0 {
            if a >= m.rz(l.player_y, first) {
                m.w(l.mux_other, y);
                return (a, y);
            }
            m.w(l.mux_other, first);
        }
        m.w(l.mux_first, y);
        m.w(l.mux_slot, y);
        let line = u16::from(a) + 0x13;
        let line = if line > 0xFF { 0xFF } else { line as u8 };
        m.w(l.mux_line, line);
        m.w(l.mux_line_start, line);
        (line, y)
    }
}
