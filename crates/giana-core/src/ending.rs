//! The ending after the last stage (`$0AC7..$0B23` of the main loop, single player only).
//!
//! The diamond at the end of stage 33 sets `$157E`; the main loop then clears the screen, switches the sprites off, prints
//! three lines (the name of the player is the first one) and waits in `sub_154F` (A = $0A, about 255 frames, fire skips).
//! The original goes on with the high score code from the disk (`L0C4E`, not part of the image): the port sets
//! `game_over`, the app then shows its own high score entry and the title.

use crate::mainloop::MainLine;
use crate::state::GameState;

/// Chains of the steps of the wait `sub_154F` (A = $0A, Y = 0), measured in `29_ending`.
pub const ENDING_STEPS: [usize; 10] = [22, 23, 23, 23, 23, 23, 22, 23, 23, 24];

impl GameState {
    /// `$0ABF`: LDA $157E; BNE L0AC7. Returns true when the ending took over (single player only).
    pub(crate) fn ending_hook_l0abf(&mut self) -> bool {
        if self.en.flag_157e == 0 {
            return false;
        }
        if self.two_players {
            self.ending_two_players();
            return true;
        }
        self.ending_start_l0ac7();
        true
    }

    /// A game for two (the ending text is for one player only, the owner's decision): the player who won stage 33 is
    /// finished (`$1575,Y = $FF`, `L0A14` skips them), the other one plays on; when both are done the game is over.
    fn ending_two_players(&mut self) {
        self.en.flag_157e = 0;
        self.flag_0431 = 0;
        self.st_046d = 0;
        self.player_save[12 + self.player as usize] = 0xFF;
        if self.player_save[12] != 0 && self.player_save[13] != 0 {
            self.game_over = true;
        } else {
            self.restart_stage_l09b6();
        }
    }

    /// `$0AC7..$0B23`: clear, sprites off, the three lines, then `sub_154F` with A = $0A
    fn ending_start_l0ac7(&mut self) {
        self.en.flag_157e = 0;
        // $0ACF: LDY $1568; LDA #$FF; STA $1575,Y (this player is finished)
        self.player_save[12 + self.player as usize] = 0xFF;
        // $0AD5: JSR sub_9F45; LDA #0; STA $D015
        self.screen_ram = [0; 1000];
        self.color_ram = [0; 1000];
        self.en.vic_me = 0;
        // $0ADD: line 1 at $0B26 (column 9, row 9), the name of the player (string $1568) at column $0C
        let mut p = 0x0B26;
        self.print_string_at_sub_9881(&mut p, 9, 9);
        self.print_string_sub_9881(self.player as usize, 0x0C, 9);
        let mut p = 0x0B3D;
        self.print_string_at_sub_9881(&mut p, 9, 0x0B);
        let mut p = 0x0B54;
        self.print_string_at_sub_9881(&mut p, 9, 0x0D);
        // $0B1B: LDY #0; LDA #$0A; JSR sub_154F (starts in the next chain)
        self.main = MainLine::Ending(None);
    }

    /// One chain of the wait; at the end (or on fire) the game is over for this player.
    pub(crate) fn ending_step(&mut self, st: Option<(usize, usize)>) {
        let Some((step, left)) = st else {
            self.banner_delay = 0x0A;
            self.main = MainLine::Ending(Some((0, ENDING_STEPS[0])));
            return;
        };
        // $1558: fire ends the wait
        if self.joystick_enabled && self.main_fire {
            self.ending_finish();
            return;
        }
        if left > 0 {
            self.main = MainLine::Ending(Some((step, left - 1)));
            return;
        }
        // $1562: DEC $157C
        self.banner_delay = self.banner_delay.wrapping_sub(1);
        let next = step + 1;
        if next >= ENDING_STEPS.len() {
            self.ending_finish();
            return;
        }
        self.main = MainLine::Ending(Some((next, ENDING_STEPS[next].saturating_sub(1))));
    }

    /// `$0B1E`: LDA #0; STA $0401; JMP L0C4E (high score entry, not ported: the app has its own)
    fn ending_finish(&mut self) {
        self.main = MainLine::Loop;
        self.game_over = true;
    }
}
