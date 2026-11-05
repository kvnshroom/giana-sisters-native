//! Engine B: water physics and the other small player hooks of the hack (docs/rom-hack-family-b.md, sections 8 and 9).
//!
//! The player code of the hack is the original's with a few inserts. This file holds the water ones:
//!
//! | Hack code | What it does | Port |
//! |---|---|---|
//! | `$8D61` | the water flag `$FF` (head tile and body tile are water: `$0420 & $0423` bit 1), strokes, sinking, hovering | [`GameState::b_water_flag`] |
//! | `$FFD7` (called at `$0F87`) | in water: horizontal momentum jumps to `$20`, `$15` is incremented | [`GameState::b_ffd7`] |
//! | `$0F77` | no direction pressed in water: momentum `$FF`, move state 0, no horizontal movement | [`GameState::b_water_no_direction`] |
//! | `$8DD4` | swimming animation (shapes `$60..$63`, `$37/$38`, `$43/$44`) instead of `$1078` | [`GameState::b_swim_animation`] |
//! | `$FFBF` | the tile surroundings of a player below the screen are read as if Giana stood at Y = `$DB` | [`GameState::b_tile_surroundings`] |
//! | `$8187` (ladder contact) | patches `$8D6B`/`$8E1D`: water physics everywhere and a flickering animation | [`Water::ladder`] |
//!
//! Every method starts with the cheap engine-B-off check of the hook it belongs to; the original engine never gets here.

use crate::state::GameState;

/// The water state of engine B (zero page `$FF` and the two self-modified bytes of the ladder).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Water {
    /// `$FF` bit 7: Giana is in water this frame (or touches a ladder)
    pub in_water: bool,
    /// ladder patch: `$8D6B = $EA` (always water) and `$8E1D = $49` (`EOR #1`: the swim frame flickers). Set by the contact
    /// with a ladder object (class `$12`, score byte bit 7), restored by the stage hook (`$2F77..$2F7E`).
    pub ladder: bool,
}

impl Water {
    /// The stage hook restores both patched bytes (`$2F77`) for every stage `< $65`.
    pub fn reset(&mut self) {
        *self = Water::default();
    }
}

impl GameState {
    /// True when engine B runs and Giana swims (`$FF` bit 7).
    #[inline]
    pub fn b_in_water(&self) -> bool {
        self.engine_b.as_ref().is_some_and(|b| b.water.in_water)
    }

    /// `$8D61`: sets `$FF` from the tiles around Giana and runs the swim controls. Called every frame before the player
    /// movement `$0EB0` (from `$88DC`).
    pub(crate) fn b_water_flag(&mut self) {
        let Some(b) = self.engine_b.as_mut() else { return };
        let f = self.surrounding_tile_flags;
        // `$0420 & $0423 & 2` (head tile and body tile); the ladder patch turns the test into "always"
        let water = b.water.ladder || f[1] & f[4] & 2 != 0;
        b.water.in_water = water;
        if !water {
            return;
        }
        // $8D74: Y is forced odd
        self.player_y_c64 |= 1;
        let joy = self.joystick_directions;
        // $8D7C: up pressed and the jump state is "rising" ($17 = $80): keep stroking
        if joy & 1 != 0 && self.jump_state == 0x80 {
            // $8D89: not too high (Y >= $4D), else the stroke ends (L8DCF)
            if self.player_y_c64 < 0x4D {
                self.jump_state = 0;
            } else {
                // $8D90: $17 = $80, arc index $042A = 4 (2 px per frame while up is held)
                self.jump_state = 0x80;
                self.jump_ascent_idx = 4;
            }
            return;
        }
        // $8D9A: down pressed?
        if joy & 2 == 0 {
            self.jump_state = 0; // L8DCF: neutral buoyancy, hover
            return;
        }
        // $8DA1: solid tile below Giana: she cannot sink
        if f[7] & 1 != 0 {
            self.jump_state = 0;
            return;
        }
        // $8DA7..: sink unless she stands between two solid tiles on the floor ($8DC5 / $8DCF)
        let sink = (f[3] | f[5]) & 1 != 0 || self.sprite_head_y & 0x06 != 0x04 || (f[6] | f[7] | f[8]) & 1 == 0;
        if sink {
            // $8DC5: $17 = $FF, arc index $042A = $0E (the fall table from there: 2, 2, 2 ... 1, 0)
            self.jump_state = 0xFF;
            self.jump_ascent_idx = 0x0E;
        } else {
            self.jump_state = 0;
        }
    }

    /// `$FFD7` (called from `$0F87` when a direction is pressed): in water the momentum `$040A` of 2 or more (as a signed
    /// byte) becomes `$20` at once, and the move state `$15` is incremented (the code that was meant to set it to `$80`/`$FF`
    /// never stores; the increment is what the hack really does and it keeps braking from ever triggering in water).
    pub(crate) fn b_ffd7(&mut self) {
        if !self.b_in_water() {
            return;
        }
        if self.player_momentum.wrapping_sub(2) & 0x80 == 0 {
            // CMP #2 / BMI: N is bit 7 of (momentum - 2); 2..=$81 pass
            self.player_momentum = 0x20;
        }
        self.move_state = self.move_state.wrapping_add(1);
    }

    /// `$0F77`: no direction pressed. In water the momentum becomes `$FF`, the move state 0 and the routine ends (no
    /// horizontal movement at all). Returns true when it handled the frame.
    pub(crate) fn b_water_no_direction(&mut self) -> bool {
        if !self.b_in_water() {
            return false;
        }
        self.player_momentum = 0xFF;
        self.move_state = 0;
        true
    }

    /// `$8DD4`, in place of `$1078`: the swimming animation. Returns false when Giana is not in water (the caller runs the
    /// original animation selector).
    pub(crate) fn b_swim_animation(&mut self) -> bool {
        let Some(b) = self.engine_b.as_ref() else { return false };
        if !b.water.in_water {
            return false;
        }
        let ladder = b.water.ladder;
        let f0 = self.sprite_0_frame;
        // $8DDB..: which pair of shapes: left (`$62`) or right (`$60`)
        let pick = if f0 < 0x60 {
            Some(f0 == 0x4B) // the left jump frame keeps facing left
        } else {
            match self.move_state {
                0 => None, // $8DEA: not moving: keep the shapes (L8E1A)
                0x80 => Some(true),
                _ => Some(false),
            }
        };
        match pick {
            Some(true) => {
                self.sprite_0_frame = f0 & 1 | 0x62;
                self.sprite_1_frame = 0x43;
                self.sprite_2_frame = 0x44;
            }
            Some(false) => {
                self.sprite_0_frame = f0 & 1 | 0x60;
                self.sprite_1_frame = 0x37;
                self.sprite_2_frame = 0x38;
            }
            None => {}
        }
        // L8E1A: ORA #1, or EOR #1 with the ladder patch ($8E1D)
        self.sprite_0_frame = if ladder { self.sprite_0_frame ^ 1 } else { self.sprite_0_frame | 1 };
        true
    }

    /// `$FFBF`, in place of the `JSR $9DF4` of the IRQ: below the screen (Y >= `$E1`) the surroundings are looked up as if
    /// Giana's Y were `$DB`, so the bottom row of the map still counts (water in a pit, "no death in the abyss").
    pub(crate) fn b_tile_surroundings(&mut self) {
        let y = self.player_y_c64;
        if self.engine_b.is_some() && y >= 0xE1 {
            self.player_y_c64 = 0xDB;
            self.update_tile_collision_sub_9df4();
            self.player_y_c64 = y;
        } else {
            self.update_tile_collision_sub_9df4();
        }
    }
}
