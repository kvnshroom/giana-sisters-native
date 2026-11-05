//! The in-game title of the original (`L084F` after the system set-up, and the input loop `L0940`): the title level (stage
//! `$27`, built by the stage decompressor) scrolls by itself under the interrupt chain (`$154D != 0` makes `sub_0EAB` auto-scroll
//! by table step `$24`), four lines of text sit in the first rows (`sub_9881`), and the main line waits for key `1`, key `2` or
//! the fire button. Everything else (sprites, colours, scroll) is the ordinary frame of the port.
//!
//! The original runs this with `$20 = 2` (`L0937`): the stage number selects the enemy and start tables, so those are the ones of
//! stage 2 while the map and the colours are the title's own (`sub_0D39` runs with `$20 = $0C`).
//!
//! Hooks for the other parts: [`GameState::new_ingame_title`] starts the title, [`GameState::title_frame`] runs one frame of it and
//! [`GameState::after_game_over`] returns to it (the original jumps to `L084F` from `L09B6` when both players are finished).

use crate::state::{ControllerInput, GameState};

/// The stage number of the title level (`$08D7`).
pub const TITLE_STAGE: u32 = 0x27;
/// The value of `$20` while the title waits (`$0937`).
pub const TITLE_WAIT_STAGE: u32 = 2;

/// Frames of the raster chain that pass between `sub_0E39` and the input loop (the decompressor, `sub_9936`, takes about
/// as long): only the animated characters (`sub_163C`) are seen to advance in them.
pub const SETUP_FRAMES: u32 = 19;

/// The waits between the fire button and the first frame of stage 1, as the program takes them (measured with the slice:
/// the stage is built in 37 frames, the four steps of the banner wait `sub_154F` last 23, 23, 24 and 23 frames, the stage
/// shows 2 frames after the last step).
fn title_start_timing() -> crate::state::StageTiming {
    crate::state::StageTiming {
        post_fire: 37,
        swap_wait: 0,
        banner_steps: vec![23, 23, 24, 23],
        setup_wait: 2,
        vars_after: 0,
        ..crate::state::StageTiming::default()
    }
}

/// The waits of a restart after a lost life, measured with the slice (enemy deaths in stage 1, two lives in a row): the death
/// jump's busy wait is 23 chains, and the stage is built in 76 chains (the build of the title start, 37, plus the sprite
/// set exchange that a restart has to do again before the banner).
fn lost_life_timing() -> crate::state::StageTiming {
    crate::state::StageTiming { post_fire: 76, death_wait: 23, ..title_start_timing() }
}

/// The keys of the title besides the joystick: the C64 keyboard column 7 is read as a whole (`$0952..$095E`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleKeys {
    pub key_1: bool,
    pub key_2: bool,
}

/// What a frame of the title decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleEvent {
    Waiting,
    /// A game has been chosen and started (stage 1 is set up and shows its banner).
    Started { two_players: bool },
}

impl GameState {
    /// The state at `L0940`, as the original reaches it: system set-up, title level, texts, tune 3.
    pub fn new_ingame_title() -> Self {
        let mut s = Self::new_boot();
        s.enter_title();
        s
    }

    /// `$08C5..$093D`: the part of `L084F` after `init_system`.
    pub fn enter_title(&mut self) {
        // $08C5: LDA #$FF; STA $0431; STA $154D
        self.flag_0431 = 0xFF;
        self.music_paused = 0xFF;
        // $08D0..$08D4: $20 = $0C; JSR sub_0D39 (colours of the table entry of stage 12)
        self.stage = 0x0C;
        self.stage_colours_sub_0d39();
        // $08D7: $20 = $27; sub_0E39 (raster chain on), sub_0E66, sub_9936, sub_0D54
        self.stage = TITLE_STAGE;
        self.vic_irq_enable = 1;
        self.status_bar_frame_sub_0e66();
        self.decompress_stage_layout();
        self.load_stage_tiles();
        // $08E7..$0924: the four lines of text, `$04/$05` = `$1580`, `$15A7`, `$15BB`, `$15D4`
        for (mut p, x, row) in [(0x1580usize, 0usize, 0usize), (0x15A7, 0x0A, 1), (0x15BB, 9, 3), (0x15D4, 2, 4)] {
            self.print_string_at_sub_9881(&mut p, x, row);
        }
        // $0927: STA $D7FF,Y for Y = $C8..1: the first five rows are green
        for c in &mut self.color_ram[..0xC8] {
            *c = 5;
        }
        // the stage timer is not set up before the game: `$0448`, `$0449`, `$044A` are zero (the first tick counts it down to 9999)
        self.timer_subsecond = 0;
        self.timer_seconds_low = 0;
        self.timer_seconds_high = 0;
        self.time = 0;
        // the player sprites were never set up (`sub_13F0` runs with the stage): the sprite registers are zero and the player falls
        self.player_x_low = 0;
        self.player_y_c64 = 0;
        self.en.spr_xy[0] = 0;
        self.en.spr_xy[1] = 0;
        // the raster chain runs while the stage is built (`sub_0E39` enabled it): the animated characters keep going
        for _ in 0..SETUP_FRAMES {
            self.animate_chars_sub_163c();
        }
        // $0931: tune 3 was requested by `select_game_mode`; $0937: $20 = 2; $093D: $0431 = 0
        self.stage = TITLE_WAIT_STAGE;
        self.flag_0431 = 0;
        self.title_active = true;
    }

    /// One frame of the title: the interrupt chain, then the input loop `L0940` looks at the keys.
    pub fn title_frame(&mut self, input: &ControllerInput, keys: TitleKeys) -> TitleEvent {
        self.irq_frame_core(input);
        // $0945: JSR sub_9F77 (the chain has read the joystick already); $0955: LDA $DC01 against `$FE` (key 1) and `$F7` (key 2)
        if keys.key_1 && !keys.key_2 {
            return self.start_from_title(false);
        }
        if keys.key_2 && !keys.key_1 {
            return self.start_from_title(true);
        }
        // $0960: LDA $0401; BEQ L0940 (fire from joystick port 2 or the space key)
        if self.joystick_fire != 0 {
            return self.start_from_title(false);
        }
        TitleEvent::Waiting
    }

    /// `L0965..L09DA`: one player (fire, key 1) or two (key 2), then stage 1. The code from `L096A` on sets up the players and
    /// falls into `L09B6` with `$1579 = 1` (the `BIT` at `$09B5` hides the `LDA #$00`), the same entry that a lost life uses:
    /// the clear screen, the banner (about 2.6 seconds, the timing of a plain restart) and the stage set-up run as frames of
    /// the main line (`update`).
    pub fn start_from_title(&mut self, two_players: bool) -> TitleEvent {
        self.title_active = false;
        // the column shift that the last chain started runs to its end inside that interrupt before the main line goes on
        self.column_scroll_finish_sub_9600();
        // $0965/$096A: `$1576` is 1 for one player (the second counts as finished), 0 for two; `$1575` = 0
        self.player_save[12] = 0;
        self.player_save[13] = if two_players { 0 } else { 1 };
        self.two_players = two_players;
        self.active_player = if two_players { 0 } else { 1 };
        // $096C: $1568 = 0 (Giana); $096F: $154D = 0
        self.player = 0;
        self.music_paused = 0;
        // $0972..$09A3: `$20` back to 0 and plus 1 for both players; scores, power-up and spawn variant of both players cleared;
        // three lives each
        self.player_save[..8].fill(0);
        self.player_save[8] = 1;
        self.player_save[9] = 1;
        let lives = self.new_game_lives();
        self.player_save[10] = lives;
        self.player_save[11] = lives;
        self.player_save[14] = 0;
        self.player_save[15] = 0;
        self.lives_c64 = lives;
        self.lives = lives as u32;
        // $09A3: $0456 = $2169 = $0417 = $046D = 0; $09B1: INC $20
        self.spawn_variant = 0;
        self.st_2169 = 0;
        self.st_046d = 0;
        self.stage = 1;
        // $09B3..$09B8: $1579 = 1
        self.exit_1579 = true;
        self.screen = crate::state::GameScreen::InGame;
        self.legacy_model = false;
        self.first_in_game_tick = false;
        self.confirm_banner = false;
        // measured: the sub-second tick of the chain in which the fire press is noticed still counts (the time display changes
        // in the frame where the slice changes it; without this, one frame early)
        self.timer_subsecond = self.timer_subsecond.wrapping_add(1);
        self.banner_fire_skips = true;
        self.restart_timing.push_front(title_start_timing());
        // a lost life runs the same `L09B6` path: the same waits (the banner of the restart does not skip itself)
        self.default_restart_timing = Some(lost_life_timing());
        self.restart_stage_l09b6();
        TitleEvent::Started { two_players }
    }

    /// The hook for the high score part: the game is over (both players finished, and the high score entry and save are done), and
    /// the original jumps back to `L084F`, the title. Everything the game left behind is cleared, as the original's own
    /// initialisation does; what must outlive a game (the high score table) has to be copied over here.
    pub fn after_game_over(&mut self) {
        *self = Self::new_ingame_title();
    }
}
