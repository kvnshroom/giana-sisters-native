//! Giana's movement (work package P of `docs/rom-hack-family-c.md`, 7.4): native ports of the preview's player
//! routines, each verified frame by frame against the original code running in the oracle machine
//! (`crates/giana-c30-oracle/tests/player.rs`).
//!
//! State model during the port: the routines work on the game's own 64 KB memory image (`ram`), at the addresses of
//! [`C30aLayout`], and read their tables from it (the tables are the player's data from the disk, never compiled in).
//! That keeps every ported routine exchangeable with the original one by one; once the whole frame is native the
//! state can move into Rust structs behind the same functions.
//!
//! Ported so far:
//!
//! | original | function | what |
//! |---|---|---|
//! | `$2880..$2A58` | [`player_move`] | vertical movement (jump, fall, spring, swim) and horizontal movement (walk, brake, scroll request) |
//! | `$8E9F..$8F59` | [`probe`] | the 3 x 4 tile cells around Giana and their properties, for the collision code |
//! | `$2BEE..$2D88` | [`vertical`] | water in/out, ceiling, landing, stepping off edges, bobbing (calls [`walls`]) |
//! | `$2D91..$2E00` | [`walls`] | walls and steps at the side of travel |
//! | `$3EC1..$3F25` | [`hazards`] | deadly tiles |
//! | `$8F95..$8FAF` | [`read_input`] | the joystick read |
//! | `$3F26..$3F57` | [`die`] | Giana dies: the logic stops, the restart column is chosen |
//! | `$20F3..$2113` | [`death_start`], [`death_freeze`] | the death sequence starts (death song, frozen logic, death frames) |
//! | `$2114..$2137` | [`death_step`] | one frame of the death jump (up, then down off the screen) |
//! | `$213D..$2146` | [`death_reset`] | Giana's state after the jump (y, diamonds, treasure room, power) |
//! | `$2164..$2171` | [`lose_life`] | a life less: restart the stage or game over |
//! | `$2AED..$2BE1` | [`frame_select`], [`set_frames`] | the sprite frames of Giana's three layers (`$80..$82`) |
//! | `$3313..$333F` | [`sprite_position`] | Giana's sprite position for the picture (the swim offset, the feet depth `$23`) |
//!
//! Per frame the game runs them in this order: [`read_input`], [`probe`], the block code (`$3B80`, package B), the
//! time (`$3E92`, package B), [`hazards`], [`vertical`], [`player_move`], `$2A59` (scroll, package S); then, in the IRQ at
//! line `$5A`, the object engine (`$72EA`), [`sprite_position`], the sprite registers (`$353C`, or the split at `$34B1`;
//! package W) and [`frame_select`].
//!
//! A death ([`die`], entered from [`hazards`], [`player_move`] or an enemy contact at `$7D8C`) freezes that logic (`$20`).
//! The main loop then runs [`death_start`], the death song, [`death_freeze`], [`death_step`] once per frame until Giana has left the screen, a delay of
//! about 17 frames (`$371E`), [`death_reset`], the parallax and IRQ resets (`$25F7`, `$27AF`; packages S and F), the
//! "time up" text when the time ran out (`$99` positive, package B) and [`lose_life`]. The restart is the stage start
//! `$1E56` (package F), which puts Giana at the restart column `$CBF3` that [`die`] chose.

use crate::layout::C30aLayout;

/// How [`player_move`] ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEnd {
    /// `RTS`: back to the frame logic.
    Done,
    /// `JMP $3F26` (Giana fell below the screen outside a treasure room): the caller continues with the death routine.
    /// The registers as the original leaves them there (`A` = the wrapped y, `X` = the joystick byte, `Y` = 0).
    Died { a: u8, x: u8, y: u8 },
}

/// `$2880`: one frame of Giana's movement, from the joystick byte `$31` (`$8F95` has run) and the movement state.
///
/// Vertical, in this order of priority: swimming (`$19`: up/down accelerate along the swim table, nothing pressed
/// decelerates), the spring phase (`$18`, table `$8D77`), falling (`$17 < 0`: the jump table read backwards), rising
/// (`$17 > 0` while up is held, up to step 27, then the top-of-jump spring phase), and the jump start on the ground (a
/// fresh up press: `$17 = 1`, bit 7 of `$31` marks the press as used, and the jump sound when input is live).
/// Horizontal: the speed counter `$33` rises by one per frame in the held direction (capped at 31, 24 swimming; at full
/// speed the pixel table cycles through the 4 entries after 31 with the frame counter `$C6`), falls by one with the
/// stick released and by two against the direction of travel; the pixels go into Giana's x, or into the scroll request
/// `$F8` when she walks right at the scroll edge.
pub fn player_move(ram: &mut [u8], l: &C30aLayout) -> MoveEnd {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    if r(ram, l.auto_scroll) != 0 {
        ram[l.scroll_px as usize] = 3;
        return MoveEnd::Done;
    }
    let joy = r(ram, l.joy);
    let up = joy & 1 != 0;
    let down = joy & 2 != 0;
    let mut y = r(ram, l.player_y);

    if r(ram, l.swim) != 0 {
        // $2890: swimming
        let step = r(ram, l.vert_step);
        let vdir = r(ram, l.vert_dir);
        // the three ways of the original's code: rise with step `s` (stored when the flag says so), sink with step `s`,
        // or stop (`$17 = dir`, step 0); "negative" is bit 7 of the 8-bit step, as `BPL`/`BMI` see it
        enum Swim {
            Rise(u8, bool),
            Sink(u8, bool),
            Stop(u8),
        }
        let neg = |v: u8| v & 0x80 != 0;
        let swim = if up {
            if neg(vdir) {
                // moving down: decelerate (step - 2), turn to rising at the top of the table
                let s = step.wrapping_sub(2);
                if neg(s) { Swim::Stop(1) } else { Swim::Sink(s, true) }
            } else {
                let s = step.wrapping_add(1);
                Swim::Rise(s, s < 25)
            }
        } else if down {
            if vdir == 1 {
                let s = step.wrapping_sub(2);
                if neg(s) { Swim::Stop(0xFF) } else { Swim::Rise(s, true) }
            } else {
                let s = step.wrapping_add(1);
                Swim::Sink(s, s < 25)
            }
        } else {
            let s = step.wrapping_sub(1);
            if neg(s) {
                Swim::Stop(0)
            } else if vdir == 1 {
                Swim::Rise(s, true)
            } else {
                Swim::Sink(s, true)
            }
        };
        match swim {
            Swim::Rise(s, store) => {
                if store {
                    ram[l.vert_step as usize] = s;
                }
                y = y.wrapping_sub(r(ram, l.swim_table + u16::from(s)));
                ram[l.player_y as usize] = y;
                ram[l.vert_dir as usize] = 1;
            }
            Swim::Sink(s, store) => {
                if store {
                    ram[l.vert_step as usize] = s;
                }
                let (ny, carry) = y.overflowing_add(r(ram, l.swim_table + u16::from(s)));
                if carry {
                    if let Some(end) = below_screen(ram, l, ny, joy) {
                        return end;
                    }
                } else {
                    ram[l.player_y as usize] = ny;
                    ram[l.vert_dir as usize] = 0xFF;
                }
            }
            Swim::Stop(dir) => {
                ram[l.vert_dir as usize] = dir;
                ram[l.vert_step as usize] = 0;
            }
        }
    } else {
        // $28F4: on land
        let bounce = r(ram, l.bounce);
        let mut vertical_done = false;
        if bounce != 0 {
            if bounce & 0x80 == 0 {
                // $290B: rising part of the spring phase, 7 steps
                let s = r(ram, l.bounce_step);
                if s == 7 {
                    ram[l.vert_dir as usize] = 0xFF;
                    ram[l.bounce as usize] = 0xFF;
                } else {
                    let (ny, borrow) = y.overflowing_sub(r(ram, l.spring_table + s as u16));
                    if !borrow {
                        ram[l.player_y as usize] = ny;
                    }
                    ram[l.bounce_step as usize] = s.wrapping_add(1);
                }
                vertical_done = true;
            } else {
                // $28FA: falling part, the steps backwards
                let s = r(ram, l.bounce_step).wrapping_sub(1);
                ram[l.bounce_step as usize] = s;
                if s & 0x80 == 0 {
                    ram[l.player_y as usize] = y.wrapping_add(r(ram, l.spring_table + s as u16));
                    vertical_done = true;
                } else {
                    ram[l.bounce as usize] = 0;
                }
            }
        }
        if !vertical_done {
            let vdir = r(ram, l.vert_dir);
            if vdir & 0x80 != 0 {
                // $2931: falling: the jump table backwards
                let mut s = r(ram, l.vert_step);
                if s != 0 {
                    s -= 1;
                    ram[l.vert_step as usize] = s;
                }
                let (ny, carry) = y.overflowing_add(r(ram, l.jump_table + s as u16));
                if carry {
                    if let Some(end) = below_screen(ram, l, ny, joy) {
                        return end;
                    }
                } else {
                    ram[l.player_y as usize] = ny;
                    ram[l.fall_frames as usize] = r(ram, l.fall_frames).wrapping_add(1);
                }
            } else if vdir != 0 {
                // $2950: rising while up is held, else the top-of-jump spring phase
                let s = r(ram, l.vert_step);
                if up && s != 0x1B {
                    ram[l.vert_step as usize] = s + 1;
                    let (ny, borrow) = y.overflowing_sub(r(ram, l.jump_table + s as u16));
                    if !borrow {
                        ram[l.player_y as usize] = ny;
                    }
                } else {
                    ram[l.bounce as usize] = 1;
                    ram[l.bounce_step as usize] = 0;
                }
            } else if joy & 0x80 == 0 && up {
                // $2972: jump start on a fresh up press
                ram[l.joy as usize] = joy | 0x80;
                ram[l.vert_dir as usize] = 1;
                ram[l.vert_step as usize] = 0;
                if r(ram, l.read_input) & 0x80 != 0 {
                    ram[l.effect as usize] = 0;
                }
            }
        }
    }
    horizontal(ram, l, joy);
    MoveEnd::Done
}

/// `$2947`: the y addition wrapped past 255. In a treasure room (`$C8 != 0`) the wrapped value is kept as on land
/// (`$2940`, which also counts a falling frame); elsewhere Giana dies.
fn below_screen(ram: &mut [u8], l: &C30aLayout, ny: u8, joy: u8) -> Option<MoveEnd> {
    if ram[l.in_room as usize] != 0 {
        ram[l.player_y as usize] = ny;
        ram[l.fall_frames as usize] = ram[l.fall_frames as usize].wrapping_add(1);
        None
    } else {
        Some(MoveEnd::Died { a: ny, x: joy, y: 0 })
    }
}

/// `$298B..$2A58`: speed counter, facing, pixels per frame and the x move or scroll request. `joy` is `$31` as it was
/// on entry to [`player_move`] (the original keeps it in X; a jump start's bit 7 does not matter here).
fn horizontal(ram: &mut [u8], l: &C30aLayout, joy: u8) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let w = |ram: &mut [u8], a: u16, v: u8| ram[a as usize] = v;
    let speed = r(ram, l.speed);
    let counter: u8 = if joy & 0x0C == 0 {
        // $2990: released: slow down by one, stop at 0
        let a = speed.wrapping_sub(1);
        if a & 0x80 != 0 {
            w(ram, l.walk_dir, 0);
            return;
        }
        a
    } else {
        let left = joy & 0x04 != 0;
        w(ram, l.facing, left as u8);
        // moving the other way (`BIT $15`: V = moving right) while the counter runs: brake
        let moving_right = r(ram, l.walk_dir) & 0x40 != 0;
        let brake = speed != 0 && if left { moving_right } else { !moving_right };
        if brake {
            // $29CD
            w(ram, l.braking, 0xFF);
            let a = speed.wrapping_sub(2);
            if a & 0x80 != 0 {
                w(ram, l.speed, 0);
                w(ram, l.walk_dir, 0);
                w(ram, l.braking, 0);
                return;
            }
            a
        } else {
            w(ram, l.braking, 0);
            if left {
                // $29DF
                w(ram, l.walk_scroll, 0);
                w(ram, l.walk_dir, 0x80);
            } else {
                // $29AD
                w(ram, l.walk_dir, 0xFF);
                let edge = r(ram, l.player_x) >= 0xA4;
                w(ram, l.walk_scroll, if edge { 0xFF } else { 0 });
            }
            speed.wrapping_add(1)
        }
    };
    // $29EE: cap and table index
    let index = if r(ram, l.swim) != 0 {
        let c = counter.min(0x18);
        w(ram, l.speed, c);
        c + 4
    } else if counter < 0x20 {
        w(ram, l.speed, counter);
        counter
    } else {
        let i = counter.wrapping_add(r(ram, l.frame4));
        if i == 0 {
            w(ram, l.speed, i);
        }
        i
    };
    let dx = r(ram, l.speed_table + index as u16);
    w(ram, l.dx, dx);
    // $2A10: walking right at the scroll edge scrolls the playfield instead (not in a treasure room, not at column 0)
    if r(ram, l.in_room) == 0 && r(ram, l.scroll_column) != 0 && r(ram, l.walk_scroll) & 0x80 != 0 {
        w(ram, l.scroll_px, dx.wrapping_add(r(ram, l.scroll_px)));
        return;
    }
    let x = r(ram, l.player_x);
    let mut msb = r(ram, l.sprite_x_msb);
    if r(ram, l.walk_dir) & 0x40 != 0 {
        // $2A3B: right; the right edge is x = $13A (x high bit set and low byte >= $3A)
        let (nx, carry) = x.overflowing_add(dx);
        if carry {
            msb |= 7;
            w(ram, l.sprite_x_msb, msb);
        }
        if msb & 1 == 0 || nx < 0x3A {
            w(ram, l.player_x, nx);
        }
    } else {
        // $2A20: left; the left edge is x - fine scroll = $18 (left border of the 38-column screen)
        let (nx, borrow) = x.overflowing_sub(dx);
        if borrow {
            msb &= 0xF8;
            w(ram, l.sprite_x_msb, msb);
        }
        if msb & 1 != 0 || nx.wrapping_sub(r(ram, l.fine_scroll)).wrapping_sub(1) >= 0x18 {
            w(ram, l.player_x, nx);
        }
    }
}

/// `$8E9F`: the tile probe. Reads the 3 x 4 screen cells around Giana (tile codes, as the screen shows them) into
/// `$39..$44` and their properties (`$BE00[tile]`) into `$45..$50`, for the collision code that runs after it.
///
/// The probe's cell: row `(min(y, $F0) - $33) / 8` (the cells are rows `+1..+4` of it) and column
/// `(x - $15 - fine scroll) / 8` (columns `+0..+2`). Above the screen (`y < $42`) only the properties are cleared. Also left
/// behind as the original does, because the block code reads them: the row (`$8F`), the column (`$DB`), the screen
/// address of the cell above (`$0310`) and the four row pointers.
pub fn probe(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let y = r(ram, l.player_y);
    if y < 0x42 {
        ram[l.probe_props as usize..l.probe_props as usize + 12].fill(0);
        return;
    }
    let row = (y.min(0xF0) - 0x33) >> 3;
    ram[l.probe_row as usize] = row;
    // 9-bit x minus 21 minus the fine scroll, divided by 8, as the original's 16-bit steps compute it
    let (lo, borrow) = r(ram, l.player_x).overflowing_sub(0x15);
    let hi = (r(ram, l.sprite_x_msb) & 1).wrapping_sub(borrow as u8);
    let (lo, borrow) = lo.overflowing_sub(r(ram, l.fine_scroll));
    let hi = hi.wrapping_sub(borrow as u8);
    let col = ((hi & 1) << 7 | lo >> 1) >> 2;
    ram[l.probe_col as usize] = col;
    let row_ptr = |ram: &[u8], k: u16| u16::from_le_bytes([r(ram, l.row_lo + u16::from(row) + k), r(ram, l.row_hi + u16::from(row) + k)]);
    let above = row_ptr(ram, 0).wrapping_add(u16::from(col));
    ram[l.probe_above as usize..l.probe_above as usize + 2].copy_from_slice(&above.to_le_bytes());
    // the row pointers as the original leaves them: rows 1..4 in $B4, $C0, $0E (scratch) and $E4
    let ptrs = [row_ptr(ram, 1), row_ptr(ram, 2), row_ptr(ram, 3), row_ptr(ram, 4)];
    for (zp, p) in [l.probe_ptr1, l.probe_ptr2, l.scratch_ptr, l.probe_ptr4].into_iter().zip(ptrs) {
        ram[zp as usize..zp as usize + 2].copy_from_slice(&p.to_le_bytes());
    }
    // the tile cells, in the original's order: row 1 at +0, row 2 at +9, row 3 at +3, row 4 at +6
    for (p, at) in ptrs.into_iter().zip([0u16, 9, 3, 6]) {
        for c in 0..3u8 {
            let a = p.wrapping_add(u16::from(col.wrapping_add(c)));
            ram[(l.probe_tiles + at + u16::from(c)) as usize] = r(ram, a);
        }
    }
    if r(ram, l.probe_skip_bottom) != 0 {
        ram[(l.probe_tiles + 6) as usize..(l.probe_tiles + 9) as usize].fill(0);
    }
    for i in 0..12 {
        let tile = r(ram, l.probe_tiles + i);
        ram[(l.probe_props + i) as usize] = r(ram, l.tile_props + u16::from(tile));
    }
}

/// The probe's cells as offsets into `$45..$50` ([`probe`]): rows from top to bottom are head (row `+1`), body (`+2`),
/// feet (`+3`) and below the feet (`+4`), columns left, middle, right. Property bit 0 = solid, bit 1 = water.
mod cell {
    pub const HEAD_L: u16 = 0;
    pub const HEAD_M: u16 = 1;
    pub const HEAD_R: u16 = 2;
    pub const FEET_L: u16 = 3;
    pub const FEET_M: u16 = 4;
    pub const FEET_R: u16 = 5;
    pub const BELOW_L: u16 = 6;
    pub const BELOW_M: u16 = 7;
    pub const BELOW_R: u16 = 8;
    pub const BODY_L: u16 = 9;
    pub const BODY_M: u16 = 10;
    pub const BODY_R: u16 = 11;
}

/// Bit 0 (solid) of the OR of the properties of `cells`.
fn solid(ram: &[u8], l: &C30aLayout, cells: &[u16]) -> bool {
    cells.iter().fold(0, |a, &c| a | ram[(l.probe_props + c) as usize]) & 1 != 0
}

/// `$2BEE`: what the probe's tiles ([`probe`], run earlier in the frame) do to Giana: getting into and out of water,
/// the walls ([`walls`]), the head against a ceiling, landing (the feet are put on the tile edge), stepping off an
/// edge (falling starts), and bobbing on the water surface. `$23` is how deep the feet are in their tile row
/// (`(y - 3) & 7`, kept by the sprite routine at `$333A`).
pub fn vertical(ram: &mut [u8], l: &C30aLayout) {
    use cell::*;
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let p = |ram: &[u8], c: u16| ram[(l.probe_props + c) as usize];
    let w = |ram: &mut [u8], a: u16, v: u8| ram[a as usize] = v;

    if r(ram, l.swim) & 0x80 == 0 {
        // $2BF2..$2C65 as a small state machine over the original's labels
        #[derive(PartialEq)]
        enum At {
            Leave,
            Water,
            Done,
        }
        let mut at = if p(ram, BODY_M) & p(ram, FEET_M) & 2 != 0 {
            At::Water
        } else {
            w(ram, l.sprite_color, r(ram, l.colour_table + u16::from(r(ram, l.player_number))));
            At::Leave
        };
        while at != At::Done {
            at = match at {
                At::Leave => {
                    // $2C02: out of the water if she was swimming: up keeps counting as used, the swim state ends and
                    // the speed counter gets 4 more (a carry out of it goes on as if in water)
                    if r(ram, l.swim) == 0 {
                        At::Done
                    } else {
                        let joy = r(ram, l.joy);
                        if joy & 1 != 0 {
                            w(ram, l.joy, joy | 0x80);
                        }
                        w(ram, l.vert_step, 4);
                        w(ram, l.swim, 0);
                        w(ram, l.fall_frames, 0);
                        let speed = r(ram, l.speed);
                        if speed == 0 {
                            At::Done
                        } else {
                            let (s, carry) = speed.overflowing_add(4);
                            w(ram, l.speed, s);
                            if carry { At::Water } else { At::Done }
                        }
                    }
                }
                At::Water => {
                    // $2C25: the cells say water
                    if r(ram, l.player_number) == 0 {
                        w(ram, l.sprite_color, 0);
                    }
                    let leave = p(ram, HEAD_M) & 3 == 0
                        && (p(ram, BELOW_M) & 1 != 0 || (!solid(ram, l, &[FEET_L, FEET_R]) && solid(ram, l, &[BELOW_L, BELOW_R])));
                    if leave {
                        At::Leave
                    } else {
                        if r(ram, l.swim) == 0 {
                            // $2C49: dive in: the swim step from the time spent falling, 4 slower
                            w(ram, l.vert_step, r(ram, l.fall_frames).wrapping_add(2).min(0x15));
                            w(ram, l.speed, r(ram, l.speed).saturating_sub(4));
                            w(ram, l.swim, 1);
                            w(ram, l.bounce, 0);
                        }
                        At::Done
                    }
                }
                At::Done => At::Done,
            };
        }
    }
    walls(ram, l);
    // $2C6A: how deep the feet may be in the floor to land: 3, or 4 when the step index is still at most 3
    let limit = if r(ram, l.bounce) == 0 && r(ram, l.vert_step) <= 3 { 4 } else { 3 };
    w(ram, l.land_depth, limit);
    let depth = r(ram, l.feet_depth);
    let swim = r(ram, l.swim);
    let vdir = r(ram, l.vert_dir);
    if vdir == 0 {
        grounded(ram, l, depth, swim);
        return;
    }
    if vdir & 0x80 == 0 {
        // $2C80: rising: the head against a solid cell
        let head = |ram: &[u8]| {
            let side = if swim != 0 { &[FEET_L, FEET_R, BODY_L, BODY_R][..] } else { &[FEET_L, FEET_R][..] };
            if solid(ram, l, side) {
                p(ram, HEAD_M) & 1 != 0
            } else {
                solid(ram, l, &[HEAD_L, HEAD_R, HEAD_M])
            }
        };
        let bump = if depth == 0 || (swim != 0 && depth == 1) {
            head(ram)
        } else if swim == 0 && !solid(ram, l, &[BELOW_L, FEET_L, BELOW_R, FEET_R]) && solid(ram, l, &[BODY_L, BODY_R]) {
            true
        } else {
            p(ram, BODY_M) & 1 != 0
        };
        if !bump {
            return;
        }
        // $2C9F: falling from now on; a swimmer only loses 5 steps
        w(ram, l.vert_dir, 0xFF);
        if swim != 0 {
            w(ram, l.vert_step, r(ram, l.vert_step).saturating_sub(5));
            return;
        }
        w(ram, l.head_bump, 0xFF);
        if r(ram, l.bounce) != 0 {
            w(ram, l.bounce, 0xFF);
        }
    }
    // $2CD8: falling: land on the floor below, or on a floor cell beside it when the feet are less deep than the limit
    let land = p(ram, BELOW_M) & 1 != 0
        || (!solid(ram, l, &[FEET_L, FEET_R])
            && (p(ram, BELOW_L) | p(ram, BELOW_R) | r(ram, l.on_object)) & 1 != 0
            && depth < limit);
    if !land {
        return;
    }
    if swim != 0 {
        // $2D01: a swimmer turns upwards
        w(ram, l.vert_dir, 1);
        w(ram, l.vert_step, r(ram, l.vert_step).saturating_sub(5));
        return;
    }
    w(ram, l.vert_dir, 0);
    w(ram, l.bounce, 0);
    w(ram, l.fall_frames, 0);
    w(ram, l.player_y, r(ram, l.player_y).wrapping_sub(depth));
}

/// `$2D11`: on the ground: stay on the floor (feet put on the tile edge; in the water a pixel down when the head row is
/// all water), or step off the edge (falling starts), or bob on the water surface (`$2D55`).
fn grounded(ram: &mut [u8], l: &C30aLayout, depth: u8, swim: u8) {
    use cell::*;
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let p = |ram: &[u8], c: u16| ram[(l.probe_props + c) as usize];
    let w = |ram: &mut [u8], a: u16, v: u8| ram[a as usize] = v;
    let floor = p(ram, BELOW_M) & 1 != 0 || (!solid(ram, l, &[FEET_L, FEET_R]) && solid(ram, l, &[BELOW_L, BELOW_R]));
    if floor && r(ram, l.player_y) < 0xE3 {
        if swim == 0 {
            w(ram, l.fall_frames, 0);
            w(ram, l.player_y, r(ram, l.player_y).wrapping_sub(depth));
        } else if p(ram, HEAD_L) & p(ram, HEAD_M) & p(ram, HEAD_R) & 2 != 0 {
            w(ram, l.player_y, r(ram, l.player_y).wrapping_sub(1));
        }
        return;
    }
    if r(ram, l.on_object) != 0 {
        return;
    }
    if swim == 0 {
        w(ram, l.fall_frames, r(ram, l.fall_frames).wrapping_add(1));
        w(ram, l.vert_dir, 0xFF);
        w(ram, l.vert_step, 0);
        return;
    }
    // $2D55: bobbing while standing still: a pixel up or down whenever the timer runs out, the next delay from the
    // table by depth; at depth 0 the direction resets, from depth 6 on it turns upwards
    if r(ram, l.walk_dir) != 0 {
        return;
    }
    let t = r(ram, l.bob_timer).wrapping_sub(1);
    w(ram, l.bob_timer, t);
    if t & 0x80 == 0 {
        return;
    }
    let bob = r(ram, l.bob_dir);
    let up = if depth == 0 {
        if bob == 0 {
            Some(false)
        } else {
            w(ram, l.bob_dir, 0);
            None
        }
    } else if depth >= 6 && bob & 0x80 == 0 {
        w(ram, l.bob_dir, 0x80);
        None
    } else {
        Some(bob & 0x80 != 0)
    };
    let y = r(ram, l.player_y);
    match up {
        Some(true) => w(ram, l.player_y, y.wrapping_sub(1)),
        // `INC $78` / `BNE` / `DEC $78`: a wrap to 0 is taken back
        Some(false) => w(ram, l.player_y, if y == 0xFF { 0xFF } else { y + 1 }),
        None => {}
    }
    w(ram, l.bob_timer, r(ram, l.bob_delays + u16::from(depth)));
}

/// `$2D91`: walls, while moving (`$15` bit 7). A swimmer that meets a solid cell at feet or body height on the side of
/// travel turns round 5 slower. On land a solid feet cell on that side stops Giana (direction and speed 0); a solid cell
/// below it does too when it is a step (nothing solid beside it below the feet) and the feet are already in the floor
/// (deeper than 0 when rising, at least 3 otherwise).
pub fn walls(ram: &mut [u8], l: &C30aLayout) {
    use cell::*;
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let p = |ram: &[u8], c: u16| ram[(l.probe_props + c) as usize];
    let w = |ram: &mut [u8], a: u16, v: u8| ram[a as usize] = v;
    let dir = r(ram, l.walk_dir);
    if dir & 0x80 == 0 {
        return;
    }
    let right = dir & 0x40 != 0;
    if r(ram, l.swim) != 0 {
        let hit = if right { solid(ram, l, &[FEET_R, BODY_R]) } else { solid(ram, l, &[FEET_L, BODY_L]) };
        if !hit {
            return;
        }
        if right {
            w(ram, l.walk_dir, 0x80);
            w(ram, l.walk_scroll, 0);
        } else {
            w(ram, l.walk_dir, 0xFF);
        }
        w(ram, l.speed, r(ram, l.speed).saturating_sub(5));
        return;
    }
    let (feet, below, below_other) = if right { (FEET_R, BELOW_R, [BELOW_M, BELOW_L]) } else { (FEET_L, BELOW_L, [BELOW_M, BELOW_R]) };
    let stop = p(ram, feet) & 1 != 0
        || (p(ram, below) & 1 != 0 && !solid(ram, l, &below_other) && {
            let depth = r(ram, l.feet_depth);
            if r(ram, l.vert_dir).wrapping_sub(1) & 0x80 != 0 { depth >= 3 } else { depth != 0 }
        });
    if stop {
        w(ram, l.walk_dir, 0);
        w(ram, l.walk_scroll, 0);
        w(ram, l.speed, 0);
    }
}

/// `$3EC1`: deadly tiles. Returns `true` when Giana dies (the original jumps to `$3F26`; the caller goes on there).
/// Nothing happens while she is already dying (`$99`) or in a treasure room.
///
/// Rising (or standing on an object whose state has bit 7 set): tile `$F4`/`$F5` at the body's middle, or property
/// bit 2 in the body row, kills. Otherwise: tiles `$A1..$A4` below the feet kill; a feet-row cell with property bit 6
/// kills unless bit 5 is set too and the power level is at least 7, and only when she is not on the ground or the cell
/// lacks bit 4; such a bit 4 cell on the ground does not kill, and stops the walk (speed 0) when the feet cell on the
/// side of travel has bit 4.
pub fn hazards(ram: &mut [u8], l: &C30aLayout) -> bool {
    use cell::*;
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let p = |ram: &[u8], c: u16| ram[(l.probe_props + c) as usize];
    let tile = |ram: &[u8], c: u16| ram[(l.probe_tiles + c) as usize];
    if r(ram, l.dying) | r(ram, l.in_room) != 0 {
        return false;
    }
    let body_check = |ram: &[u8]| {
        let t = tile(ram, BODY_M);
        t == 0xF4 || t == 0xF5 || (p(ram, BODY_L) | p(ram, BODY_M) | p(ram, BODY_R)) & 4 != 0
    };
    if r(ram, l.vert_dir) == 1 {
        return body_check(ram);
    }
    if r(ram, l.on_object) != 0 {
        let state = r(ram, l.object_state + u16::from(r(ram, l.object_index)));
        if state & 0x80 != 0 {
            return body_check(ram);
        }
    }
    if (0xA1..0xA5).contains(&tile(ram, BELOW_M)) {
        return true;
    }
    let feet = p(ram, FEET_L) | p(ram, FEET_M) | p(ram, FEET_R);
    if feet & 0x40 == 0 {
        return false;
    }
    if feet & 0x20 != 0 && r(ram, l.power) >= 7 {
        return false;
    }
    if r(ram, l.vert_dir) != 0 || feet & 0x10 == 0 {
        return true;
    }
    let dir = r(ram, l.walk_dir);
    if dir & 0x80 == 0 {
        return false;
    }
    let side = if dir & 0x40 != 0 { p(ram, FEET_R) } else { p(ram, FEET_L) };
    if side & 0x10 != 0 {
        ram[l.speed as usize] = 0;
    }
    false
}

/// The registers a ported routine leaves where its callers read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regs {
    pub a: u8,
    pub x: u8,
    pub y: u8,
}

/// `$8F95`: the joystick read. `port` is `$DC00` as read (active low: bit 0 up .. 3 right, 4 fire; the original first
/// writes `$FF` to `$DC00`, which deselects the key matrix, a caller that models the CIA does that).
///
/// `$31` gets the pressed directions, with bit 7 = "this up press is used already": a jump start sets it
/// ([`player_move`]) and it stays while up is held, so holding up does not jump again. `$32` gets fire (`$10`). The first
/// byte of the routine doubles as a switch: `$2BE8` writes an `RTS` there to turn the input off (`$2BE2` writes the
/// `LDA` back); then nothing happens and `None` is returned. Otherwise the registers as the original leaves them (`A` =
/// fire, `X` = the port byte or, with up pressed, the previous `$31`; `Y` is unchanged).
pub fn read_input(ram: &mut [u8], l: &C30aLayout, port: u8, y: u8) -> Option<Regs> {
    if ram[l.read_input as usize] == 0x60 {
        return None;
    }
    let (mut a, mut x) = (port, port);
    if port & 1 == 0 {
        x = ram[l.joy as usize];
        if x & 0x80 != 0 {
            a &= 0x7F;
        }
    }
    a ^= 0xFF;
    ram[l.joy as usize] = a;
    a &= 0x10;
    ram[l.fire as usize] = a;
    Some(Regs { a, x, y })
}

/// `$3F26` (`a` = `$FF`; the pause menu's "lose a life" enters at `$3F28` with `a` = `$FE`): Giana dies. `$99` (dying)
/// and `$20` (logic frozen) get `a`; the main loop sees `$99` and runs the death sequence ([`death_start`]).
///
/// The restart column `$CBF3` (0 = the start of the stage) is chosen here, once per stage: while it is still 0 and the
/// scroll column `$21` is past the middle of the stage (column `$7F`; `$98` in stage `$24`, `$E9` in stage `$25`), the
/// stage will restart at column `$59` (`$78`, `$BD`). `y` is `Y` on entry (one path keeps it); returns the registers as
/// the original leaves them (the object code that jumps here from `$7D8C` returns to its caller with them).
pub fn die(ram: &mut [u8], l: &C30aLayout, a: u8, y: u8) -> Regs {
    ram[l.dying as usize] = a;
    ram[l.game_state as usize] = a;
    let restart = ram[l.restart_column as usize];
    if restart != 0 {
        return Regs { a, x: restart, y };
    }
    let col = ram[l.scroll_column as usize].wrapping_sub(1);
    let stage = ram[l.stage as usize];
    let (middle, column) = match stage {
        0x24 => (0x98, 0x78),
        0x25 => (0xE9, 0xBD),
        _ => (0x7F, 0x59),
    };
    let x = if col >= middle { column } else { 0 };
    ram[l.restart_column as usize] = x;
    Regs { a: stage, x, y: col }
}

/// How [`death_start`] goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathStart {
    /// `$99 = $FF` in a treasure room: no death sequence, the main loop goes on (`$20E1`).
    Skip,
    /// The sequence runs: the caller starts song `song` (`$5C93`: unless it plays already), then [`death_freeze`], then
    /// [`death_step`] from `DeathPhase::Rise(0)`.
    Dying { song: u8 },
}

/// `$20F3..$2104`: the main loop found `$99` set (a death, or `$28` when the time ran out). In a treasure room the stage
/// number comes back from `$0129` first; the shot timer `$02D5` = 7, and the death song (7) is next.
pub fn death_start(ram: &mut [u8], l: &C30aLayout) -> DeathStart {
    if ram[l.in_room as usize] != 0 {
        if ram[l.dying as usize] == 0xFF {
            return DeathStart::Skip;
        }
        ram[l.stage as usize] = ram[l.room_return as usize];
    }
    ram[l.shot_timer as usize] = 7;
    DeathStart::Dying { song: 7 }
}

/// `$2108..$2113`, after the song start: the logic stays frozen (`$20 = 1`), Giana no longer stands on an object, and
/// the death frames `$D2..$D4` ([`set_frames`]).
pub fn death_freeze(ram: &mut [u8], l: &C30aLayout) {
    ram[l.game_state as usize] = 1;
    ram[l.on_object as usize] = 0;
    set_frames(ram, l, 0xD2);
}

/// A frame of the death jump, with the index the original keeps in `Y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathPhase {
    /// `$2114`: up by `$8D6A[i]` pixels while that stays on the screen (y >= the step), else on to `Fall(i)`
    Rise(u8),
    /// `$2124`: down by `$8D69[i]` pixels
    Fall(u8),
    /// `$2134`: down by 3 pixels (the table is used up; `i` keeps counting down as in the original)
    Fast(u8),
}

impl DeathPhase {
    /// The phase of the next frame after this one moved Giana (the original's `INY` / `DEY` after the frame wait).
    pub fn next(self) -> DeathPhase {
        match self {
            DeathPhase::Rise(0xFF) => DeathPhase::Fall(0),
            DeathPhase::Rise(i) => DeathPhase::Rise(i + 1),
            DeathPhase::Fall(i) | DeathPhase::Fast(i) => {
                let i = i.wrapping_sub(1);
                if i & 0x80 == 0 { DeathPhase::Fall(i) } else { DeathPhase::Fast(i) }
            }
        }
    }

    fn index(self) -> u8 {
        match self {
            DeathPhase::Rise(i) | DeathPhase::Fall(i) | DeathPhase::Fast(i) => i,
        }
    }
}

/// How a [`death_step`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathStep {
    /// Giana moved to `y` in `phase` (a `Rise` that did not fit turns into `Fall`); the frame waits (`$4194`, until the
    /// sprite is shown there) and the next frame runs `phase.next()`.
    Moved { phase: DeathPhase, y: u8 },
    /// The move would go past the bottom of the screen: the jump is over, y is not changed (`a` = the wrapped sum and `i`
    /// the index, the original's `A` and `Y`).
    Gone { a: u8, i: u8 },
}

/// `$2114..$2137`: one frame of the death jump: up along the table at `$8D6A` while the step fits below the top of the
/// screen, then down along the same table backwards (`$8D69`), then 3 pixels a frame until Giana is below the screen.
pub fn death_step(ram: &mut [u8], l: &C30aLayout, phase: DeathPhase) -> DeathStep {
    let y = ram[l.player_y as usize];
    let table = |ram: &[u8], i: u8| ram[(l.death_table + u16::from(i)) as usize];
    let (phase, add) = match phase {
        DeathPhase::Rise(i) => {
            // `$8D6A,Y`: the same table one further on (16-bit index, Y = $FF reads `$8E69`)
            let t = ram[(l.death_table + 1 + u16::from(i)) as usize];
            if y >= t {
                ram[l.player_y as usize] = y - t;
                return DeathStep::Moved { phase, y: y - t };
            }
            (DeathPhase::Fall(i), table(ram, i))
        }
        DeathPhase::Fall(i) => (phase, table(ram, i)),
        DeathPhase::Fast(_) => (phase, 3),
    };
    match y.overflowing_add(add) {
        (a, true) => DeathStep::Gone { a, i: phase.index() },
        (ny, false) => {
            ram[l.player_y as usize] = ny;
            DeathStep::Moved { phase, y: ny }
        }
    }
}

/// `$213D..$2146`, after the death jump and its delay: Giana's y, the diamond counter `$FE`, the treasure-room flag and
/// the power level all go to 0.
pub fn death_reset(ram: &mut [u8], l: &C30aLayout) {
    for a in [l.player_y, l.diamonds, l.in_room, l.power] {
        ram[a as usize] = 0;
    }
}

/// `$2164..$2171`: a life less (`$FB`, BCD, with the 6502's decimal subtraction). `true` = there was one left and the
/// stage restarts (`$1E56`); `false` = game over (`$2172`; `$FB` has wrapped to `$99`).
pub fn lose_life(ram: &mut [u8], l: &C30aLayout) -> bool {
    let lives = ram[l.lives as usize];
    // NMOS `SBC #1` with the carry set, decimal mode
    let mut lo = i16::from(lives & 0x0F) - 1;
    let mut hi = i16::from(lives >> 4);
    if lo < 0 {
        lo -= 6;
        hi -= 1;
    }
    if hi < 0 {
        hi -= 6;
    }
    ram[l.lives as usize] = (hi << 4) as u8 | (lo as u8 & 0x0F);
    lives != 0
}

/// `$2BD8`: the sprite frames of Giana's three layers: `a` for sprite 0 (`$80`), `a + 1` for sprite 2 (`$82`), `a + 2`
/// for sprite 1 (`$81`). Returns `A` and `X` as the original leaves them (`a`, `a + 2`), for the callers that enter it
/// by `JSR` (`$2111`, `$35A6`, `$362F`).
pub fn set_frames(ram: &mut [u8], l: &C30aLayout, a: u8) -> (u8, u8) {
    ram[l.player_frames as usize] = a;
    ram[(l.player_frames + 2) as usize] = a.wrapping_add(1);
    ram[(l.player_frames + 1) as usize] = a.wrapping_add(2);
    (a, a.wrapping_add(2))
}

/// The 6502's `SBC` in binary mode: `a - m - (1 - carry)` and the carry out (no borrow).
fn sbc(a: u8, m: u8, carry: bool) -> (u8, bool) {
    let v = i16::from(a) - i16::from(m) - i16::from(!carry);
    (v as u8, v >= 0)
}

/// `DEC $38` / `BPL`: the frame timer runs out (`true` when it went below 0).
fn tick(ram: &mut [u8], l: &C30aLayout) -> bool {
    let t = ram[l.anim_timer as usize].wrapping_sub(1);
    ram[l.anim_timer as usize] = t;
    t & 0x80 != 0
}

/// The animation index `$1A` for a cycle: 3 after a pose (negative), else the timer counts and on running out the index
/// goes one down. Returns the index for this frame (the value before the decrement, as the original's `X`) and whether
/// the timer has to be loaded again.
fn cycle_step(ram: &mut [u8], l: &C30aLayout) -> (u8, bool) {
    let x = ram[l.anim_index as usize];
    if x & 0x80 != 0 {
        ram[l.anim_index as usize] = 3;
        (3, true)
    } else if tick(ram, l) {
        ram[l.anim_index as usize] = x.wrapping_sub(1);
        (x, true)
    } else {
        (x, false)
    }
}

/// `$2B6E`: Giana's sprite frames for the next picture (`$80..$82`, through [`set_frames`]); nothing while the logic is
/// frozen (`$20`: the death and the stage end set their own frames).
///
/// On land: in the air one frame per facing (`$C0` right, `$E1` left), standing `$C3`/`$E4`, braking `$F0`/`$CF`, and
/// walking a cycle of 4 frames (tables `$2AE1` right, `$2ADD` left, index `$1A` counting down 3..0) whose frame time
/// `$38` is `(52 - speed) / 4` frames: the faster, the quicker the steps. Swimming (`$19` positive): the swim tables
/// (`$2AE9` right, `$2AE5` left); moving, a stroke frame lasts `(42 - speed - $34) / 4` frames, at least 5 (4 when that
/// is negative) and the bobbing timer `$26` is held at 5; floating still, the cycle runs down to index 1 and stays, a
/// step per 11 frames. With `$19` negative (swimming without water, set by a pickup) the frame alternates between `$F9`
/// and `$FA` (index `$1A` 3..0, `$FA` for 3 and 2).
pub fn frame_select(ram: &mut [u8], l: &C30aLayout) {
    let r = |ram: &[u8], a: u16| ram[a as usize];
    let w = |ram: &mut [u8], a: u16, v: u8| ram[a as usize] = v;
    let left = |ram: &[u8]| ram[l.facing as usize] != 0;
    // a 0 in a table makes the original fall through to the next case, and from the swim tables back to its own start
    // (never with the game's tables)
    loop {
        if r(ram, l.game_state) != 0 {
            return;
        }
        let swim = r(ram, l.swim);
        if swim & 0x80 != 0 {
            // $2AED
            let mut x = r(ram, l.anim_index).wrapping_sub(1);
            if x & 0x80 != 0 {
                x = 3;
            }
            w(ram, l.anim_index, x);
            let (f1, f2) = if left(ram) { (0xDD, 0xDC) } else { (0xBC, 0xBB) };
            w(ram, l.player_frames + 2, f2);
            w(ram, l.player_frames + 1, f1);
            w(ram, l.player_frames, 0xF9 + (x >= 2) as u8);
            return;
        }
        if swim != 0 {
            let x = if r(ram, l.walk_dir) != 0 || r(ram, l.vert_dir) != 0 {
                // $2B39: strokes
                w(ram, l.bob_timer, 5);
                let (x, reload) = cycle_step(ram, l);
                if reload {
                    let (t, c) = sbc(0x2A, r(ram, l.speed), true);
                    let (t, c) = sbc(t, r(ram, l.vert_step), c);
                    let time = if !c || t >> 2 < 5 { 4 } else { t >> 2 };
                    w(ram, l.anim_timer, time);
                }
                x
            } else {
                // $2B21: floating still
                let x = r(ram, l.anim_index);
                if x & 0x80 != 0 {
                    3
                } else {
                    if x != 1 && tick(ram, l) {
                        w(ram, l.anim_index, x.wrapping_sub(1));
                        w(ram, l.anim_timer, 10);
                    }
                    x
                }
            };
            // $2B60
            let mut a = 0;
            if left(ram) {
                a = r(ram, l.swim_frames_left + u16::from(x));
            }
            if a == 0 {
                a = r(ram, l.swim_frames_right + u16::from(x));
            }
            if a != 0 {
                set_frames(ram, l, a);
                return;
            }
            continue;
        }
        let a = if r(ram, l.vert_dir) != 0 {
            w(ram, l.anim_index, 0xFF);
            if left(ram) { 0xE1 } else { 0xC0 }
        } else if r(ram, l.walk_dir) == 0 {
            w(ram, l.anim_index, 0xFF);
            if left(ram) { 0xE4 } else { 0xC3 }
        } else {
            let mut a = 0;
            if r(ram, l.braking) == 0 {
                // $2B91: walking
                let (x, reload) = cycle_step(ram, l);
                if reload {
                    w(ram, l.anim_timer, 0x34u8.saturating_sub(r(ram, l.speed)) >> 2);
                }
                if left(ram) {
                    a = r(ram, l.walk_frames_left + u16::from(x));
                }
                if a == 0 {
                    a = r(ram, l.walk_frames_right + u16::from(x));
                }
            }
            if a == 0 {
                // $2BBC: braking (also after a 0 in a walking table)
                w(ram, l.anim_index, 0xFF);
                a = if left(ram) { 0xCF } else { 0xF0 };
            }
            a
        };
        set_frames(ram, l, a);
        return;
    }
}

/// `$3313..$333F` (inside the IRQ at line `$5A`, after the object engine): Giana's sprite position for the picture,
/// `$71`/`$72`/`$79` from `$70`/`$88`/`$78`. Swimming (`$19` positive) the sprite is drawn 2 pixels off: x - 2 facing
/// right, x + 2 facing left, the x high bits of sprites 0..2 (`$72`) flipped when that crosses 0 or 255. Also the feet
/// depth `$23 = (y - 3) & 7` that the collision code ([`vertical`]) reads.
pub fn sprite_position(ram: &mut [u8], l: &C30aLayout) {
    let x0 = ram[l.player_x as usize];
    let msb = ram[l.sprite_x_msb as usize];
    let swim = ram[l.swim as usize];
    let (x, flip) = if swim != 0 && swim & 0x80 == 0 {
        if ram[l.facing as usize] != 0 { (x0.wrapping_add(2), x0 >= 0xFE) } else { (x0.wrapping_sub(2), x0 < 2) }
    } else {
        (x0, false)
    };
    ram[l.player_sprite_msb as usize] = if flip { msb ^ 7 } else { msb };
    ram[l.player_sprite_x as usize] = x;
    let y = ram[l.player_y as usize];
    ram[l.player_sprite_y as usize] = y;
    ram[l.feet_depth as usize] = y.wrapping_sub(3) & 7;
}
