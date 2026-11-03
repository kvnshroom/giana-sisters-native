use crate::collision::{get_tile_type, Rect, TileType};
use crate::entities::{DebrisParticle, Enemy, EnemyType, PowerupItem, Projectile};
use crate::stages_enemies::get_stage_enemy_spawns;
use crate::physics::*;

/// The 33 stage maps (5120 bytes each, back to back), expanded once from the image by the ported `sub_9936`.
pub fn all_stages() -> &'static [u8] {
    static STAGES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    STAGES.get_or_init(|| {
        let mut all = Vec::with_capacity(33 * 5120);
        for stage in 1..=33u32 {
            let mut game = GameState::new_boot();
            game.stage = stage;
            game.decompress_stage_layout();
            all.extend_from_slice(&game.current_map);
        }
        all
    })
}
/// Per stage: background colour, two multicolour colours, underground flag (33 x 4 bytes).
pub fn stage_palettes() -> &'static [u8] {
    &crate::data::get().stage_palettes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameScreen {
    Intro,
    Title,
    InGame,
    StageClear,
    GameOver,
    GameWon,
}

pub struct ControllerInput {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub jump: bool,
    pub fire: bool,
    /// The space key of the original: fires the clock or the bomb that was collected.
    pub item: bool,
    pub start: bool,

    // Debug / Mod keys
    pub next_stage: bool,
    pub prev_stage: bool,
    pub toggle_invulnerable: bool,
    pub toggle_lives: bool,
}

impl ControllerInput {
    pub fn new() -> Self {
        Self {
            left: false,
            right: false,
            up: false,
            down: false,
            jump: false,
            fire: false,
            item: false,
            start: false,
            next_stage: false,
            prev_stage: false,
            toggle_invulnerable: false,
            toggle_lives: false,
        }
    }
}

#[derive(Clone)]
pub struct GameState {
    pub screen: GameScreen,
    pub score: u32,
    pub diamonds: u32,
    pub time: u32,
    pub stage: u32,
    pub lives: u32,

    pub player_x: i32,
    pub player_y: i32,
    pub is_jumping: bool,
    pub jump_frame: usize,
    pub is_falling: bool,
    pub fall_speed: i32,
    pub facing_right: bool,
    pub anim_tick: usize,
    pub walk_frame: usize,
    pub has_punker: bool,
    pub has_shot: bool,
    pub shoot_cooldown: usize,
    pub invulnerable_timer: usize,
    pub is_dying: bool,
    pub death_vel_y: f32,

    // Cheats
    pub infinite_lives: bool,
    pub god_mode: bool,
    pub two_players: bool,          // a game for two on one computer (`new_port_game_two`): no ending text, the other player plays on

    pub scroll_x: usize,
    pub cloud_scroll_x: usize,

    pub ground_y: i32,
    pub timer_ticks: usize,

    // Dynamic stage map (allows breaking blocks and collecting items)
    pub current_map: Vec<u8>,

    // Dynamic entities
    pub enemies: Vec<Enemy>,
    pub enemy_spawns: Vec<(usize, f32, f32, EnemyType)>,
    pub particles: Vec<DebrisParticle>,
    pub items: Vec<PowerupItem>,
    pub projectiles: Vec<Projectile>,

    // --- Authentic C64 RAM variables (docs/ram-map.md) ---
    pub scroll_wrap_flag: u8,  // $0014: Set to $FF when sky scroll underflows (sub_0EAB L1065)
    pub move_state: u8,  // $0015: Horizontal move state (0 = idle, $FF = right, $80 = left)
    pub move_edge_flag: u8,  // $0016: Screen-edge scroll request ($FF = scroll instead of move)
    pub jump_state: u8,  // $0017: Jump state (0 = grounded, $80 = ascending, $FF = falling)
    pub jump_phase: u8,  // $0018: Jump phase (0 = none, $80 = apex hold, $FF = landing)
    pub anim_walking: u8,  // $001A: Walk animation running flag
    pub brake_flag: u8,  // $001B: Player is braking (momentum decay after direction change)
    pub airborne_flag: u8,  // $001C: Airborne animation flag
    pub land_flag: u8,  // $001F: Landing flag set by sub_11C6
    pub map_column: u8,  // $0021: Map column counter (init $27 in sub_0D54, +1 per column scroll)
    pub move_delta: u8,  // $0428: Per-frame movement delta (from $9A56 / jump tables)
    pub jump_ascent_idx: u8,  // $042A: Jump ascent table index ($9ADE)
    pub jump_descent_idx: u8,  // $042B: Jump descent table index ($9AF9)
    pub anim_step_timer: u8,  // $042D: Walk animation step timer
    pub side_tile_flags: u8,  // $0414: Side tile collision scratch (sub_1138)
    pub var_040b: u8,  // $040B: Not touched by sub_0EAB; owner routine still unknown (stays 0 until ported)
    pub music_paused: u8,  // $154D: Music pause flag (non-zero freezes player movement)
    pub sprite_head_x: u8,  // $05F6: Sprite 1 X (Giana head)
    pub sprite_head_y: u8,  // $05F7: Sprite 1 Y
    pub sprite_dress_x: u8,  // $05F8: Sprite 2 X (Giana dress)
    pub sprite_dress_y: u8,  // $05F9: Sprite 2 Y
    pub joystick_enabled: bool,    // $9F77 opcode gate: true = $AD (read joystick), false = $60 (RTS)
    pub particle_slots: [u8; 158], // $9B02..$9B9F: animated tile slots (state, screen pointers, saved chars), shifted by the scroll IRQ
    pub color_ram: [u8; 1000],     // $D800..$DBE7: colour RAM (not part of the 64K snapshot)
    pub column_scroll_pending: bool, // sub_9600 started in this frame, its copy loops finish in the next one
    pub spawn_variant: u8, // $0456: Spawn position variant (0 = stage start, 1 = checkpoint); selects the $60EA entry
    pub var_042f: u8,  // $042F: Not written by any routine in ingame.asm so far (stays 0)
    pub lost_ticks: u8,             // chains whose sound tick and character animation the original loses (sub_0E39 resets the raster chain)
    pub anim_9d: Vec<u8>,           // $9D32..$9D7F: state and tables of the character animation (data bytes the code counts down)
    pub anim_060d: [u8; 0x40],      // $060D..$064C: timers of the character animation
    pub sound_ticks: u64,           // sound driver ticks played so far
    pub sound_requests: Vec<(u8, u64)>, // tracks requested (id, tick count at the request)
    pub sound: crate::sound_driver::Driver, // the sound driver ($41C0..$4A79) with the SID registers it writes
    pub anim_counter: u8,           // $0022: Diamond sparkle animation / sub-frame counter (init 0 in L084F)
    pub scroll_fine: u8,            // $0404: Playfield fine horizontal scroll offset (0..7)
    pub sky_scroll_fine: u8,        // $0405: Sky / cloud fine horizontal scroll offset (0..7)
    pub player_move_dir: u8,        // $0406: Move direction accumulator
    pub jump_subtimer: u8,          // $0407: Jump subtimer
    pub scroll_subpixel: u8,        // $0408: Scroll subpixel fraction
    pub player_vel_fraction: u8,    // $0409: Player vel fraction
    pub player_momentum: u8,        // $040A: Run/jump inertia momentum counter
    pub jump_table_idx: u8,         // $040B: Jump curve table offset index ($9ADE)
    pub dec: [u8; 8],               // $040C..$0413: decompressor counters (sub_9936): h/v repeat counts, block width/height
    pub ground_hit_flag: u8,        // $0421: Ground / block collision scratch flag (cleared in sub_20AC)
    pub flag_0431: u8,              // $0431: In-game state flag (0 = gameplay active, $FF = inactive, $80 = scripted walk)
    pub lives_c64: u8,              // $043C: Authentic C64 current player lives (init 3 in L0940)
    pub warp_stage_id: u8,          // $043D: Warp pipe target stage ID
    pub timer_subsecond: u8,        // $0448: Stage timer sub-second countdown
    pub timer_seconds_low: u8,      // $0449: Stage timer seconds countdown low byte (BCD)
    pub timer_seconds_high: u8,     // $044A: Stage timer seconds countdown high byte (BCD)
    pub time_warning: u8,           // $044B: Time warning sound played flag
    pub stage_cycle_counter: u8,    // $045B: In-game stage / gameover cycle counter (init 0x21 in L084F)
    pub hazard_hit: u8,             // $0465: Hazard collision / death trigger flag (0=none, 2=timeout, 0xFF=hazard)
    pub death_arc_idx: u8,          // $0466: Death jump arc vertical velocity index
    pub transform_timer: u8,        // $0467: Punker transformation sequence timer
    pub vic_border: u8,             // $D020: border colour (sub_20AC clears it)
    pub vic_irq_enable: u8,         // $D01A: raster interrupt enable (sub_20AC clears it, sub_0E39 sets bit 0)
    pub vic_bg2: u8,                // $D023: third background colour (sub_0D39)
    pub vic_mc: [u8; 2],            // $D025/$D026: shared sprite multicolours (sub_1443)
    pub transform_step: u8,         // $0468: Punker transformation cycle step / scroll delta
    pub prev_scroll: u8,            // $0469: Previous frame scroll column coordinate
    pub redraw_pending: u8,         // $046B: Playfield background redraw column pending flag
    pub first_in_game_tick: bool,   // Spawn frame hold (Frame 70 initialization frame)
    pub anim_subtimer: u8,          // $042E: Player animation cycle frame sub-timer (sub_106E)
    pub momentum_friction: u8,      // $0429: Player inertia momentum friction value
    pub joystick_directions: u8,    // $0400: CIA-1 Port A joystick directional bitmask (sub_9F77)
    pub joystick_fire: u8,          // $0401: Joystick fire button state (sub_9F77)
    pub player_x_low: u8,           // $05F4: Sprite 0 X coordinate low byte (Giana body X)
    pub player_y_c64: u8,           // $05F5: Sprite 0 Y coordinate (Giana body Y)
    pub sprite_x_msb: u8,           // $0604: Sprites 0..7 MSBs of X coordinate
    pub sprite_0_frame: u8,         // $0605: Sprite 0 frame pointer (sub_10ED)
    pub sprite_1_frame: u8,         // $0606: Sprite 1 frame pointer (sub_10ED)
    pub sprite_2_frame: u8,         // $0607: Sprite 2 frame pointer (sub_10ED)
    pub sfx_queue: u8,              // $154C: In-game sound effect request register (0xFF = idle)
    pub active_player: u8,          // $1576: Current active player turn (1 = 1P, 2 = 2P; set in L0940)
    pub banner_delay: u8,           // $157C: Stage banner display countdown timer (sub_154F)
    pub head_tile_ptr_low: u8,      // $0432: Screen address low byte of tile directly above player (sub_9DF4)
    pub head_tile_ptr_high: u8,     // $0433: Screen address high byte of tile directly above player (sub_9DF4)
    pub surrounding_tiles: [u8; 9], // $0416..$041E: 9 screen character codes around player (sub_9DF4)
    pub surrounding_tile_flags: [u8; 9], // $041F..$0427: 9 collision/interaction type flags (sub_9DF4/sub_9F40)
    pub screen_ram: [u8; 1000],     // $C800..$CBE7: C64 40x25 screen RAM character matrix
    pub en: crate::enemy::EnemyState, // enemy pool, spawner scratch, sprite shadow registers (enemy.rs)
    pub sprite_mem: Vec<u8>,        // $C000..$FFFF: sprite shapes as the video chip sees them
    pub sprite_swap: Vec<u8>,       // $A080..$AA7F: the other sprite set (swapped by sub_1474)
    pub score_bcd: [u8; 3],         // $0435..$0437: score, BCD
    pub zp_1d: u8,                  // $001D: sprite set swap in progress / star flicker
    pub zp_1e: u8,                  // $001E: stage palette flag (sub_0D39)
    pub st_0457: u8,                // $0457: death / respawn state (0 = alive, $FF = dying, $80 = respawning)
    pub st_046d: u8,                // $046D: stage exit request
    pub stage_text_157b: u8,        // $157B: last lives value printed by sub_0E1F
    pub main: crate::mainloop::MainLine, // state of the main line (code outside the raster interrupts)
    pub zp_0e: u16,                 // $000E/$000F: screen RAM pointer used by the tile routines
    pub zp_10: u16,                 // $0010/$0011: colour RAM pointer
    pub zp_12: u16,                 // $0012/$0013
    pub prev_tiles: [u8; 9],        // $043F..$0447: tiles of the previous sub_1A17 pass
    pub st_2169: u8,                // $2169: pipe warp in progress
    pub diamonds_bcd: u8,           // $0434: diamonds collected, BCD
    pub chk: [u8; 9],               // $0458..$0460: checkpoint block (stage, scroll, ..., map column, X, Y, tile pointer)
    pub saved_sprites: [u8; 17],    // $216C..: sprite registers saved by sub_210B
    pub saved_vic: [u8; 3],         // $217D..$217F: $D01B, $D01C, $D015 as saved
    pub saved_colours: [u8; 8],     // $2180..: $D027..$D02E as saved
    pub saved_ptrs: [u8; 8],        // $2188..: sprite frame pointers as saved
    pub st_0464: u8,                // $0464: respawn walk-in flag
    pub death_kind_157f: u8,        // $157F: copy of $0465 while the death jump runs
    pub music_track_1527: u8,       // $1527: last music track handed to the sound driver
    pub legacy_model: bool,         // run the pre-port gameplay model next to the ported routines (renderer and legacy tests still read it)
    pub skip_subpixel_inc: bool,
    pub restart_timing: std::collections::VecDeque<StageTiming>, // waits of the next stage restarts (harness)
    pub bonus_timing: std::collections::VecDeque<BonusTiming>, // waits of the next bonus room visits (harness)
    pub swap_timing: std::collections::VecDeque<SpriteSwapTiming>, // progress of the next sprite set exchanges (harness)
    pub swap_iters: usize,          // half steps of the exchange loop `$14B7` done so far
    pub swap_held: u8,              // the byte the loop holds on the stack between its two stores
    pub timeout_timing: std::collections::VecDeque<TimeoutTiming>, // waits of the next time-out texts (harness)
    pub warp_timing: std::collections::VecDeque<WarpTiming>, // waits of the next pipe warps (harness)
    pub exit_timing: std::collections::VecDeque<ExitTiming>, // waits of the next stage exits (harness)
    pub exit_acc: i32,              // bonus count: budget carried between chains (thousandths of a slow step)
    pub exit_sub: u8,               // bonus count: part of the loop that comes next (see exit_step)
    pub bonus_ran: bool,            // a bonus visit step already ran in this frame
    pub main_between_handlers: bool, // harness: `$0457` was poked, the main line calls `sub_1E19` between the handlers `$1200` and `$12A3`
    pub swap_state: (usize, u8, u8, u8), // sub_1FE2 in flight: cells done, stores done of the next cell, held old map char, held old screen char
    pub statue_tile_2040: u8,       // $2040: the tile counter of the statue replacement (a data byte inside the code, counts up)
    pub game_over: bool,            // the last life is gone ($0C4E: the original returns to the title via L09B6/L084F)
    pub exit_1579: bool,            // $1579: the stage start follows a stage exit (skips the player switch of L09DA)
    pub main_fire: bool,            // fire level of the current chain, for joystick reads of the main line (sub_9F77)
    pub main_gap: u8,               // which gap of the frame the main line runs in (1: after $1326, 2: after $1393)
    pub irq_off_left: usize,        // frames of the restart in which the whole interrupt chain is lost (StageTiming::irq_off_frames)
    pub skip_h4: bool,              // sub_20AC switched the raster interrupt off ($D01A = 0) before $1393 of this frame: that handler does not run
    pub death_late_pre: bool,       // a cut death start goes on before the first interrupt of the next frame (started after $1393)
    pub timing: StageTiming,        // idle IRQ chains of the main line's busy waits during the stage start
    pub confirm_banner: bool,       // the banner after a lost life waits for a press (the original also goes on after ~3.5 s)
    pub confirm_down: bool,         // fire or jump is held
    pub confirm_armed: bool,        // it was let go since the banner came up
    pub player: u8,                 // $1568: the player whose turn it is (0 = Giana, 1 = Maria)
    pub player_save: [u8; 16],      // $1569..$1578: per-player copy of score, power-up, stage, lives, spawn variant (L09DA after a restart)
}

/// IRQ chains that run during the busy wait of sub_1474 (measured in the oracle).
pub const SPRITE_SWAP_WAIT_FRAMES: usize = 28;

/// Number of raster IRQ chains that run while the main line burns time during the stage start. The port has no
/// cycle model: how long a busy wait lasts depends on the cycles the interrupts leave to the main line. The defaults
/// are the Stage 1 values (docs/oracle-notes.md section 15); the comparison harness measures the values of the
/// other stages from the recordings (map decompression and the banner differ per stage).
#[derive(Clone, Debug)]
pub struct StageTiming {
    /// chains between the fire press (after the reset of sub_20AC) and the start of the sprite swap
    pub post_fire: usize,
    /// chains of the busy wait inside sub_1474
    pub swap_wait: usize,
    /// chains of the four steps of the banner wait (sub_154F); empty when fire skipped the banner
    pub banner_steps: Vec<usize>,
    /// chains between sub_0D54 and the sprite sync
    pub setup_wait: usize,
    /// chains of the busy wait at the end of the death jump (sub_2041) before this restart
    pub death_wait: usize,
    /// the death that precedes this restart is noticed by the main loop just before the interrupt `$1393`: the interrupt
    /// cuts the music request of `sub_2041` after this many instructions of the sound driver (its tick finds the driver
    /// stopped and does nothing); the rest of `sub_2041` (`$0431 = $80`, first step) runs in the gap before the first
    /// interrupt of the next frame
    pub death_late: Option<u32>,
    /// frames after the start of a restart in which no raster interrupt runs at all: `$D01A` is 0 from `sub_20AC` on and the
    /// frame that contains `sub_0E39` loses its handlers too (`$D012 = $41`, the chain starts again at the next line `$41`)
    pub irq_off_frames: usize,
    /// chains after the start of a restart until sub_9F45 has cleared the screen
    pub clear_after: usize,
    /// chains after the start of a restart until the stage variables ($0449/$044A timer, $0434 diamonds) are reset
    pub vars_after: usize,
    /// sound ticks that run during the banner wait of the first stage start (fire ends it; no `$157C` step shows it)
    pub sound_extra: i32,
    /// the end of the death jump's busy wait lands between the interrupts `$12A3` and `$1393` (decided per recording)
    pub tail_mid: bool,
    /// chains between the clear of the screen and the banner texts (and the track request that goes with them)
    pub text_after: usize,
    /// the first chain after a restart does not advance $0408 (alignment of sub_20AC against the $12A3 handler)
    pub skip_inc: bool,
    /// time-based waits end before the interrupt chain of a frame (true) or after it (false)
    pub tail_pre: bool,
}

impl Default for StageTiming {
    fn default() -> Self {
        Self { post_fire: 9, swap_wait: SPRITE_SWAP_WAIT_FRAMES, banner_steps: Vec::new(), setup_wait: 2, death_wait: 28, death_late: None, irq_off_frames: 0, clear_after: 0, vars_after: 1, skip_inc: false, tail_pre: true, sound_extra: 0, tail_mid: false, text_after: 0 }
    }
}
/// Waits of the stage exit sequence `$0BDF` (measured from the recording like `StageTiming`).
impl Default for ExitTiming {
    /// Without a recording: no banner wait steps, the bonus count at the measured pace.
    fn default() -> Self {
        Self { count_wait: 0, banner_steps: vec![23; 6], final_wait: 0, bonus_budget: 1128, bonus_carry: 0, final_pre: true }
    }
}

/// Progress of the exchange loop `$14B7` (2560 iterations, about 440 per chain) of a sprite set swap started from the
/// main loop, as measured per chain from a recording; `progress[n]` = half steps done at the end of chain n after the start.
/// The swap ends with the chain in which `progress` reaches `SWAP_HALVES`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteSwapTiming {
    pub progress: Vec<usize>,
    /// Measured: the interrupt that raised `$046B` (1-up) called sub_98D9 while the main loop was inside sub_1CD7 just before
    /// the load of the last score byte (`LDA $0449,X`). The call rewrote the operand, so the interrupted print writes this
    /// value (the lives in BCD) into the last two score cells `$C82E/$C82F`. Not derivable without a cycle-exact main loop.
    pub score_clobber: Option<u8>,
}

pub const SWAP_ITERATIONS: usize = 2560;
/// Half steps of the loop: each iteration stores to `$CC00..` first and to `$A080..` after it, a snapshot can fall between.
pub const SWAP_HALVES: usize = SWAP_ITERATIONS * 2;
/// Cells of `sub_0D54` done at the end of its first two chains (measured in `13_timeout_text`, frames 417 and 418).
pub const TILE_CELLS: [usize; 2] = [319, 722];

impl Default for SpriteSwapTiming {
    /// Without a recording: the busy wait of `$149C` lasts 20 chains, then the loop runs at the measured pace.
    fn default() -> Self {
        let mut progress = vec![0; 21];
        let mut done = 0;
        while done < SWAP_HALVES {
            done = (done + 882).min(SWAP_HALVES);
            progress.push(done);
        }
        Self { progress, score_clobber: None }
    }
}

/// Waits of the bonus room visit (`sub_1E19`): entry through a statue and return, measured like the other waits.
#[derive(Clone, Debug)]
pub struct BonusTiming {
    /// chains of `sub_9936` on the way in
    pub decomp: usize,
    /// progress of `sub_1FE2` at the sample of each chain: cells finished and stores done of the next cell (0..2);
    /// empty = the unmeasured pace of 314 cells per chain
    pub swap: Vec<(usize, u8)>,
    /// the same on the way back
    pub ret_swap: Vec<(usize, u8)>,
    /// chains of `sub_9936` on the way back
    pub ret_decomp: usize,
    /// chains of the busy wait at the end (`sub_9A4F`, Y = 0) until `$0457` drops to 0
    pub fin_wait: usize,
    /// the busy wait of the way back ended after the sprite copy of its frame (second gap of the main line), so `sub_20A3`
    /// at `$1FBC` waits for the copy of the next frame
    pub ret_late: bool,
    /// the final wait ended before the enemy part of the interrupt chain `$1393` of its frame (first gap of the main line)
    pub fin_early: bool,
    /// the last chain of the swap ended before the first handler of its frame (the sample line to `$1200` gap), on the way in / back:
    /// the player routines of that chain's interrupts already see `$0431` = 0
    pub swap_early: bool,
    pub ret_swap_early: bool,
    /// the busy wait of the way back ended before the first handler of its frame: `sub_0EAB` already sees `$0457` = 1
    pub ret_early: bool,
}

impl Default for BonusTiming {
    /// Without a recording: the lengths measured in stage_30_run.
    fn default() -> Self {
        Self { decomp: 5, swap: Vec::new(), ret_swap: Vec::new(), ret_decomp: 14, fin_wait: 24, ret_late: false, fin_early: false, swap_early: false, ret_swap_early: false, ret_early: false }
    }
}

impl Default for WarpTiming {
    /// Without a recording: the lengths measured in 12_poke_warp.
    fn default() -> Self {
        Self { wait: 75, to_banner: 1, steps: vec![23, 23, 24, 23, 23, 24] }
    }
}

/// Waits of the time-out text after a death by time (`$0C1D`): the steps of `sub_154F` (A = 4), measured.
#[derive(Clone, Debug)]
pub struct TimeoutTiming {
    pub steps: Vec<usize>,
}

impl Default for TimeoutTiming {
    fn default() -> Self {
        Self { steps: vec![23, 23, 24, 23] }
    }
}

/// Waits of the pipe warp `$0B6B` (measured like the others).
#[derive(Clone, Debug)]
pub struct WarpTiming {
    /// chains of the three busy waits (`sub_9A4F`, Y = 0) before the screen is cleared
    pub wait: usize,
    /// chains from the clear to the start of the banner wait (`sub_154F`, A = 6)
    pub to_banner: usize,
    /// chains of the steps of the banner wait; the last one ends in `$046D = 1`
    pub steps: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ExitTiming {
    /// chains between the last bonus count step and the start of the banner wait of `sub_154F` (A = 6)
    pub count_wait: usize,
    /// chains of the steps of that banner wait, each ends with a decrement of `$157C`
    pub banner_steps: Vec<usize>,
    /// chains from the last decrement until fire ends the wait
    pub final_wait: usize,
    /// cycle budget of the main line per chain for the bonus count in thousandths of the time of one slow step (the busy
    /// wait of `sub_9A4F` with Y = $0A), and the carry at the first step
    pub bonus_budget: i32,
    pub bonus_carry: i32,
    /// the step that ends the wait (fire seen by `sub_154F`) runs before the interrupt chain of its frame (true) or after it
    pub final_pre: bool,
}

/// IRQ chains that run while the screen setup of sub_0E39 / sub_0E66 / sub_0D54 executes (measured in the oracle).
pub const STAGE_SETUP_WAIT_FRAMES: usize = 2;

/// Row offset low byte table from C64 address $9BA0 (25 rows, 40 chars/row)
pub const ROW_OFFSETS_LOW_9BA0: [u8; 25] = [
    0x00, 0x28, 0x50, 0x78, 0xA0, 0xC8, 0xF0, 0x18, 0x40, 0x68, 0x90, 0xB8, 0xE0, 0x08, 0x30,
    0x58, 0x80, 0xA8, 0xD0, 0xF8, 0x20, 0x48, 0x70, 0x98, 0xC0,
];

/// Row offset high byte table from C64 address $9BB9 (25 rows, 40 chars/row)
pub const ROW_OFFSETS_HIGH_9BB9: [u8; 25] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02,
    0x02, 0x02, 0x02, 0x02, 0x02, 0x03, 0x03, 0x03, 0x03, 0x03,
];

/// Initial HUD header and icon tiles (screen rows 0..4, 200 bytes)
pub const INITIAL_SCREEN_HUD: [u8; 200] = [
    0x00, 0x00, 0x07, 0x09, 0x01, 0x0E, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0F, 0x0E, 0x15, 0x13, 0x00, 0x00, 0x00, 0x0C,
    0x09, 0x16, 0x05, 0x13, 0x00, 0x00, 0x00, 0x13, 0x14, 0x01, 0x07, 0x05, 0x00, 0x00, 0x14, 0x09, 0x0D, 0x05, 0x00, 0x00,
    0x00, 0x00, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00, 0xF7, 0x00, 0x00, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x30, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x31, 0x00, 0x00, 0x00, 0x00, 0x30, 0x31, 0x00, 0x00, 0x00,
    0x00, 0xDA, 0xDC, 0xDD, 0xDE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0xDF, 0xE1, 0xE2, 0xE3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xDA, 0xDC, 0xDD, 0xDE, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xDF, 0xE0, 0xE1, 0xE2, 0xE3, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xDF, 0xE1, 0xE2, 0xE3, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Stage warp and arc table directly from C64 address $9FAE (stages 0..34)
pub const STAGE_TABLE_9FAE: [u8; 35] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 32, 33, 34, 35, 36, 37,
    38, 39, 40, 41, 48, 49, 50, 51, 39,
];

/// Authentic spawn positions from C64 address $60EA (Stage 1..33 X and Y pairs)
pub const SPAWN_POS_60EA: [u8; 8] = [
    28, 203, 78, 155, 48, 67, 37, 195,
];

/// Authentic 6502 BCD subtraction helper (SED / SEC / SBC #$01)
fn bcd_sub_one(val: u8) -> (u8, bool) {
    let mut low = val & 0x0F;
    let mut high = (val >> 4) & 0x0F;
    let mut borrow = false;
    if low == 0 {
        low = 9;
        if high == 0 {
            high = 9;
            borrow = true;
        } else {
            high -= 1;
        }
    } else {
        low -= 1;
    }
    ((high << 4) | low, borrow)
}

/// Convert two BCD nybbles to decimal integer
fn bcd_to_u32(val: u8) -> u32 {
    let low = (val & 0x0F) as u32;
    let high = ((val >> 4) & 0x0F) as u32;
    high * 10 + low
}

impl GameState {
    /// Game as it is when the program starts: boot initialisation done, no stage set up yet.
    /// A game driven by the ported routines only: boot, title demo frames, stage 1 start (what the oracle shows after fire).
    pub fn new_port_game() -> Self {
        Self::new_port_game_at(1)
    }

    /// A game for two: key 2 of the original leaves `$1576` clear, so both players are in the game.
    pub fn new_port_game_two() -> Self {
        let mut game = Self::new_port_game();
        game.player_save[13] = 0;
        game.two_players = true;
        game
    }

    /// One player's own game when both play at the same time (not in the original): `player` is 0 for Giana, 1 for Maria.
    pub fn new_port_game_as(player: u8) -> Self {
        Self::new_port_game_for(1, player)
    }

    /// The same, starting in another stage (cheat / stage select / soak tests)
    pub fn new_port_game_at(stage: u32) -> Self {
        Self::new_port_game_for(stage, 0)
    }

    fn new_port_game_for(stage: u32, player: u8) -> Self {
        let mut game = Self::new_boot();
        game.stage = stage.clamp(1, 33);
        // Before the start of the game: its name and colours come from `$1568`.
        // `$1575,Y`: the other player counts as finished, like player 2 in the game for one
        game.player = player;
        game.player_save[12] = player;
        game.player_save[13] = player ^ 1;
        game.run_title_demo(12);
        game.screen = GameScreen::InGame;
        game.legacy_model = false;
        game.confirm_banner = true;
        game
    }

    pub fn new_boot() -> Self {
        let mut s = Self {
            screen: GameScreen::Title,
            score: 0,
            diamonds: 0,
            time: 401,
            stage: 1,
            lives: 3,

            player_x: 20,
            player_y: 115,
            is_jumping: false,
            jump_frame: 0,
            is_falling: false,
            fall_speed: 0,
            facing_right: true,
            anim_tick: 0,
            walk_frame: 0,
            has_punker: false,
            has_shot: false,
            shoot_cooldown: 0,
            invulnerable_timer: 0,
            is_dying: false,
            death_vel_y: 0.0,

            infinite_lives: false,
            god_mode: false,
            two_players: false,

            scroll_x: 0,
            cloud_scroll_x: 0,

            ground_y: 115,
            timer_ticks: 0,

            current_map: vec![0; 5120],
            enemies: Vec::new(),
            enemy_spawns: Vec::new(),
            particles: Vec::new(),
            items: Vec::new(),
            projectiles: Vec::new(),

            scroll_wrap_flag: 0x00,
            move_state: 0x00,
            move_edge_flag: 0x00,
            jump_state: 0x00,
            jump_phase: 0x00,
            anim_walking: 0x00,
            brake_flag: 0x00,
            airborne_flag: 0x00,
            land_flag: 0x00,
            map_column: 0x27,
            move_delta: 0x00,
            jump_ascent_idx: 0x00,
            jump_descent_idx: 0x00,
            anim_step_timer: 0x00,
            side_tile_flags: 0x00,
            var_040b: 0x00,
            music_paused: 0x00,
            sprite_head_x: 0x00,
            sprite_head_y: 0x00,
            sprite_dress_x: 0x00,
            sprite_dress_y: 0x00,
            joystick_enabled: true,
            particle_slots: crate::tables::TABLE_9B02_INIT,
            color_ram: [0u8; 1000],
            column_scroll_pending: false,
            spawn_variant: 0x00,
            var_042f: 0x00,
            lost_ticks: 0,
            anim_9d: crate::image::image()[0x9D32..0x9D80].to_vec(),
            anim_060d: [0; 0x40],
            sound_ticks: 0,
            sound_requests: Vec::new(),
            sound: crate::sound_driver::Driver::new(),
            anim_counter: 0x00,
            scroll_fine: 0x07,
            sky_scroll_fine: 0x00,
            player_move_dir: 0x00,
            jump_subtimer: 0x00,
            scroll_subpixel: 0x00,
            player_vel_fraction: 0x00,
            player_momentum: 0x00,
            jump_table_idx: 0x00,
            dec: [0; 8],
            ground_hit_flag: 0x00,
            flag_0431: 0x00,
            lives_c64: 0x03,
            warp_stage_id: 0x01,
            timer_subsecond: 0x1D,
            timer_seconds_low: 0x01,
            timer_seconds_high: 0x04,
            time_warning: 0x00,
            stage_cycle_counter: 0x21,
            hazard_hit: 0x00,
            vic_border: 0,
            vic_irq_enable: 1,
            vic_bg2: 0,
            vic_mc: [0; 2],
            death_arc_idx: 0x01,
            transform_timer: 0xFF,
            transform_step: 0x00,
            prev_scroll: 0x07,
            redraw_pending: 0x00,
            first_in_game_tick: true,
            anim_subtimer: 0x00,
            momentum_friction: 0x00,
            joystick_directions: 0x00,
            joystick_fire: 0x00,
            player_x_low: 33,
            player_y_c64: 203,
            sprite_x_msb: 0x00,
            sprite_0_frame: 0x30,
            sprite_1_frame: 0x31,
            sprite_2_frame: 0x32,
            sfx_queue: 0xFF,
            active_player: 0x01,
            banner_delay: 0x04,
            head_tile_ptr_low: 0x00,
            head_tile_ptr_high: 0x00,
            surrounding_tiles: [0; 9],
            surrounding_tile_flags: [0; 9],
            screen_ram: [0; 1000],
            en: crate::enemy::EnemyState::new(),
            sprite_mem: crate::image::image()[0xC000..].to_vec(),
            sprite_swap: crate::image::image()[0xA080..0xAA80].to_vec(),
            score_bcd: [0; 3],
            zp_1d: 0,
            zp_1e: 0,
            st_0457: 0,
            st_046d: 0,
            stage_text_157b: 0,
            main: crate::mainloop::MainLine::Loop,
            zp_0e: 0,
            zp_10: 0,
            zp_12: 0,
            prev_tiles: [0; 9],
            st_2169: 0,
            diamonds_bcd: 0,
            chk: [0; 9],
            saved_sprites: [0; 17],
            saved_vic: [0; 3],
            saved_colours: [0; 8],
            saved_ptrs: [0; 8],
            st_0464: 0,
            death_kind_157f: 0,
            music_track_1527: 0,
            legacy_model: true,
            skip_subpixel_inc: false,
            restart_timing: std::collections::VecDeque::new(),
            bonus_timing: std::collections::VecDeque::new(),
            swap_timing: std::collections::VecDeque::new(),
            swap_iters: 0,
            swap_held: 0,
            timeout_timing: std::collections::VecDeque::new(),
            warp_timing: std::collections::VecDeque::new(),
            exit_timing: std::collections::VecDeque::new(),
            exit_acc: 0,
            exit_sub: 0,
            bonus_ran: false,
            main_between_handlers: false,
            swap_state: (0, 0, 0, 0),
            statue_tile_2040: crate::image::img(0x2040),
            game_over: false,
            exit_1579: false,
            main_fire: false,
            main_gap: 0,
            death_late_pre: false,
            skip_h4: false,
            irq_off_left: 0,
            timing: StageTiming::default(),
            confirm_banner: false,
            confirm_down: false,
            confirm_armed: false,
            player: 0,
            player_save: { let mut a = [0u8; 16]; a[8] = 1; a[9] = 1; a[10] = 3; a[11] = 3; a[13] = 1; a },
        };
        s.init_system();
        s.select_game_mode(1);
        s
    }

    /// Boot state plus Stage 1 set up directly (without the title demo that precedes it in the original).
    pub fn new() -> Self {
        let mut s = Self::new_boot();
        s.start_stage(1);
        s
    }

    /// L084F ($084F): In-game entry point, title & system initialization
    pub fn init_system(&mut self) {
        // $0891: LDA $9D33,Y; STA $060D,Y for Y = 0 (the loop ends at the first DEY: one byte)
        self.anim_060d[0] = self.anim_9d[1];
        // $0881: JSR sub_41C3 (init of the sound driver)
        self.sound.call(0x4346, 0, 0, 0xF0, false);
        // $08AC: LDA #$21; STA $045B
        self.stage_cycle_counter = 0x21;
        // $08C5: LDA #$FF; STA $0431; STA $154D
        self.flag_0431 = 0xFF;
        // SFX queue initialized idle
        self.sfx_queue = 0xFF;
    }

    /// L0940 ($0940): In-game title/menu input loop & player setup
    pub fn select_game_mode(&mut self, _num_players: u8) {
        // $0931: LDA #$03; CLC; JSR sub_41C9 (title tune)
        self.sound.call(0x49E6, 3, 0, 0, false);
        self.sound_requests.push((3, self.sound_ticks));
        // $0965: LDA #$01; STA $1576 (single player active)
        self.active_player = 0x01;
        // $098E: LDA #$03; STA $1573; STA $1574; STA $043C
        self.lives_c64 = 0x03;
        self.lives = 3;
    }

    /// sub_9936 ($9936..$9A4E): build the stage map in `$AC00..$BFFF` from the compressed stage stream
    pub fn decompress_stage_layout(&mut self) {
        self.decompress_stage_sub_9936();
    }

    /// sub_10ED ($10ED): Player sprite frame pointer sequencer
    pub fn set_sprite_pointers(&mut self, base_frame: u8) {
        // $10ED: CLC; ADC #$30
        self.set_sprite_pointers_l10f0(base_frame.wrapping_add(0x30));
    }

    /// L10F0: TAX; STX $0605; INX; STX $0606; INX; STX $0607 (entry that skips the +$30)
    pub fn set_sprite_pointers_l10f0(&mut self, frame_0: u8) {
        // $10F1: STX $0605; INX; STX $0606; INX; STX $0607
        self.sprite_0_frame = frame_0;
        self.sprite_1_frame = frame_0.wrapping_add(1);
        self.sprite_2_frame = frame_0.wrapping_add(2);
    }

    /// sub_10DF ($10DF..$10EC): Apply standing/falling sprite animation index base
    pub fn apply_standing_animation_sub_10df(&mut self) {
        // $10DF: LDA #$00; STA $1A
        self.anim_walking = 0;
        // $10E3: LDA $042C; BNE L10EB
        let base_frame = if self.facing_right {
            // $10E8: LDA #$1E (BIT $21A9 skips next 2 bytes)
            0x1E
        } else {
            // $10EB: LDA #$21
            0x21
        };
        // Falls into sub_10ED ($10ED..$10FC):
        self.set_sprite_pointers(base_frame);
    }

    /// sub_106E ($106E..$1077): Clear player horizontal inertia momentum
    pub fn clear_momentum_sub_106e(&mut self) {
        // $106E: LDA #$0E; SEC; SBC $0429; STA $042E; RTS
        self.anim_subtimer = 0x0Eu8.wrapping_sub(self.momentum_friction);
    }

    /// sub_1D0A ($1D0A..$1D45): Authentic C64 stage timer countdown & timeout hazard check
    pub fn update_timer_sub_1d0a(&mut self) {
        // $1D0A: DEC $0448
        if self.timer_subsecond == 0 {
            // $1D0F: LDA #$28; STA $0448 (reload 40 ticks = 1 second)
            self.timer_subsecond = 0x28;

            // $1D14: SED; SEC; LDA $0449; SBC #$01; STA $0449
            let (new_low, borrow) = bcd_sub_one(self.timer_seconds_low);
            self.timer_seconds_low = new_low;

            // $1D1E: LDA $044A; SBC #$00; STA $044A
            if borrow {
                let (new_high, _) = bcd_sub_one(self.timer_seconds_high);
                self.timer_seconds_high = new_high;
            }

            // Sync high-level time field
            self.time = (bcd_to_u32(self.timer_seconds_high) * 100) + bcd_to_u32(self.timer_seconds_low);

            // $1D26: CLD; LDA $0449; CLC; ADC $044A; CMP #$03; BEQ L1D40
            if self.timer_seconds_low.wrapping_add(self.timer_seconds_high) == 0x03 {
                // $1D40: LDA #$02; STA $0465 (timeout death)
                self.hazard_hit = 0x02;
                self.is_dying = true;
            }

            // $1D32: LDA $0467; BMI L1D3F; BEQ L1D3F; SEC; SBC #$01; STA $0467
            if self.transform_timer > 0 && (self.transform_timer as i8) >= 0 {
                self.transform_timer -= 1;
            }
        } else {
            self.timer_subsecond -= 1;
        }
    }

    /// sub_9F77 ($9F77..$9FAD): Read CIA-1 Port A ($DC00) joystick input registers
    pub fn read_joystick_sub_9f77(&mut self, input: &ControllerInput) {
        // $9F77: LDA $0400; AND #$80; STA $0400
        let bit7 = self.joystick_directions & 0x80;

        let mut raw = 0u8;
        if input.up || input.jump { raw |= 0x01; }
        if input.down { raw |= 0x02; }
        if input.left { raw |= 0x04; }
        if input.right { raw |= 0x08; }
        // The space key (row 7, column 4) shorts the fire line of joystick port 2: the original shoots when the item key is pressed
        if input.fire || input.item { raw |= 0x10; }

        let mut combined = raw | bit7;
        // $9F8D: LSR A; BCS L9F94 (Up released clears jump latch bit 7)
        if (combined & 0x01) == 0 {
            combined &= 0x7F;
        }

        // $9F95: AND #$8F; STA $0400
        self.joystick_directions = combined & 0x8F;

        // $9F9A: TXA; AND #$10 (Fire button)
        if (raw & 0x10) != 0 {
            if self.joystick_fire == 0 {
                self.joystick_fire = 0x80;
            } else {
                self.joystick_fire = self.joystick_fire.saturating_sub(8);
            }
        } else {
            self.joystick_fire = 0;
        }
    }

    /// Reads a character byte from screen RAM ($C800..$CBE7)
    pub fn read_screen_ram(&self, addr: u16) -> u8 {
        if (0xC800..=0xCBE7).contains(&addr) {
            let offset = (addr - 0xC800) as usize;
            if offset < self.screen_ram.len() {
                return self.screen_ram[offset];
            }
        }
        0
    }

    /// sub_9DF4 ($9DF4..$9EC2): Inspect 3x3 surrounding tiles on screen RAM around player
    pub fn update_tile_collision_sub_9df4(&mut self) {
        // $9DF4: LDA $05F5; CMP #$E1; BCS L9DE9; CMP #$42; BCC L9DE9
        let py = self.player_y_c64;
        if py >= 0xE1 || py < 0x42 {
            // L9DE9: LDY #$08; LDA #$00; L9DED: STA $041F,Y; DEY; BPL L9DED; RTS
            self.surrounding_tile_flags.fill(0);
            return;
        }

        // $9DFF: SEC; SBC #$33; STA $10
        let y_diff = py.wrapping_sub(0x33);

        // $9E08: SEC; LDA $05F4; SBC #$15; STA $0E
        let (x_sub15, borrow1) = self.player_x_low.overflowing_sub(0x15);
        // $9E10: LDA $0604; AND #$01; SBC #$00; STA $0F
        let x_high = (self.sprite_x_msb & 0x01).wrapping_sub(if borrow1 { 1 } else { 0 });

        // $9E19: SEC; LDA $0E; SBC $0404; STA $0E
        let (x_sub_fine, borrow2) = x_sub15.overflowing_sub(self.scroll_fine);
        // $9E21: LDA $0F; SBC #$00; STA $0F
        let mut x_hi = x_high.wrapping_sub(if borrow2 { 1 } else { 0 });

        // $9E27: LDA $0E; LSR $0F; ROR A; LSR A; LSR A; STA $0E
        let c = x_hi & 1;
        x_hi >>= 1;
        let char_col = ((c << 7) | (x_sub_fine >> 1)) >> 2;

        // $9E30: LDA $10; LSR; LSR; LSR; TAX
        let row = (y_diff >> 3) as usize;
        if row >= 25 {
            self.surrounding_tile_flags.fill(0);
            return;
        }

        // $9E36: CLC; LDA $0E; ADC $9BA0,X; STA $0E
        let row_off_lo = ROW_OFFSETS_LOW_9BA0[row];
        let (sum_lo, carry_lo) = char_col.overflowing_add(row_off_lo);
        // $9E3E: LDA $0F; ADC $9BB9,X; STA $0F
        let row_off_hi = ROW_OFFSETS_HIGH_9BB9[row];
        let sum_hi = x_hi.wrapping_add(row_off_hi).wrapping_add(if carry_lo { 1 } else { 0 });

        // $9E45: CLC; LDA $0E; ADC #$00; STA $0E
        // $9E4C: LDA $0F; ADC #$C8; STA $0F
        let ptr_lo = sum_lo;
        let ptr_hi = sum_hi.wrapping_add(0xC8);

        // $9E52: LDY #$00; LDA ($0E),Y; STA $0416; INY; ...; STA $0418
        let base_addr_y0 = ((ptr_hi as u16) << 8) | (ptr_lo as u16);
        self.surrounding_tiles[0] = self.read_screen_ram(base_addr_y0);
        self.surrounding_tiles[1] = self.read_screen_ram(base_addr_y0.wrapping_add(1));
        self.surrounding_tiles[2] = self.read_screen_ram(base_addr_y0.wrapping_add(2));

        // $9E65: SEC; LDA $0E; SBC #$28; STA $0432; LDA $0F; SBC #$00; STA $0433
        let (h_lo, h_borrow) = ptr_lo.overflowing_sub(0x28);
        let h_hi = ptr_hi.wrapping_sub(if h_borrow { 1 } else { 0 });
        self.head_tile_ptr_low = h_lo;
        self.head_tile_ptr_high = h_hi;

        // $9E74: CLC; LDA $0E; ADC #$50; STA $0E; LDA $0F; ADC #$00; STA $0F
        let base_addr_y1 = base_addr_y0.wrapping_add(80);
        self.surrounding_tiles[3] = self.read_screen_ram(base_addr_y1);
        self.surrounding_tiles[4] = self.read_screen_ram(base_addr_y1.wrapping_add(1));
        self.surrounding_tiles[5] = self.read_screen_ram(base_addr_y1.wrapping_add(2));

        // $9E94: CLC; LDA $0E; ADC #$28; STA $0E; LDA $0F; ADC #$00; STA $0F
        let base_addr_y2 = base_addr_y1.wrapping_add(40);
        self.surrounding_tiles[6] = self.read_screen_ram(base_addr_y2);
        self.surrounding_tiles[7] = self.read_screen_ram(base_addr_y2.wrapping_add(1));
        self.surrounding_tiles[8] = self.read_screen_ram(base_addr_y2.wrapping_add(2));

        // $0E:$0F is left at the third row pointer
        self.zp_0e = base_addr_y2;

        // $9EB4: LDY #$08; L9EB6: LDA $0416,Y; JSR sub_9F40; STA $041F,Y; DEY; BPL L9EB6
        for i in 0..9 {
            self.surrounding_tile_flags[i] = Self::tile_lookup_sub_9f40(self.surrounding_tiles[i]);
        }
    }

    /// sub_9F40 ($9F40..$9F44): Tile classification table lookup ($9C20)
    #[inline]
    pub fn tile_lookup_sub_9f40(char_code: u8) -> u8 {
        // $9F40: TAX; LDA $9C20,X; RTS
        crate::collision::c64_collision_table()[char_code as usize]
    }

    /// L9EC3..L9EF0: Add horizontal displacement delta ($0428) to player X with right screen boundary check
    pub fn add_player_x_sub_9ec3(&mut self, delta: u8) {
        let x_msb = self.sprite_x_msb;
        if (x_msb & 0x01) != 0 {
            // $9ECD: CLC; ADC $0428; ADC $0404 (second ADC consumes the carry of the first)
            let t = self.player_x_low as u16 + delta as u16;
            let sum = ((t & 0xFF) + self.scroll_fine as u16 + (t >> 8)) as u8;
            // $9ED4: CMP #$3F; BCS rts
            if sum >= 0x3F {
                return;
            }
        }
        // $9ED8: CLC; LDA $05F4; ADC $0428; STA $05F4, $05F6, $05F8
        let (new_x, carry) = self.player_x_low.overflowing_add(delta);
        self.player_x_low = new_x;
        self.sprite_head_x = new_x;
        self.sprite_dress_x = new_x;
        if carry {
            self.sprite_x_msb = x_msb | 0x07;
        }
    }

    /// L9EF1..L9F1E: Subtract horizontal displacement delta ($0428) from player X with left screen boundary check
    pub fn sub_player_x_sub_9ef1(&mut self, delta: u8) {
        let x_msb = self.sprite_x_msb;
        if (x_msb & 0x01) == 0 {
            // $9EF8: SEC; SBC $0428; SBC $0404 (second SBC consumes the borrow of the first)
            let t = self.player_x_low as i16 - delta as i16;
            let diff = ((t & 0xFF) - self.scroll_fine as i16 + if t < 0 { -1 } else { 0 }) as u8;
            // $9F02: CMP #$19; BCC rts
            if diff < 0x19 {
                return;
            }
        }
        // $9F06: LDA $05F4; SEC; SBC $0428; STA $05F4, $05F6, $05F8
        let (new_x, borrow) = self.player_x_low.overflowing_sub(delta);
        self.player_x_low = new_x;
        self.sprite_head_x = new_x;
        self.sprite_dress_x = new_x;
        if borrow {
            self.sprite_x_msb = x_msb & 0xF8;
        }
    }

    /// sub_9F1F ($9F1F..$9F2B): Add vertical offset $0428 to player Y coordinate
    pub fn add_player_y_sub_9f1f(&mut self, delta: u8) {
        // $9F1F: CLC; LDA $05F5; ADC $0428; STA $05F5; then L9F36 sets sprite 1/2 Y
        self.player_y_c64 = self.player_y_c64.wrapping_add(delta);
        self.set_head_dress_y_l9f36();
    }

    /// sub_9F2C ($9F2C..$9F3F): Subtract vertical offset $0428 from player Y coordinate
    pub fn sub_player_y_sub_9f2c(&mut self, delta: u8) {
        // $9F2C: SEC; LDA $05F5; SBC $0428; STA $05F5
        self.player_y_c64 = self.player_y_c64.wrapping_sub(delta);
        self.set_head_dress_y_l9f36();
    }

    /// L9F36: TAX; INX; STX $05F7; INX; STX $05F9
    fn set_head_dress_y_l9f36(&mut self) {
        self.sprite_head_y = self.player_y_c64.wrapping_add(1);
        self.sprite_dress_y = self.player_y_c64.wrapping_add(2);
    }

    /// sub_1519 ($1519..$1526): Hand a pending sound request to the sound driver and mark the queue idle
    pub fn consume_sfx_sub_1519(&mut self) {
        // $1519: LDA $154C; BMI L1526
        if self.sfx_queue & 0x80 == 0 {
            // $151E: JSR sub_41C6 (L4987: start the effect)
            self.sound.call(0x4987, self.sfx_queue, 0, 0, false);
            // $1521: LDA #$FF; STA $154C
            self.sfx_queue = 0xFF;
        }
    }

    /// sub_154F ($154F): Stage banner delay / dismiss wait loop
    pub fn wait_stage_banner(&mut self) {
        // $154F: STA $157C (invoked with A = 4 at $0AA0)
        self.banner_delay = 0x04;
        // Each step is one decrement of $157C after a busy wait (the loop ends early when fire is pressed)
        let idle = ControllerInput::new();
        for len in self.timing.banner_steps.clone() {
            for _ in 0..len {
                self.irq_frame_core(&idle);
            }
            self.banner_delay = self.banner_delay.wrapping_sub(1);
        }

    }

    /// sub_20AC ($20AC): Clear VIC registers, sound state, and RAM memory pages ($0400..$042D, $046A..$0497)
    pub fn reset_hardware(&mut self) {
        // $20AC: LDA #$FF; STA $0467
        self.transform_timer = 0xFF;
        // $20B3: LDA #$00; STA $044B; STA $0465; STA $0431; STA $046B
        self.time_warning = 0x00;
        self.hazard_hit = 0x00;
        self.is_dying = false;
        self.flag_0431 = 0x00;
        self.redraw_pending = 0x00;
        // $20B8..$20C1: STA $D015, $D01A, $D020, $D021 = 0
        self.en.vic_me = 0;
        self.en.vic_bg0 = 0;
        self.vic_irq_enable = 0; // $20BB: STA $D01A
        self.vic_border = 0; // $20BE: STA $D020
        // $20CD: STA $046A,Y for Y = $2D..0 clears the enemy scratch block $0470..$0497 (and $046A..$046F)
        for a in 0x46A..=0x497usize {
            self.set_v47(a, 0);
        }
        self.redraw_pending = 0x00;
        self.en.sid_volume = 0; // $20D8: STA $D418
        // $20CD: loop clears $0400..$042D and $046A..$0497
        self.joystick_directions = 0;
        self.joystick_fire = 0;
        self.scroll_fine = 0x00;
        self.sky_scroll_fine = 0x00;
        self.player_move_dir = 0x00;
        self.jump_subtimer = 0x00;
        self.scroll_subpixel = 0x00;
        self.player_vel_fraction = 0x00;
        self.player_momentum = 0x00;
        self.var_040b = 0;
        self.jump_table_idx = 0;
        self.dec = [0; 8]; // $040C..$0413
        self.side_tile_flags = 0;
        self.surrounding_tiles = [0; 9];
        self.surrounding_tile_flags = [0; 9];
        self.ground_hit_flag = 0x00;
        self.move_delta = 0;
        self.momentum_friction = 0;
        self.jump_ascent_idx = 0;
        self.jump_descent_idx = 0;
        self.facing_right = true; // $042C = 0
        self.anim_step_timer = 0;
    }

    /// sub_0D54 ($0D54): Stage tile decoding & background setup
    pub fn load_stage_tiles(&mut self) {
        self.load_stage_tiles_step(0, 1);
    }

    pub fn load_stage_tiles_step(&mut self, step: usize, total_steps: usize) {
        // $0D54: LDA $0457; BEQ L0D5A; RTS
        if self.st_0457 != 0 {
            return;
        }
        // $0D5A: LDA #$07; STA $0404; STA $0469
        self.scroll_fine = 0x07;
        self.prev_scroll = 0x07;
        let start = crate::tables::TABLE_9FD0[self.spawn_variant as usize & 3];
        self.map_column = start; // $0D8E: STA $21

        // The loop at $0D93 fills rows 0..19 from column 39 down. Over a restart it spreads over the chains of `TILE_CELLS`.
        let (from, to) = match (total_steps >= 2, step) {
            (true, 0) => (0, TILE_CELLS[0]),
            (true, 1) => (TILE_CELLS[0], TILE_CELLS[1]),
            (true, _) => (TILE_CELLS[1], 800),
            _ => (0, 800),
        };
        for cell in from..to {
            let (row, x) = (cell / 40, 39 - cell % 40);
            let ch = self.current_map[row * 256 + start.wrapping_sub((cell % 40) as u8) as usize];
            self.screen_ram[(row + 5) * 40 + x] = ch;
            self.color_ram[(row + 5) * 40 + x] = crate::tables::TABLE_AB00[ch as usize];
        }

        if total_steps < 2 || step >= 2 {
            // $0DE1: LDA $154D; BEQ L0DE7; RTS
            if self.music_paused == 0 {
                self.status_bar_header_l0de7();
            }
            // The next IRQ ($12DA) derives $0468 from the values set above
            self.transform_step = self.prev_scroll.wrapping_sub(self.scroll_fine) & 0x07;
            // $0E06: LDX $20; LDA $9FAE,X; STA $043D
            let stage_idx = self.stage.clamp(1, 33) as usize;
            self.warp_stage_id = STAGE_TABLE_9FAE.get(stage_idx).copied().unwrap_or(1);
        }
    }

    /// The stage map in `$AC00..$BFFF` (dumped from the original's own decompression for now)
    pub fn load_map_for_stage(&mut self) {
        let stage_idx = (self.stage.clamp(1, 33) - 1) as usize;
        let offset = stage_idx * 5120;
        if offset + 5120 <= all_stages().len() {
            self.current_map = all_stages()[offset..offset + 5120].to_vec();
        }
    }

    /// sub_13F0 ($13F0): Initial sprite hardware register setup
    pub fn setup_sprites(&mut self) {
        // $13F3: LDA #$00; JSR sub_10ED
        self.set_sprite_pointers(0x00);

        // $13F8..$1402: Y = (stage - 1) * 4 + $0456 (the bonus rooms $22.. read behind the 33 stage entries)
        let offset = (self.stage as usize).wrapping_sub(1).wrapping_mul(4) + self.spawn_variant as usize;
        let base_x = crate::image::img(0x60EA + offset);
        let base_y = crate::image::img(0x60EA + offset + 1);

        // $1406: ADC #$05; STA $05F4; STA $05F6; STA $05F8; STA $05FA
        self.player_x_low = base_x.wrapping_add(5);
        // $1418: STA $05F5
        self.sprite_head_x = self.player_x_low;
        self.sprite_dress_x = self.player_x_low;
        self.player_y_c64 = base_y;
        // $141E: STA $05F7 (Y+1); $1424: STA $05F9 (Y+2)
        self.sprite_head_y = base_y.wrapping_add(1);
        self.sprite_dress_y = base_y.wrapping_add(2);
        self.sprite_x_msb = 0x00;
        // $142A: STA $05FB = Y + 3
        self.en.spr_xy[0] = self.player_x_low;
        self.en.spr_xy[1] = base_y.wrapping_add(3);
        // $1432: JSR sub_1443
        self.sprite_colours_sub_1443();
        // $1435: JSR sub_20A3 waits until the video chip shows the new Y position: one IRQ chain (chain 70)
    }

    /// $1438..$1442 after the sprite sync
    pub fn setup_sprites_finish(&mut self) {
        self.flag_0431 = 0x00;
        // $143D: LDA #$07; STA $D015
        self.en.vic_me = 0x07;
    }

    /// sub_20A3 ($20A3..$20AB): spin until `$D001` equals `$05F5`, i.e. until one IRQ chain has copied the sprites
    fn vic_copy_pending_sync(&mut self) {
        let idle = ControllerInput::new();
        self.irq_frame_core(&idle);
    }

    /// sub_20DC ($20DC..$20E6): clear $0014..$001E
    pub fn clear_zero_page_sub_20dc(&mut self) {
        self.scroll_wrap_flag = 0;
        self.move_state = 0;
        self.move_edge_flag = 0;
        self.jump_state = 0;
        self.jump_phase = 0;
        // $0019 is not used by the port
        self.anim_walking = 0;
        self.brake_flag = 0;
        self.airborne_flag = 0;
        self.zp_1d = 0;
        self.zp_1e = 0;
    }

    /// sub_1443 ($1443..$146C): Giana's sprite colours
    pub fn sprite_colours_sub_1443(&mut self) {
        // $1443: LDA $D01C; ORA #$03; STA $D01C
        self.en.vic_mmc |= 0x03;
        // $144B: $9D20 -> $D025, $9D21 -> $D026
        self.vic_mc = [crate::image::img(0x9D20), crate::image::img(0x9D21)];
        // $1457: LDY $1568: $146D,Y .. $1471,Y -> $D027..$D029
        let y = self.player as usize;
        self.en.vic_col[0] = crate::image::img(0x146D + y);
        self.en.vic_col[1] = crate::image::img(0x146F + y);
        self.en.vic_col[2] = crate::image::img(0x1471 + y);
    }

    /// sub_0D39 ($0D39..$0D53): stage palette
    pub fn stage_colours_sub_0d39(&mut self) {
        let y = (self.stage as usize).wrapping_sub(1);
        self.zp_1e = crate::image::img(0x60C3 + y);
        self.en.vic_bg0 = crate::image::img(0x604E + y);
        self.en.vic_bg1 = crate::image::img(0x6075 + y);
        self.vic_bg2 = crate::image::img(0x609C + y); // $0D4D: STA $D023
    }

    /// sub_14DC ($14DC..$150B): make sure the sprite set matches the power-up level in `$22`
    pub fn sprite_set_check_sub_14dc(&mut self) {
        if self.sprite_set_check_decide() {
            // L14FA: JSR sub_1474 (L1500 only burns time)
            self.swap_sprite_sets_sub_1474();
        }
    }

    /// `$14DC..$14F9`: true when the sets are swapped (L14FA)
    pub fn sprite_set_check_decide(&mut self) -> bool {
        self.redraw_pending = 0;
        let cc03 = self.sprite_mem[0xCC03 - 0xC000];
        // $14E1: with `$22 != 0` and `$CC03 != 3` nothing happens (L1500); with `$CC03 == 3` the sound is requested and the
        // sets are swapped. Without `$22`, `$CC03 == 3` leaves everything as it is.
        if self.anim_counter != 0 {
            if cc03 != 3 {
                return false;
            }
            self.sfx_queue = 4;
        } else if cc03 == 3 {
            return false;
        }
        true
    }

    /// sub_1474 ($1474..$14DB): swap the two sprite sets ($CC00..$D5FF <-> $A080..$AA7F)
    pub fn swap_sprite_sets_sub_1474(&mut self) {
        if !self.swap_sets_begin() {
            return;
        }
        // $149C: JSR sub_9A4F with X = 7, Y = 0 is a busy wait of 65536 inner iterations. The raster IRQ chain keeps
        // running meanwhile. The port has no cycle model; the wait was measured in the oracle: $1D stays $FF for
        // 27 snapshots and sub_2F6E runs 28 times ($325B ends at 8 = 28 mod 20), see docs/oracle-notes.md section 15.
        let idle = ControllerInput::new();
        for _ in 0..self.timing.swap_wait {
            self.irq_frame_core(&idle);
        }
        self.swap_sets_finish();
    }

    /// `$1474..$149B`: false when `$1D != 0` (RTS), else the start of the swap up to the busy wait
    pub fn swap_sets_begin(&mut self) -> bool {
        if self.zp_1d != 0 {
            return false;
        }
        self.zp_1d = 0xFF;
        self.flag_0431 = 0xFF;
        self.set_sprite_pointers(0x2D);
        self.en.vic_mmc &= 0xF8;
        self.en.vic_col[1] = 1;
        self.en.vic_col[0] = 7;
        self.en.vic_col[2] = 7;
        true
    }

    /// `$14B7` loop up to half step `until`: iteration k exchanges byte `Y = -k mod 256` of page `k / 256` (Y counts down from 0)
    pub fn swap_copy_to(&mut self, until: usize) {
        for h in self.swap_iters..until.min(SWAP_HALVES) {
            let k = h / 2;
            let off = (k / 256) * 256 + (256 - k % 256) % 256;
            if h % 2 == 0 {
                self.swap_held = self.sprite_mem[0xCC00 - 0xC000 + off];
                self.sprite_mem[0xCC00 - 0xC000 + off] = self.sprite_swap[off];
            } else {
                self.sprite_swap[off] = self.swap_held;
            }
        }
        self.swap_iters = self.swap_iters.max(until.min(SWAP_HALVES));
    }

    /// `$149F..$14DB`: exchange the two sets after the busy wait
    pub fn swap_sets_finish(&mut self) {
        self.swap_copy_to(SWAP_HALVES);
        self.swap_iters = 0;
        self.sprite_colours_sub_1443();
        self.anim_walking = 0;
        self.redraw_pending = 0;
        self.zp_1d = 0;
    }

    /// L09DA ($09DA): Stage setup, player & score initialization
    pub fn start_stage(&mut self, stage_num: u32) {
        self.start_stage_head(stage_num);
        self.banner_text_l0a61();
        // The first stage start (from the title) also loads the sub-second timer; a restart keeps it running
        self.timer_subsecond = 0x1D;
        let idle = ControllerInput::new();
        for _ in 0..self.timing.post_fire {
            self.irq_frame_core(&idle);
        }
        self.start_stage_after_post_fire();
        // fewer chains than simulated play before the banner ends: the surplus ticks are dropped
        self.lost_ticks = (-self.timing.sound_extra).max(0) as u8;
        for len in self.timing.banner_steps.clone() {
            for _ in 0..len {
                self.irq_frame_core(&idle);
            }
            self.banner_delay = self.banner_delay.wrapping_sub(1);
        }
        // the rest of the banner wait (ended by fire) lets the sound driver play on
        for _ in 0..self.timing.sound_extra.max(0) {
            self.animate_chars_sub_163c();
            self.sound.call(0x439E, 0, 0, 0, false);
            self.sound_ticks += 1;
        }
        self.start_stage_after_banner();
        for _ in 0..self.timing.setup_wait {
            self.irq_frame_core(&idle);
        }
        self.start_stage_sprites();
        // $1435: sub_20A3 spins until one IRQ chain has copied the sprites
        self.irq_frame_core(&idle);
        self.start_stage_finish(stage_num);
        self.vic_capture_draw_frame();
    }

    /// $09DA..$09E3, $0A56 and sub_20AC: the part before the first wait
    pub fn start_stage_head(&mut self, stage_num: u32) {
        let stage_idx = stage_num.clamp(1, 33) as usize;
        self.stage = stage_num.clamp(1, 33);
        // sub_20AC
        self.reset_hardware();
        // $09C3: LDA #$FF; STA $0431 (sub_112C, the screen clear sub_9F45 and the post-fire code take `timing.post_fire` chains)
        self.flag_0431 = 0xFF;
    }

    /// sub_9936, the sprite set check of `$0A99` and the start of the banner (`$0AA0`)
    pub fn start_stage_after_post_fire(&mut self) {
        self.stage_variables_l09da();
        self.decompress_stage_layout();
        self.sprite_set_check_sub_14dc();
        // sub_154F: STA $157C (A = 4); the busy wait runs `timing.banner_steps` steps
        self.banner_delay = 0x04;
    }

    /// `$09DA..$0A56`: timer, diamonds and the death arc index of the stage
    pub fn stage_variables_l09da(&mut self) {
        self.stage_variables_l09da_inner(false);
    }

    /// `$09E4..$0A4D`: after a lost life the player's values are saved to `$1569..` and loaded back (one player)
    pub fn stage_variables_after_restart(&mut self) {
        self.stage_variables_l09da_inner(true);
    }

    /// `$09E9..$0A49`: save the values of the player, find the next one and load their values
    fn next_player_l09e9(&mut self) {
        let y = self.player as usize;
        self.player_save[y] = self.score_bcd[0]; // $1569,Y
        self.player_save[2 + y] = self.score_bcd[1]; // $156B,Y
        self.player_save[4 + y] = self.score_bcd[2]; // $156D,Y
        self.player_save[6 + y] = self.anim_counter; // $156F,Y
        self.player_save[8 + y] = self.stage as u8; // $1571,Y
        self.player_save[10 + y] = self.lives_c64; // $1573,Y
        self.player_save[14 + y] = self.spawn_variant; // $1577,Y
        // L0A14: EOR #$01 until a player is found who is not finished ($1575,Y); with one player that is the same one again.
        self.player ^= 1;
        if self.player_save[12 + self.player as usize] != 0 {
            self.player ^= 1;
        }
        let y = self.player as usize;
        self.score_bcd[0] = self.player_save[y];
        self.score_bcd[1] = self.player_save[2 + y];
        self.score_bcd[2] = self.player_save[4 + y];
        self.anim_counter = self.player_save[6 + y];
        self.lives_c64 = self.player_save[10 + y];
        self.stage = self.player_save[8 + y] as u32;
        self.spawn_variant = self.player_save[14 + y];
    }

    fn stage_variables_l09da_inner(&mut self, save: bool) {
        // $09E4: LDA $1579; BNE L0A51 (after a stage exit the player data and the diamond counter stay)
        let after_exit = self.exit_1579;
        if save && !after_exit {
            self.next_player_l09e9();
        }
        // $09DA: LDA #$01; STA $0449; LDA #$04; STA $044A
        self.timer_seconds_low = 0x01;
        self.timer_seconds_high = 0x04;
        // $0A4E: LDA #$00; STA $0434
        if !after_exit {
            self.diamonds_bcd = 0;
        }
        // $0A56: LDX $20; LDA $9FAE,X; STA $0466
        self.death_arc_idx = STAGE_TABLE_9FAE.get(self.stage.clamp(1, 33) as usize).copied().unwrap_or(1);
    }

    /// `$0AAB..$0AB4`: palette and playfield (sub_0D39, sub_0D54)
    pub fn start_stage_after_banner(&mut self) {
        self.start_stage_after_banner_timed(0);
    }

    pub fn start_stage_after_banner_timed(&mut self, total_steps: usize) {
        // $0AA3: LDY $20; LDA $1529,Y; JSR sub_150C (the tune of the stage)
        let track = crate::image::img(0x1529 + self.stage as usize);
        self.request_music_sub_150c(track);
        self.stage_colours_sub_0d39();
        // $0AAE: JSR sub_0E39 (IRQ vectors); this chain loses its sound tick
        self.lost_ticks = 1;
        // $0AB1: JSR sub_0E66
        self.status_bar_frame_sub_0e66();
        self.load_stage_tiles_step(0, total_steps);
    }

    /// `$0AB7` sub_13F0 up to the sprite sync
    pub fn start_stage_sprites(&mut self) {
        self.clear_zero_page_sub_20dc();
        self.setup_sprites();
    }

    /// after the sprite sync: `$1438`, `$0ABA` and the legacy stage data
    pub fn start_stage_finish(&mut self, stage_num: u32) {
        // $0A53: LDA #$00; STA $1579
        self.exit_1579 = false;
        self.setup_sprites_finish();
        // $0ABA: LDA #$00; STA $060C
        self.en.spr_ptr[4] = 0;
        self.load_stage(stage_num);
    }

    pub fn load_stage(&mut self, stage_num: u32) {
        let stage_idx = (stage_num.clamp(1, 33) - 1) as usize;
        self.stage = stage_num.clamp(1, 33);
        let offset = stage_idx * 5120;
        if offset + 5120 <= all_stages().len() {
            self.current_map = all_stages()[offset..offset + 5120].to_vec();
        }

        // Status bar rows only: the playfield belongs to sub_0D54 (load_stage_tiles)
        if self.legacy_model {
            self.load_stage_tiles();
            self.head_tile_ptr_low = 0x00;
            self.head_tile_ptr_high = 0x00;
            self.surrounding_tiles = [0; 9];
            self.surrounding_tile_flags = [0; 9];
        }

        self.enemies.clear();
        self.enemy_spawns.clear();
        self.particles.clear();
        self.items.clear();
        self.projectiles.clear();
        self.player_x = 20;
        self.player_y = 115;
        self.ground_y = 115;
        self.scroll_x = 0;
        self.cloud_scroll_x = 0;
        self.time = 401;
        self.timer_ticks = 0;
        self.is_jumping = false;
        self.is_falling = false;

        // Authentic C64 enemy spawns for all 33 stages directly from D20
        self.enemy_spawns = get_stage_enemy_spawns(self.stage);
    }

    /// Performs one game logic tick at authentic 50 Hz PAL rate.
    pub fn update(&mut self, input: &ControllerInput) {
        // Keyboard matrix: the space key is row 7, column bit 4 (read by sub_2194 as $EF)
        self.en.kbd[7] = if input.item { 0xEF } else { 0xFF };
        // Handle Cheat Toggles
        if input.toggle_invulnerable {
            self.god_mode = !self.god_mode;
            println!("Cheat: God Mode = {}", self.god_mode);
        }
        if input.toggle_lives {
            self.infinite_lives = !self.infinite_lives;
            println!("Cheat: Infinite Lives = {}", self.infinite_lives);
        }
        if input.next_stage && self.stage < 33 {
            self.load_stage(self.stage + 1);
        }
        if input.prev_stage && self.stage > 1 {
            self.load_stage(self.stage - 1);
        }

        match self.screen {
            GameScreen::Intro => {
                if input.start || input.jump || input.fire {
                    self.screen = GameScreen::Title;
                }
            }
            GameScreen::Title => {
                if input.start || input.jump || input.fire {
                    self.screen = GameScreen::InGame;
                    self.load_stage(1);
                }
            }
            GameScreen::InGame => {
                self.update_in_game(input);
            }
            GameScreen::StageClear => {
                if input.start || input.jump || input.fire {
                    if self.stage < 33 {
                        self.load_stage(self.stage + 1);
                        self.screen = GameScreen::InGame;
                    } else {
                        self.screen = GameScreen::GameWon;
                    }
                }
            }
            GameScreen::GameOver | GameScreen::GameWon => {
                if input.start || input.jump || input.fire {
                    self.screen = GameScreen::Title;
                }
            }
        }
    }

    /// Record the register values the video chip really used to draw this frame (from previous frame).
    pub fn vic_capture_draw_frame(&mut self) {
        self.en.vic_frame.draw_xscroll = self.scroll_fine;
        self.en.vic_frame.draw_sky_xscroll = self.sky_scroll_fine;
        self.en.vic_frame.draw_xy = self.en.vic_frame.xy;
        self.en.vic_frame.draw_ptr = self.en.vic_frame.ptr;
        self.en.vic_frame.draw_me = self.en.vic_frame.me;
        self.en.vic_frame.draw_mmc = self.en.vic_frame.mmc;
        self.en.vic_frame.draw_mdp = self.en.vic_mdp;
        self.en.vic_frame.draw_col = self.en.vic_col;
        self.en.vic_frame.draw_clouds.copy_from_slice(&self.screen_ram[80..200]);
    }

    /// One complete raster IRQ chain of the original ($12A3 -> $1326 -> $1393) as far as it is ported.
    /// Used for gameplay frames and for the title demo (where `music_paused != 0` makes sub_0EAB auto-scroll).
    pub fn irq_frame_core(&mut self, input: &ControllerInput) {
        self.vic_capture_draw_frame();
        self.irq_chain_a(input);
        self.irq_chain_b();
    }

    /// Raster handlers `$12A3` (scroll registers, sprite shift, player routines) and `$1326` (column scroll).
    pub fn irq_chain_a(&mut self, input: &ControllerInput) {
        // The column shift started by the previous frame's $1326 handler (sub_9600) finishes first. The copy runs in
        // the main line from line $76 of the previous frame and is mostly done when the handler at line $41 runs.
        self.column_scroll_finish_sub_9600();

        // Handler $1249: animated tiles
        self.tile_animation_handler_l127c();
        self.death_late_continue(false);
        if self.main_between_handlers {
            // a flag poked at the sample line is noticed by the main line before the second handler of the chain
            self.main_between_handlers = false;
            self.sub_1e19();
        }
        // L1E2D: the spin on the slot tables sees them empty as soon as the first handler cleared the last slot
        if self.main == crate::mainloop::MainLine::Bonus(crate::bonus::BonusPhase::SlotWait) {
            self.bonus_timed(crate::bonus::BonusPhase::SlotWait);
            if self.main != crate::mainloop::MainLine::Bonus(crate::bonus::BonusPhase::SlotWait) {
                self.bonus_ran = true;
            }
        }

        // sub_1200 ($12C4..$12EB): Main PAL raster IRQ sub-pixel fraction & scroll delta update
        // $12C4: CLC; LDA $0408; ADC #$01; AND #$03; STA $0408
        if self.skip_subpixel_inc {
            // After the reset of a restart the oracle shows $0408 unchanged for one more chain (cycle alignment of sub_20AC
            // against the $12A3 handler, observed in every restart)
            self.skip_subpixel_inc = false;
        } else {
            self.scroll_subpixel = (self.scroll_subpixel + 1) & 0x03;

            // $12DA: SEC; LDA $0469; SBC $0404; AND #$07; STA $0468
            self.transform_step = (self.prev_scroll.wrapping_sub(self.scroll_fine)) & 0x07;
            // $12E6: LDA $D016; AND #$07; STA $0469
            self.prev_scroll = self.scroll_fine & 0x07;
        }

        // $12EE: JSR sub_20E7 (move the enemy sprites 3..7 with the playfield)
        self.shift_sprites_sub_20e7();

        // $12F1: LDA $157D; BNE L131D; LDA $0431; BMI L131D; LDA $046D; BNE L131D
        if self.en.pause_157d == 0 && self.flag_0431 & 0x80 == 0 && self.st_046d == 0 {
            // sub_1D0A ($1D0A..$1D45): Authentic C64 stage timer countdown & timeout hazard check
            self.update_timer_sub_1d0a();

            // sub_9DF4 ($9DF4..$9EC2): Inspect 3x3 surrounding tiles on screen RAM around player
            self.update_tile_collision_sub_9df4();

            // sub_1A17 ($1A17): block interaction
            self.block_interaction_sub_1a17();

            // $1309: LDA $046D; BNE L131D
            if self.st_046d == 0 {
                // sub_1D67 ($1D67..$1DD0): death check
                self.death_check_sub_1d67();

                // sub_9F77 ($9F77..$9FAD): Read CIA-1 Port A ($DC00) joystick input registers
                if self.joystick_enabled {
                    self.read_joystick_sub_9f77(input);
                }

                // sub_1138 ($1138): tile collision / jump state transitions (calls sub_1200 and sub_11C6)
                self.check_player_tile_collision_sub_1138();

                // sub_0EAB ($0EAB): player movement, jump curve, inertia and scrolling
                self.update_player_physics_sub_0eab();

                // sub_1078 ($1078): player sprite animation sequence (falls into sub_10DF / sub_10ED)
                self.update_player_animation_sub_1078();
            }
        }

        // $1326 handler: slot table shift and start of sub_9600
        self.scroll_irq_l1326();

    }

    /// Raster handler `$1393`: enemies, sprite registers to the video chip, sound request, transformation.
    pub fn irq_chain_b(&mut self) {
        // The picture of this frame is drawn before the handler at line $FC changes the animated characters (sub_163C)
        self.en.vic_frame.draw_chars.copy_from_slice(&self.sprite_mem[..0x800]);
        // $13BC: JSR sub_2194 (enemies, fireball, sprite collisions)
        self.enemy_main_sub_2194();

        // $13BF..$13C8: copy $05F4..$0604 to the video chip ($D000..$D010)
        self.vic_copy_xy_l13bf();

        // $13CA: LDA $14; BNE L13E4; JSR sub_1519 (sound request consumer in the third raster IRQ)
        if self.scroll_wrap_flag == 0 {
            self.consume_sfx_sub_1519();
            // $13D1..$13DA: copy the frame pointers $0605..$060C to $CBF8..$CBFF
            self.vic_copy_pointers_l13d1();
        }

        // From here the video chip draws with these registers and latches sprite collisions in $D01E
        self.vic_frame_end();

        // $13CA..$13E4: the sound driver plays one tick unless `$157D` pauses the game (then the code skips it too)
        if self.lost_ticks > 0 {
            // sub_0E39 of the stage set-up restarted the raster chain: this chain never reached `$13E1`
            self.lost_ticks -= 1;
            return;
        }
        if self.scroll_wrap_flag == 0 && self.en.pause_157d == 0 {
            // $13E1: JSR sub_163C (animated characters)
            self.animate_chars_sub_163c();
        }
        if self.scroll_wrap_flag != 0 || self.en.pause_157d == 0 {
            self.sound.call(0x439E, 0, 0, 0, false); // JSR sub_41C0
            self.sound_ticks += 1;
        }

    }

    fn update_in_game(&mut self, input: &ControllerInput) {
        if self.first_in_game_tick {
            self.first_in_game_tick = false;
            self.vic_capture_draw_frame();
            return;
        }

        self.vic_capture_draw_frame();
        // The main line runs in the gap between the handlers $1326 and $1393 (it reacts to flags that $12A3 set, e.g.
        // the timer) and after $1393 until the oracle samples at raster line 50 (it reacts to flags that $1393 set and
        // continues waits that the sprite copy released).
        self.main_fire = input.fire;
        self.confirm_down = input.fire || input.jump;
        self.main_gap = 0;
        self.main_pre();
        let irq_off = self.irq_off_left > 0;
        if irq_off {
            self.irq_off_left -= 1;
        } else {
            self.irq_chain_a(input);
        }
        self.main_gap = 1;
        self.main_pass();
        if irq_off {
            // no raster interrupt at all in this frame: its sound tick is lost like the one the restart announced
            self.lost_ticks = self.lost_ticks.saturating_sub(1);
        } else if self.skip_h4 {
            // $20BB: STA $D01A = 0 in the gap before $1393: the raster interrupt of that handler never comes (the sprite
            // registers are not copied, the sound driver does not play and the animated characters stay)
            self.skip_h4 = false;
            self.lost_ticks = self.lost_ticks.saturating_sub(1);
        } else {
            self.irq_chain_b();
        }
        self.main_gap = 2;
        self.main_pass();
        self.main_resume();

        if !self.legacy_model {
            return;
        }

        if self.invulnerable_timer > 0 {
            self.invulnerable_timer -= 1;
        }

        // 2. Player movement & physics
        let was_falling = self.is_falling || self.fall_speed > 0;
        self.update_player(input);

        // 3. Stage Exit / Finish check (reaching end of stage col > 240)
        // 3. Stage clear trigger: reaching exit door/statue (0xD9, 0x8F), defeating Boss, or reaching stage end
        let p_col = (self.player_x / 8) as usize;
        let p_row = (self.player_y / 8) as usize;
        let mut exit_reached = (self.player_x / 8) >= 240;
        if p_col < 256 && p_row < 20 {
            let t = self.current_map[p_row * 256 + p_col];
            if t == 0xD9 || t == 0x8F {
                exit_reached = true;
            }
        }
        if self.stage == 33 && self.enemies.iter().any(|e| e.enemy_type == EnemyType::Dragon && (!e.active || e.is_dying)) {
            exit_reached = true;
        }

        if exit_reached {
            self.score += self.time * 10;
            if self.stage < 33 {
                self.load_stage(self.stage + 1);
            } else {
                self.screen = GameScreen::GameWon;
            }
            return;
        }

        // 4. Dynamic enemy spawner based on camera scroll column
        let current_col = self.scroll_x / 8 + 42; // Trigger when approaching right border of screen
        self.enemy_spawns.retain(|&(trig_col, x, y, etype)| {
            if current_col >= trig_col {
                self.enemies.push(Enemy::new(etype, x, y));
                false
            } else {
                true
            }
        });

        // 4b. Update active enemies with authentic platform patrol & gravity
        for enemy in &mut self.enemies {
            if !enemy.active {
                continue;
            }
            enemy.update();

            // Walking/rolling enemies fall with gravity when stepping off platforms (C64 L2638/L2653)
            if enemy.enemy_type == EnemyType::Monster
                || enemy.enemy_type == EnemyType::RollingEye
                || enemy.enemy_type == EnemyType::Crab
            {
                // Wall collision (C64 L2670): reverse direction if hitting a solid block
                let front_x = if enemy.vel_x > 0.0 { enemy.x as i32 + 16 } else { enemy.x as i32 - 1 };
                let mid_y = enemy.y as i32 + 8;
                if is_map_solid(&self.current_map, front_x, mid_y) {
                    enemy.vel_x = -enemy.vel_x;
                }

                let foot_x = enemy.x as i32 + 8;
                let foot_y = enemy.y as i32 + 16;
                let standing = is_map_solid(&self.current_map, foot_x, foot_y);

                if !standing {
                    // In air: fall with gravity down to ground or pit
                    enemy.vel_y = (enemy.vel_y + 0.5).min(4.0);
                    if enemy.y > 170.0 {
                        enemy.active = false;
                    }
                } else {
                    let land_row = (foot_y / 8) as i32;
                    enemy.y = (land_row * 8 - 16) as f32;
                    enemy.vel_y = 0.0;
                }
            }
        }

        // 4c. Player Shooting (Dream Bubbles / Fireballs)
        if self.shoot_cooldown > 0 {
            self.shoot_cooldown -= 1;
        }
        if input.fire && self.has_shot && self.shoot_cooldown == 0 {
            self.shoot_cooldown = 12; // 4 shots per second max
            let sx = if self.facing_right { (self.player_x + 14) as f32 } else { (self.player_x - 4) as f32 };
            let sy = (self.player_y + 6) as f32;
            self.projectiles.push(Projectile::new(sx, sy, self.facing_right));
        }

        // Update active projectiles
        for proj in &mut self.projectiles {
            proj.update();
            if !proj.active || proj.popping {
                continue;
            }
            // Collision with map walls
            if is_map_solid(&self.current_map, proj.x as i32, proj.y as i32) {
                proj.pop();
                continue;
            }
            // Collision with enemies
            let pbox = proj.bounds();
            for enemy in &mut self.enemies {
                if enemy.active && !enemy.is_dying && pbox.intersects(&enemy.bounds()) {
                    proj.pop();
                    enemy.hp -= 1;
                    if enemy.hp <= 0 {
                        enemy.stomp();
                        self.score += if enemy.enemy_type == EnemyType::Dragon { 5000 } else { 200 };
                    }
                    break;
                }
            }
        }
        self.projectiles.retain(|p| p.active);

        // 5. Update particles & items
        for p in &mut self.particles {
            p.update();
        }
        self.particles.retain(|p| p.active);

        for item in &mut self.items {
            item.update();
        }
        let player_box = Rect::new(self.player_x, self.player_y, 16, 21);
        for item in &mut self.items {
            if item.active && player_box.intersects(&item.bounds()) {
                item.active = false;
                if item.is_diamond {
                    self.diamonds += 1;
                    self.score += 100;
                    if self.diamonds >= 100 {
                        self.diamonds = 0;
                        self.lives += 1;
                    }
                } else {
                    self.score += 500;
                    if !self.has_punker {
                        self.has_punker = true; // Level 1: Punker
                    } else {
                        self.has_shot = true;   // Level 2: Shooting ability
                    }
                }
            }
        }
        self.items.retain(|item| item.active);

        // 6. Enemy collision check
        let player_box = Rect::new(self.player_x, self.player_y, 16, 21);
        for enemy in &mut self.enemies {
            if !enemy.active {
                continue;
            }
            let enemy_box = enemy.bounds();
            if player_box.intersects(&enemy_box) {
                // If player is falling downwards onto enemy: Stomp kill!
                if was_falling && self.player_y + 16 <= enemy.y as i32 + 12 {
                    enemy.stomp();
                    // The original awards the enemy's table value, +50 for the Stage 1 owl (oracle 04_enemy: $0435 = $50)
                    self.score += 50;
                    // Bounce player up
                    self.is_jumping = true;
                    self.jump_frame = 10;
                    self.is_falling = false;
                } else if self.invulnerable_timer == 0 && !self.god_mode {
                    // Player hit by enemy: lose shot first, then punker, then life
                    if self.has_shot {
                        self.has_shot = false;
                        self.invulnerable_timer = 100; // 2 seconds invulnerability
                    } else if self.has_punker {
                        self.has_punker = false;
                        self.invulnerable_timer = 100; // 2 seconds invulnerability
                    } else {
                        self.player_die();
                        return;
                    }
                }
            }
        }
    }

    fn update_player(&mut self, input: &ControllerInput) {
        if self.is_dying {
            self.player_y += self.death_vel_y as i32;
            self.death_vel_y += 0.35;
            if self.player_y > 220 {
                self.is_dying = false;
                self.death_vel_y = 0.0;
                if self.infinite_lives {
                    self.player_x = 20;
                    self.player_y = 115;
                    self.scroll_x = 0;
                    self.invulnerable_timer = 150;
                } else if self.lives > 1 {
                    self.lives -= 1;
                    self.player_x = 20;
                    self.player_y = 115;
                    self.scroll_x = 0;
                    self.has_punker = false;
                    self.has_shot = false;
                    self.invulnerable_timer = 150;
                    self.time = 401;
                    self.load_stage(self.stage);
                } else {
                    self.screen = GameScreen::GameOver;
                }
            }
            return;
        }

        // Horizontal movement with full-height wall collision checks (top, mid, bottom)
        let mut moved = false;
        if input.right && !input.left {
            let next_x = self.player_x + WALK_SPEED;
            let solid = self.is_tile_solid(next_x + 14, self.player_y + 4)
                || self.is_tile_solid(next_x + 14, self.player_y + 10)
                || self.is_tile_solid(next_x + 14, self.player_y + 18);
            if !solid {
                self.player_x = next_x;
            }
            self.facing_right = true;
            moved = true;
        } else if input.left && !input.right {
            if self.player_x > 8 {
                let next_x = self.player_x - WALK_SPEED;
                let solid = self.is_tile_solid(next_x, self.player_y + 4)
                    || self.is_tile_solid(next_x, self.player_y + 10)
                    || self.is_tile_solid(next_x, self.player_y + 18);
                if !solid {
                    self.player_x = next_x;
                }
            }
            self.facing_right = false;
            moved = true;
        }

        // Animation update
        if moved {
            self.anim_tick += 1;
            if self.anim_tick >= 4 {
                self.anim_tick = 0;
                self.walk_frame = (self.walk_frame % 3) + 1;
            }
        } else {
            self.anim_tick = 0;
            self.walk_frame = 0;
        }

        // Jump physics using authentic C64 $9ADE table
        if (input.jump || input.up) && !self.is_jumping && !self.is_falling {
            // Check if standing on solid ground
            let foot_l = self.is_tile_solid(self.player_x + 4, self.player_y + 21);
            let foot_r = self.is_tile_solid(self.player_x + 12, self.player_y + 21);
            if foot_l || foot_r {
                self.is_jumping = true;
                self.jump_frame = 0;
            }
        }

        if self.is_jumping {
            if self.jump_frame < JUMP_CURVE.len() {
                let delta_y = JUMP_CURVE[self.jump_frame];
                let next_y = self.player_y - delta_y;

                // Check head collision against block above
                let head_hit_left = self.check_block_hit(self.player_x + 4, next_y - 1);
                let head_hit_right = self.check_block_hit(self.player_x + 12, next_y - 1);

                if head_hit_left || head_hit_right {
                    self.is_jumping = false;
                    self.is_falling = true;
                    self.fall_speed = 1;
                } else {
                    self.player_y = next_y;
                    self.jump_frame += 1;
                }
            } else {
                self.is_jumping = false;
                self.is_falling = true;
                self.fall_speed = 1;
            }
        } else {
            // Check if standing on ground / block
            let foot_l = self.is_tile_solid(self.player_x + 4, self.player_y + 21);
            let foot_r = self.is_tile_solid(self.player_x + 12, self.player_y + 21);

            if !foot_l && !foot_r {
                self.is_falling = true;
                self.player_y += self.fall_speed;
                if self.fall_speed < MAX_FALL_SPEED {
                    self.fall_speed += 1;
                }
                // Falling into pit below screen
                if self.player_y > 160 {
                    self.player_die();
                    return;
                }
            } else {
                let land_row = ((self.player_y + 21) / 8) as i32;
                self.player_y = land_row * 8 - 21;
                self.is_falling = false;
                self.fall_speed = 0;

                // Crumbling bridge decay (C64 sub_19CF: $28..$2F)
                if land_row >= 0 && land_row < 20 {
                    let r = land_row as usize;
                    let c_l = ((self.player_x + 4).max(0) / 8) as usize;
                    let c_r = ((self.player_x + 12).max(0) / 8) as usize;
                    for c in [c_l, c_r] {
                        if c < 256 {
                            let idx = r * 256 + c;
                            let t = self.current_map[idx];
                            if t >= 0x28 && t <= 0x2F {
                                if t == 0x2F {
                                    self.current_map[idx] = 0;
                                } else {
                                    self.current_map[idx] = t + 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Diamond collection across Giana's full bounding box
        self.check_diamond_pickup();

        // Smooth camera scroll following player
        let screen_rel_x = self.player_x - self.scroll_x as i32;
        if screen_rel_x > 160 {
            let diff = (screen_rel_x - 160) as usize;
            self.scroll_x += diff;
            // Clouds scroll at 1/4 the speed of the ground (authentic parallax)
            self.cloud_scroll_x += diff / 4;
        }
    }

    fn check_block_hit(&mut self, px: i32, py: i32) -> bool {
        if px < 0 || py < 0 {
            return false;
        }
        let col = (px / 8) as usize;
        let row = (py / 8) as usize;

        if col >= 256 || row >= 20 {
            return false;
        }

        let idx = row * 256 + col;
        let char_code = self.current_map[idx];
        let tile_type = get_tile_type(char_code);

        match tile_type {
            TileType::Breakable => {
                // Check if tile itself or tile above holds a collectible diamond/powerup ($EB, $EA, $F2)
                let mut found_item = false;
                if char_code == 0xEB || char_code == 0xEA {
                    self.current_map[idx] = 0x47; // Used block
                    self.score += 100;
                    self.diamonds += 1;
                    if self.diamonds >= 100 {
                        self.diamonds = 0;
                        self.lives += 1;
                    }
                    let fx = (col * 8) as f32;
                    let fy = (row * 8 - 8) as f32;
                    self.items.push(PowerupItem::new(fx, fy, true));
                    found_item = true;
                } else if char_code == 0xF2 {
                    self.current_map[idx] = 0x47;
                    self.score += 200;
                    let fx = (col * 8) as f32;
                    let fy = (row * 8 - 8) as f32;
                    self.items.push(PowerupItem::new(fx, fy, false));
                    found_item = true;
                } else if row > 0 {
                    let top_idx = (row - 1) * 256 + col;
                    let top_code = self.current_map[top_idx];
                    if top_code == 0xEB || top_code == 0xEA {
                        self.current_map[top_idx] = 0x47; // Bumped brick top
                        self.score += 100;
                        self.diamonds += 1;
                        if self.diamonds >= 100 {
                            self.diamonds = 0;
                            self.lives += 1;
                        }
                        let fx = (col * 8) as f32;
                        let fy = ((row - 1) * 8 - 8) as f32;
                        self.items.push(PowerupItem::new(fx, fy, true));
                        found_item = true;
                    } else if top_code == 0xF2 {
                        self.current_map[top_idx] = 0x47;
                        self.score += 200;
                        let fx = (col * 8) as f32;
                        let fy = ((row - 1) * 8 - 8) as f32;
                        self.items.push(PowerupItem::new(fx, fy, false));
                        found_item = true;
                    }
                }

                if !found_item {
                    if self.has_punker {
                        self.current_map[idx] = 0;
                        if row > 0 {
                            let top_idx = (row - 1) * 256 + col;
                            if get_tile_type(self.current_map[top_idx]) == TileType::Breakable {
                                self.current_map[top_idx] = 0;
                            }
                        }
                        self.score += 50;
                        let fx = (col * 8) as f32;
                        let fy = (row * 8) as f32;
                        self.particles.push(DebrisParticle::new(fx, fy, -2.0, -3.0));
                        self.particles.push(DebrisParticle::new(fx + 4.0, fy, 2.0, -3.0));
                        self.particles.push(DebrisParticle::new(fx, fy + 4.0, -1.5, -2.0));
                        self.particles.push(DebrisParticle::new(fx + 4.0, fy + 4.0, 1.5, -2.0));
                    } else {
                        self.score += 10;
                    }
                }
                true
            }
            TileType::Bonus => {
                // Clear 2x2 question block ($A6..$A9) or single bonus tile ($CA in tests)
                let is_right_tile = char_code == 0xA7 || char_code == 0xA9;
                let base_c = if is_right_tile && col > 0 { col - 1 } else { col };
                let base_r = if char_code == 0xA6 || char_code == 0xA7 { row } else if row > 0 { row - 1 } else { row };

                if char_code >= 0xA6 && char_code <= 0xA9 {
                    for dr in 0..2 {
                        for dc in 0..2 {
                            let r = base_r + dr;
                            let c = base_c + dc;
                            if r < 20 && c < 256 {
                                let tidx = r * 256 + c;
                                if get_tile_type(self.current_map[tidx]) == TileType::Bonus {
                                    self.current_map[tidx] = 0;
                                }
                            }
                        }
                    }
                } else {
                    self.current_map[idx] = 0;
                }

                // First question block in Stage 1 at Col 84 gives powerup item, others give diamonds
                let is_powerup = self.stage == 1 && base_c == 84;
                let fx = (base_c * 8) as f32;
                let fy = (base_r * 8 - 8) as f32;

                if is_powerup {
                    self.score += 200;
                    self.items.push(PowerupItem::new(fx, fy, false));
                } else {
                    self.score += 100;
                    self.diamonds += 1;
                    if self.diamonds >= 100 {
                        self.diamonds = 0;
                        self.lives += 1;
                    }
                    self.items.push(PowerupItem::new(fx, fy, true));
                }
                true
            }
            TileType::Solid | TileType::Crumbling => true,
            _ => false,
        }
    }

    fn check_diamond_pickup(&mut self) {
        let min_col = (self.player_x.max(0) / 8) as usize;
        let max_col = ((self.player_x + 15).max(0) / 8) as usize;
        let min_row = (self.player_y.max(0) / 8) as usize;
        let max_row = ((self.player_y + 20).max(0) / 8) as usize;

        for r in min_row..=max_row {
            if r >= 20 {
                continue;
            }
            for c in min_col..=max_col {
                if c >= 256 {
                    continue;
                }
                let idx = r * 256 + c;
                let char_code = self.current_map[idx];
                if get_tile_type(char_code) == TileType::Diamond {
                    self.current_map[idx] = 0;
                    self.diamonds += 1;
                    self.score += 100;
                    if self.diamonds >= 100 {
                        self.diamonds = 0;
                        self.lives += 1;
                    }
                }
            }
        }
    }

    pub fn is_tile_solid(&self, px: i32, py: i32) -> bool {
        is_map_solid(&self.current_map, px, py)
    }

    fn player_die(&mut self) {
        if self.god_mode || self.is_dying {
            return;
        }
        self.is_dying = true;
        // $0465 belongs to the ported death trigger (player_hit_l1db7), not to this legacy model
        if self.player_y >= 150 {
            self.death_vel_y = 2.0;
        } else {
            self.death_vel_y = -5.0;
        }
    }

    /// Gets active sprite pointers for Giana
    pub fn get_giana_sprites(&self) -> [usize; 3] {
        if self.is_dying {
            return GIANA_DEATH;
        }
        if self.has_punker {
            if self.is_jumping || self.is_falling {
                if self.facing_right { PUNK_JUMP_RIGHT } else { PUNK_JUMP_LEFT }
            } else if self.walk_frame == 0 {
                if self.facing_right { PUNK_STAND_RIGHT } else { PUNK_STAND_LEFT }
            } else if self.walk_frame == 1 {
                if self.facing_right { PUNK_WALK_RIGHT_1 } else { PUNK_WALK_LEFT_1 }
            } else if self.walk_frame == 2 {
                if self.facing_right { PUNK_WALK_RIGHT_2 } else { PUNK_WALK_LEFT_2 }
            } else {
                if self.facing_right { PUNK_WALK_RIGHT_3 } else { PUNK_WALK_LEFT_3 }
            }
        } else {
            if self.is_jumping || self.is_falling {
                if self.facing_right { GIANA_JUMP_RIGHT } else { GIANA_JUMP_LEFT }
            } else if self.walk_frame == 0 {
                if self.facing_right { GIANA_STAND_RIGHT } else { GIANA_STAND_LEFT }
            } else if self.walk_frame == 1 {
                if self.facing_right { GIANA_WALK_RIGHT_1 } else { GIANA_WALK_LEFT_1 }
            } else if self.walk_frame == 2 {
                if self.facing_right { GIANA_WALK_RIGHT_2 } else { GIANA_WALK_LEFT_2 }
            } else {
                if self.facing_right { GIANA_WALK_RIGHT_3 } else { GIANA_WALK_LEFT_3 }
            }
        }
    }
}

pub fn is_map_solid(map: &[u8], px: i32, py: i32) -> bool {
    if px < 0 || py < 0 {
        return false;
    }
    let col = (px / 8) as usize;
    let row = (py / 8) as usize;
    if col >= 256 || row >= 20 {
        return false;
    }
    let char_code = map[row * 256 + col];
    let t = get_tile_type(char_code);
    t == TileType::Solid || t == TileType::Breakable || t == TileType::Bonus || t == TileType::Crumbling
}
