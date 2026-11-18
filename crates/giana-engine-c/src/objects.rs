//! The object engine core (work package O1 of `docs/rom-hack-family-c.md`, 7.4): native ports of the preview's object
//! slots, the spawn from the stage's spawn list, the per-frame update loop with its dispatch to the class code, the
//! helpers every class uses (tile probe, animation, horizontal move, turning) and the code that carries Giana on an
//! object (`$1F`, `$22`, `$02C1`, read by the player routines). Each routine is verified call by call and frame by frame
//! against the original running in the oracle machine (`crates/giana-c30-oracle/tests/objects.rs`).
//!
//! # State
//!
//! As in [`crate::player`], the routines work on the game's own memory image at the addresses of [`C30aLayout`] (the
//! state moves into Rust structs once the whole frame is native). The VIC registers the object code touches (`$D01B`
//! priority, `$D01C` multicolour, `$D027+n` colours) are a separate 64-byte register image `vic` (as written; a read of a
//! colour register gives the high nibble set, as on the VIC).
//!
//! **Slots.** Sprites 3..7 are object slots `n`; the update loop and the spawn use 4..7, slot 3 is Giana's shot
//! (package O3). Per slot: x `$70+n` (high bit in `$88` bit `n`), y `$78+n`, sprite frame `$80+n`, enabled bit `n` of
//! `$7A` (also the `$D015` value), and these arrays (index `n`):
//!
//! | array | role |
//! |---|---|
//! | `$0200` | `$10` = sprite behind the background (`$D01B`), from descriptor byte 1 bit 6 |
//! | `$0204` | descriptor byte 1: bit 0 = hi-res (else multicolour), bit 1 = behind the background, bit 7 = ping-pong animation |
//! | `$0209 + 5k` | descriptor bytes `3 + k` (k = 0..17, [`Field`]) |
//! | `$0263` | animation delay counter (reloaded from [`Field::AnimDelay`]) |
//! | `$0268` | direction: 0 = the "A" frames (right), 1 = the "B" frames (left) |
//! | `$026D` | step index (jump table `$8D5C` for the jumpers, script position for others) |
//! | `$0277` | colour as spawned (bit 7 = flashing colour from `$4C5B[$C5]`) |
//! | `$02A4/$02A8/$02AC/$02B0` | members of a group led by this slot (0 = none) |
//! | `$02B4` | the leader of this slot's group (0 = not in a group) |
//! | `$02B8` | saved y / `$FF` after a flip |
//! | `$02BC` | animation phase (ping-pong direction; 1 after the spawn) |
//! | `$02C1` | state: 0, `$FF`/negative (e.g. falling, deadly top), 1 (e.g. rising); read by the player routines |
//! | `$02C6`, `$02CB` | first script byte and the script position (`$022C/$0231` point to the script) |
//! | `$1800` | phase: 0 = alive, non-zero = dying, collected or another class-specific phase |
//!
//! **Descriptors.** The spawn list entry's type (bits 0..6) indexes the word table `$42A8`; each record has 21 bytes: byte
//! 0 = the first sprite frame, 1 = the flags (`$0204`), 2 = the colour (`$0277`, `$D027+n`), 3..20 = the [`Field`]s.
//!
//! # Ported routines
//!
//! | original | function | what |
//! |---|---|---|
//! | `$7349..$7526` | [`spawn`] ([`spawn_step`], [`spawn_type`], [`spawn_member`]) | spawn the objects whose column the scroll has reached (picks a slot, groups) |
//! | `$73E8..` | [`instantiate`] | one object from the descriptor at `$EE` into a slot (also entered from `$83ED`) |
//! | `$75D6..$75F3` | [`release_group`] | frees the members of a group |
//! | `$75F4..$7608` | [`clear_groups`] | before a special spawn: all object slots off, group links cleared |
//! | `$7527..$75D5` | [`despawn`] | objects that left the screen (both edges, and the top) |
//! | `$7613..$765C` | [`dispatch`] | the per-frame update loop: the next live slot and the class code it goes to ([`Route`]) |
//! | `$7822..$784F` | [`update_tail`] | after the class code: flashing colour, and the contact that kills an object |
//! | `$7B06..$7CB6` | [`probe`], [`probe_clear`] | the tiles around an object (`$53..$5B`, read by every moving class) |
//! | `$7A74..$7B05` | [`animate`] | one animation step |
//! | `$79F8..$7A16` | [`move_x`] | horizontal move by `$5C` in the direction `$5D` |
//! | `$79D0..$79F7` | [`turn`] | turn around (frames of the other direction) |
//! | `$7A17..$7A39` | [`flip`] | the state `$02C1` flips sign (`EOR #$FE`) |
//! | `$7A3A..$7A6B` | [`powerup_colour`] | the power-up's colours by the power level |
//! | `$81C6..$8276` | [`ride`] | Giana stands on an object: carried along (x, y, scroll), `$1F` |
//! | `$8277..$828B` | [`near`] | `$1F` = Giana is horizontally over the object |
//! | `$8188..$81C5` | [`lift`] | the object Giana rides when `$2F` is set (the lift into the next stage) |
//!
//! # The class interface (packages O2..O4)
//!
//! [`dispatch`] is the loop of `$7613`: it returns the next slot to update and the [`Route`] the original takes for it,
//! by the class code `$0209+n` and the phase `$1800+n`. The class code is the rest of the original's update for that
//! slot: a native class is a function `fn(ram, vic, layout, slot) -> ClassEnd` that ends where the original's class code
//! ends: [`ClassEnd::Next`] (`JMP $7622`, the loop goes on with [`dispatch`] from the slot), [`ClassEnd::Tail`] (`JMP
//! $7822`, [`update_tail`] runs first), [`ClassEnd::Probe`] (`JMP $781F`: [`animate`] and then the tail) or
//! [`ClassEnd::Return`] (`RTS`: the loop ends). The class code itself is in [`crate::classes`]. The helpers
//! of this module are what the class code calls (`JSR $7B11` = [`probe`], `$7A74` = [`animate`], `$79F8` = [`move_x`],
//! `$79D0` = [`turn`], `$7A17` = [`flip`]); each class package adds its routes' functions and the frame driver calls them
//! in the order of `$72EA`.

use crate::layout::C30aLayout;

/// VIC register numbers the object code writes (the same on every version: hardware).
pub const VIC_PRIORITY: usize = 0x1B;
pub const VIC_MULTICOLOR: usize = 0x1C;
pub const VIC_COLOR: usize = 0x27;

/// The per-slot arrays filled from descriptor bytes 3..20: `$0209 + 5 * k + n` for slot `n`. Roles from the code that
/// reads them (O2 refines them per class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Field {
    /// `$0209`: class code (0 items, 1 jumpers, 2 walkers, 3, 4, 5.. other classes, negative: risers)
    Class = 0,
    /// `$020E`: first frame of the "A" direction
    FramesA = 1,
    /// `$0213`: last frame of the "A" direction
    FramesAEnd = 2,
    /// `$0218`: first frame of the "B" direction
    FramesB = 3,
    /// `$021D`: last frame of the "B" direction
    FramesBEnd = 4,
    /// `$0222`: kind within the class (class 0: 2 = bonus, `$12` = power-up ..)
    Kind = 5,
    /// `$0227`: score amount (BCD) / bit 7: landing on it does something
    Score = 6,
    /// `$022C`: script pointer low / vertical speed
    ScriptLo = 7,
    /// `$0231`: script pointer high / start of the jump index
    ScriptHi = 8,
    /// `$0236`: frame when hit, direction A
    HitFrameA = 9,
    /// `$023B`: frame when hit, direction B
    HitFrameB = 10,
    /// `$0240`: horizontal speed (bit 7: special)
    Speed = 11,
    /// `$0245`: animation delay
    AnimDelay = 12,
    /// `$024A`: movement flags
    Movement = 13,
    /// `$024F`: what the left screen edge does (0 remove, positive stop, negative nothing)
    Edge = 14,
    /// `$0254`: score, high part
    ScoreHigh = 15,
    /// `$0259`: group size (members spawned with it) / frame stride of the members
    Group = 16,
    /// `$025E`: probe width (extra columns), 0 = from [`Field::Group`], negative = none
    Width = 17,
}

impl Field {
    /// Address of this field for slot `n`.
    pub fn at(self, l: &C30aLayout, n: u8) -> u16 {
        l.obj_record + 5 * self as u16 + u16::from(n)
    }
}

/// Memory of one routine call: the game's RAM image and the layout.
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
    /// `base,X`
    fn rx(&self, base: u16, x: u8) -> u8 {
        self.ram[base.wrapping_add(u16::from(x)) as usize]
    }
    fn wx(&mut self, base: u16, x: u8, v: u8) {
        self.ram[base.wrapping_add(u16::from(x)) as usize] = v;
    }
    fn f(&self, f: Field, x: u8) -> u8 {
        self.r(f.at(self.l, x))
    }
    fn word(&self, zp: u16) -> u16 {
        u16::from_le_bytes([self.r(zp), self.r((zp + 1) & 0xFF)])
    }
    /// `(zp),Y`
    fn ind(&self, zp: u16, y: u8) -> u8 {
        self.r(self.word(zp).wrapping_add(u16::from(y)))
    }
    fn bit(&self, x: u8) -> u8 {
        self.rx(self.l.bit_set, x)
    }
    fn mask(&self, x: u8) -> u8 {
        self.rx(self.l.bit_clear, x)
    }
}

/// How [`instantiate`] ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instantiated {
    /// `JMP $7349`: the spawn list goes on
    Spawn,
    /// `JMP $739A`: another member of the group, from the same descriptor
    Member,
    /// `RTS` (`$F6` = `$80`: the object was spawned by a script, `$83ED`)
    Return,
}

/// `$7349`: spawns every object of the spawn list whose column the scroll has reached (`$21`): skips the entries left
/// behind, takes a slot (a free one, else one whose phase is not 0, else the leftmost on the screen) and fills it from
/// the descriptor ([`instantiate`]), with all members of a group. The loop of [`spawn_step`] and [`spawn_member`].
pub fn spawn(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) {
    let mut step = spawn_step(ram, l);
    loop {
        step = match step {
            Instantiated::Return => return,
            Instantiated::Spawn => spawn_step(ram, l),
            Instantiated::Member => spawn_member(ram, vic, l).0,
        }
    }
}

/// One pass of `$7349` up to its next `JMP $7349` (an entry left behind, or one object spawned: [`Instantiated::Spawn`]),
/// `$739A` (the descriptor is set up, an object or a group member follows: [`Instantiated::Member`]) or its `RTS` (the list ends or the next entry lies
/// ahead: [`Instantiated::Return`]). The stage start calls `$7349` with IRQs on, so the port ends where the original
/// can be interrupted between its parts.
pub fn spawn_step(ram: &mut [u8], l: &C30aLayout) -> Instantiated {
    let mut m = Mem { ram: &mut *ram, l };
    let mut y = m.r(l.spawn_index);
    let col = m.ind(l.spawn_list, y);
    if col == 0xFF {
        return Instantiated::Return;
    }
    let c = col.wrapping_sub(1);
    let s = m.r(l.scroll_column);
    if c != s {
        if c > s {
            return Instantiated::Return;
        }
        // left behind
        m.w(l.spawn_index, y.wrapping_add(3));
        return Instantiated::Spawn;
    }
    y = y.wrapping_add(1);
    let param = m.ind(l.spawn_list, y);
    y = y.wrapping_add(1);
    let t = m.ind(l.spawn_list, y);
    m.w(l.spawn_param, param);
    m.w(l.spawn_index, y.wrapping_add(1));
    spawn_type(ram, l, t)
}

/// `$7370`: the descriptor of type `t` (bit 7: a special spawn, [`clear_groups`] first) becomes the one to spawn
/// (`$EE`), the probe offsets are reset; `$739A` follows ([`spawn_member`]). Also entered by the boss code (`$84F6`, with
/// `$F6` = `$80`: the caller places the object).
pub fn spawn_type(ram: &mut [u8], l: &C30aLayout, t: u8) -> Instantiated {
    if t & 0x80 != 0 {
        clear_groups(ram, l);
    }
    let mut m = Mem { ram: &mut *ram, l };
    m.w(l.probe_x_offset, 0x15);
    m.w(l.probe_y_offset, 0x33);
    let xi = t << 1;
    let lo = m.rx(l.descriptor_table, xi);
    let hi = m.rx(l.descriptor_table + 1, xi);
    m.w(l.descriptor_ptr, lo);
    m.w(l.descriptor_ptr + 1, hi);
    m.w(l.group_count, 0xFF);
    if m.ind(l.descriptor_ptr, 0x13) == 3 {
        clear_groups(ram, l);
    }
    // `$739A` follows: the slot and the object ([`spawn_member`])
    Instantiated::Member
}

/// `$739A`: a slot for the descriptor at `$EE`, and [`instantiate`] into it. Returns the slot too (the original leaves
/// it in `X`; the caller of `$7370` at `$84F6` uses it).
pub fn spawn_member(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout) -> (Instantiated, u8) {
    let x = choose_slot(ram, l);
    (instantiate(ram, vic, l, x), x)
}

/// `$739A..$73E6`: the slot for a new object.
fn choose_slot(ram: &mut [u8], l: &C30aLayout) -> u8 {
    let mut m = Mem { ram: &mut *ram, l };
    let en = m.r(l.sprite_enable);
    if let Some(x) = (4..=7u8).rev().find(|&x| en & (1 << x) == 0) {
        return x;
    }
    let x = match (4..=7u8).rev().find(|&x| m.rx(l.obj_phase, x) != 0) {
        Some(x) => x,
        None => {
            // the leftmost object on the screen, or the first beyond x = $178
            m.w(l.spawn_pick, 7);
            m.w(l.spawn_min_x, 0xFF);
            let mut pick = None;
            for x in (4..=7u8).rev() {
                let ox = m.rx(l.player_x, x);
                if m.r(l.sprite_x_msb) & m.bit(x) != 0 {
                    if ox >= 0x78 {
                        pick = Some(x);
                        break;
                    }
                } else if ox < m.r(l.spawn_min_x) {
                    m.w(l.spawn_min_x, ox);
                    m.w(l.spawn_pick, x);
                }
            }
            pick.unwrap_or_else(|| m.r(l.spawn_pick))
        }
    };
    m.w(l.spawn_pick, x);
    let leader = m.rx(l.group_leader, x);
    if leader != 0 {
        release_group(ram, l, leader);
    }
    x
}

/// `$73E8`: fills slot `x` from the descriptor at `$EE/$EF` (sprite frame, flags, colour, the 18 fields), sets the
/// class-specific start state, the position (at the right edge, or at the stage-start column when `$F6` = `$FF`, or
/// left to the caller when `$F6` = `$80`) and links the members of a group.
pub fn instantiate(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> Instantiated {
    {
        let mut m = Mem { ram: &mut *ram, l };
        m.w(l.spawn_slot, x);
        let d0 = m.ind(l.descriptor_ptr, 0);
        m.wx(l.player_frames, x, d0);
        let d1 = m.ind(l.descriptor_ptr, 1);
        m.wx(l.obj_flags, x, d1);
        let t = d1 & 0x43;
        let bit = m.bit(x);
        let mask = m.mask(x);
        let v = vic[VIC_MULTICOLOR];
        vic[VIC_MULTICOLOR] = if t & 1 == 0 { v | bit } else { v & mask };
        let t = t >> 1;
        m.wx(l.obj_behind, x, t >> 1);
        let v = vic[VIC_PRIORITY];
        vic[VIC_PRIORITY] = if t & 1 != 0 { v | bit } else { v & mask };
        let c = m.ind(l.descriptor_ptr, 2);
        m.wx(l.obj_colour, x, c);
        vic[VIC_COLOR + x as usize] = c;
        for k in 0..18u8 {
            let v = m.ind(l.descriptor_ptr, 3 + k);
            m.wx(l.obj_record, x.wrapping_add(5 * k), v);
        }
    }
    let m = Mem { ram: &mut *ram, l };
    let class = m.f(Field::Class, x);
    let zero_from_script = |ram: &mut [u8], dir: u8| {
        let mut m = Mem { ram, l };
        m.wx(l.obj_dir, x, dir);
        m.w(l.obj_script, m.f(Field::ScriptLo, x));
        m.w(l.obj_script + 1, m.f(Field::ScriptHi, x));
        let b = m.ind(l.obj_script, 0);
        m.wx(l.obj_script_byte, x, b);
        m.wx(l.obj_script_pos, x, 0);
        m.wx(l.object_state, x, 0);
    };
    if class == 1 || class & 0x80 != 0 {
        let mut m = m;
        let s = m.f(Field::ScriptHi, x);
        m.wx(l.obj_step, x, s);
        m.wx(l.object_state, x, 0xFF);
        m.wx(l.obj_dir, x, 1);
    } else if class == 0 && m.f(Field::Kind, x) == 2 {
        let mut m = m;
        vic[VIC_COLOR + x as usize] = m.r(l.stage_header + 10);
        m.w(l.bonus_flag, 0);
        zero_from_script(ram, 0);
    } else {
        let mut y = class.wrapping_sub(1);
        if class == 0 {
            y = 0;
            if m.f(Field::Kind, x) == 0x12 {
                if let Some(p) = powerup_colour(ram, vic, l, x) {
                    y = p;
                }
            }
        }
        if y == 5 {
            ram[l.probe_x_offset as usize] = 0x11;
        }
        zero_from_script(ram, 1);
    }
    let mut m = Mem { ram: &mut *ram, l };
    for a in [l.obj_phase, l.group_members, l.group_members + 4, l.group_members + 8, l.group_members + 12, l.group_leader] {
        m.wx(a, x, 0);
    }
    let d = m.f(Field::AnimDelay, x);
    m.wx(l.obj_anim_timer, x, d);
    m.wx(l.anim_phase, x, 1);
    let mode = m.r(l.spawn_mode);
    let bit = m.bit(x);
    if mode & 0x80 != 0 {
        if mode & 0x40 == 0 {
            m.w(l.spawn_mode, 1);
            return Instantiated::Return;
        }
        // the stage start: at the object's column relative to the restart column
        let mut a = m.r(l.scroll_column);
        let rc = m.r(l.restart_column);
        let mut c = 0u16;
        if rc != 0 {
            c = u16::from(a >= rc);
            a = a.wrapping_sub(rc);
        }
        let a = (u16::from(a) + 1 + c) as u8;
        m.wx(l.player_x, x, (a << 3) | 7);
        if a & 0x20 != 0 {
            let v = m.r(l.sprite_x_msb) | bit;
            m.w(l.sprite_x_msb, v);
        }
    } else {
        let fx = 0x48u8.wrapping_add(m.r(l.fine_scroll));
        m.wx(l.player_x, x, fx);
        let v = m.r(l.sprite_x_msb) | bit;
        m.w(l.sprite_x_msb, v);
    }
    let p = m.r(l.spawn_param);
    let y = m.rx(l.spawn_y_table, p);
    m.wx(l.player_y, x, y);
    let v = m.r(l.sprite_enable) | bit;
    m.w(l.sprite_enable, v);
    if m.f(Field::Group, x) == 0 {
        return Instantiated::Spawn;
    }
    if m.r(l.group_count) & 0x80 != 0 {
        m.w(l.group_link_operand, 0xA4);
        m.w(l.group_leader_tmp, x);
    }
    let n = m.r(l.group_count).wrapping_add(1);
    m.w(l.group_count, n);
    let leader = m.r(l.group_leader_tmp);
    m.wx(l.group_leader, x, leader);
    // `STA $02A4,Y` with its operand rewritten (`$750F`)
    let op = m.r(l.group_link_operand);
    let link = u16::from_le_bytes([op, m.r(l.group_link_operand + 1)]).wrapping_add(u16::from(leader));
    m.w(link, x);
    m.w(l.group_link_operand, op.wrapping_add(4));
    if n < m.f(Field::Group, leader) { Instantiated::Member } else { Instantiated::Spawn }
}

/// `$75D6`: frees the members of the group led by slot `leader` (their links, their enabled bits).
pub fn release_group(ram: &mut [u8], l: &C30aLayout, leader: u8) {
    let mut m = Mem { ram, l };
    let mut y = leader;
    loop {
        let x = m.rx(l.group_members, y);
        if x == 0 {
            return;
        }
        m.wx(l.group_members, y, 0);
        m.wx(l.group_leader, x, 0);
        let v = m.r(l.sprite_enable) & m.mask(x);
        m.w(l.sprite_enable, v);
        y = y.wrapping_add(4);
        if y >= 0x14 {
            return;
        }
    }
}

/// `$75F4`: before a special spawn: the object sprites 4..7 off (and their x high bits), all group links cleared.
pub fn clear_groups(ram: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram, l };
    let en = m.r(l.sprite_enable) & 0x0F;
    m.w(l.sprite_enable, en);
    let msb = en & m.r(l.sprite_x_msb);
    m.w(l.sprite_x_msb, msb);
    for y in 0..=0x1B {
        m.w(l.group_members + 4 + y, 0);
    }
}

/// `$7527`: objects (slots 7..3) that left the screen: off the left edge (or held at it, by [`Field::Edge`]), above
/// the top, beyond the right edge; a group goes with its leader. Clears `$1F` when Giana stood on a removed object.
pub fn despawn(ram: &mut [u8], l: &C30aLayout) {
    let mut x = 7u8;
    loop {
        let mut m = Mem { ram: &mut *ram, l };
        // `true` = remove the object (`$75AD`)
        let mut remove = false;
        let edge = m.f(Field::Edge, x);
        if m.r(l.sprite_x_msb) & m.bit(x) == 0 {
            let mut check_class = edge == 0;
            if edge != 0 && edge & 0x80 == 0 {
                let ld = m.rx(l.group_leader, x);
                if (ld == 0 || ld == m.rx(l.group_members, x)) && m.rx(l.player_x, x) < 0x20 {
                    m.wx(l.player_x, x, 0x20);
                    if m.rx(l.obj_dir, x) != 0 {
                        m.wx(l.obj_dir, x, 0);
                        let f = m.f(Field::FramesA, x);
                        m.wx(l.player_frames, x, f);
                        check_class = f == 0;
                    }
                }
            }
            if check_class && m.f(Field::Class, x) < 2 && m.rx(l.player_x, x) < 8 {
                remove = true;
            }
        } else {
            let mut check_x = edge == 0;
            if edge != 0 && edge & 0x80 == 0 {
                let ld = m.rx(l.group_leader, x);
                if (ld == 0 || ld == x) && m.rx(l.player_x, x) >= 0x40 && m.rx(l.obj_dir, x) == 0 {
                    m.wx(l.obj_dir, x, 1);
                    let f = m.f(Field::FramesB, x);
                    m.wx(l.player_frames, x, f);
                    check_x = f == 0;
                }
            }
            if check_x {
                let ox = m.rx(l.player_x, x);
                remove = (0x5A..0xD8).contains(&ox);
            }
        }
        if !remove && m.rx(l.player_y, x) < 0x16 {
            if x == 3 {
                return;
            }
            remove = true;
        }
        if remove {
            let ld = m.rx(l.group_leader, x);
            if ld != 0 && x != 3 {
                if ld == x {
                    release_group(ram, l, x);
                    return;
                }
            } else {
                let en = m.r(l.sprite_enable) & m.mask(x);
                m.w(l.sprite_enable, en);
                let msb = en & m.r(l.sprite_x_msb);
                m.w(l.sprite_x_msb, msb);
                if x == m.r(l.object_index) {
                    m.w(l.on_object, 0);
                }
            }
        }
        x -= 1;
        if x < 3 {
            return;
        }
    }
}

/// Where the update loop hands a slot to (the class code of the original, by the class `$0209+n` and the phase
/// `$1800+n`). The address is where the original goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `$765D`: class 0 (items: bonus, power-ups, by [`Field::Kind`])
    Items,
    /// `$760D`: negative classes (`JSR $7964`, then the next slot)
    Rising,
    /// `$76B8`: classes 1, 2 (the probe, then falling or the class code at `$76CE`), and classes >= 5 not in phase 0
    Moving,
    /// `$76D5`: class 3 in phase 0 (`JSR $8418`, the script, then `$781F`)
    Scripted,
    /// `$765A`: class 3 not in phase 0 (`JMP $7852`, `A` = the phase)
    ScriptedPhase,
    /// `$7647`: class 4 (`JSR $7878`, then the next slot)
    Class4,
    /// `$7652`: classes >= 5 in phase 0 (`JMP $78BE`)
    Floating,
}

impl Route {
    /// The original's address of this route.
    pub fn pc(self) -> u16 {
        match self {
            Route::Items => 0x765D,
            Route::Rising => 0x760D,
            Route::Moving => 0x76B8,
            Route::Scripted => 0x76D5,
            Route::ScriptedPhase => 0x765A,
            Route::Class4 => 0x7647,
            Route::Floating => 0x7652,
        }
    }
}

/// How the class code of a slot ends (for the native class code of O2..O4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassEnd {
    /// `JMP $7622`: [`dispatch`] from this slot
    Next,
    /// `JMP $7822`: [`update_tail`], then [`dispatch`]
    Tail,
    /// `JMP $781F`: [`animate`], then the tail
    Probe,
    /// `RTS` (added by O2): the update loop ends for this frame, the slots below are not updated (the class code of
    /// classes >= 5 in phase 0, `$78BE`, is entered by `JMP` from the loop and returns from it)
    Return,
}

/// Result of [`dispatch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    /// `RTS`: no slot left (or the objects are frozen: `$02DE` counted down). `a`, `x` = the original's `A` and `X`
    /// (`x = None`: frozen, `X` unchanged).
    Done { a: u8, x: Option<u8> },
    /// The class code of slot `x` runs next (`$57` = 0); `a` = the original's `A` there.
    Run { route: Route, x: u8, a: u8 },
}

/// `$7613` (`from = None`: the frame's first call) and `$7622` (`from = Some(n)`: after the class code of slot `n`):
/// the next enabled slot below (7..4, by the bits of `$7A` shifted through `$52`) that is not a group member following
/// its leader, and its route.
pub fn dispatch(ram: &mut [u8], l: &C30aLayout, from: Option<u8>, a: u8) -> Dispatch {
    let mut m = Mem { ram, l };
    let mut a = a;
    let mut x = match from {
        None => {
            let t = m.r(l.freeze_timer);
            if t != 0 {
                m.w(l.freeze_timer, t.wrapping_sub(1));
                return Dispatch::Done { a: t, x: None };
            }
            a = m.r(l.sprite_enable);
            m.w(l.update_bits, a);
            8u8
        }
        Some(n) => n,
    };
    loop {
        // `$7622`: DEX, CPX #4, BCS `$761E` (ASL `$52`)
        x = x.wrapping_sub(1);
        if x < 4 {
            return Dispatch::Done { a, x: Some(x) };
        }
        let bits = m.r(l.update_bits);
        m.w(l.update_bits, bits << 1);
        if bits & 0x80 == 0 {
            continue;
        }
        a = m.rx(l.group_leader, x);
        if a != 0 && a != m.rx(l.group_members, x) {
            continue;
        }
        m.w(l.obj_probe_props + 4, 0);
        let class = m.f(Field::Class, x);
        let phase = m.rx(l.obj_phase, x);
        let (route, a) = match class {
            0 => (Route::Items, class),
            c if c & 0x80 != 0 => (Route::Rising, class),
            1 | 2 => (Route::Moving, class),
            3 if phase == 0 => (Route::Scripted, phase),
            3 => (Route::ScriptedPhase, phase),
            4 => (Route::Class4, class),
            _ if phase != 0 => (Route::Moving, phase),
            _ => (Route::Floating, phase),
        };
        return Dispatch::Run { route, x, a };
    }
}

/// How [`update_tail`] ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailEnd {
    /// `JMP $7622`
    Next,
    /// `JSR $7D9E` at `$784C` (a deadly tile below a walker: the object is killed, scores), `a` = its kind
    Kill { a: u8 },
}

/// `$7822`: after the class code of slot `x`: a flashing colour (`$0277` bit 7) from `$4C5B[$C5]`, and the kill of a
/// live object whose probe found a deadly tile (`$57` = `$11`) unless its kind is 2..`$FD`. The second value is the `Y`
/// the original leaves (`$C5` after the flashing colour).
pub fn update_tail(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> (TailEnd, Option<u8>) {
    let m = Mem { ram, l };
    let kind = m.f(Field::Kind, x);
    let mut y = None;
    if m.rx(l.obj_colour, x) & 0x80 != 0 {
        if kind == 2 {
            return (TailEnd::Next, y);
        }
        let c = m.r(l.frame_counter);
        y = Some(c);
        vic[VIC_COLOR + x as usize] = m.rx(l.flash_colours, c);
    }
    if m.r(l.obj_probe_props + 4) != 0x11 || m.rx(l.obj_phase, x) != 0 {
        return (TailEnd::Next, y);
    }
    // `CMP #2; BPL` and `CMP #$FE; BMI`: the sign of the 8-bit difference
    if kind.wrapping_sub(2) & 0x80 == 0 || kind.wrapping_sub(0xFE) & 0x80 != 0 {
        return (TailEnd::Next, y);
    }
    (TailEnd::Kill { a: kind }, y)
}

/// `$7B06` (`a = 0`) / `$7B08`: the nine probe results `$53..$5B` = `a`.
pub fn probe_clear(ram: &mut [u8], l: &C30aLayout, a: u8) {
    for y in 0..9 {
        ram[(l.obj_probe_props + y) as usize] = a;
    }
}

/// `$7B11`: the tile properties around object `x` (from the screen at the object's position, with the offsets in the
/// code at `$7B19`/`$7B36` that the spawn and the classes rewrite): `$53/$56/$59` the column left of it (rows top, mid,
/// bottom), `$54/$57/$5A` its column (`$54` all bits, `$57` bits 0 and 4: the deadly `$11`), `$55/$58/$5B` the column
/// right of it; [`Field::Width`] or [`Field::Group`] more columns ORed into `$54/$5A`. Above or below the playfield all
/// nine are 0; at the screen edges the outer columns copy the inner ones.
pub fn probe(ram: &mut [u8], l: &C30aLayout, x: u8) {
    probe_carry(ram, l, x);
}

/// [`probe`] with the carry flag it returns with (the shot `$7F94` adds it to a sum when it explodes right after the
/// probe, `$805A`/`$804A`): above or below the playfield the comparison with `$F9`, after the scan the last `CPY`/`CPX`.
pub fn probe_carry(ram: &mut [u8], l: &C30aLayout, x: u8) -> bool {
    let mut m = Mem { ram, l };
    let oy = m.rx(l.player_y, x);
    let yoff = m.r(l.probe_y_offset);
    if oy >= 0xF9 || oy < yoff {
        probe_clear(m.ram, l, 0);
        return oy >= 0xF9;
    }
    let row = (oy - yoff) >> 3;
    let mut d: u8 = u8::from(m.bit(x) & m.r(l.sprite_x_msb) != 0);
    let (mut a, borrow) = m.rx(l.player_x, x).overflowing_sub(m.r(l.fine_scroll));
    if borrow {
        d = d.wrapping_sub(1);
    }
    let xoff = m.r(l.probe_x_offset);
    let borrow = a < xoff;
    a = a.wrapping_sub(xoff);
    if borrow {
        d = d.wrapping_sub(1);
    }
    for _ in 0..3 {
        let c = d & 1;
        d >>= 1;
        a = (a >> 1) | (c << 7);
    }
    let _ = d; // `$0D`: overwritten by the row pointer below
    m.w(l.obj_probe_col, a);
    let row_at = |m: &Mem, k: u16| -> (u8, u8) {
        let i = u16::from(row).wrapping_add(k);
        (m.r(l.row_lo.wrapping_add(i)), m.r(l.row_hi.wrapping_add(i)))
    };
    let set = |m: &mut Mem, zp: u16, (lo, hi): (u8, u8)| {
        m.w(zp, lo);
        m.w(zp + 1, hi);
    };
    let (p1, p2) = (l.obj_probe_ptr1, l.obj_probe_ptr2);
    // `$7B46`..`$7B89`: the three row pointers and the extra width `$9C`
    let width = m.f(Field::Width, x);
    let mut pc = match width {
        0 => 0x7B6F,
        w if w & 0x80 != 0 => 0x7B73,
        w => {
            m.w(l.obj_probe_width, w);
            let r = row_at(&m, 0xFFFF);
            set(&mut m, p1, r);
            if m.f(Field::ScriptLo, x) != 0 || m.rx(l.object_state, x) != 0 {
                let r = row_at(&m, 1);
                set(&mut m, p2, r);
                if r.1 != 0 { 0x7B8B } else { 0x7B6F }
            } else {
                0x7B81
            }
        }
    };
    if pc == 0x7B6F || pc == 0x7B73 {
        let w = if pc == 0x7B6F { m.f(Field::Group, x) } else { 0 };
        m.w(l.obj_probe_width, w);
        let r = row_at(&m, 1);
            set(&mut m, p1, r);
        pc = 0x7B81;
    }
    if pc == 0x7B81 {
        let r = row_at(&m, 3);
            set(&mut m, p2, r);
    }
    let r3 = if row >= 0x16 { 0x15 } else { row };
    let i = u16::from(r3) + 4;
    let (lo, hi) = (m.r(l.row_lo.wrapping_add(i)), m.r(l.row_hi.wrapping_add(i)));
    set(&mut m, l.obj_probe_ptr3, (lo, hi));
    m.w(l.obj_probe_slot, x);
    // the carry the scan keeps from `$7B8B` (`CPY #$16`) when it starts left of the screen
    probe_scan(&mut m, x, row >= 0x16)
}

/// `$7B9D..$7CB6`: the scan of the probe over the columns (a state machine over the original's labels: the code jumps
/// between its parts); returns the carry flag of the original's exit (`c0` = the carry on entry).
fn probe_scan(m: &mut Mem, x: u8, c0: bool) -> bool {
    let l = m.l;
    let p = l.obj_probe_props;
    let (p1, p2, p3) = (l.obj_probe_ptr1, l.obj_probe_ptr2, l.obj_probe_ptr3);
    let prop = |m: &Mem, zp: u16, y: u8| m.rx(l.tile_props, m.ind(zp, y));
    let mut y = m.r(l.obj_probe_col);
    let mut a: u8;
    let mut pc: u16 = if y & 0x80 == 0 { 0x7BDB } else { 0x7BA1 };
    // the carry when `$7BA1` is reached (no instruction there changes it)
    let mut carry = c0;
    loop {
        match pc {
            0x7BA1 => {
                y = 0;
                m.w(p, prop(m, p1, y) & 1);
                m.w(p + 3, 0);
                a = prop(m, p3, y) & 1;
                m.w(p + 6, a);
                let c = m.r(l.obj_probe_col).wrapping_add(1);
                m.w(l.obj_probe_col, c);
                y = c;
                if y & 0x80 == 0 {
                    pc = 0x7C07;
                    continue;
                }
                m.w(p + 7, a);
                m.w(p + 4, 0);
                a = m.r(p);
                m.w(p + 1, a);
                y = y.wrapping_add(1);
                if y & 0x80 == 0 {
                    pc = 0x7C21;
                    continue;
                }
                m.w(p + 2, a);
                let v = m.r(p + 7);
                m.w(p + 8, v);
                if dec_width(m) {
                    pc = 0x7C76;
                    continue;
                }
                return carry;
            }
            0x7BDB => {
                if y >= 0x28 {
                    if y >= 0x34 {
                        y = 0xFC;
                        m.w(l.obj_probe_col, y);
                        carry = true;
                        pc = 0x7BA1;
                        continue;
                    }
                    y = 0x27;
                }
                m.w(p, prop(m, p1, y) & 1);
                m.w(p + 3, prop(m, p2, y) & 1);
                m.w(p + 6, prop(m, p3, y) & 1);
                y = y.wrapping_add(1);
                pc = 0x7C07;
            }
            0x7C07 => {
                m.w(p + 1, prop(m, p1, y));
                m.w(p + 4, prop(m, p2, y) & 0x11);
                m.w(p + 7, prop(m, p3, y) & 1);
                y = y.wrapping_add(1);
                pc = 0x7C21;
            }
            0x7C21 => {
                m.w(p + 2, prop(m, p1, y) & 1);
                m.w(p + 8, prop(m, p3, y) & 1);
                pc = 0x7C33;
            }
            0x7C33 => {
                m.w(p + 5, prop(m, p2, y) & 1);
                if y < 0x28 {
                    if dec_width(m) {
                        pc = 0x7C76;
                        continue;
                    }
                    return false;
                }
                if y == 0x28 {
                    let v = m.r(p + 1) & 1;
                    m.w(p + 2, v);
                    let v = m.r(p + 7);
                    m.w(p + 8, v);
                    let v = m.r(p + 4);
                    m.w(p + 5, v);
                    return true;
                }
                let v = m.r(p);
                m.w(p + 1, v);
                m.w(p + 2, v);
                let v = m.r(p + 6);
                m.w(p + 7, v);
                m.w(p + 8, v);
                pc = 0x7C5F;
            }
            0x7C5F => {
                m.w(p + 5, u8::from(x != 3));
                m.w(p + 4, 0);
                // `CPX #$03` with the slot
                return x >= 3;
            }
            0x7C76 => {
                y = y.wrapping_add(1);
                if m.f(Field::Movement, x) & 0x80 != 0 {
                    m.w(p + 1, prop(m, p1, y));
                    m.w(p + 7, prop(m, p3, y) & 1);
                    y = y.wrapping_add(2);
                    pc = if y >= 0x28 { 0x7C5F } else { 0x7C33 };
                    continue;
                }
                y = y.wrapping_add(1);
                if y >= 0x28 {
                    pc = 0x7C5F;
                    continue;
                }
                let v = prop(m, p1, y) | m.r(p + 1);
                m.w(p + 1, v);
                let v = (prop(m, p3, y) & 1) | m.r(p + 7);
                m.w(p + 7, v);
                y = y.wrapping_add(1);
                pc = if y < 0x28 { 0x7C21 } else { 0x7C5F };
            }
            _ => unreachable!(),
        }
    }
}

/// `DEC $9C; BPL`: one more column to scan.
fn dec_width(m: &mut Mem) -> bool {
    let w = m.r(m.l.obj_probe_width).wrapping_sub(1);
    m.w(m.l.obj_probe_width, w);
    w & 0x80 == 0
}

/// `$7A74`: one animation step of object `x` when its delay has run out: the frame counts up (or down, ping-pong:
/// [`Field`] byte 1 bit 7) within the frames of its direction (by `$0268`, or by `$02C1` for a moving class without
/// speed); members of a group step by the group size.
pub fn animate(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    let t = m.rx(l.obj_anim_timer, x).wrapping_sub(1);
    m.wx(l.obj_anim_timer, x, t);
    if t & 0x80 == 0 {
        return;
    }
    let d = m.f(Field::AnimDelay, x);
    m.wx(l.obj_anim_timer, x, d);
    let b_frames = if m.f(Field::Class, x) != 0 && m.f(Field::Speed, x) == 0 {
        m.rx(l.object_state, x) & 0x80 != 0
    } else {
        m.rx(l.obj_dir, x) != 0
    };
    let (mut y, a) = if b_frames {
        (m.f(Field::FramesB, x), m.f(Field::FramesBEnd, x))
    } else {
        (m.f(Field::FramesA, x), m.f(Field::FramesAEnd, x))
    };
    m.w(l.anim_limit, a);
    let pingpong = m.rx(l.obj_flags, x) & 0x80 != 0;
    let grouped = m.rx(l.group_members, x) != 0;
    let stride = m.f(Field::Group, x);
    let f = l.player_frames;
    // `true` = count down (`$7ABD`)
    let down = if pingpong {
        if m.rx(l.anim_phase, x) != 0 {
            false
        } else {
            let last = m.r(l.anim_limit);
            m.w(l.anim_limit, y);
            y = last;
            if last != 0 { true } else { y > m.r(l.anim_limit) }
        }
    } else {
        y > m.r(l.anim_limit)
    };
    let limit = m.r(l.anim_limit);
    let wrapped = if down {
        let mut v = m.rx(f, x).wrapping_sub(1);
        if grouped {
            v = v.wrapping_sub(stride);
        }
        m.wx(f, x, v);
        limit >= v
    } else {
        let mut v = m.rx(f, x).wrapping_add(1);
        if grouped {
            v = v.wrapping_add(stride);
        }
        m.wx(f, x, v);
        limit < v
    };
    if !wrapped {
        return;
    }
    if pingpong {
        let mut a = limit;
        if m.rx(l.anim_phase, x) != 0 {
            a = a.wrapping_sub(stride).wrapping_sub(1);
        }
        m.wx(f, x, a);
    } else {
        m.wx(f, x, y);
    }
    let ph = m.rx(l.anim_phase, x) ^ 1;
    m.wx(l.anim_phase, x, ph);
}

/// `$79F8` (`dir = $5D`) / `$79FA` (`dir` = the caller's `A`): object `x` moves `$5C` pixels, left when `dir` != 0, with
/// the x high bit.
pub fn move_x(ram: &mut [u8], l: &C30aLayout, x: u8, dir: u8) {
    let mut m = Mem { ram, l };
    let ox = m.rx(l.player_x, x);
    let dx = m.r(l.obj_dx);
    let carry = if dir != 0 {
        m.wx(l.player_x, x, ox.wrapping_sub(dx));
        ox < dx
    } else {
        m.wx(l.player_x, x, ox.wrapping_add(dx));
        u16::from(ox) + u16::from(dx) > 0xFF
    };
    if carry {
        let v = m.r(l.sprite_x_msb) ^ m.bit(x);
        m.w(l.sprite_x_msb, v);
    }
}

/// `$79D0`: object `x` turns around (`$0268` flips, the frame jumps to the other direction's first frame when the two
/// differ), unless it is the object Giana just rode (`$6E`, which is cleared instead).
pub fn turn(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    if x == m.r(l.ride_slot) {
        m.w(l.ride_slot, 0);
        return;
    }
    let d = m.rx(l.obj_dir, x) ^ 1;
    m.wx(l.obj_dir, x, d);
    let (a, b) = (m.f(Field::FramesB, x), m.f(Field::FramesA, x));
    let f = if d != 0 { a } else { b };
    if a != b {
        m.wx(l.player_frames, x, f);
    }
}

/// `$7A17`: the state of object `x` flips (`$02C1 ^= $FE`); without horizontal speed the frame follows (B frames when
/// negative) and `$02B8` = `$FF`.
pub fn flip(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    let s = m.rx(l.object_state, x) ^ 0xFE;
    m.wx(l.object_state, x, s);
    if m.f(Field::Speed, x) != 0 {
        return;
    }
    let f = if s & 0x80 != 0 { m.f(Field::FramesB, x) } else { m.f(Field::FramesA, x) };
    m.wx(l.player_frames, x, f);
    m.wx(l.obj_saved_y, x, 0xFF);
}

/// `$7A3A`: the power-up (first frame `$AC`) outside a treasure room: frames by the power level, colour and multicolour
/// from the table `$7A6C[$FD]`. Returns `Some(power)` when it ran (the original leaves it in `Y`).
pub fn powerup_colour(ram: &mut [u8], vic: &mut [u8], l: &C30aLayout, x: u8) -> Option<u8> {
    let mut m = Mem { ram, l };
    if m.r(l.in_room) != 0 || m.f(Field::FramesA, x) != 0xAC {
        return None;
    }
    let y = m.r(l.power);
    if y != 0 {
        m.w(Field::FramesAEnd.at(l, x), 0xAC);
        m.w(Field::FramesB.at(l, x), 0xAC);
    }
    let c = m.rx(l.powerup_colours, y);
    m.wx(l.obj_colour, x, c);
    vic[VIC_COLOR + x as usize] = c;
    vic[VIC_MULTICOLOR] =
        if c & 0x80 == 0 { m.bit(x) | vic[VIC_MULTICOLOR] } else { m.mask(x) & vic[VIC_MULTICOLOR] };
    Some(y)
}

/// `$8277`: `$1F` = 1 when Giana's x is within -19..+19 of object `x`, else 0.
pub fn near(ram: &mut [u8], l: &C30aLayout, x: u8) {
    let mut m = Mem { ram, l };
    let d = m.rx(l.player_x, x).wrapping_sub(m.r(l.player_x));
    let on = d.wrapping_sub(0x14) & 0x80 != 0 && d.wrapping_sub(0xED) & 0x80 == 0;
    m.w(l.on_object, u8::from(on));
}

/// `$81C6`: Giana stands on object `$22` (`$1F` != 0): she stays on it while she is over it and not jumping, the state
/// of the object (or its leader) reacts to the tiles around Giana (`$45..$4D` from the player's probe), she is carried
/// along horizontally (walking speed, or the scroll request `$F8` at the scroll edge), and stands on its top. Returns the
/// original's `X` (the object, or its leader).
pub fn ride(ram: &mut [u8], vic: &[u8], l: &C30aLayout, x_in: u8) -> u8 {
    if ram[l.on_object as usize] == 0 {
        return x_in;
    }
    let mut x = ram[l.object_index as usize];
    near(ram, l, x);
    let mut m = Mem { ram, l };
    if m.r(l.vert_dir) == 1 {
        m.w(l.on_object, 0);
    }
    let ld = m.rx(l.group_leader, x);
    if ld != 0 {
        x = ld;
    }
    let pp = |m: &Mem, i: u16| m.r(l.probe_props + i) & 1 != 0;
    let s = m.rx(l.object_state, x);
    if s != 0 {
        if s & 0x80 != 0 {
            let set = pp(&m, 1) || ((pp(&m, 0) || pp(&m, 2)) && !(pp(&m, 3) || pp(&m, 5)));
            if set {
                m.wx(l.object_state, x, 1);
                let v = m.rx(l.obj_step, x).wrapping_add(1);
                m.wx(l.obj_step, x, v);
            }
        } else if pp(&m, 7) || (!(pp(&m, 3) || pp(&m, 5)) && (pp(&m, 6) || pp(&m, 8))) {
            m.w(l.on_object, 0);
            return x;
        }
    }
    let speed = m.f(Field::Speed, x);
    // `Some(v)` = `STY $70`
    let mut new_x = None;
    if m.rx(l.obj_dir, x) == 0 {
        if !pp(&m, 5) {
            let gx = m.r(l.player_x);
            if gx >= 0xA0 && m.r(l.scroll_column) != 0 {
                m.w(l.scroll_px, speed & 0x7F);
            } else {
                let s = u16::from(gx) + u16::from(speed);
                if s > 0xFF {
                    let v = m.r(l.sprite_x_msb) | 7;
                    m.w(l.sprite_x_msb, v);
                }
                new_x = Some(s as u8);
            }
        }
    } else if !pp(&m, 3) {
        let gx = m.r(l.player_x);
        let y = gx.wrapping_sub(speed);
        if gx < speed {
            let v = m.r(l.sprite_x_msb) & 0xF8;
            m.w(l.sprite_x_msb, v);
        }
        if m.r(l.sprite_x_msb) & 1 != 0 || y >= 0x1E {
            new_x = Some(y);
        } else {
            turn(m.ram, l, x);
        }
    }
    let mut m = Mem { ram, l };
    if let Some(v) = new_x {
        m.w(l.player_x, v);
    }
    let oy = m.rx(l.player_y, x);
    let top = if vic[VIC_COLOR + x as usize] | 0xF0 == 0xFF { 0x17 } else { 0x12 };
    m.w(l.player_y, oy.wrapping_sub(top));
    x
}

/// `$8188`: with `$2F` set (Giana rides the lift that ends the stage): the time tick held, Giana faces where the lift
/// is, and once she is low enough she sinks 2 pixels a frame with the logic frozen; at y >= `$FC` the input comes back
/// (`$8F95` = `LDA #`) and the stage is done (`$98` = 1).
pub fn lift(ram: &mut [u8], l: &C30aLayout) {
    let mut m = Mem { ram, l };
    m.w(l.time_frames, 0x28);
    let x = m.r(l.object_index);
    let y = m.rx(l.group_leader, x);
    let mut a = m.rx(l.player_x, y).wrapping_sub(m.r(l.player_x));
    if a.wrapping_sub(0x12) & 0x80 == 0 {
        a = 0xFF;
        m.w(l.walk_dir, a);
    }
    if a.wrapping_sub(0xBF) & 0x80 != 0 {
        m.w(l.walk_dir, 0x80);
        m.w(l.walk_scroll, 0);
    }
    let gy = m.r(l.player_y);
    if gy < 0xE8 {
        return;
    }
    m.w(l.player_y, gy.wrapping_add(2));
    m.w(l.on_object, 0);
    let s = m.r(l.game_state).wrapping_sub(1);
    m.w(l.game_state, s);
    if gy >= 0xFC {
        m.w(l.read_input, 0xA9);
        m.w(l.stage_end, 1);
    }
}
