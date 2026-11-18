//! The object classes (work package O2 of `docs/rom-hack-family-c.md`, 7.4): native ports of the class code behind every
//! [`Route`] of the update loop [`objects::dispatch`]. Each function is the rest of the original's update for one slot
//! and ends where the original goes on ([`ClassEnd`]); it uses the helpers of [`objects`] where the original calls them.
//! Verified call by call, frame by frame and on random states against the original running in the oracle machine
//! (`crates/giana-c30-oracle/tests/classes.rs`).
//!
//! # Routes and classes
//!
//! | route | original | function | what |
//! |---|---|---|---|
//! | [`Route::Items`] | `$765D..$76B7` | [`items`] | class 0: a bonus/power-up rising out of a block (kinds 2..`$11`), else [`moving`] |
//! | [`Route::Moving`] | `$76B8..$781E` | [`moving`] ([`moving_class`] = `$76CE`) | the probe, the fall of a dying object, then by class: 1 jumpers, 2 scripts, 0 and >= 5 walkers |
//! | [`Route::Scripted`] | `$76D5..$76DA` | [`scripted`] | class 3 (and 2): one step of the object's script |
//! | [`Route::ScriptedPhase`] | `$7852..$7877` | [`scripted_phase`] | class 3 once hit: the script of `$4296` from phase 5 on |
//! | [`Route::Class4`] | `$7878..$78BD` | [`class4`] | class 4: walks until the tile below it is deadly, then sinks away |
//! | [`Route::Floating`] | `$78BE..$7949` | [`floating`] | classes >= 5 in phase 0: up/down and sideways between walls |
//! | [`Route::Rising`] | `$7964..$79CF` | [`rising`] | negative classes: rises out of the ground, jumps, falls back |
//!
//! Subroutines: `$8418` [`script`] (the script interpreter: moves, frame steps, the object turning into another type
//! `$FB`, spawning another object `$FD`, jumps `$FE`, restart `$FF`), `$86E4` [`follow_vertical`] (a jumper turns
//! towards Giana's height), `$867E` [`follow_horizontal`] (a walker with speed bit 7 turns towards Giana).
//!
//! # Scripts
//!
//! An object's script (pointer [`Field::ScriptLo`]/[`Field::ScriptHi`], position `$02CB`, count `$02C6`) is a list of
//! 3-byte entries `count, dx, dy`: the object moves `dx` (signed; `$024A` bit 7: towards Giana with `|dx|`) and `dy`
//! (signed, also stored in `$02C1`) per frame for `count + 1` frames. A count byte `>= $FB` is a command: `$FB t` the
//! object becomes type `t` (`$73E8` on its own slot), `$FC d` the sprite frame moves by `d` (no frame time), `$FD dir t`
//! a new object of type `t` at the object's place facing `dir`, `$FE lo hi` go to another script, `$FF` start again.

use crate::layout::C30aLayout;
use crate::objects::{self, ClassEnd, Field, Route, VIC_PRIORITY};

/// Memory of one routine call (as in [`objects`]).
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
    fn rx(&self, base: u16, x: u8) -> u8 {
        self.ram[base.wrapping_add(u16::from(x)) as usize]
    }
    fn wx(&mut self, base: u16, x: u8, v: u8) {
        self.ram[base.wrapping_add(u16::from(x)) as usize] = v;
    }
    fn f(&self, f: Field, x: u8) -> u8 {
        self.r(f.at(self.l, x))
    }
    fn wf(&mut self, f: Field, x: u8, v: u8) {
        self.w(f.at(self.l, x), v);
    }
    /// `$53 + i`: the object probe's results
    fn p(&self, i: u16) -> u8 {
        self.r(self.l.obj_probe_props + i)
    }
    /// `(zp),Y`
    fn ind(&self, zp: u16, y: u8) -> u8 {
        let a = u16::from_le_bytes([self.r(zp), self.r((zp + 1) & 0xFF)]);
        self.r(a.wrapping_add(u16::from(y)))
    }
    fn bit(&self, x: u8) -> u8 {
        self.rx(self.l.bit_set, x)
    }
    fn mask(&self, x: u8) -> u8 {
        self.rx(self.l.bit_clear, x)
    }
    fn y(&self, x: u8) -> u8 {
        self.rx(self.l.player_y, x)
    }
    fn set_y(&mut self, x: u8, v: u8) {
        self.wx(self.l.player_y, x, v);
    }
}

/// The class code of a [`Route`] for slot `x` (the update loop's `Run { route, x, .. }`).
pub fn run(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, route: Route, x: u8) -> ClassEnd {
    match route {
        Route::Items => items(ram, vic, l, x),
        Route::Rising => rising(ram, l, x),
        Route::Moving => moving(ram, vic, l, x),
        Route::Scripted => scripted(ram, vic, l, x),
        Route::ScriptedPhase => scripted_phase(ram, vic, l, x),
        Route::Class4 => class4(ram, l, x),
        Route::Floating => floating(ram, l, x),
    }
}

/// `$765D`: class 0. Kinds 2..`$11` rise out of a block 2 pixels a frame (the kind counts up as they rise, `$0A..$10`
/// put the sprite in front of the background and stop rising one row into a solid tile, `$11` waits for a free tile;
/// kind 2 is the bonus, which only rises while `$02DC` is set and then takes the power-up colours); everything else is a
/// walker ([`moving`]).
pub fn items(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let kind = ram[Field::Kind.at(l, x) as usize];
    if !(2..0x12).contains(&kind) {
        return moving(ram, vic, l, x);
    }
    // `true` = `$769A` (the probe decides), else `$76AB` (rise)
    let check = if kind == 2 {
        let mut m = Mem { ram: &mut *ram, l };
        if m.r(l.bonus_flag) == 0 {
            return ClassEnd::Tail;
        }
        m.w(l.bonus_flag, 0);
        objects::powerup_colour(ram, vic, l, x);
        false
    } else if kind == 0x11 {
        objects::probe(ram, l, x);
        let mut m = Mem { ram: &mut *ram, l };
        if m.p(4) == 0 {
            true
        } else {
            // `DEC`, `BPL` (always taken: the kind is `$11`); `INC` below gives it back
            m.wf(Field::Kind, x, kind.wrapping_sub(1));
            false
        }
    } else if kind < 0x0A {
        false
    } else {
        vic[VIC_PRIORITY] &= ram[l.bit_clear.wrapping_add(u16::from(x)) as usize];
        objects::probe(ram, l, x);
        true
    };
    let mut m = Mem { ram, l };
    let stop = check && m.p(1) & 1 != 0 && m.p(4) == 0 && m.y(x) & 7 == 5;
    if !stop {
        let v = m.y(x).wrapping_sub(2);
        m.set_y(x, v);
    }
    let k = m.f(Field::Kind, x).wrapping_add(1);
    m.wf(Field::Kind, x, k);
    ClassEnd::Tail
}

/// `$76B8`: the probe, then a dying object (phase != 0) falls 4 pixels a frame until there is ground below it
/// ([`ClassEnd::Tail`]); a live one runs its class ([`moving_class`]).
pub fn moving(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    objects::probe(ram, l, x);
    let mut m = Mem { ram: &mut *ram, l };
    if m.rx(l.obj_phase, x) != 0 {
        if m.p(7) == 0 {
            let v = m.y(x).wrapping_add(4);
            m.set_y(x, v);
        }
        return ClassEnd::Tail;
    }
    moving_class(ram, vic, l, x)
}

/// `$76CE`: the class code of a live moving object (also called on its own by the frame driver, `$7303`, for the
/// object Giana stands on while the playfield steps): class 2 runs its script ([`scripted`]), class 1 jumps
/// ([`jumper`]), the others walk ([`walker`]). Ends at `$781F` ([`ClassEnd::Probe`]).
pub fn moving_class(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    match ram[Field::Class.at(l, x) as usize] {
        2 => scripted(ram, vic, l, x),
        1 => jumper(ram, l, x),
        _ => walker(ram, l, x),
    }
}

/// `$76D5`: one step of the object's script ([`script`]), then `$781F`.
pub fn scripted(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    script(ram, vic, l, x);
    ClassEnd::Probe
}

/// `$7852`: class 3 in a phase: phase 5 (just hit) starts the script `$4296` with the hit frame and counts the phase
/// down; then a script step and the tail.
pub fn scripted_phase(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    if m.rx(l.obj_phase, x) == 5 {
        m.wx(l.obj_phase, x, 4);
        let f = m.f(Field::HitFrameB, x);
        m.wx(l.player_frames, x, f);
        let [lo, hi] = l.hit_script.to_le_bytes();
        m.wf(Field::ScriptLo, x, lo);
        m.wf(Field::ScriptHi, x, hi);
        m.wx(l.obj_script_byte, x, 9);
        m.wx(l.obj_script_pos, x, 0);
    }
    script(ram, vic, l, x);
    ClassEnd::Tail
}

/// The labels of [`jumper`] and [`walker`] where they meet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum At {
    /// `$7721`: [`objects::flip`], then `$77FC`
    Flip,
    /// `$77FC`: turn when blocked in the direction of travel, then `$780B`
    Blocked,
    /// `$780B`: the horizontal move
    Move,
}

/// `$76E2`: class 1, the jumpers: while `$02C1` >= 0 the object falls (down the jump table `$8D5C` from `$026D`, or
/// straight down with [`Field::Movement`] bit 0), while negative it rises (up the table, or straight up to y `$30`/`$58`);
/// [`Field::ScriptLo`] adds a constant vertical speed and bit 7 of it makes the object turn towards Giana's height
/// ([`follow_vertical`]). Landing and hitting the ceiling flip `$02C1` ([`objects::flip`]); then the horizontal part
/// ([`walk`]).
fn jumper(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    let f1 = m.f(Field::ScriptLo, x) & 0x7F;
    m.w(l.spawn_pick, f1);
    let rising = m.rx(l.object_state, x) & 0x80 != 0;
    follow_vertical(ram, l, x);
    let mut m = Mem { ram: &mut *ram, l };
    let movement = m.f(Field::Movement, x);
    let at = if !rising {
        // `$76F1`: falling
        let delta = if movement & 1 != 0 {
            if m.y(x) >= 0xF6 {
                None
            } else {
                Some(0)
            }
        } else {
            let s = m.rx(l.obj_step, x).wrapping_sub(1);
            m.wx(l.obj_step, x, s);
            if s == m.f(Field::ScriptHi, x) {
                None
            } else {
                Some(m.rx(l.jump_table, s))
            }
        };
        match delta {
            None => At::Flip,
            Some(d) => {
                // `$7711`
                let v = d.wrapping_add(m.y(x)).wrapping_add(m.r(l.spawn_pick));
                m.set_y(x, v);
                landing(&mut m, x)
            }
        }
    } else {
        // `$772A`: rising; `None` = `$7721`, else the rise `$5F`
        let delta = if movement & 1 != 0 {
            let top = if m.f(Field::Kind, x) == 0x7F { 0x58 } else { 0x30 };
            if top >= m.y(x) && m.rx(l.object_state, x) & 0x80 != 0 {
                None
            } else {
                Some(0)
            }
        } else {
            let s = m.rx(l.obj_step, x).wrapping_add(1);
            m.wx(l.obj_step, x, s);
            let d = m.rx(l.jump_table, s);
            m.w(l.jump_delta, d);
            if d & 0x80 != 0 {
                None
            } else {
                Some(d)
            }
        };
        match delta {
            None => At::Flip,
            Some(d) => {
                // `$7754`
                m.w(l.jump_delta, d);
                let v = m.y(x).wrapping_sub(d).wrapping_sub(m.r(l.spawn_pick));
                m.set_y(x, v);
                if movement >= 2 {
                    At::Move
                } else {
                    let a = if m.f(Field::Speed, x) != 0 { 0 } else { m.p(0) | m.p(2) } | m.p(1);
                    let head = a & 1 != 0 || (m.rx(l.obj_behind, x) != 0 && a & 2 == 0);
                    if head && m.rx(l.object_state, x) & 0x80 != 0 {
                        // `$7784`: the ceiling: fall from the table's top
                        m.wx(l.obj_step, x, 0x24);
                        objects::flip(m.ram, l, x);
                    }
                    At::Blocked
                }
            }
        }
    };
    walk(ram, l, x, at)
}

/// `$7792`: a falling jumper on the ground lands (`$02C1` flips, the jump starts again at [`Field::ScriptHi`]); in the
/// air the fall speed index stays above the table's start.
fn landing(m: &mut Mem, x: u8) -> At {
    let l = m.l;
    if m.f(Field::Movement, x) >= 2 {
        return At::Move;
    }
    let ground = if m.f(Field::Speed, x) != 0 { m.p(7) } else { m.p(7) | m.p(8) | m.p(6) };
    if ground == 0 {
        let s = m.rx(l.obj_step, x);
        if s.wrapping_sub(1) == m.f(Field::ScriptHi, x) {
            m.wx(l.obj_step, x, s.wrapping_add(1));
        }
    } else if m.rx(l.object_state, x) & 0x80 == 0 {
        let s = m.f(Field::ScriptHi, x);
        m.wx(l.obj_step, x, s);
        objects::flip(m.ram, l, x);
    }
    At::Blocked
}

/// `$77CC`: the walkers (class 0 beyond the items, classes >= 5 out of phase 0, by [`Field::Movement`]): 0 falls 4
/// pixels a frame without ground below; 1 also turns at the edge of its ground (falls when there is none at all); 2
/// never falls; 3 and more do not even turn at walls. Then the horizontal part ([`walk`]).
fn walker(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    let mv = m.f(Field::Movement, x);
    let at = match mv {
        0 => {
            if m.p(6) | m.p(7) | m.p(8) == 0 {
                fall4(&mut m, x);
            }
            At::Blocked
        }
        2 => At::Blocked,
        1 => {
            if m.p(7) == 0 {
                fall4(&mut m, x);
                At::Blocked
            } else {
                let ahead = if m.rx(l.obj_dir, x) != 0 { m.p(6) } else { m.p(8) };
                if ahead == 0 {
                    objects::turn(m.ram, l, x);
                }
                At::Blocked
            }
        }
        _ => At::Move,
    };
    walk(ram, l, x, at)
}

/// `$77F5`: 4 pixels down.
fn fall4(m: &mut Mem, x: u8) {
    let v = m.y(x).wrapping_add(4);
    m.set_y(x, v);
}

/// `$7721`/`$77FC`/`$780B`: (flip,) turn at a wall in the direction of travel, then move by [`Field::Speed`] in the
/// direction `$0268` (speed bit 7: towards Giana, [`follow_horizontal`]); ends at `$781F`.
fn walk(ram: &mut [u8], l: &C30aLayout, x: u8, at: At) -> ClassEnd {
    if at == At::Flip {
        objects::flip(ram, l, x);
    }
    if at != At::Move {
        let m = Mem { ram: &mut *ram, l };
        let wall = if m.rx(l.obj_dir, x) != 0 { m.p(3) } else { m.p(5) };
        if wall != 0 {
            objects::turn(ram, l, x);
        }
    }
    let mut m = Mem { ram: &mut *ram, l };
    let dir = m.rx(l.obj_dir, x);
    m.w(l.obj_dir_tmp, dir);
    let mut speed = m.f(Field::Speed, x);
    if speed & 0x80 != 0 {
        speed = follow_horizontal(ram, l, x);
    }
    ram[l.obj_dx as usize] = speed & 0x7F;
    objects::move_x(ram, l, x, dir);
    ClassEnd::Probe
}

/// `$86E4`: a jumper with [`Field::ScriptLo`] bit 7 turns towards Giana's height: below her it rises (`$02C1` = 1)
/// unless there is ground under it, above her it falls (`$FF`) unless there is a ceiling (for a sprite behind the
/// background: unless the tile is not solid). At her height, or blocked, the constant speed `$F1` is 0. `$02B8` bit 7
/// (set by [`objects::flip`]) skips it for one frame.
pub fn follow_vertical(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    let clear = |m: &mut Mem| m.wx(l.obj_saved_y, x, 0);
    let stop = |m: &mut Mem| m.w(l.spawn_pick, 0);
    if m.f(Field::ScriptLo, x) & 0x80 == 0 || m.rx(l.obj_saved_y, x) & 0x80 != 0 {
        return clear(&mut m);
    }
    let (gy, oy) = (m.r(l.player_y), m.y(x));
    if gy == oy {
        return stop(&mut m);
    }
    if gy > oy {
        // `$871E`: Giana below
        if m.p(7) != 0 {
            return stop(&mut m);
        }
        if m.rx(l.object_state, x) & 0x80 == 0 {
            return clear(&mut m);
        }
        m.wx(l.object_state, x, 1);
        if m.f(Field::Speed, x) == 0 {
            let f = m.f(Field::FramesA, x);
            m.wx(l.player_frames, x, f);
        }
        return;
    }
    let a = m.p(1);
    if a & 1 != 0 || (m.rx(l.obj_behind, x) != 0 && a & 2 == 0) {
        return stop(&mut m);
    }
    if m.rx(l.object_state, x) & 0x80 == 0 {
        m.wx(l.object_state, x, 0xFF);
        if m.f(Field::Speed, x) == 0 {
            let f = m.f(Field::FramesB, x);
            m.wx(l.player_frames, x, f);
        }
    }
    clear(&mut m);
}

/// `$867E`: a walker with speed bit 7 faces Giana (by x with the high bits; [`Field::Width`] 1 = a wide object, 12
/// pixels more to the right) unless a wall is in the way; returns the speed (`A`), 0 when it stands (blocked, or level
/// with her).
pub fn follow_horizontal(ram: &mut [u8], l: &C30aLayout, x: u8) -> u8 {
    let mut m = Mem { ram, l };
    let msb = m.r(l.sprite_x_msb);
    let (g_hi, o_hi) = (msb & 1 != 0, msb & m.bit(x) != 0);
    // `true` = Giana is to the right (`$86AB`), `false` = left (`$86C5`)
    let right = match (g_hi, o_hi) {
        (false, true) => false,
        (true, false) => true,
        _ => {
            let (gx, ox) = (m.r(l.player_x), m.rx(l.player_x, x));
            if gx < ox {
                false
            } else {
                let mut a = gx - ox;
                let mut left = false;
                if m.f(Field::Width, x) == 1 {
                    if a < 0x0C {
                        left = true;
                    } else {
                        a -= 0x0C;
                    }
                }
                if !left && a == 0 {
                    return 0;
                }
                !left
            }
        }
    };
    if right {
        if m.p(5) != 0 {
            return 0;
        }
        if m.rx(l.obj_dir, x) != 0 {
            m.wx(l.obj_dir, x, 0);
            let fa = m.f(Field::FramesA, x);
            // `STA $80,X; BNE`: the zero flag is still the one of the `CMP` (not equal), so it always branches
            if fa != m.f(Field::FramesB, x) {
                m.wx(l.player_frames, x, fa);
            }
        }
        return m.f(Field::Speed, x);
    }
    if m.p(3) != 0 {
        return 0;
    }
    if m.rx(l.obj_dir, x) == 0 {
        m.wx(l.obj_dir, x, 1);
        let fb = m.f(Field::FramesB, x);
        if fb != m.f(Field::FramesA, x) {
            m.wx(l.player_frames, x, fb);
        }
    }
    m.f(Field::Speed, x)
}

/// `$8418`: one step of the script of object `x` (see the module documentation): the commands at the next entry run
/// when the count `$02C6` has run out, then the entry's move.
pub fn script(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram: &mut *ram, l };
    let (lo, hi) = (m.f(Field::ScriptLo, x), m.f(Field::ScriptHi, x));
    m.w(l.obj_script, lo);
    m.w(l.obj_script + 1, hi);
    let mut y = m.rx(l.obj_script_pos, x);
    if m.rx(l.obj_script_byte, x) != 0 {
        let c = m.rx(l.obj_script_byte, x).wrapping_sub(1);
        m.wx(l.obj_script_byte, x, c);
    } else {
        y = y.wrapping_add(3);
        m.wx(l.obj_script_pos, x, y);
        loop {
            // `$8431`
            let c = m.ind(l.obj_script, y);
            m.wx(l.obj_script_byte, x, c);
            match c {
                0..=0xFA => break,
                0xFB => {
                    // `$83ED`: the object becomes type `t` (a new descriptor on its own slot)
                    m.w(l.spawn_mode, 0x80);
                    let t = m.ind(l.obj_script, y.wrapping_add(1)) << 1;
                    let (dl, dh) = (m.rx(l.descriptor_table, t), m.rx(l.descriptor_table + 1, t));
                    m.w(l.descriptor_ptr, dl);
                    m.w(l.descriptor_ptr + 1, dh);
                    m.w(l.group_count, 0xFF);
                    objects::instantiate(ram, vic, l, x);
                    return;
                }
                0xFC => {
                    // `$8408`: the frame moves on, the next entry at once
                    m.wx(l.obj_anim_timer, x, 0x7F);
                    y = y.wrapping_add(1);
                    let f = m.rx(l.player_frames, x).wrapping_add(m.ind(l.obj_script, y));
                    m.wx(l.player_frames, x, f);
                    y = y.wrapping_add(2);
                    m.wx(l.obj_script_pos, x, y);
                }
                0xFD => return spawn_child(ram, vic, l, x, y),
                0xFE => {
                    let lo = m.ind(l.obj_script, y.wrapping_add(1));
                    let hi = m.ind(l.obj_script, y.wrapping_add(2));
                    m.wf(Field::ScriptHi, x, hi);
                    m.w(l.obj_script + 1, hi);
                    m.wf(Field::ScriptLo, x, lo);
                    m.w(l.obj_script, lo);
                    m.wx(l.obj_script_pos, x, 0);
                    y = 0;
                }
                0xFF => {
                    m.wx(l.obj_script_pos, x, 0);
                    y = 0;
                }
            }
        }
    }
    // `$8466`: the entry's move
    y = y.wrapping_add(1);
    let dx = m.ind(l.obj_script, y);
    let mut left = dx & 0x80 != 0;
    let speed = if left { (dx ^ 0xFF).wrapping_add(1) } else { dx };
    m.wf(Field::Speed, x, speed);
    if m.f(Field::Movement, x) & 0x80 != 0 {
        // `$84B3`: towards Giana
        let msb = m.r(l.sprite_x_msb);
        let target = ((msb & 1) << 7 | m.r(l.player_x) >> 1).wrapping_add(7);
        m.w(l.follow_x, target);
        let half = u8::from(msb & m.bit(x) != 0) << 7 | m.rx(l.player_x, x) >> 1;
        left = !(half >= 0xB0 || half < target);
    }
    let ox = m.rx(l.player_x, x);
    let carry = if left {
        m.wx(l.obj_dir, x, 1);
        m.wx(l.player_x, x, ox.wrapping_sub(speed));
        ox < speed
    } else {
        m.wx(l.obj_dir, x, 0);
        m.wx(l.player_x, x, ox.wrapping_add(speed));
        u16::from(ox) + u16::from(speed) > 0xFF
    };
    if carry {
        let v = m.r(l.sprite_x_msb) ^ m.bit(x);
        m.w(l.sprite_x_msb, v);
    }
    let dy = m.ind(l.obj_script, y.wrapping_add(1));
    m.wx(l.object_state, x, dy);
    let v = dy.wrapping_add(m.y(x));
    m.set_y(x, v);
}

/// `$84D3`: script command `$FD dir t`: a new object of type `t` ([`objects::spawn_type`], [`objects::spawn_member`]
/// with `$F6` = `$80`) at the place of object `x`, facing `dir`.
fn spawn_child(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8, y: u8) {
    let mut m = Mem { ram: &mut *ram, l };
    m.w(l.script_slot, x);
    m.w(l.spawn_mode, 0x80);
    let ox = m.rx(l.player_x, x);
    m.w(l.group_leader_tmp, ox);
    let hi = m.r(l.sprite_x_msb) & m.bit(x);
    m.w(l.child_msb, hi);
    let oy = m.y(x);
    m.w(l.spawn_param, oy);
    m.wx(l.obj_script_byte, x, 0);
    let dir = m.ind(l.obj_script, y.wrapping_add(1));
    m.w(l.child_dir, dir);
    let t = m.ind(l.obj_script, y.wrapping_add(2));
    objects::spawn_type(ram, l, t);
    let (_, n) = objects::spawn_member(ram, vic, l);
    let mut m = Mem { ram, l };
    let msb = m.r(l.sprite_x_msb);
    let msb = if m.r(l.child_msb) != 0 { msb | m.bit(n) } else { msb & m.mask(n) };
    m.w(l.sprite_x_msb, msb);
    let v = m.r(l.group_leader_tmp);
    m.wx(l.player_x, n, v);
    let v = m.r(l.spawn_param);
    m.set_y(n, v);
    let v = m.r(l.sprite_enable) | m.bit(n);
    m.w(l.sprite_enable, v);
    let v = m.r(l.child_dir);
    m.wx(l.obj_dir, n, v);
}

/// `$7878` (route `$7647`, then [`ClassEnd::Next`]): class 4 walks (by [`Field::Speed`], animated) while the tile at its
/// middle one pixel higher is not deadly; on it, it enters phase 1: frames `$FA..$FE`, one every 3 frames, then the
/// sprite goes off.
pub fn class4(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    if m.rx(l.obj_phase, x) == 0 {
        let oy = m.y(x);
        m.set_y(x, oy.wrapping_sub(1));
        objects::probe(m.ram, l, x);
        let oy = m.y(x);
        m.set_y(x, oy.wrapping_add(1));
        if m.p(4) != 0 {
            let p = m.rx(l.obj_phase, x).wrapping_add(1);
            m.wx(l.obj_phase, x, p);
            return ClassEnd::Next;
        }
        let s = m.f(Field::Speed, x);
        m.w(l.obj_dx, s);
        let d = m.rx(l.obj_dir, x);
        objects::move_x(ram, l, x, d);
        objects::animate(ram, l, x);
        return ClassEnd::Next;
    }
    let f = m.rx(l.player_frames, x);
    if f >= 0xFB {
        let t = m.rx(l.obj_anim_timer, x).wrapping_sub(1);
        m.wx(l.obj_anim_timer, x, t);
        if t & 0x80 == 0 {
            return ClassEnd::Next;
        }
    } else {
        m.wx(l.player_frames, x, 0xFA);
    }
    m.wx(l.obj_anim_timer, x, 2);
    let v = m.rx(l.player_frames, x).wrapping_add(1);
    m.wx(l.player_frames, x, v);
    if f >= 0xFE {
        let v = m.r(l.sprite_enable) & m.mask(x);
        m.w(l.sprite_enable, v);
    }
    ClassEnd::Next
}

/// `$78BE` (route `$7652`, entered by `JMP`: its `RTS` ends the update loop, [`ClassEnd::Return`]): classes >= 5 in
/// phase 0 move up or down by [`Field::ScriptLo`] (`$02C1` negative/positive) until a tile stops them, then sideways by
/// [`Field::Speed`] until a wall, then up or down again; the probe offsets `$7B19`/`$7B36` follow the direction.
pub fn floating(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    objects::animate(ram, l, x);
    objects::probe(ram, l, x);
    let mut m = Mem { ram, l };
    let s = m.rx(l.object_state, x);
    // `true` = `$790E` (stop the vertical move), `false` = `$792D` (start one)
    let vertical_end = if s != 0 {
        let blocked = if s & 0x80 != 0 { (m.p(2) | m.p(1)) & 1 != 0 } else { m.p(8) | m.p(7) != 0 };
        if !blocked {
            let d = m.f(Field::ScriptLo, x);
            let oy = m.y(x);
            m.set_y(x, if s & 0x80 != 0 { oy.wrapping_sub(d) } else { oy.wrapping_add(d) });
            return ClassEnd::Return;
        }
        true
    } else {
        let s = m.f(Field::Speed, x);
        m.w(l.obj_dx, s);
        let d = m.rx(l.obj_dir, x);
        m.w(l.obj_dir_tmp, d);
        let wall = if d == 0 {
            m.p(5)
        } else if m.r(l.scroll_column) == 0 && m.r(l.sprite_x_msb) & 0x80 == 0 && m.r(l.player_x + 7) < 0x1D {
            1
        } else {
            m.p(3)
        };
        if wall == 0 {
            objects::move_x(m.ram, l, x, d);
            return ClassEnd::Return;
        }
        false
    };
    if vertical_end {
        m.wx(l.object_state, x, 0);
        let (d, off) = if m.p(5) != 0 { (1, 0x11) } else { (0, 0x17) };
        m.wx(l.obj_dir, x, d);
        m.w(l.probe_x_offset, off);
    } else {
        let (s, off) = if (m.p(2) | m.p(1)) & 1 != 0 { (1, 0x38) } else { (0xFF, 0x33) };
        m.wx(l.object_state, x, s);
        m.w(l.probe_y_offset, off);
    }
    ClassEnd::Return
}

/// `$7964` (route `$760D`, then [`ClassEnd::Next`]): negative classes rise out of the ground one pixel every 3 frames
/// while animating up to [`Field::FramesAEnd`], then jump (down the jump table from `$23`, with the probe) until they
/// land or wrap past the bottom, then sink back animating to [`Field::FramesBEnd`] and go off (phase counting up to 0:
/// back to the saved y and turned). `$02BC` set: a jump 3 to the right and 4 down first.
pub fn rising(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    if m.rx(l.anim_phase, x) != 0 {
        // `$794A`
        let ox = m.rx(l.player_x, x);
        m.wx(l.player_x, x, ox.wrapping_add(3));
        let c = u8::from(u16::from(ox) + 3 > 0xFF);
        let v = m.y(x).wrapping_add(4).wrapping_add(c);
        m.set_y(x, v);
        m.wx(l.obj_saved_y, x, v);
        return rising_reset(ram, l, x);
    }
    if m.rx(l.obj_phase, x) == 0 {
        if m.f(Field::Movement, x) == 0 {
            let s = m.f(Field::Speed, x).wrapping_sub(1);
            m.wf(Field::Speed, x, s);
            if s & 0x80 == 0 {
                return ClassEnd::Next;
            }
            m.wf(Field::Speed, x, 2);
            let v = m.y(x).wrapping_add(1);
            m.set_y(x, v);
            objects::animate(m.ram, l, x);
            if m.rx(l.player_frames, x) >= m.f(Field::FramesAEnd, x) {
                let v = m.f(Field::Movement, x).wrapping_add(1);
                m.wf(Field::Movement, x, v);
                m.wx(l.obj_step, x, 0x23);
            }
            return ClassEnd::Next;
        }
        // `$7992`: the jump, until it lands or wraps past the bottom
        objects::probe(m.ram, l, x);
        if m.p(7) == 0 {
            // `BEQ` over the `DEY`: the index stops at 0
            let s = m.rx(l.obj_step, x).saturating_sub(1);
            m.wx(l.obj_step, x, s);
            let d = m.rx(l.jump_table, s);
            let oy = m.y(x);
            m.set_y(x, oy.wrapping_add(d));
            if u16::from(oy) + u16::from(d) <= 0xFF {
                return ClassEnd::Next;
            }
        }
        // `$79AD`
        let p = m.rx(l.obj_phase, x).wrapping_sub(1);
        m.wx(l.obj_phase, x, p);
        if p != 0 {
            objects::turn(ram, l, x);
            return ClassEnd::Next;
        }
    }
    // `$79B2`
    objects::animate(ram, l, x);
    let mut m = Mem { ram: &mut *ram, l };
    if m.rx(l.player_frames, x) < m.f(Field::FramesBEnd, x) {
        return ClassEnd::Next;
    }
    let p = m.rx(l.obj_phase, x).wrapping_add(1);
    m.wx(l.obj_phase, x, p);
    if p != 0 {
        let v = m.r(l.sprite_enable) & m.mask(x);
        m.w(l.sprite_enable, v);
        return ClassEnd::Next;
    }
    let v = m.rx(l.obj_saved_y, x);
    m.set_y(x, v);
    if v != 0 {
        return rising_reset(ram, l, x);
    }
    objects::turn(ram, l, x);
    ClassEnd::Next
}

/// `$795A`: back to the start (`$02BC` and the movement 0), turned.
fn rising_reset(ram: &mut [u8], l: &C30aLayout, x: u8) -> ClassEnd {
    let mut m = Mem { ram: &mut *ram, l };
    m.wx(l.anim_phase, x, 0);
    m.wf(Field::Movement, x, 0);
    objects::turn(ram, l, x);
    ClassEnd::Next
}
