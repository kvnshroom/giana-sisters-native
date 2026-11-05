//! Engine B: standing on and riding scripted moving platforms (elevators) and the player routine around them.
//!
//! Hack code (docs/rom-hack-family-b.md, sections 6, 7.4 and 9):
//!
//! | Hack code | What it does | Port |
//! |---|---|---|
//! | `$88B0` | the player routine of the IRQ: scroll push of a platform (`$F8`), jump re-alignment, water flag, then `$0EB0` | [`GameState::b_player_move`] |
//! | `$8132..$815D` | contact with a class `$7F` sprite: stand on it (`$FD`, `$F9`, `$FA`) or bump (`$17 = 0`) | [`GameState::b_platform_contact`] |
//! | `$89CD` | horizontal overlap of Giana and the platform: `$FD` | [`GameState::b_platform_overlap`] |
//! | `$890A` | carry Giana with the platform, leave it | [`GameState::b_platform_ride`] |
//! | `$89ED` | script mover (kind 2): 3-byte steps `(frames, dx, dy)` | `Sprites::b_script_mover` (E4) |
//!
//! # Division of work with the sprite program (E4/E5)
//!
//! The sprite slots belong to `sprites.rs`: its spawner creates the platforms (kind 0/1/2 objects of class `$7F`), its movers move
//! them (script mover `$89ED` for kind 2, walker, hopper), its retire code removes them. This file holds what is special about
//! class `$7F`: [`GameState::b_platform_contact`] is called by the collision handler where `$8132` finds the class (the hit test
//! `$821A` takes the highest slot that touches Giana, so an enemy in a higher slot shadows a platform under it),
//! [`GameState::b_platform_ride`] runs once per frame where `$77EF` calls `$890A`, and [`GameState::b_player_move`] is the
//! player routine `$88B0`. The platform bookkeeping the hack keeps in the slot arrays (`$0774` speed, `$07B4` direction, `$07D0`,
//! `$07BC`) is read and written in the slot arrays of the sprite program.

use crate::image::img;
use crate::state::GameState;

/// Platform state of engine B: slots and the zero page items `$F8..$FD`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub struct Platforms {
    /// `$FD`: Giana stands on a platform (1)
    pub fd: u8,
    /// `$FB`: set to `$FF` when a fall starts (read and cleared by the next `$88B0`)
    pub fb: u8,
    /// `$F8`: the platform wants the playfield to scroll
    pub f8: u8,
    /// `$F9`: slot of the platform Giana stands on
    pub f9: u8,
    /// `$FA`: sprite register offset (2 * slot) of that platform
    pub fa: u8,
}


impl Platforms {
    /// Stage build (`$2F5D`: `$FD = 0`, spawner restarts at the stage start).
    pub fn reset(&mut self) {
        *self = Platforms::default();
    }
}

/// `$9FF5`: arc indices at which a jump is tile aligned.
const ALIGN_TABLE: usize = 0x9FF5;

impl GameState {
    /// `$88B0`: the player routine of the hack in the IRQ (in place of the call of `$0EB0`).
    pub(crate) fn b_player_move(&mut self) {
        let Some(b) = self.engine_b.as_mut() else {
            return self.update_player_physics_sub_0eab();
        };
        let (f8, f9) = (b.plat.f8, b.plat.f9 as usize);
        let (fb, fd, water) = (b.plat.fb, b.plat.fd, b.water.in_water);
        b.plat.f8 = 0;
        let speed = self.b_slot(0x774 + (f9 & 7)) & 0x7F;
        // $88B0: a platform that wants the playfield to scroll (Giana is at the scroll column) scrolls it by its speed
        if f8 != 0 {
            let before = self.scroll_fine;
            self.scroll_fine = before.wrapping_sub(speed) & 7;
            if before < speed {
                self.scroll_wrap_flag = 0xFF;
            }
        }
        // $88CC..: re-align a jump that did not start on the tile grid at the tile aligned arc indices
        let off = self.player_y_c64.wrapping_sub(3) & 7;
        if off != 0 && fb | fd | self.jump_phase == 0 && !water && (0..=8).any(|y| img(ALIGN_TABLE + y) == self.jump_ascent_idx) {
            // $88FD / $89C0: Y -= (Y - 3) & 7, the other two sprites follow
            self.player_y_c64 = self.player_y_c64.wrapping_sub(off);
            self.sprite_head_y = self.player_y_c64.wrapping_add(1);
            self.sprite_dress_y = self.player_y_c64.wrapping_add(2);
        }
        // $88D8
        if let Some(b) = self.engine_b.as_mut() {
            b.plat.fb = 0;
        }
        self.b_water_flag();
        self.update_player_physics_sub_0eab();
    }

    /// The ground check `$11FE` of the hack: no fall starts on a platform or in water; a fall that starts sets `$FB`.
    /// Returns true when the fall must not start.
    pub(crate) fn b_fall_blocked(&mut self) -> bool {
        let Some(b) = self.engine_b.as_mut() else { return false };
        if b.plat.fd != 0 || b.water.in_water {
            return true;
        }
        b.plat.fb = 0xFF;
        false
    }

    /// `$11CE`: `$FD` counts as a solid tile under Giana when she lands.
    #[inline]
    pub(crate) fn b_platform_flag(&self) -> u8 {
        self.engine_b.as_ref().map_or(0, |b| b.plat.fd)
    }

    /// `$89CD`: Giana's X against the platform at sprite register offset `y` (low bytes only, like the code): `$FD` = 1 when
    /// she is within -20..+19 of the platform's X, else 0.
    pub(crate) fn b_platform_overlap(&mut self, slot: usize) {
        let px = self.spr(slot * 2);
        let gx = self.player_x_low;
        let stands = match px.checked_sub(0x14) {
            None => true, // platform X < $14: stands
            Some(left) => {
                if left >= gx {
                    false
                } else {
                    match px.checked_add(0x13) {
                        None => true,
                        Some(right) => right >= gx,
                    }
                }
            }
        };
        if let Some(b) = self.engine_b.as_mut() {
            b.plat.fd = stands as u8;
        }
    }

    /// `$8132..$815D`: contact of Giana with the platform in `slot` (class `$7F`, after the hit test `$821A`).
    pub fn b_platform_contact(&mut self, slot: usize) {
        let Some(b) = self.engine_b.as_mut() else { return };
        b.plat.f9 = slot as u8;
        let platform_y = self.spr(slot * 2 + 1);
        // CPX $05F9: the platform top (Y - 1) is not above Giana's lowest sprite: she is above it, so she stands
        if platform_y.wrapping_sub(1) >= self.sprite_dress_y {
            self.b_platform_overlap(slot);
            if let Some(b) = self.engine_b.as_mut() {
                b.plat.fa = (slot * 2) as u8;
            }
        } else {
            // from the side or below: the jump is cut
            self.jump_state = 0;
        }
    }

    /// `$890A`: carry Giana with the platform she stands on.
    pub fn b_platform_ride(&mut self) {
        let Some(b) = self.engine_b.as_ref() else { return };
        if b.plat.fd == 0 {
            return;
        }
        let slot = (b.plat.fa / 2) as usize & 7;
        let x = b.plat.f9 as usize & 7;
        self.b_platform_overlap(slot);
        if self.jump_state == 0x80 {
            self.b_platform_leave(); // $89E8
        }
        let f = self.surrounding_tile_flags;
        // $891E: head against a solid tile: the platform's `$07D0` flag drops and `$07BC` counts
        if f[1] & 1 != 0
            && self.b_slot(0x7D0 + x) != 0 {
                self.b_set_slot(0x7D0 + x, 0);
                let bc = self.b_slot(0x7BC + x).wrapping_add(1);
                self.b_set_slot(0x7BC + x, bc);
            }
        let (d0, speed, left) = (self.b_slot(0x7D0 + x), self.b_slot(0x774 + x) & 0x7F, self.b_slot(0x7B4 + x) != 0);
        // $8931: landed on solid ground beside the platform: she leaves it
        if d0 == 0 && (f[3] | f[5]) & 1 == 0 && (f[6] | f[7] | f[8]) & 1 != 0 {
            self.b_platform_leave();
            return;
        }
        let mut gx = self.player_x_low;
        let mut store_x = true;
        if !left {
            // $8953: moving right
            if f[8] & 1 != 0 {
                store_x = false;
            } else if gx >= 0xA0 && self.map_column != 0 {
                // the playfield scrolls instead of Giana walking ($8972..$897E)
                self.engine_b.as_mut().unwrap().plat.f8 = 0xFF;
            } else {
                let (n, carry) = gx.overflowing_add(speed);
                gx = n;
                if carry {
                    self.sprite_x_msb |= 0x07;
                }
            }
        } else {
            // $8982: moving left
            if f[6] & 1 != 0 {
                store_x = false;
            } else {
                let (n, borrow) = gx.overflowing_sub(speed);
                gx = n;
                if borrow {
                    self.sprite_x_msb &= 0xF8;
                }
                // $899B: with an MSB set she is on the right half: nothing more; else the platform turns round
                // before it would carry her off the left edge
                if self.sprite_x_msb & 7 == 0 && gx < 0x1E {
                    let l = self.b_slot(0x7B4 + x) ^ 1;
                    self.b_set_slot(0x7B4 + x, l);
                }
            }
        }
        if store_x {
            self.player_x_low = gx;
            self.sprite_head_x = gx;
            self.sprite_dress_x = gx;
        }
        // $89BA: Y = platform Y - 18
        let y = self.spr(slot * 2 + 1).wrapping_sub(0x12);
        self.player_y_c64 = y;
        self.sprite_head_y = y.wrapping_add(1);
        self.sprite_dress_y = y.wrapping_add(2);
    }

    /// `$89E8`: Giana is no longer on a platform.
    fn b_platform_leave(&mut self) {
        if let Some(b) = self.engine_b.as_mut() {
            b.plat.fd = 0;
        }
    }
}
