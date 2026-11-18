//! Contacts, pickups, power-ups and Giana's shot (work package O3 of `docs/rom-hack-family-c.md`, 7.4): native ports of
//! the object code that decides what touching an object does, the power-up levels `$FD`, the shot (sprite 3) and Giana
//! landing on an object. Verified call by call, frame by frame and on random states against the original running in the
//! oracle machine (`crates/giana-c30-oracle/tests/pickups.rs`).
//!
//! Same state model as [`crate::objects`]: the game's own 64 KB image at the addresses of [`C30aLayout`] and the VIC
//! register image `vic` (as written; a colour register reads back with the high nibble set). The two routines that read
//! the sprite-sprite collisions get the byte the game read from `$D01E` (`coll`; the read clears it on the VIC).
//!
//! | original | function | what |
//! |---|---|---|
//! | `$7CB7..$7D8E` | [`contact`] | per frame (`$7329`): the shot against objects, then Giana against objects: enemies, pickups |
//! | `$7D8F..$7D9D` | [`smash`] | an object hit by the power-up level 6 flash |
//! | `$7D9E..$7DBD` | [`hit`] | an enemy is hit: its hit frame, phase 5, sound 5, its score (`$8BDD`) |
//! | `$7DBE..$7E03` | [`step_contact`] | during a column step (`$72F3`): Giana touching a platform object (kind `$7F`) |
//! | `$7E04..$7E6C` | [`touching`] | the next object (from slot `x` down) that touches Giana; lands her on a platform |
//! | `$7E6D..$7F10` | [`shot_target`] | the object the shot touches; a bouncing shot turns at a platform |
//! | `$7F11..$7F80` | [`shot_fire`] | fire with power level >= 2 starts the shot (level >= 4: [`shot_aim`]) |
//! | `$7F81..$8145` | [`shot_move`] | the shot flies, bounces (level >= 3), homes in (level >= 4), explodes |
//! | `$8146..$8187` | [`shot_aim`] | the first live enemy in the facing direction becomes the homing shot's target |
//! | `$828C..$83EC` (with `$2C9F..$2D47`) | [`land`] | Giana meets an object from above or the side: standing on it, pushed, carried |
//! | `$72CC..$72E9` | [`power_flash`] | one frame of the power-up level 6 flash (`$02DF` = 4..1) |
//!
//! # Power-up levels (`$FD`)
//!
//! A pickup (an object of kind `$12`) with parameter (`$0227`) `0..$7F` adds a level; past 7 the level goes back to 4 and
//! Giana gets a life. Parameter `$FD` = a life, `$FE`/`$FF` = the swimming flag `$19` (water levels), `$80..$FC` = the
//! stage ends (`$98 = $20 = $02D6`, `$21 = 0`). Every pickup scores 50 and sets `$02DB`. Levels: 2 = the shot, 3 = the
//! shot bounces off walls and platforms and can be caught again, 4 = it homes in on an enemy, 5 and 6 are items used
//! with the space key (`$7222`, package F: 5 freezes the objects for 255 frames, 6 flashes the screen and kills every
//! object, [`power_flash`]); both set the level back to 4. A hit takes a level away (`$4840`).

use crate::blocks::{adc_decimal, extra_life, score_add};
use crate::layout::C30aLayout;
use crate::objects::{self, Field, VIC_COLOR, VIC_MULTICOLOR};

/// The shot's slot (sprite 3).
pub const SHOT: u8 = 3;
/// `$D021`, the background colour (the flash of power-up level 6).
pub const VIC_BACKGROUND: usize = 0x21;

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
        self.ram[f.at(self.l, 0).wrapping_add(u16::from(x)) as usize]
    }
    fn wf(&mut self, f: Field, x: u8, v: u8) {
        let a = f.at(self.l, 0).wrapping_add(u16::from(x));
        self.w(a, v);
    }
    fn bit(&self, x: u8) -> u8 {
        self.rx(self.l.bit_set, x)
    }
    /// The object's x high bit as 0/1 (`AND $4C37,X` / `BEQ` / `LDA #1`), stored in `$66`.
    fn obj_msb(&mut self, x: u8) -> u8 {
        let hi = u8::from(self.r(self.l.sprite_x_msb) & self.bit(x) != 0);
        self.w(self.l.contact_msb, hi);
        hi
    }
    /// The group leader of slot `x`, or `x` (`LDA $02B4,X` / `BEQ` / `TAX`).
    fn leader_or(&self, x: u8) -> u8 {
        match self.rx(self.l.group_leader, x) {
            0 => x,
            g => g,
        }
    }
}

/// The N flag of `CMP #m` with `a`.
fn minus(a: u8, m: u8) -> bool {
    a.wrapping_sub(m) & 0x80 != 0
}

/// `SBC` in binary mode: the result and the carry (no borrow).
fn sbc(a: u8, m: u8, c: bool) -> (u8, bool) {
    let r = i16::from(a) - i16::from(m) - i16::from(!c);
    (r as u8, r >= 0)
}

/// `ADC` in binary mode: the result and the carry.
fn adc(a: u8, m: u8, c: bool) -> (u8, bool) {
    let r = u16::from(a) + u16::from(m) + u16::from(c);
    (r as u8, r > 0xFF)
}

/// Reads VIC register `r` as the CPU sees it (colour registers with the high nibble set, unused ones `$FF`).
fn vic_read(vic: &[u8], r: usize) -> u8 {
    match r & 0x3F {
        r @ 0x20..=0x2E => vic[r] | 0xF0,
        0x2F..=0x3F => 0xFF,
        r => vic[r],
    }
}

/// The (horizontal) distance of the two 9-bit x positions `a` (high bit `ahi`) minus `b` (`bhi`): `SEC`, `SBC` on the
/// low bytes, then on the high bits: the low byte and the high part with its carry.
fn x_diff(a: u8, ahi: u8, b: u8, bhi: u8) -> (u8, u8, bool) {
    let (lo, c) = sbc(a, b, true);
    let (hi, c) = sbc(ahi, bhi, c);
    (lo, hi, c)
}

/// How [`contact`] ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactEnd {
    /// `RTS`
    Return,
    /// `JMP $3F26`: Giana dies (an enemy touched her from the side or below, or a deadly object); the registers as the
    /// original has them there
    Die { a: u8, x: u8, y: u8 },
}

/// `$7CB7`: what the contacts of this frame do (`coll` = the byte read from `$D01E`). First the shot (sprite 3): against
/// an object ([`shot_target`]) it hits an enemy of kind 0 ([`hit`]), counts a negative kind (a boss's hit points) up and
/// turns that sprite multicolour, or bounces off anything else (colour 2); the shot alone touching Giana is caught when
/// it bounced. Then the objects touching Giana from slot 7 down ([`touching`]; a member stands for its group leader):
/// kind 0 is an enemy, which she kills by falling on it ([`hit`]; when swimming only from at least 4 pixels above) and
/// which kills her otherwise; kind 1 and negative kinds kill her; kind `$12` is a pickup (see the module documentation);
/// other kinds do nothing.
pub fn contact(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, coll: u8) -> ContactEnd {
    let mut m = Mem { ram, l };
    let a = coll & m.r(l.sprite_enable) & 0xF8;
    m.w(l.contact_bits, a);
    m.w(l.contact_mask, a);
    if a == 0 {
        return ContactEnd::Return;
    }
    if a == 8 {
        shot_caught(&mut m, vic);
        return ContactEnd::Return;
    }
    if a & 8 != 0 {
        let hi = (m.r(l.sprite_x_msb) >> 3) & 1;
        m.w(l.contact_shot_msb, hi);
        if let Some(x) = shot_target(m.ram, l) {
            if m.r(l.object_frames) < 0xFB {
                m.w(l.object_frames, 0xFF);
                let x = m.leader_or(x);
                let mc = vic[VIC_MULTICOLOR] | 8;
                let kind = m.f(Field::Kind, x);
                if kind == 0 {
                    vic[VIC_MULTICOLOR] = mc;
                    hit(m.ram, l, x);
                    return ContactEnd::Return;
                }
                if kind & 0x80 != 0 {
                    m.wf(Field::Kind, x, kind.wrapping_add(1));
                    vic[VIC_MULTICOLOR] = mc;
                } else {
                    vic[VIC_COLOR + SHOT as usize] = 2;
                }
            }
            let b = m.r(l.contact_bits) & 0xF0;
            m.w(l.contact_bits, b);
        }
    }
    let mut x = 7;
    loop {
        let Some(found) = touching(m.ram, &*vic, l, x).0 else { return ContactEnd::Return };
        if found == SHOT {
            shot_caught(&mut m, vic);
            return ContactEnd::Return;
        }
        // `Y` as `$7E04` left it (`LDY $025E,X`, `INY`)
        let y = m.f(Field::Width, found).wrapping_add(1);
        x = m.leader_or(found);
        let kind = m.f(Field::Kind, x);
        if kind == 0 {
            return stomp(&mut m, x, y);
        }
        if minus(kind, 1) || kind == 1 {
            return ContactEnd::Die { a: kind, x, y };
        }
        if kind == 0x12 {
            pickup(&mut m, x);
            return ContactEnd::Return;
        }
    }
}

/// `$7CC8`: the shot touches Giana: caught if it bounced (`$CF`): colour `$FF`, frame `$FF`.
fn shot_caught(m: &mut Mem, vic: &mut [u8]) {
    let l = m.l;
    let c = m.r(l.shot_bounce);
    if c != 0 {
        vic[VIC_COLOR + SHOT as usize] = 0xFF;
        m.w(l.object_frames, 0xFF);
        m.w(l.shot_bounce, c.wrapping_sub(1));
    }
}

/// `$7D7B`: an enemy touches Giana: she kills it when falling (and, swimming, from 4 pixels above it), else it kills her.
fn stomp(m: &mut Mem, x: u8, y: u8) -> ContactEnd {
    let l = m.l;
    let vd = m.r(l.vert_dir);
    if vd & 0x80 == 0 {
        return ContactEnd::Die { a: vd, x, y };
    }
    if m.r(l.swim) == 0 {
        hit(m.ram, l, x);
        return ContactEnd::Return;
    }
    let (a, c) = sbc(m.rx(l.player_y, x), 4, true);
    let (a, c) = sbc(a, m.r(l.player_y), c);
    if c {
        hit(m.ram, l, x);
        ContactEnd::Return
    } else {
        ContactEnd::Die { a, x, y }
    }
}

/// `$7D3C`: Giana collects the pickup in slot `x` (see the module documentation), the slot goes off.
fn pickup(m: &mut Mem, x: u8) {
    let l = m.l;
    let p = m.f(Field::Score, x);
    let sound = if p & 0x80 == 0 {
        let level = m.r(l.power).wrapping_add(1);
        m.w(l.power, level);
        if level < 8 {
            3
        } else {
            m.w(l.power, 4);
            extra_life(m.ram, l)
        }
    } else if p == 0xFD {
        extra_life(m.ram, l)
    } else if p > 0xFD {
        m.w(l.swim, p);
        3
    } else {
        m.w(l.stage_end, p);
        m.w(l.game_state, p);
        m.w(l.pickup_end, p);
        m.w(l.scroll_column, 0);
        3
    };
    m.w(l.effect, sound);
    m.w(l.pickup_taken, 1);
    let e = m.r(l.sprite_enable) & m.rx(l.bit_clear, x);
    m.w(l.sprite_enable, e);
    score_add(m.ram, l, 0x50);
}

/// `$7D9E`: the enemy in slot `x` is hit (by the shot, Giana falling on it, a deadly tile `$784C` or the flash): its hit
/// frame of its direction, phase 5 (dying), sound 5, and its score (BCD, `$0227` low and `$0254` high byte) is added.
pub fn hit(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    let frame = if m.rx(l.obj_dir, x) != 0 { m.f(Field::HitFrameB, x) } else { m.f(Field::HitFrameA, x) };
    m.wx(l.player_frames, x, frame);
    m.wx(l.obj_phase, x, 5);
    m.w(l.effect, 5);
    let (lo, hi) = (m.f(Field::Score, x), m.f(Field::ScoreHigh, x));
    score_add_word(m.ram, l, lo, hi);
}

/// `$8BDD` with `Y`: adds the BCD amount `hi:lo` to the score, saturating at 999999 (B's [`score_add`] is `Y = 0`).
pub fn score_add_word(ram: &mut [u8], l: &C30aLayout, lo: u8, hi: u8) {
    let s = l.score as usize;
    let (b0, c) = adc_decimal(ram[s], lo, false);
    let (b1, c) = adc_decimal(ram[s + 1], hi, c);
    let (b2, c) = adc_decimal(ram[s + 2], 0, c);
    let bytes = if c { [0x99; 3] } else { [b0, b1, b2] };
    ram[s..s + 3].copy_from_slice(&bytes);
}

/// `$7D8F`: the flash of power-up level 6 hits slot `x` (its group leader): everything but a negative kind of `-3` or
/// lower (`kind + 3` negative: a boss with hit points left) is [`hit`].
pub fn smash(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let m = Mem { ram, l };
    let x = m.leader_or(x);
    if m.f(Field::Kind, x).wrapping_add(3) & 0x80 == 0 {
        hit(m.ram, l, x);
    }
}

/// `$72CC`: one frame of the power-up level 6 flash, `y` = `$02DF` (4..1, the frame driver `$72EA` runs this instead of
/// the objects while it is set): the background colour of the frame from `$4C47` (the saved one last), and the object in
/// slot `y + 3` is [`smash`]ed unless its kind is 2 or more (signed); sound `$0D`.
pub fn power_flash(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, y: u8) {
    let mut m = Mem { ram, l };
    let y = y.wrapping_sub(1);
    m.w(l.power_flash, y);
    vic[VIC_BACKGROUND] = m.rx(l.power_colours, y);
    let x = y.wrapping_add(4);
    if minus(m.f(Field::Kind, x), 2) {
        smash(m.ram, l, x);
    }
    m.w(l.effect, 0x0D);
}

/// `$7E04`: from slot `x` down to 3, the next object whose bit in `$60` is set (shifted out one per slot), whose phase is
/// 0 and that touches Giana: horizontally within 16 pixels on her left (the difference is `Giana - object`), vertically
/// within 16 above or below her (an object with negative width only below her feet) while she is at least 16 pixels down
/// the screen. A platform (kind `$7F`) within 20 pixels is not a contact: Giana [`land`]s on it. Returns the slot (the
/// original's carry set) and the `X` the original leaves (the slot, or 2 when none touches).
pub fn touching(ram: &mut [u8], vic: &[u8], l: &C30aLayout, mut x: u8) -> (Option<u8>, u8) {
    loop {
        let mut m = Mem { ram: &mut *ram, l };
        let bits = m.r(l.contact_bits);
        m.w(l.contact_bits, bits << 1);
        if bits & 0x80 != 0 && m.rx(l.obj_phase, x) == 0 {
            let ohi = m.obj_msb(x);
            let (lo, hi, c) = x_diff(m.r(l.player_x), m.r(l.sprite_x_msb) & 1, m.rx(l.player_x, x), ohi);
            let dist = if c {
                (hi == 0).then_some(lo)
            } else if hi == 0xFF {
                Some(lo ^ 0xFF).map(|v| v.wrapping_add(1)).filter(|&v| v != 0)
            } else {
                None
            };
            if let Some(d) = dist {
                if m.f(Field::Kind, x) == 0x7F {
                    if d < 0x14 {
                        land(ram, vic, l, x);
                    }
                } else if d < 0x10 {
                    let gy = m.r(l.player_y);
                    let oy = m.rx(l.player_y, x);
                    if gy >= 0x10 {
                        if m.f(Field::Width, x).wrapping_add(1) & 0x80 != 0 {
                            // `$7E68`: below her only
                            if gy >= oy {
                                return (Some(x), x);
                            }
                        } else {
                            let dy = gy.wrapping_sub(oy);
                            if !minus(dy, 0xF0) && minus(dy, 0x10) {
                                return (Some(x), x);
                            }
                        }
                    }
                }
            }
        }
        x = x.wrapping_sub(1);
        if x < 3 {
            return (None, x);
        }
    }
}

/// `$7E6D`: from slot 7 down to 4, the object whose bit in `$6D` is set, whose phase is 0 and that the shot touches
/// (horizontally `shot - object` in `0..16`): an object of kind < 2 (signed) within `-20..8` pixels vertically (an object
/// with negative width: below the shot's top), or a platform (kind `$7F`): the shot bounces off its top or sides
/// (`$CE`, `$CD`, `$CC`, `$CF` by the power level; level < 3 and level >= 4 with a target explode there instead).
pub fn shot_target(ram: &mut [u8], l: &C30aLayout) -> Option<u8> {
    let mut m = Mem { ram, l };
    let mut x = 7u8;
    loop {
        let bits = m.r(l.contact_mask);
        m.w(l.contact_mask, bits << 1);
        if let Some(found) = shot_target_slot(&mut m, x, bits) {
            if found {
                return Some(x);
            }
        }
        x = x.wrapping_sub(1);
        if x < 4 {
            return None;
        }
    }
}

/// One slot of [`shot_target`]: `Some(true)` = found, `Some(false)`/`None` = next slot.
fn shot_target_slot(m: &mut Mem, x: u8, bits: u8) -> Option<bool> {
    let l = m.l;
    if bits & 0x80 == 0 || m.rx(l.obj_phase, x) != 0 {
        return None;
    }
    let ohi = m.obj_msb(x);
    let (lo, hi, c) = x_diff(m.r(l.object_sprite_x), m.r(l.contact_shot_msb), m.rx(l.player_x, x), ohi);
    m.w(l.spawn_pick, hi);
    let near = if c { hi == 0 && lo < 0x10 } else { hi == 0xFF && lo >= 0xF1 };
    if !near {
        return None;
    }
    let kind = m.f(Field::Kind, x);
    let sy = m.r(l.object_sprite_y);
    let oy = m.rx(l.player_y, x);
    if minus(kind, 2) {
        if m.f(Field::Width, x).wrapping_add(1) & 0x80 != 0 {
            // `$7EC9`: `ADC #8` with the carry of `CMP #$02`
            let (a, c) = adc(sy, 8, kind >= 2);
            return Some(sbc(a, oy, c).1);
        }
        let d = sy.wrapping_sub(oy);
        return Some(!minus(d, 0xEC) && minus(d, 8));
    }
    if kind != 0x7F {
        return None;
    }
    // `$7ED0`: a platform
    let d = sy.wrapping_sub(oy);
    if !minus(d, 4) {
        return None;
    }
    if minus(d, 0xF4) {
        if !minus(d, 0xF0) {
            m.w(l.shot_vstep, 0x0E);
            m.w(l.shot_vdir, 1);
        }
        return None;
    }
    let f1 = m.r(l.spawn_pick);
    let away = if m.r(l.shot_dir) != 0 { f1 & 0x80 != 0 } else { f1 & 0x80 == 0 };
    if away {
        return None;
    }
    let level = m.r(l.power);
    if level < 3 || level != 3 && m.r(l.shot_target) != 0 {
        return Some(true);
    }
    let d = m.r(l.shot_dir) ^ 1;
    m.w(l.shot_dir, d);
    m.w(l.shot_bounce, 1);
    None
}

/// `$7DBE`: during a column step (the logic runs only this and [`land`]): for the platforms (kind `$7F`) among the objects
/// in `coll & $F0` (`$D01E`), slots 7..4, Giana [`land`]s on one that is horizontally within `-19..20` pixels.
pub fn step_contact(ram: &mut [u8], vic: &[u8], l: &C30aLayout, coll: u8) {
    let mut m = Mem { ram, l };
    m.w(l.contact_mask, coll & 0xF0);
    let mut x = 7u8;
    loop {
        let bits = m.r(l.contact_mask);
        m.w(l.contact_mask, bits << 1);
        if bits & 0x80 != 0 && m.f(Field::Kind, x) == 0x7F {
            let ohi = m.obj_msb(x);
            let (lo, hi, c) = x_diff(m.r(l.player_x), m.r(l.sprite_x_msb) & 1, m.rx(l.player_x, x), ohi);
            let near = if c { hi == 0 && lo < 0x14 } else { hi == 0xFF && lo >= 0xED };
            if near {
                land(m.ram, vic, l, x);
            }
        }
        x = x.wrapping_sub(1);
        if x < 4 {
            return;
        }
    }
}

/// `$828C`: Giana meets the object in slot `x` (a platform from [`touching`]/[`step_contact`]). By the vertical
/// distance and her movement: she stands on it (`$22`, [`objects::near`], `$17 = 0`; an object with a negative
/// parameter starts the stage-end lift or turns into a walker, `$83C9`), is carried down with it (a jumper along the
/// jump table, others by `$02C1`), bumps her head on it from below ([`fall_from_head`]), or is pushed sideways by it or
/// stopped by it (`$8339`). `X` stays the slot.
pub fn land(ram: &mut [u8], vic: &[u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram: &mut *ram, l };
    let d = m.rx(l.player_y, x).wrapping_sub(m.r(l.player_y));
    // `LDY $D027,X` / `INY`: a light grey (`$F`) object is 5 pixels taller
    if vic_read(vic, VIC_COLOR + x as usize) == 0xFF {
        if !minus(d, 0x17) {
            return;
        }
    } else if !minus(d, 0x12) {
        return;
    }
    if minus(d, 0xEB) {
        return;
    }
    m.w(l.land_slot, x);
    #[derive(Clone, Copy)]
    enum At {
        B8,
        Bc,
        C0,
        C4,
        E6,
    }
    let state = m.rx(l.object_state, x);
    let vd = m.r(l.vert_dir);
    let mut at = if m.r(l.bounce) & 0x80 != 0 {
        At::Bc
    } else if vd == 0 {
        At::C0
    } else if vd & 0x80 != 0 {
        At::B8
    } else if state & 0x80 != 0 {
        At::C0
    } else {
        At::E6
    };
    loop {
        at = match at {
            At::B8 => {
                if minus(d, 6) {
                    At::Bc
                } else {
                    At::C4
                }
            }
            At::Bc => {
                if minus(d, 0x0B) {
                    At::C0
                } else {
                    At::C4
                }
            }
            At::C0 => {
                if minus(d, 0x0E) {
                    At::E6
                } else {
                    At::C4
                }
            }
            At::C4 => {
                // `$82C4`: stands on it, unless the cell at her middle is solid and the object is not falling
                if m.r(l.probe_props + 1) & 1 != 0 && m.rx(l.object_state, x) & 0x80 == 0 {
                    return;
                }
                m.w(l.object_index, x);
                objects::near(m.ram, l, x);
                m.w(l.vert_dir, 0);
                let p = m.f(Field::Score, x);
                if p & 0x80 != 0 {
                    ride_trigger(&mut m, x, p);
                }
                return;
            }
            At::E6 => break,
        };
    }
    // `$82E6`
    let side = if m.f(Field::Group, x) != 0 { !minus(d, 0xFD) } else { !minus(d, 0xFA) };
    if side {
        side_push(&mut m, x, d);
        return;
    }
    // `$82EF`
    if m.r(l.bounce) != 0 || m.r(l.vert_dir) & 0x80 != 0 {
        carried(&mut m, x);
        return;
    }
    let s = m.rx(l.object_state, x);
    if s & 0x80 == 0 && s != 0 {
        let v = m.f(Field::ScriptHi, x);
        m.wx(l.obj_step, x, v);
        objects::flip(m.ram, l, x);
    }
    head_hit(m.ram, l);
}

/// `$8307`: when Giana is moving vertically, she falls from here on (`$2C9F`).
fn head_hit(ram: &mut [u8], l: &C30aLayout) {
    if ram[l.vert_dir as usize] != 0 {
        fall_from_head(ram, l);
    }
}

/// `$830E`: Giana falling or bouncing onto a falling object is carried down with it.
fn carried(m: &mut Mem, x: u8) {
    let l = m.l;
    let s = m.rx(l.object_state, x);
    if s & 0x80 != 0 {
        return;
    }
    if m.r(l.bounce) != 0 {
        m.w(l.bounce, 0xFF);
    }
    m.w(l.vert_dir, 0xFF);
    let y = if m.f(Field::Class, x) == 1 {
        let (a, c) = adc(m.rx(l.jump_table, m.rx(l.obj_step, x)), m.f(Field::ScriptLo, x), false);
        adc(a, m.r(l.player_y), c).0
    } else {
        s.wrapping_add(m.r(l.player_y))
    };
    m.w(l.player_y, y);
}

/// `$8339`: the object is beside Giana: it pushes her (moving away from it stops her walking), or she pushes against it
/// (the object's direction is set by the solid cells beside her).
fn side_push(m: &mut Mem, x: u8, d: u8) {
    let l = m.l;
    let e = m.rx(l.player_x, x).wrapping_sub(m.r(l.player_x));
    if !minus(e, 0x0B) {
        // `$838B`: the object is right of her
        if m.r(l.walk_dir) & 0x40 != 0 {
            m.w(l.walk_dir, 0);
            m.w(l.walk_scroll, 0);
            m.w(l.speed, 0);
        }
        if m.r(l.probe_props + 3) & 1 != 0 || m.r(l.sprite_x_msb) & 1 == 0 && m.r(l.player_x) < 0x1E {
            m.wx(l.obj_dir, x, 0);
            return;
        }
        if m.rx(l.obj_dir, x) == 0 {
            return;
        }
        let sp = m.f(Field::Speed, x) & 0x7F;
        m.w(l.obj_dx, sp);
        let (v, c) = sbc(m.r(l.player_x), sp, true);
        m.w(l.player_x, v);
        if !c {
            let h = m.r(l.sprite_x_msb) & 0xF8;
            m.w(l.sprite_x_msb, h);
        }
        return;
    }
    if !minus(e, 0xF5) {
        if d == 0xFA {
            head_hit(m.ram, l);
        }
        return;
    }
    // `$834C`: the object is left of her
    if m.r(l.walk_dir) & 0x40 == 0 {
        m.w(l.walk_dir, 0);
        m.w(l.speed, 0);
    }
    if m.r(l.probe_props + 5) & 1 != 0 {
        m.wx(l.obj_dir, x, 1);
        return;
    }
    if m.rx(l.obj_dir, x) != 0 {
        return;
    }
    let sp = m.f(Field::Speed, x) & 0x7F;
    if m.r(l.player_x) >= 0xA0 && m.r(l.scroll_column) != 0 {
        m.w(l.scroll_px, sp);
        return;
    }
    let (v, c) = adc(m.r(l.player_x), sp, false);
    m.w(l.player_x, v);
    if c {
        let h = m.r(l.sprite_x_msb) | 7;
        m.w(l.sprite_x_msb, h);
    }
}

/// `$83C9`: Giana stands on an object with a negative parameter `p`: `$FF` makes it a class-2 object of kind... (score
/// and class 2), anything else starts the stage-end lift: the input goes off (`$2BE8`), `$2F` is set, the group leader's
/// script jumps to position `$0A` with `$80` frames, `$31 = 0`.
fn ride_trigger(m: &mut Mem, x: u8, p: u8) {
    let l = m.l;
    if p == 0xFF {
        m.wf(Field::Score, x, 2);
        m.wf(Field::Class, x, 2);
        return;
    }
    // `$2BE8`: an `RTS` over the first byte of the joystick read
    m.w(l.read_input, 0x60);
    m.w(l.probe_skip_bottom, 0x60);
    let y = m.rx(l.group_leader, x);
    m.wx(l.obj_script_pos, y, 0x0A);
    m.wx(l.obj_script_byte, y, 0x80);
    m.w(l.joy, 0);
}

/// `$2C9F..$2D47` (entered by `JMP` from [`land`]): Giana's head hit something, she falls from now on (`$17 = $FF`; a
/// swimmer only loses 5 steps); then the landing check of `$2CD8` (the same as at the end of
/// [`crate::player::vertical`]).
pub fn fall_from_head(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let p = |ram: &[u8], c: u16| ram[(l.probe_props + c) as usize];
    ram[l.vert_dir as usize] = 0xFF;
    if r(ram, l.swim) != 0 {
        ram[l.vert_step as usize] = r(ram, l.vert_step).saturating_sub(5);
        return;
    }
    ram[l.head_bump as usize] = 0xFF;
    if r(ram, l.bounce) != 0 {
        ram[l.bounce as usize] = 0xFF;
    }
    let depth = r(ram, l.feet_depth);
    let land = p(ram, 7) & 1 != 0
        || ((p(ram, 3) | p(ram, 5)) & 1 == 0
            && (p(ram, 6) | p(ram, 8) | r(ram, l.on_object)) & 1 != 0
            && depth < r(ram, l.land_depth));
    if !land {
        return;
    }
    ram[l.vert_dir as usize] = 0;
    ram[l.bounce as usize] = 0;
    ram[l.fall_frames as usize] = 0;
    ram[l.player_y as usize] = r(ram, l.player_y).wrapping_sub(depth);
}

/// `$7F11`: fire (`$32`) while playing, with no shot on the screen and power level >= 2 starts the shot at Giana's
/// position (8 or 15 pixels up; swimming 8 and rising 6 frames), in her facing direction, for `$C0` frames, colour 1,
/// hi-res, frame `$FA`, sound `$0A`; level >= 4 picks its target first ([`shot_aim`]).
pub fn shot_fire(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram, l };
    if m.r(l.fire) == 0 || m.r(l.game_state) != 0 || m.r(l.sprite_enable) & 8 != 0 {
        return;
    }
    m.w(l.shot_target, 0);
    let level = m.r(l.power);
    if level < 2 {
        return;
    }
    m.w(l.shot_timer, 0xC0);
    if level >= 4 {
        shot_aim(m.ram, l);
    }
    m.w(l.shot_bounce, 0);
    m.w(l.shot_vdir, 0);
    m.w(l.effect, 0x0A);
    let f = m.r(l.facing);
    m.w(l.shot_dir, f);
    vic[VIC_COLOR + SHOT as usize] = 1;
    vic[VIC_MULTICOLOR] &= 0xF7;
    m.w(l.object_frames, 0xFA);
    let gx = m.r(l.player_x);
    m.w(l.object_sprite_x, gx);
    let h = m.r(l.sprite_x_msb);
    m.w(l.sprite_x_msb, (h & 0xF7) | ((h & 1) << 3));
    let gy = m.r(l.player_y);
    let (y, c) = if m.r(l.swim) == 1 {
        let v = m.r(l.shot_vdir).wrapping_add(1);
        m.w(l.shot_vdir, v);
        m.w(l.shot_vstep, 6);
        sbc(gy, 8, true)
    } else {
        sbc(gy, 0x0F, true)
    };
    m.w(l.object_sprite_y, if c { y } else { 0 });
    let e = m.r(l.sprite_enable) | 8;
    m.w(l.sprite_enable, e);
}

/// `$8146`: the homing shot's target `$D0`: the first object from slot 7 down that is on (`$7A`), alive (phase 0) and an
/// enemy (kind 0 or negative), on the side Giana faces (`$37`).
pub fn shot_aim(ram: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram, l };
    let mut a = m.r(l.sprite_enable);
    let mut x = 7u8;
    loop {
        let c = a & 0x80 != 0;
        a <<= 1;
        if c && m.f(Field::Kind, x).wrapping_sub(1) & 0x80 != 0 && m.rx(l.obj_phase, x) == 0 {
            m.w(l.contact_mask, a);
            let ohi = m.obj_msb(x);
            let (_, _, c) = x_diff(m.r(l.player_x), m.r(l.sprite_x_msb) & 1, m.rx(l.player_x, x), ohi);
            if c == (m.r(l.facing) != 0) {
                m.w(l.shot_target, x);
                return;
            }
        }
        x = x.wrapping_sub(1);
        if x < 4 {
            return;
        }
    }
}

/// `$7F8C`: the shot's next explosion frame (`$FF` wraps to `$FB`).
fn next_frame(m: &mut Mem, x: u8) {
    let x = match x.wrapping_add(1) {
        0 => 0xFB,
        v => v,
    };
    m.w(m.l.object_frames, x);
}

/// `$804A`: the shot explodes: colour `$0D`, 7 pixels down (plus the carry the original has there), next frame.
fn explode(m: &mut Mem, vic: &mut [u8], c: bool) {
    let l = m.l;
    vic[VIC_COLOR + SHOT as usize] = 0x0D;
    let y = adc(m.r(l.object_sprite_y), 7, c).0;
    m.w(l.object_sprite_y, y);
    let f = m.r(l.object_frames);
    next_frame(m, f);
}

/// `$7F94`: one frame of the shot. Explosion frames `$FB..$FE` (then it goes off); the timer `$02D5` running out starts
/// them. Else the object probe of slot 3 ([`objects::probe`]) decides: a homing shot ([`shot_target`] set) flies at its
/// target ([`homing`]); the others fly 8 pixels a frame sideways and 2 down (rising `$CE` frames 2 up after a bounce),
/// and against a wall or the floor explode below level 3 or bounce from level 3 on.
pub fn shot_move(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram: &mut *ram, l };
    if m.r(l.sprite_enable) & 8 == 0 {
        return;
    }
    let f = m.r(l.object_frames);
    if f >= 0xFB {
        if f == 0xFE {
            let e = m.r(l.sprite_enable) & 0xF7;
            m.w(l.sprite_enable, e);
        } else {
            next_frame(&mut m, f);
        }
        return;
    }
    let t = m.r(l.shot_timer).wrapping_sub(1);
    m.w(l.shot_timer, t);
    if t == 0 {
        next_frame(&mut m, f);
        return;
    }
    let c = objects::probe_carry(m.ram, l, SHOT);
    let target = m.r(l.shot_target);
    if target != 0 {
        homing(&mut m, vic, target, c);
        return;
    }
    let p = |m: &Mem, i: u16| m.r(l.obj_probe_props + i);
    #[derive(Clone, Copy)]
    enum At {
        Toggle,
        Side,
        Floor,
        Bounce,
        Move,
        Vertical,
    }
    let mut at = if m.r(l.shot_vdir) == 0 {
        if p(&m, 7) == 0 {
            At::Side
        } else if p(&m, 4) != 0 {
            At::Floor
        } else {
            m.w(l.shot_vstep, 0x0E);
            At::Toggle
        }
    } else if p(&m, 4) == 0 {
        // `$7FE2`
        let y = if m.r(l.shot_dir) != 0 { p(&m, 3) } else { p(&m, 5) };
        if y != 0 { At::Bounce } else { At::Move }
    } else if p(&m, 7) != 0 {
        At::Floor
    } else {
        At::Toggle
    };
    loop {
        at = match at {
            At::Toggle => {
                let v = m.r(l.shot_vdir) ^ 1;
                m.w(l.shot_vdir, v);
                return;
            }
            At::Side => {
                // `$7FD2`
                let y = if m.r(l.shot_dir) != 0 { p(&m, 6) } else { p(&m, 8) };
                if y != 0 { At::Bounce } else { At::Move }
            }
            At::Bounce => {
                // `$8019`: a wall: turn (level >= 3)
                if m.r(l.power) < 3 {
                    explode(&mut m, vic, false);
                    return;
                }
                m.w(l.shot_bounce, 1);
                let d = m.r(l.shot_dir) ^ 1;
                m.w(l.shot_dir, d);
                At::Vertical
            }
            At::Floor => {
                // `$802A`: the floor (or a ceiling when rising): bounce (level >= 3) unless walled in
                if m.r(l.power) < 3 {
                    explode(&mut m, vic, false);
                    return;
                }
                m.w(l.shot_bounce, 1);
                let d = m.r(l.shot_dir);
                if d == 0 {
                    if p(&m, 3) | p(&m, 6) == 0 {
                        m.w(l.shot_dir, 1);
                        at = At::Move;
                        continue;
                    }
                } else {
                    m.w(l.shot_dir, d.wrapping_sub(1));
                }
                if p(&m, 5) | p(&m, 8) == 0 {
                    At::Move
                } else {
                    explode(&mut m, vic, true);
                    return;
                }
            }
            At::Move => {
                // `$7FF0`: 8 pixels sideways
                m.w(l.obj_dx, 8);
                let d = m.r(l.shot_dir);
                objects::move_x(m.ram, l, SHOT, d);
                At::Vertical
            }
            At::Vertical => {
                // `$7FFB`
                if m.r(l.shot_vdir) == 0 {
                    let (y, c) = adc(m.r(l.object_sprite_y), 2, false);
                    if c {
                        let f = m.r(l.object_frames);
                        next_frame(&mut m, f);
                    } else {
                        m.w(l.object_sprite_y, y);
                    }
                    return;
                }
                let s = m.r(l.shot_vstep);
                if s == 0 {
                    at = At::Toggle;
                    continue;
                }
                if s >= 3 {
                    let y = m.r(l.object_sprite_y).wrapping_sub(2);
                    m.w(l.object_sprite_y, y);
                }
                m.w(l.shot_vstep, s - 1);
                return;
            }
        };
    }
}

/// `$805A`: the homing shot flies at slot `x` (`c` = the carry the probe returned with): it explodes when the target is
/// gone; else the step towards it (bigger from farther away, table `$4C4C`), along walls when the probe finds one.
fn homing(m: &mut Mem, vic: &mut [u8], x: u8, c: bool) {
    let l = m.l;
    let p = |m: &Mem, i: u16| m.r(l.obj_probe_props + i);
    if m.rx(l.obj_phase, x) != 0 || m.r(l.sprite_enable) & m.bit(x) == 0 {
        explode(m, vic, c);
        return;
    }
    let (a, _) = adc(m.r(l.object_sprite_y), 8, false);
    let (a, cc) = sbc(a, m.rx(l.player_y, x), true);
    let (a, vd) = if cc { (a, 0xFF) } else { (a ^ 0xFF, 0) };
    m.w(l.shot_vdir, vd);
    let d3 = a >> 1;
    m.w(l.shot_dist, d3);
    let bit = m.bit(x);
    let ohi = m.r(l.sprite_x_msb) & bit >= bit;
    let d4 = (u8::from(ohi) << 7) | (m.rx(l.player_x, x) >> 1);
    m.w(l.follow_x, d4);
    let shi = m.r(l.sprite_x_msb) & 8 >= 8;
    let a = (u8::from(shi) << 7) | (m.r(l.object_sprite_x) >> 1);
    let (a, c) = sbc(a, d4, true);
    let (a, dir) = if c { (a, 1) } else { (a ^ 0xFF, 0) };
    m.w(l.shot_dir, dir);
    let (sub, a) = if a < 2 {
        // `$80EC`: right above or below it
        m.w(l.shot_dy, 3);
        let sub = if m.r(l.shot_vdir) != 0 { p(m, 4) == 0 } else { p(m, 7) != 0 };
        (sub, m.r(l.object_sprite_y))
    } else {
        let (a, c) = sbc(a, d3, true);
        let (a, mut dx, mut dy) = if c { (a, 3u8, 1u8) } else { (a ^ 0xFF, 1, 3) };
        if a < 8 {
            dx = 1;
            dy = 1;
        }
        m.w(l.shot_dx, dx);
        let t = m.rx(l.shot_steps, a >> 4);
        let (d2, c1) = adc(t, dy, false);
        m.w(l.shot_dy, d2);
        let (a, c2) = adc(t, dx, c1);
        let walled = m.r(l.shot_vdir) != 0 && p(m, 3) != 0 && p(m, 5) != 0;
        if walled {
            let v = if m.r(l.shot_dir) != 0 { p(m, 6) } else { p(m, 8) };
            if v != 0 {
                explode(m, vic, c2);
                return;
            }
            m.w(l.shot_vdir, 0);
            m.w(l.shot_dy, a);
        } else {
            // `$8102`
            let (first, second, third) = if m.r(l.shot_dir) != 0 { (6, 5, 3) } else { (8, 3, 5) };
            if p(m, first) == 0 {
                m.w(l.obj_dx, a);
                let d = m.r(l.shot_dir);
                objects::move_x(m.ram, l, SHOT, d);
            } else if p(m, second) == 0 || p(m, third) == 0 {
                m.w(l.shot_dy, a);
                m.w(l.shot_vdir, 1);
            } else {
                explode(m, vic, c2);
                return;
            }
        }
        (m.r(l.shot_vdir) != 0, m.r(l.object_sprite_y))
    };
    let d2 = m.r(l.shot_dy);
    m.w(l.object_sprite_y, if sub { a.wrapping_sub(d2) } else { a.wrapping_add(d2) });
}
