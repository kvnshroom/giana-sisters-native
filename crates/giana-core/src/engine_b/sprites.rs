//! Engine B, sprite program I (work package E4): the part of the hack's sprite control program (`$7700..$8EFF`, see
//! `docs/rom-hack-family-b.md` section 7) that handles the object list spawner, the slot arrays, walkers, hoppers and
//! scripted movers, the sprite collisions with Giana and her shot, scoring and the clean-up of sprites that left the
//! screen. It is a port of the hack's routines (addresses in the comments are those of the Ridley X image).
//!
//! The hack keeps its state in fixed RAM like the original; the port keeps the layout where it matters:
//!
//! - `$0700..$07FF` (the slot arrays, 8 bytes per property, slot X = 4..7): [`Sprites::m`], addressed by the C64 address;
//! - the scratch bytes `$0474..$0493` and `$046C` (a block was bumped): `GameState::v47`, shared with the original's;
//! - zero page scratch `$0A..$0D`, `$EC..$F9`: [`Sprites::z`];
//! - the sprite registers: `GameState::spr/sptr/en.vic_*` as for the original engine (sprite 3 is Giana's shot,
//!   slots 4..7 are the objects).
//!
//! Not in here (later packages): platforms (class `$7F`), plants and chain followers (chains of sprites), ladders,
//! boss kinds 3 and 4, the second weapon of power level 4 and boss hits. Objects of such types are skipped at the
//! spawner (they never appear) so that no half implemented object can kill Giana.

use super::EngineB;
use crate::image::img;
use crate::state::GameState;

/// What follows the vertical movement of an object: `$7DE1` (turn at a wall, then move) or `$7DFB` (only move).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tail {
    Turn,
    Move,
}

/// State of the sprite program that is not part of the original engine's state.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Sprites {
    /// `$0700..$07FF`
    #[serde(with = "giana_engine_b::snap::arr")]
    pub m: [u8; 0x100],
    /// zero page scratch (`$0A..$0D`, `$EC..$F9`), indexed by address
    #[serde(with = "giana_engine_b::snap::arr")]
    pub z: [u8; 0x100],
    /// `$FE` bit 7: the stage was just built, the objects of the first screen are spawned
    pub fe: bool,
    /// the self modified operand `$7B83` of the chain linking (`$E0`, `$E4`, ... = the link array of the next follower)
    pub chain_op: u8,
    /// Measured (`rx36_scripted_platforms`): the objects of the stage start fill do not move in their first frame and move
    /// twice in the next one (the script counter of a platform shows 100, 98, 97). 1 = the double step is due.
    pub lag: u8,
}

impl Sprites {
    pub fn new() -> Sprites {
        let mut z = [0; 0x100];
        // `$F0` (the flame's flicker state of the dragon, `$8D30`) holds `$87` from earlier code when the first stage frame
        // runs (recorded in `rx42_boss_idle`); a start value of 0 would shift the flame's shapes by 2
        z[0xF0] = 0x87;
        Sprites { m: [0; 0x100], z, fe: true, lag: 0, chain_op: 0xE0 }
    }
}

impl Default for Sprites {
    fn default() -> Self {
        Sprites::new()
    }
}

/// The Y position of an object in row `row` (`$3205,X`), before the sprite offset is added.
pub(super) fn row_y(row: u8) -> u8 {
    img(0x3205 + row as usize)
}

impl GameState {
    #[inline]
    pub(super) fn eb(&self) -> &EngineB {
        self.engine_b.as_ref().expect("engine B")
    }
    /// A byte of the program's memory as the sprite program sees it: the loaded monster file lives at `$7505` (its record is
    /// followed by the movement script), everything else comes from the image.
    pub(super) fn b_mem(&self, a: usize) -> u8 {
        if let Some(g) = &self.eb().gem {
            return g.byte(a);
        }
        let m = &self.eb().monster_mem;
        match a.checked_sub(0x7505).and_then(|i| m.get(i)) {
            Some(&v) => v,
            None => img(a),
        }
    }
    /// A byte of the slot arrays `$0700..$07FF` (engine B; 0 for the original engine).
    pub fn b_slot(&self, a: usize) -> u8 {
        self.engine_b.as_ref().and_then(|b| b.sprites.m.get(a.wrapping_sub(0x700)).copied()).unwrap_or(0)
    }
    /// Write a byte of the slot arrays (for `platform.rs`, which shares `$0774/$07B4/$07D0/$07BC` with the sprite program).
    pub(crate) fn b_set_slot(&mut self, a: usize, v: u8) {
        if let Some(b) = self.engine_b.as_mut() {
            if let Some(m) = b.sprites.m.get_mut(a.wrapping_sub(0x700)) {
                *m = v;
            }
        }
    }
    #[inline]
    pub(crate) fn bm_chain(&self, slot: usize) -> bool {
        self.bm(0x79C + slot) != 0
    }
    #[inline]
    pub(super) fn bm(&self, a: usize) -> u8 {
        self.eb().sprites.m[a - 0x700]
    }
    #[inline]
    pub(super) fn set_bm(&mut self, a: usize, v: u8) {
        self.engine_b.as_mut().expect("engine B").sprites.m[a - 0x700] = v;
    }
    #[inline]
    pub(super) fn bz(&self, a: usize) -> u8 {
        self.eb().sprites.z[a]
    }
    #[inline]
    pub(super) fn set_bz(&mut self, a: usize, v: u8) {
        self.engine_b.as_mut().expect("engine B").sprites.z[a] = v;
    }
    /// Sprite bit mask of sprite `x` (`$3224,X`)
    #[inline]
    pub(super) fn bit(&self, x: usize) -> u8 {
        self.d32(0x3224 + x)
    }
    /// Inverted sprite bit mask (`$322C,X`)
    #[inline]
    pub(super) fn nbit(&self, x: usize) -> u8 {
        self.d32(0x322C + x)
    }
    /// `LDA $9C20,X; LSR A` carry: bit 0 of the tile class (solid)
    #[inline]
    pub(super) fn b_solid(&self, ch: u8) -> bool {
        self.tile_flags(ch) & 1 != 0
    }

    /// The place of the hack's frame handler `$776E` from `$77E5` on: spawn, move, collide, clean up, shoot. Called once
    /// per frame while the game runs (`enemy_main_sub_2194`).
    pub(crate) fn b_sprite_frame_impl(&mut self) {
        if self.engine_b.is_none() {
            return;
        }
        let first = self.eb().sprites.fe;
        self.b_spawn(); // $7846
        // the start fill frame does not move anything, the next one moves twice (see `Sprites::lag`)
        let lag = std::mem::replace(&mut self.engine_b.as_mut().unwrap().sprites.lag, u8::from(first));
        if !first {
            self.b_move_all(); // $7C84
            if lag != 0 {
                self.b_move_all();
            }
        }
        self.b_collisions(); // $8064
        self.b_platform_ride(); // $890A
        self.b_wrap_hold(); // $7BC7
        self.b_retire_vertical(); // $7C44
        self.b_chain_frame(); // $2D2D -> $8BAE
        self.b_breath(); // $8CB9
        self.b_shot_fire(); // $82A7
        self.b_shot_flight(); // $8312
    }

    // ---- spawner $7846 / $7869 ---------------------------------------------------------------------------------------------------------

    /// `$7846`: at a new stage the objects of the first screen are spawned column by column, then the normal spawner runs.
    fn b_spawn(&mut self) {
        if self.eb().sprites.fe {
            // `$0456 != 0` (checkpoint start): `$21 = $59`, else 0; 39 columns
            let base = if self.spawn_variant == 0 { 0 } else { 0x59 };
            self.map_column = base;
            // WIDESCREEN ON: the columns up to the new right edge are part of the first screen
            let extra = self.wide_k_now();
            for i in 0..=0x26 + extra as usize {
                self.b_spawn_column(true);
                if i > 0x26 {
                    self.wide_stats.prefilled += 1;
                }
                self.map_column = self.map_column.wrapping_add(1);
            }
            self.map_column = base.wrapping_add(0x27);
            self.wide_rebuild_left();
            self.engine_b.as_mut().unwrap().sprites.fe = false;
        }
        self.b_spawn_column(false);
    }

    /// `$7869`: spawn the objects whose trigger column has been reached.
    fn b_spawn_column(&mut self, prefill: bool) {
        let stage = self.stage as u8;
        let mut first = false;
        if stage != self.v47(0x484) {
            self.set_v47(0x484, stage);
            self.engine_b.as_mut().unwrap().spawn_next = 0;
            first = true;
            if !prefill && self.wide_on() {
                self.wide_rebuild_left();
            }
        }
        // WIDESCREEN ON: the trigger looks `k` columns ahead (not in the loop of the first screen, which has the columns itself)
        let k = if prefill { 0 } else { self.wide_k_now() };
        let look = self.map_column as u16 + k as u16;
        if self.map_column.wrapping_add(k) == self.v47(0x475) {
            return;
        }
        self.set_v47(0x475, self.map_column.wrapping_add(k));
        if self.en.vic_me == 0xFF {
            return;
        }
        // treasure rooms: their object list is not known (see the doc, 5.4)
        if self.eb().is_room(self.eb().current.number) {
            return;
        }
        // The hack indexes the object list with the byte `$0474` (offset of an entry, +3 per object): lists of more than 85
        // objects wrap and are read misaligned. The port keeps the offset in `spawn_next` and reads the entries as bytes
        // (the list is rebuilt from the parsed objects, `$FF` ends it; what lies behind the list is unknown, it ends there too).
        let raw: Vec<u8> = {
            let b = self.eb();
            let mut v: Vec<u8> = b.current.spawns.iter().flat_map(|s| [s.trigger, s.row, s.kind]).collect();
            v.push(0xFF);
            v
        };
        // ON, after a room (the first screen is not rebuilt, the objects scroll in): the columns the C64 would create beyond its
        // edge one by one are due at once, each at its own X (one pass per column)
        let m0 = self.map_column as u16;
        if first && k != 0 {
            for d in 0..k as u16 {
                self.wide_back = 8 * (k as u16 - d);
                if self.b_spawn_scan(&raw, m0 + d, prefill) {
                    self.wide_stats.prefilled += 1;
                }
            }
            self.wide_back = 0;
        }
        self.b_spawn_scan(&raw, look, prefill);
    }

    /// One look at the object list `raw` for the map column `look`: the entries before it are skipped, the one at it is created.
    /// True when an entry was created.
    fn b_spawn_scan(&mut self, raw: &[u8], look: u16, prefill: bool) -> bool {
        let rd = |o: u8| raw.get(o as usize).copied().unwrap_or(0xFF);
        // at most one round over all offsets (the hack would loop for ever on a list without a way out)
        for _ in 0..=256 {
            let o = self.eb().spawn_next as u8;
            let t = rd(o);
            if t == 0xFF {
                return false;
            }
            let x = t.wrapping_sub(1) as u16;
            if x == look {
                self.engine_b.as_mut().unwrap().spawn_next = o.wrapping_add(3) as usize;
                self.b_create(rd(o.wrapping_add(1)), rd(o.wrapping_add(2)), t, prefill);
                return true;
            } else if x > look {
                return false;
            } else {
                self.engine_b.as_mut().unwrap().spawn_next = o.wrapping_add(3) as usize;
            }
        }
        false
    }

    /// `$78B5..`: create one object (for a chain: the head and its followers).
    fn b_create(&mut self, row: u8, kind: u8, trigger: u8, prefill: bool) {
        self.set_d32(0x3221, trigger);
        self.set_d32(0x3222, row);
        let kind = if kind == 0x0B { img(0x8ECF + self.anim_counter as usize) } else { kind };
        self.set_d32(0x3223, kind);
        let Some(rec) = self.eb().type_record(kind).map(|r| r.raw) else { return };
        if !prefill {
            self.wide_stats.spawned += 1;
        }
        if let Some(l) = self.wide_stats.log.as_mut() {
            l.push((self.stage as u8, trigger, row, kind));
        }
        self.set_bz(0xF4, 0xFF);
        if rec[19] == 3 {
            // `$78F0`: a chain of four takes all four enemy sprites
            if self.wide_on() && !prefill && (4..8).any(|s| self.wide_enemy_in_view(s)) {
                // ON: it does not wipe enemies that the player sees: the chain is not created
                self.wide_stats.lost_spawns += 1;
                return;
            }
            if self.en.vic_me & 0xF0 != 0 {
                self.wide_stats.wipes += 1;
            }
            self.en.vic_me &= 0x0F;
        }
        // `$78F8`: one pass per sprite of a chain (a chain of n followers has n + 1 sprites)
        for _ in 0..=256 {
            if self.b_create_one(&rec, row, prefill) {
                return;
            }
        }
    }

    /// `$78F8..$7B98`: the slot, the record and the position of one sprite. True: the object is complete.
    fn b_create_one(&mut self, rec: &[u8; 21], row: u8, prefill: bool) -> bool {
        // slot: the highest free one; if all are in use one that is squashed/carried (`$07C4`), else 7
        let me = self.en.vic_me;
        let mut slot = None;
        for y in (4..=7).rev() {
            if me & (1 << y) == 0 {
                slot = Some(y);
                break;
            }
        }
        let mut x = slot.unwrap_or_else(|| (4..=7).rev().find(|&y| self.bm(0x7C4 + y) != 0).unwrap_or(7));
        if slot.is_none() && self.bm(0x7C4 + x) == 0 && rec[3] < 3 {
            if self.wide_on() && !prefill && self.bz(0xF4) & 0x80 != 0 && self.wide_enemy_in_view(x) {
                // ON: no free sprite, and slot 7 holds an enemy that the player sees: it stays, the new object is lost
                self.wide_stats.lost_spawns += 1;
                return true;
            }
            self.wide_stats.overwrites += 1;
        }
        let boss = rec[3] >= 3;
        if boss {
            // `$791E`: bosses always use slot 7 and clear the state of the parts
            self.set_d32(0x3252, 0);
            for a in [0x7CB, 0x7CA, 0x7C9, 0x7C8, 0x7A0, 0x7A1, 0x7A2, 0x7A3] {
                self.set_bm(a, 0);
            }
            x = 7;
        }
        self.set_v47(0x476, x as u8);
        // `$7940`: flags and the 18 property bytes
        self.set_bm(0x718 + x, rec[1] & 0x80);
        self.set_bm(0x714 + x, rec[1] & 0x40);
        for i in 0..18 {
            self.set_bm(0x71C + 8 * i + x, rec[3 + i]);
        }
        if boss {
            self.b_boss_spawn(rec, rec[3], row);
            return true;
        }
        // `$7A84`: single sprite
        self.set_sptr(x, rec[0]);
        let script = self.bm(0x754 + x) as usize | (self.bm(0x75C + x) as usize) << 8;
        self.set_bm(0x7D8 + x, self.b_mem(script));
        for (a, v) in [(0x7B4, 1), (0x7D0, 1), (0x7F8, 1), (0x7C4, 0), (0x7DC, 0), (0x7E0, 0), (0x7E4, 0), (0x7E8, 0), (0x7EC, 0), (0x7F0, 0)] {
            self.set_bm(a + x, v);
        }
        self.set_bm(0x7BC + x, self.bm(0x75C + x));
        self.set_bm(0x7AC + x, self.bm(0x77C + x));
        let (mask, nmask) = (self.bit(x), self.nbit(x));
        if self.bm(0x744 + x) == 2 {
            // hidden item: invisible until a block is bumped
            self.set_v47(0x46C, 0);
            self.set_v47(0x492, rec[1]);
            self.set_v47(0x493, rec[2]);
            self.en.vic_mmc &= nmask;
            self.en.vic_mdp |= mask;
            self.en.vic_col[x] = self.en.vic_bg1;
            self.set_bm(0x7B4 + x, 0);
        } else {
            if rec[1] & 1 != 0 {
                self.en.vic_mmc &= nmask;
            } else {
                self.en.vic_mmc |= mask;
            }
            if rec[1] & 2 != 0 {
                self.en.vic_mdp |= mask;
            } else {
                self.en.vic_mdp &= nmask;
            }
            self.set_bm(0x7CC + x, rec[2]);
            self.en.vic_col[x] = rec[2] & 0x7F;
        }
        // `$7B3A`: position
        if prefill {
            // `$7B9E`: the object stands where its column is now: (column - $58 + 2) * 8
            let mut c = self.map_column;
            if self.spawn_variant != 0 {
                c = c.wrapping_sub(0x58);
            }
            let c = c.wrapping_add(2);
            self.set_spr(2 * x, c << 3);
            if c & 0x20 != 0 {
                self.sprite_x_msb |= mask;
            }
        } else {
            self.sprite_x_msb |= mask;
            self.set_spr(2 * x, 0x4F + (self.wide_e() - self.wide_back) as u8);
        }
        let ty = img(0x3205 + row as usize).wrapping_add(0x1D); // bounds: `img` wraps at 64K, rows come from untrusted files
        self.set_spr(2 * x + 1, ty);
        self.en.vic_me |= mask;
        // `$7B66`: chains
        if self.bm(0x79C + x) == 0 {
            return true;
        }
        self.b_chain_link(x)
    }

    // ---- the loop over the slots $7C84 -----------------------------------------------------------------------------------------------

    fn b_move_all(&mut self) {
        // `$0494`: after the F-key cheat the objects stand still for `$0495` frames
        if self.v47(0x494) != 0 {
            let n = self.v47(0x495).wrapping_sub(1);
            self.set_v47(0x495, n);
            if n != 0 {
                return;
            }
            self.set_v47(0x494, 0);
        }
        let me = self.en.vic_me;
        for x in (4..=7usize).rev() {
            if me & (1 << x) == 0 {
                continue;
            }
            self.set_v47(0x47A, x as u8);
            self.set_v47(0x47B, 2 * x as u8);
            let kind = self.bm(0x71C + x);
            if kind >= 3 {
                // `$7CB1`: kind 4 does nothing; a kind 3 boss moves (`$8611`) and ends the frame's loop
                if kind != 4 {
                    self.b_boss_move();
                }
                return;
            }
            self.b_move_slot(x, kind);
        }
    }

    fn b_move_slot(&mut self, x: usize, kind: u8) {
        if self.b_hidden(x) || self.b_carried(x) {
            self.b_hit_colour(x);
            return;
        }
        self.b_probe(x);
        match kind {
            2 => self.b_script_mover(x),
            0 => {
                let t = self.b_walker(x);
                self.b_tail(x, t);
            }
            _ => {
                let t = self.b_hopper(x);
                self.b_tail(x, t);
            }
        }
        self.b_animate(x); // $7E15
        self.b_hit_colour(x); // $7E18
    }

    /// `$7DE1` (turn at a wall first) or `$7DFB` (only move).
    fn b_tail(&mut self, x: usize, t: Tail) {
        if t == Tail::Turn {
            let tile = if self.bm(0x7B4 + x) != 0 { self.v47(0x47C) } else { self.v47(0x481) };
            if self.b_solid(tile) {
                self.b_turn(x);
            }
        }
        self.set_v47(0x482, self.bm(0x774 + x) & 0x7F);
        self.set_v47(0x483, self.bm(0x7B4 + x));
        self.b_chase_face(x); // $8C24
        self.b_move_x(x, 2 * x); // $7E3E
    }

    /// `$7F58`: objects that wait for a block to be bumped (classes 2..`$11`) or that finish their reveal; true = done
    /// for this frame.
    fn b_hidden(&mut self, x: usize) -> bool {
        let c = self.bm(0x744 + x);
        if c == 0x11 {
            let m = self.nbit(x);
            self.en.vic_mdp &= m;
        } else if c == 0x12 {
            self.set_v47(0x46C, 0);
            return false;
        } else if !(2..=0x12).contains(&c) {
            return false;
        }
        if self.v47(0x46C) == 0 {
            return true;
        }
        if self.v47(0x492) & 1 != 0 {
            let m = self.nbit(x);
            self.en.vic_mmc &= m;
        } else {
            let m = self.bit(x);
            self.en.vic_mmc |= m;
        }
        let c = self.v47(0x493);
        self.set_bm(0x7CC + x, c);
        self.en.vic_col[x] = c & 0x7F;
        let n = self.bm(0x744 + x).wrapping_add(1);
        self.set_bm(0x744 + x, n);
        let y = self.spr(2 * x + 1).wrapping_sub(2);
        self.set_spr(2 * x + 1, y);
        true
    }

    /// `$7F34`: squashed/carried objects fall until they stand on something.
    fn b_carried(&mut self, x: usize) -> bool {
        if self.bm(0x7C4 + x) == 0 {
            return false;
        }
        self.b_probe(x);
        if !self.b_solid(self.v47(0x47E)) {
            let y = self.spr(2 * x + 1).wrapping_add(4);
            self.set_spr(2 * x + 1, y);
        }
        true
    }

    /// `$7E72`: the tiles around the object from the screen RAM into `$047C..$0481`; `$0C/$0D` keep the pointer.
    pub(super) fn b_probe(&mut self, x: usize) {
        let xs = 2 * x;
        let ypos = self.spr(xs + 1);
        if ypos >= 0xE1 {
            for a in [0x47D, 0x47E, 0x47F] {
                self.set_v47(a, 0);
            }
            return;
        }
        let r0a = ypos.wrapping_sub(0x23);
        // WIDESCREEN ON: an object outside of the 38 columns reads the cells of the wide picture instead of stale values
        if let Some(t) = self.wide_probe(x, (r0a >> 3) as usize, 0x9BB9) {
            for (a, v) in [0x47C, 0x481, 0x47D, 0x47E, 0x47F].into_iter().zip(t) {
                self.set_v47(a, v);
            }
            return;
        }
        let hi = u8::from(self.bit(x) & self.sprite_x_msb != 0);
        let (r0c, b) = self.spr(xs).overflowing_sub(self.scroll_fine);
        let r0d = hi.wrapping_sub(b as u8);
        self.set_bz(0x0C, r0c);
        self.set_bz(0x0D, r0d);
        if r0d == 0 {
            if r0c < 0x15 {
                return;
            }
        } else if r0c >= 0x4C {
            return;
        }
        let (r0c, b) = r0c.overflowing_sub(0x15);
        let r0d = r0d.wrapping_sub(b as u8);
        let carry = r0d & 1;
        let r0d = r0d >> 1;
        let a = ((carry << 7) | (r0c >> 1)) >> 2;
        let row = (r0a >> 3) as usize;
        let (lo, c) = a.overflowing_add(img(0x9BA0 + row));
        let hi2 = r0d.wrapping_add(img(0x9BB9 + row)).wrapping_add(c as u8).wrapping_add(0xC8);
        self.set_bz(0x0C, lo);
        self.set_bz(0x0D, hi2);
        let ptr = ((hi2 as usize) << 8) | lo as usize;
        self.set_v47(0x47C, self.b_scr(ptr));
        self.set_v47(0x481, self.b_scr(ptr + 2));
        self.set_v47(0x47D, self.b_scr(ptr + 0x28));
        self.set_v47(0x47E, self.b_scr(ptr + 0x29));
        self.set_v47(0x47F, self.b_scr(ptr + 0x2A));
    }

    /// A byte of the screen RAM (`$C800..`), anything else from the image.
    pub(super) fn b_scr(&self, addr: usize) -> u8 {
        if (0xC800..0xC800 + 1000).contains(&addr) {
            self.screen_ram[addr - 0xC800]
        } else {
            img(addr)
        }
    }

    // ---- walkers and hoppers ------------------------------------------------------------------------------------------------------

    /// `$7D83` (kind 0): fall, turn at edges.
    fn b_walker(&mut self, x: usize) -> Tail {
        let mode = self.bm(0x784 + x);
        let (t_d, t_e, t_f) = (self.v47(0x47D), self.v47(0x47E), self.v47(0x47F));
        let mut fall = false;
        if mode == 0 {
            fall = !(self.b_solid(t_d) || self.b_solid(t_e) || self.b_solid(t_f));
        } else if mode != 2 {
            if !self.b_solid(t_e) {
                fall = true;
            } else if self.bm(0x7B4 + x) != 0 {
                if !self.b_solid(t_d) {
                    self.b_turn(x);
                }
            } else if !self.b_solid(t_f) {
                self.b_turn(x);
            }
        }
        if fall {
            let y = self.spr(2 * x + 1).wrapping_add(4);
            self.set_spr(2 * x + 1, y);
        }
        Tail::Turn
    }

    /// `$7E24`: turn around.
    fn b_turn(&mut self, x: usize) {
        let f = self.bm(0x7B4 + x) ^ 1;
        self.set_bm(0x7B4 + x, f);
        let p = if f != 0 { self.bm(0x734 + x) } else { self.bm(0x724 + x) };
        self.set_sptr(x, p);
    }

    /// `$7E3E`: move sprite offset `xs` (slot `x`) by `$0482` in direction `$0483`, fixing the X MSB.
    pub(super) fn b_move_x(&mut self, x: usize, xs: usize) {
        let amount = self.v47(0x482);
        let toggle = if self.v47(0x483) != 0 {
            let (v, borrow) = self.spr(xs).overflowing_sub(amount);
            self.set_spr(xs, v);
            borrow
        } else {
            let (v, carry) = self.spr(xs).overflowing_add(amount);
            self.set_spr(xs, v);
            carry
        };
        if toggle {
            self.sprite_x_msb ^= self.bit(x);
        }
    }

    /// `$8C24`: objects with bit 7 of the speed turn towards Giana.
    fn b_chase_face(&mut self, y: usize) {
        if self.bm(0x774 + y) & 0x80 == 0 {
            return;
        }
        let ys = 2 * y;
        let msb = self.sprite_x_msb;
        let face_left;
        // `$0604 & 1` is Giana's X MSB
        let enemy_msb = msb & self.bit(y) != 0;
        let giana_msb = msb & 1 != 0;
        if !giana_msb {
            if !enemy_msb {
                face_left = self.player_x_low < self.spr(ys);
            } else {
                face_left = true;
            }
        } else if enemy_msb {
            face_left = self.player_x_low < self.spr(ys);
        } else {
            face_left = false;
        }
        if face_left {
            if self.bm(0x7B4 + y) == 0 {
                self.set_bm(0x7B4 + y, 1);
                let p = self.bm(0x734 + y);
                self.set_sptr(y, p);
            }
        } else if self.bm(0x7B4 + y) != 0 {
            self.set_bm(0x7B4 + y, 0);
            let p = self.bm(0x724 + y);
            self.set_sptr(y, p);
        }
    }

    /// `$8C6F`: hoppers with bit 7 of the arc byte jump up/down towards Giana's height.
    fn b_chase_jump(&mut self, x: usize) {
        if self.bm(0x754 + x) & 0x80 != 0 && self.bm(0x7F4 + x) & 0x80 == 0 {
            let ys = 2 * x;
            if self.player_y_c64 >= self.spr(ys + 1) {
                // Giana is lower (`$8CA3`: already falling goes on to `$8C9D`, which clears `$07F4`)
                if self.bm(0x7D0 + x) == 0 {
                    self.set_bm(0x7F4 + x, 0);
                    return;
                }
                self.set_bm(0x7D0 + x, 0);
                if self.bm(0x774 + x) == 0 {
                    let p = self.bm(0x724 + x);
                    self.set_sptr(x, p);
                }
                return;
            }
            if self.bm(0x7D0 + x) == 0 {
                self.set_bm(0x7D0 + x, 1);
                if self.bm(0x774 + x) == 0 {
                    let p = self.bm(0x734 + x);
                    self.set_sptr(x, p);
                }
            }
        }
        self.set_bm(0x7F4 + x, 0);
    }

    /// `$87CB`: switch between rising and falling.
    fn b_flip_vertical(&mut self, x: usize) {
        let f = self.bm(0x7D0 + x) ^ 1;
        self.set_bm(0x7D0 + x, f);
        if self.bm(0x774 + x) == 0 {
            let p = if f != 0 { self.bm(0x734 + x) } else { self.bm(0x724 + x) };
            self.set_sptr(x, p);
            self.set_bm(0x7F4 + x, 0xFF);
        }
    }

    /// `$7CE5` (kind 1): the hop on the arc table `$9ADE`.
    fn b_hopper(&mut self, x: usize) -> Tail {
        let xs = 2 * x;
        let f1 = self.bm(0x754 + x) & 0x7F;
        self.set_bz(0xF1, f1);
        let mode = self.bm(0x784 + x);
        // `$7CEC`: the branch is chosen first, then the chase code `$8C6F` runs in either of them (it may switch `$07D0`,
        // which only matters from the next frame on, and the falling code below reads it as its step)
        let rising = self.bm(0x7D0 + x) != 0;
        self.b_chase_jump(x); // $8C6F
        if !rising {
            // falling (the arc table is walked backwards)
            let a;
            if mode == 1 {
                if self.spr(xs + 1) >= 0xF8 {
                    // `$7D0B`: `$07D0` may have been set by the chase code
                    a = self.bm(0x7D0 + x);
                    if a == 0 {
                        self.b_flip_vertical(x);
                        return Tail::Turn;
                    }
                } else {
                    a = 0;
                }
            } else {
                let n = self.bm(0x7BC + x).wrapping_sub(1);
                self.set_bm(0x7BC + x, n);
                if n == self.bm(0x75C + x) {
                    self.b_flip_vertical(x);
                    return Tail::Turn;
                }
                a = img(0x9ADE + n as usize);
            }
            let y = self.spr(xs + 1).wrapping_add(a).wrapping_add(f1);
            self.set_spr(xs + 1, y);
            return self.b_hop_landing(x);
        }
        // rising
        if mode == 1 {
            if self.spr(xs + 1) < 0x58
                && self.bm(0x7D0 + x) != 0 {
                    self.b_flip_vertical(x);
                    return Tail::Turn;
                }
            self.set_v47(0x488, 0);
        } else {
            let n = self.bm(0x7BC + x).wrapping_add(1);
            self.set_bm(0x7BC + x, n);
            let d = img(0x9ADE + n as usize);
            self.set_v47(0x488, d);
            if d == 0xFF {
                self.b_flip_vertical(x);
                return Tail::Turn;
            }
        }
        let y = self.spr(xs + 1).wrapping_sub(self.v47(0x488)).wrapping_sub(f1);
        self.set_spr(xs + 1, y);
        self.b_hop_ceiling(x)
    }

    /// `$87F2`/`$2D04`: after the upward step check the tiles above (head bump).
    fn b_hop_ceiling(&mut self, x: usize) -> Tail {
        if self.bm(0x784 + x) == 2 {
            return Tail::Move; // $885F
        }
        if self.spr(2 * x + 1) >= 0xE0 {
            return Tail::Turn;
        }
        // `$87FC`: the pointer `$0C/$0D` of the probe, two rows up
        let ptr = ((self.bz(0x0D) as usize) << 8 | self.bz(0x0C) as usize).wrapping_sub(0x50) & 0xFFFF;
        if (ptr >> 8) < 0xC8 {
            return Tail::Turn;
        }
        let a0 = self.b_scr(ptr);
        let a1 = self.b_scr(ptr + 1);
        let a2 = self.b_scr(ptr + 2);
        let cls = if self.bm(0x774 + x) & 0x80 != 0 {
            self.tile_flags(a1)
        } else {
            self.tile_flags(a2) | self.tile_flags(a0) | self.tile_flags(a1)
        };
        // `$884F`: the hop hits a ceiling: turn into falling
        if (cls & 1 != 0 || (self.bm(0x714 + x) != 0 && (cls >> 1) & 1 == 0))
            && self.bm(0x7D0 + x) != 0 {
                self.set_bm(0x7BC + x, 0x24);
                self.b_flip_vertical(x);
            }
        Tail::Turn
    }

    /// `$8862`: after the downward step: landing on the ground turns into rising.
    fn b_hop_landing(&mut self, x: usize) -> Tail {
        if self.bm(0x784 + x) == 2 {
            return Tail::Move; // $885F
        }
        let (t_e, t_f) = (self.v47(0x47E), self.v47(0x47F));
        let cls = if self.bm(0x774 + x) & 0x80 != 0 {
            self.tile_flags(t_e)
        } else {
            // `$8883`: ORA $9C20,X with X = `$047B` (2 * slot): the hack reads the class of that character number
            self.tile_flags(t_e) | self.tile_flags(t_f) | self.tile_flags(self.v47(0x47B))
        };
        if cls & 1 != 0 {
            if self.bm(0x7D0 + x) == 0 {
                let v = self.bm(0x75C + x);
                self.set_bm(0x7BC + x, v);
                self.b_flip_vertical(x);
            }
        } else {
            let y = self.bm(0x7BC + x).wrapping_sub(1);
            if y == self.bm(0x75C + x) {
                self.set_bm(0x7BC + x, self.bm(0x7BC + x).wrapping_add(1));
            }
        }
        Tail::Turn
    }

    /// `$89ED` (kind 2): the next step of the 3-byte script `(frames, dx, dy)`.
    fn b_script_mover(&mut self, x: usize) {
        let script = self.bm(0x754 + x) as usize | (self.bm(0x75C + x) as usize) << 8;
        let mut y = self.bm(0x7DC + x) as usize;
        let n = self.bm(0x7D8 + x).wrapping_sub(1);
        self.set_bm(0x7D8 + x, n);
        if n == 0xFF {
            y = (y + 3) & 0xFF;
            self.set_bm(0x7DC + x, y as u8);
            loop {
                let a = self.b_mem(script + y);
                self.set_bm(0x7D8 + x, a);
                if a != 0xFF {
                    break;
                }
                self.set_bm(0x7DC + x, 0);
                y = 0;
            }
        }
        let y = (y + 1) & 0xFF;
        let dx = self.b_mem(script + y) as i8;
        let xs = 2 * x;
        
        let toggle = if dx >= 0 {
            self.set_bm(0x774 + x, dx as u8);
            self.set_bm(0x7B4 + x, 0);
            let (v, c) = self.spr(xs).overflowing_add(dx as u8);
            self.set_spr(xs, v);
            c
        } else {
            self.set_bm(0x774 + x, (dx as u8).wrapping_neg());
            self.set_bm(0x7B4 + x, 1);
            let (v, c) = self.spr(xs).overflowing_add(dx as u8);
            self.set_spr(xs, v);
            !c
        };
        if toggle {
            self.sprite_x_msb ^= self.bit(x);
        }
        let dy = self.b_mem(script + ((y + 1) & 0xFF));
        let v = self.spr(xs + 1).wrapping_add(dy);
        self.set_spr(xs + 1, v);
    }

    /// `$7FB8`: animation step.
    fn b_animate(&mut self, x: usize) {
        let n = self.bm(0x7AC + x).wrapping_sub(1);
        self.set_bm(0x7AC + x, n);
        if n & 0x80 == 0 {
            return;
        }
        self.set_bm(0x7AC + x, self.bm(0x77C + x));
        let left = if self.bm(0x71C + x) == 0 || self.bm(0x774 + x) != 0 { self.bm(0x7B4 + x) != 0 } else { self.bm(0x7D0 + x) != 0 };
        let (mut y, mut end) = if left { (self.bm(0x734 + x), self.bm(0x73C + x)) } else { (self.bm(0x724 + x), self.bm(0x72C + x)) };
        let backwards = self.bm(0x718 + x) != 0;
        // true: the pointer is counted down (`$8008`), false: up (`$8024`)
        let down = if !backwards {
            y > end
        } else if self.bm(0x7F8 + x) != 0 {
            false
        } else {
            std::mem::swap(&mut y, &mut end);
            // `TAY; BNE $8008`, zero falls into the compare
            end != 0 || y > end
        };
        let chain = self.bm(0x7E0 + x) != 0;
        let len = self.bm(0x79C + x);
        let mut p = self.sptr(x);
        if down {
            p = p.wrapping_sub(1);
            if chain {
                p = p.wrapping_sub(len);
            }
            self.set_sptr(x, p);
            if end < p {
                return;
            }
        } else {
            p = p.wrapping_add(1);
            if chain {
                p = p.wrapping_add(len);
            }
            self.set_sptr(x, p);
            if end >= p {
                return;
            }
        }
        // `$803E`: the range is run through: start again
        let mut start = y;
        if backwards {
            start = end;
            if self.bm(0x7F8 + x) != 0 {
                start = start.wrapping_sub(1);
            }
        }
        self.set_sptr(x, start);
        let f = self.bm(0x7F8 + x) ^ 1;
        self.set_bm(0x7F8 + x, f);
    }

    /// `$87A5`: colour flash for objects with colour bit 7.
    fn b_hit_colour(&mut self, x: usize) {
        if self.bm(0x7CC + x) & 0x80 != 0 && self.bm(0x744 + x) != 2 {
            self.sprite_colour_cycle_sub_2f6e(x);
        }
    }

    // ---- clean-up $7BC7 / $7C44 -------------------------------------------------------------------------------------------------

    /// `$7BC7`: objects with the extra flag stop at the left edge; objects far off the right edge are retired.
    fn b_wrap_hold(&mut self) {
        // WIDESCREEN ON: every limit moves out by `E` (0 = the C64's)
        let e = self.wide_e() as u8;
        for y in (3..=7usize).rev() {
            let mut xs = 2 * y;
            let extra = self.bm(0x78C + y);
            let bound = extra != 0 && extra & 0x80 == 0;
            if e != 0 && bound {
                // the left wall of an edge-bound object is `E` further left, which for a wide view is X < 0: signed
                let wall = 0x20 - e as i32;
                if self.wide_x9(y) < wall {
                    self.wide_set_x9(y, wall);
                    if self.bm(0x7B4 + y) != 0 {
                        self.set_bm(0x7B4 + y, 0);
                        let p = self.bm(0x724 + y);
                        self.set_sptr(y, p);
                    }
                }
            }
            if self.sprite_x_msb & self.bit(y) == 0 {
                if e == 0 && bound && self.spr(xs) < 0x20 {
                    self.set_spr(xs, 0x20);
                    if self.bm(0x7B4 + y) != 0 {
                        self.set_bm(0x7B4 + y, 0);
                        let p = self.bm(0x724 + y);
                        self.set_sptr(y, p);
                    }
                }
                continue;
            }
            let chain = self.bm(0x79C + y) != 0;
            if chain {
                xs = 2 * self.bm(0x7F0 + y) as usize;
            }
            // msb set: right of X = 255, or (ON) already left of X = 0 (an edge-bound object at its left wall)
            if bound && self.spr(xs) >= 0x40 + e && (e == 0 || self.spr(xs) < 0x80) && self.bm(0x7B4 + y) == 0 {
                self.set_bm(0x7B4 + y, 1);
                // `STA $0605,X` with X = 2 * slot: the hack writes into the pointer area of another sprite
                let p = self.bm(0x734 + y);
                self.set_sptr(xs, p);
            }
            let xv = self.spr(xs);
            // a chain (head plus followers up to 72 px to its right) leaves a little earlier on the right and later on the left
            // (`+ 8`: with E = 48 the C64's hole `$5A..$B8` would be shorter than 2 E, so the two edges would cross)
            let (lo, hi) = match (chain, e != 0) {
                (false, _) => (0x5A + e, 0xF0 - e),
                (true, false) => (0x5A, 0xB8),
                (true, true) => (0x5A + e - 8, 0xB8 - e + 8),
            };
            if xv >= lo && xv < hi {
                self.b_retire_far(y);
            }
        }
    }

    /// `$7C61`
    fn b_retire_far(&mut self, y: usize) {
        if self.bm(0x78C + y) & 0x80 != 0 {
            return;
        }
        if self.bm(0x71C + 7) >= 3 && y != 3 {
            return;
        }
        self.wide_note_clip(y);
        self.b_retire(y);
    }

    /// `$7C71`: switch the sprite off.
    fn b_retire(&mut self, y: usize) {
        let m = self.nbit(y);
        self.en.vic_me &= m;
        self.sprite_x_msb &= m;
    }

    /// `$7C44`: sprites 7..3 above or below the screen are retired.
    fn b_retire_vertical(&mut self) {
        for y in (3..=7usize).rev() {
            let v = self.spr(2 * y + 1);
            if !(0x1E..0xFA).contains(&v) {
                self.b_retire(y);
            }
        }
    }

    // ---- collisions $8064 ------------------------------------------------------------------------------------------------------------

    fn b_collisions(&mut self) {
        let latch = self.en.vic_d01e;
        self.en.vic_d01e = 0;
        let mut a = latch & 0xF8;
        while let Some(next) = self.b_collide_step(a) {
            a = next;
        }
    }

    /// One pass of `$8069`; `Some(a)` = `$8059` found more collisions to look at.
    fn b_collide_step(&mut self, a: u8) -> Option<u8> {
        self.set_v47(0x489, a);
        if a == 0 {
            return None;
        }
        if a == 0x08 {
            // `$8175`: the shot touches nothing the program knows
            if self.d32(0x3246) == 1 {
                self.en.spr_ptr[0] = self.en.spr_ptr[0].wrapping_add(1);
                self.set_d32(0x3246, 2);
            }
            return None;
        }
        let shot = a & 8 != 0;
        let (xl, msb, yv);
        if shot {
            xl = self.spr(6);
            msb = (self.sprite_x_msb >> 3) & 1;
            yv = self.spr(7);
        } else {
            if self.sptr(7) == 0xD0 {
                // `$808A`: the end of stage figure (`$857F`): the original's walk to the door
                self.cutscene_walk_l2d41();
                return None;
            }
            if self.bm(0x71C + 7) == 4 {
                self.b_goal_touch(); // `$8EB7`
                return None;
            }
            xl = self.spr(0);
            msb = self.sprite_x_msb & 1;
            yv = self.spr(1);
        }
        self.set_v47(0x48A, xl);
        self.set_v47(0x48B, msb);
        self.set_v47(0x490, yv);
        let x = self.b_hit_test()?;
        let ys = 2 * x;
        if shot {
            // `$80B8`
            if self.en.spr_ptr[0] < 0xFB {
                self.en.spr_ptr[0] = 0xFB;
            }
            if self.bm(0x71C + 7) >= 3 {
                self.b_boss_hit(); // `$80D2`
                return None;
            }
            if self.en.spr_ptr[0] >= 0xFC {
                return None;
            }
            let xc = if self.bm(0x79C + x) != 0 { self.bm(0x7F0 + x) as usize & 7 } else { x };
            let c = self.bm(0x744 + xc);
            if c & 0x80 != 0 {
                self.set_bm(0x744 + xc, c.wrapping_add(1));
                return None;
            }
            if c == 0 {
                self.b_squash(xc);
                return None;
            }
            self.en.vic_col[3] = self.en.vic_col[3].wrapping_add(1);
            return self.b_more_collisions(xc);
        }
        // `$8132`: Giana touches an object
        if self.bm(0x71C + 7) >= 3 {
            self.player_hit_l1db7();
            return None;
        }
        let c = self.bm(0x744 + x);
        let _ = ys;
        match c {
            0x7F => {
                self.b_platform_contact(x);
                self.b_more_collisions(x)
            }
            0 => {
                if self.land_flag == 0xFF || self.jump_state == 0xFF {
                    self.b_squash(x);
                } else {
                    self.player_hit_l1db7();
                }
                None
            }
            1 | 0x80..=0xFF => {
                self.player_hit_l1db7();
                None
            }
            0x12 => {
                self.b_pick_up(x);
                None
            }
            _ => self.b_more_collisions(x),
        }
    }

    /// `$8059`: look at the collisions of the sprites below `x`.
    fn b_more_collisions(&mut self, x: usize) -> Option<u8> {
        let mask = img(0x8D59 + x);
        let a = self.v47(0x489) & 0xF0 & mask;
        if a == 0 {
            None
        } else {
            Some(a)
        }
    }

    /// `$821A`: the object (slot 7..4) within 16 pixels of the point `$048A/$048B/$0490`.
    fn b_hit_test(&mut self) -> Option<usize> {
        let a = self.v47(0x489);
        let mut x = 7usize;
        while x != 0 && a & self.bit(x) == 0 {
            x -= 1;
        }
        if x == 0 {
            return None;
        }
        loop {
            if self.en.vic_me & self.bit(x) != 0 && self.bm(0x7C4 + x) == 0 {
                let ex = self.spr(2 * x);
                let emsb = (self.sprite_x_msb & self.bit(x) != 0) as i32;
                let ey = self.spr(2 * x + 1);
                let dx = ((self.v47(0x48B) as i32) << 8 | self.v47(0x48A) as i32) - (emsb << 8 | ex as i32);
                let dy = (self.v47(0x490) as i32 - ey as i32).abs();
                if dx.abs() < 0x10 && dy < 0x10 {
                    return Some(x);
                }
            }
            x -= 1;
            if x < 4 {
                return None;
            }
        }
    }

    /// `$81E1`: stomp or shot kill: squashed shape, dying, score, sound.
    fn b_squash(&mut self, x: usize) {
        let mut x = x;
        if self.bm(0x79C + x) != 0 {
            x = self.bm(0x7F0 + x) as usize & 7;
        }
        if self.bm(0x744 + x).wrapping_add(3) & 0x80 != 0 {
            return;
        }
        let p = if self.bm(0x7B4 + x) != 0 { self.bm(0x76C + x) } else { self.bm(0x764 + x) };
        self.set_sptr(x, p);
        self.set_bm(0x7C4 + x, 2);
        self.sfx_queue = 5;
        self.add_score_sub_991d(self.bm(0x74C + x), self.bm(0x794 + x));
    }

    /// `$8187` (class `$12`): the chest raises the power level, 50 points; (ladders set the water patch).
    fn b_pick_up(&mut self, x: usize) {
        let sfx;
        if self.bm(0x74C + x) & 0x80 != 0 {
            // ladder (`$818C`): patches `$8D6B`/`$8E1D` of the water code (water physics everywhere and a flickering
            // animation until the stage hook restores them), then the tail of the chest: sound 3, object gone, 50 points
            if let Some(b) = self.engine_b.as_mut() {
                b.water.ladder = true;
            }
            sfx = 3;
        } else {
            self.anim_counter = self.anim_counter.wrapping_add(1);
            if self.anim_counter >= 8 {
                self.anim_counter = 4;
                self.lives_c64 = self.lives_c64.wrapping_add(1);
                self.draw_lives_sub_0e1f();
                sfx = 8;
            } else {
                sfx = 3;
            }
        }
        self.sfx_queue = sfx;
        self.redraw_pending = 1;
        let m = self.nbit(x);
        self.en.vic_me &= m;
        self.add_score_sub_991d(0x50, 0);
    }

    // ---- Giana's shot $82A7 / $8312 -------------------------------------------------------------------------------------------------

    /// `$82A7`: fire pressed, no shot in flight, power level 2 or more.
    fn b_shot_fire(&mut self) {
        if self.joystick_fire == 0 || self.en.vic_me & 8 != 0 {
            return;
        }
        self.set_d32(0x3248, 0);
        if self.anim_counter < 2 {
            return;
        }
        if self.anim_counter >= 4 {
            self.b_target_search(); // `$8519`
        }
        self.set_d32(0x3246, 0);
        self.sfx_queue = 0x0A;
        self.set_d32(0x3244, 0);
        let f = if self.facing_right { 0 } else { 0xFF };
        self.set_d32(0x3243, f);
        self.en.vic_col[3] = 0x0D;
        self.en.spr_ptr[0] = 0xFA;
        self.en.spr_xy[0] = self.player_x_low;
        let m = self.sprite_x_msb;
        self.sprite_x_msb = (m & 0xF7) | ((m << 1) & 8);
        self.en.spr_xy[1] = self.player_y_c64.wrapping_sub(0x0F);
        self.en.vic_me |= 8;
        self.b_shot_flight();
    }

    /// `$8312`: the shot's flight (8 pixels per step, bounces on the ground).
    pub(crate) fn b_shot_flight(&mut self) {
        if self.engine_b.is_none() || self.en.vic_me & 8 == 0 {
            return;
        }
        let p = self.en.spr_ptr[0];
        if p != 0xFA {
            if p < 0xFB {
                return;
            }
            if p >= 0xFE {
                self.en.vic_me &= 0xF7;
            } else {
                self.en.spr_ptr[0] = p + 1;
            }
            return;
        }
        // `$8335`: first step of the flight: probe the tiles around the shot
        self.b_probe(3);
        if self.b_solid(self.v47(0x47C)) && self.b_solid(self.v47(0x481)) {
            self.en.spr_ptr[0] = 0xFB;
        }
        // `$8359`: power level 4 homes in on the target of `$8519`
        if self.d32(0x3248) == 0 && self.anim_counter >= 4 {
            self.b_homing();
            return;
        }
        let dir = self.d32(0x3243);
        let blocked = if dir == 0xFF { self.b_solid(self.v47(0x47C)) } else { self.b_solid(self.v47(0x481)) };
        if blocked {
            // `$8377`
            if self.anim_counter >= 3 {
                self.set_d32(0x3246, 1);
                self.set_d32(0x3243, !dir);
            } else {
                self.en.spr_ptr[0] = 0xFB;
            }
            return;
        }
        self.set_v47(0x482, 8);
        self.set_v47(0x483, dir);
        self.b_move_x(3, 6);
        if self.d32(0x3244) != 0 {
            if self.b_solid(self.v47(0x47C)) {
                self.b_shot_bounce();
                return;
            }
            let n = self.d32(0x3245).wrapping_sub(1);
            self.set_d32(0x3245, n);
            if n & 0x80 != 0 {
                self.b_shot_bounce();
                return;
            }
            if n < 3 {
                return;
            }
            self.en.spr_xy[1] = self.en.spr_xy[1].wrapping_sub(2);
            return;
        }
        if self.b_solid(self.v47(0x47E)) {
            self.set_d32(0x3245, 0x0E);
            self.b_shot_bounce();
            return;
        }
        self.en.spr_xy[1] = self.en.spr_xy[1].wrapping_add(2);
    }

    fn b_shot_bounce(&mut self) {
        let v = self.d32(0x3244) ^ 1;
        self.set_d32(0x3244, v);
    }
}
