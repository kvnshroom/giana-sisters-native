//! Main line of the original: the code outside the raster interrupts ($0ABF main loop and what it calls).
//!
//! The original main loop spins and reacts to flags the interrupts set. Routines that wait for the video chip
//! (`sub_20A3`) or burn time (`sub_9A4F`) advance in lock step with the IRQ chain. The port models that with an
//! explicit state: `GameState::update` runs the interrupt handlers `$12A3`/`$1326`, then one `main_pass`, then
//! `$1393`. A routine that has to wait leaves its state in `MainLine` and is resumed by the next pass.

use crate::image::img;
use crate::state::{ControllerInput, GameState};

/// Cost of one pass of the bonus count loop with the redraw and the busy wait (`sub_9A4F`, Y = $0A), and without them
/// (fire held), in thousandths of a slow pass.
pub const BONUS_FAST_COST: i32 = 8;
/// Parts of a slow pass: counting down and adding the score, drawing the timer, drawing the score, the rest is the wait.
/// The draws take a few hundred cycles; one snapshot of 12_poke_warp (frame 329) falls between the two draws (the score
/// digits are one pass behind). The cost of the parts could not be fitted to that single event, so they are free here.
pub const BONUS_COST_DEC: i32 = 0;
pub const BONUS_COST_TIMER: i32 = 0;
pub const BONUS_COST_SCORE: i32 = 0;
pub const BONUS_COST_WAIT: i32 = 1000 - BONUS_COST_DEC - BONUS_COST_TIMER - BONUS_COST_SCORE;

/// Busy-wait chains of `sub_9A4F` with Y = 0 and a small X (measured: see docs/oracle-notes.md section 15).
pub const LONG_WAIT_CHAINS: u32 = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MainLine {
    /// `$0ABF` main loop: one pass per interrupt chain
    Loop,
    /// `sub_2041`: death jump of Giana
    DeathArc(DeathArc),
    /// `sub_2041` was entered late in a frame: `$043C` is decremented, the music request and the rest follow in the next gap
    DeathLate,
    /// `$09B6`: stage restart after a lost life, one raster chain per step
    Restart(RestartPhase),
    /// `$0BDF`: stage exit (bonus count, banner wait), ends in the stage start of the next stage
    StageExit(ExitPhase),
    /// `$0C1D`: the text after a death by time; `Some(left)` once `sub_154F` waits (step, chains left)
    Timeout(Option<(usize, usize)>),
    /// `$0B6B`: pipe warp (skips a stage, ends in the stage exit)
    Warp(WarpPhase),
    /// `sub_1E19`: the statue (bonus room visit)
    Bonus(crate::bonus::BonusPhase),
    /// `sub_1474` called from the main loop (`$0C89`): busy wait of `timing.swap_wait` chains, then the sets are exchanged
    SpriteSwap { n: usize, plan: crate::state::SpriteSwapTiming },
    /// `$0AC7`: the ending after the last stage (`ending.rs`)
    Ending(Option<(usize, usize)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarpPhase {
    /// the three busy waits
    Wait(usize),
    /// after the screen was cleared and the texts drawn, until `sub_154F` starts
    ToBanner(usize),
    /// `sub_154F` (A = 6): `step` decrements done, `left` chains until the next one
    Banner { step: usize, left: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitPhase {
    /// `sub_0CA2` after `sub_9F45`: the rest of the routine and the bonus count run in the next chain
    Cleared,
    /// `$0CD3` loop: one step per chain while the time bonus is counted
    Bonus,
    /// waiting between the last bonus step and `sub_154F`
    Count(usize),
    /// `sub_154F` (A = 6): `step` decrements done, `left` chains until the next one
    Banner { step: usize, left: usize },
    /// fire ends `sub_154F`: chains until `$09A3`
    Final(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestartPhase {
    PostFire(usize),
    Banner { step: usize, left: usize },
    Setup(usize),
    Sync,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeathArc {
    phase: ArcPhase,
    /// index into the table at `$9AEC`
    y: usize,
    /// the sync wait of `sub_20A3` after the last step is still pending
    awaiting_sync: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ArcPhase {
    /// `$2055` loop: rising part, table walked forward
    Rise,
    /// `$2068` loop: falling part, table walked backward
    Fall,
    /// `$2083` loop: constant fall by 3
    Drop,
    /// `$2090`: `sub_9A4F` with Y = 0
    Wait(u32),
}

impl GameState {
    /// sub_20A3 ($20A3..$20AB): true when the video chip already shows Giana's current Y
    fn sprite_y_synced(&self) -> bool {
        self.en.vic_frame.xy[1] == self.player_y_c64
    }

    /// One pass of the main loop (only while no routine of the main line is waiting).
    pub fn main_pass(&mut self) {
        match self.main.clone() {
            MainLine::Loop => self.main_loop_pass(),
            MainLine::Bonus(crate::bonus::BonusPhase::Rise) => self.bonus_rise_step(),
            // L1E2D spins on the slot tables: it goes on in the gap after the interrupt that cleared the last slot
            MainLine::Bonus(crate::bonus::BonusPhase::SlotWait) if !self.bonus_ran => {
                self.bonus_timed(crate::bonus::BonusPhase::SlotWait);
                if self.main != MainLine::Bonus(crate::bonus::BonusPhase::SlotWait) {
                    self.bonus_ran = true;
                }
            }
            MainLine::DeathArc(arc)
                if arc.phase == ArcPhase::Wait(0) && !self.bonus_ran && self.restart_timing.front().map_or(false, |t| t.tail_mid) =>
            {
                self.bonus_ran = true;
                self.death_arc_step(arc);
            }
            MainLine::StageExit(ExitPhase::Final(0)) if !self.bonus_ran => {
                self.bonus_ran = true;
                self.exit_step(ExitPhase::Final(0));
            }
            MainLine::Timeout(st) if !self.bonus_ran && (st.is_none() || self.timeout_is_last(st)) => {
                self.bonus_ran = true;
                self.timeout_step(st);
            }
            MainLine::Warp(WarpPhase::Wait(0)) if !self.bonus_ran => {
                self.bonus_ran = true;
                self.warp_step(WarpPhase::Wait(0));
            }
            MainLine::Restart(RestartPhase::Setup(0)) if !self.bonus_ran => {
                self.bonus_ran = true;
                self.restart_step(RestartPhase::Setup(0));
            }
            // the last chain of the swap goes on into the sprite set-up between the interrupts $12A3 and $1393
            MainLine::Bonus(p) if !self.bonus_ran && self.bonus_phase_is_last(&p) => {
                self.bonus_ran = true;
                self.bonus_timed(p);
            }
            _ => {}
        }
    }

    /// Busy waits that only burn time end somewhere between the sample line and the first interrupt of the next frame:
    /// they are handled before the interrupt chain (the oracle shows the reset of sub_20AC in the snapshot but the
    /// chain after it already computes with the reset values).
    pub fn main_pre(&mut self) {
        self.bonus_ran = false;
        self.death_late_continue(true);
        if self.main == MainLine::Restart(RestartPhase::Sync) {
            self.restart_step(RestartPhase::Sync);
        }
        self.main_timed(true);
    }

    /// Continue a routine of the main line that waited for the sprite copy of the interrupt chain.
    pub fn main_resume(&mut self) {
        match self.main.clone() {
            MainLine::DeathArc(arc) if !matches!(arc.phase, ArcPhase::Wait(_)) => self.death_arc_step(arc),
            MainLine::Restart(RestartPhase::Sync) => self.restart_step(RestartPhase::Sync),
            MainLine::SpriteSwap { .. } if !self.bonus_ran && self.swap_step_is_last() => {
                self.bonus_ran = true;
                self.swap_step();
            }
            MainLine::Bonus(crate::bonus::BonusPhase::Sync) => self.bonus_timed(crate::bonus::BonusPhase::Sync),
            MainLine::Bonus(crate::bonus::BonusPhase::FinSync) => self.bonus_timed(crate::bonus::BonusPhase::FinSync),
            // the end of the busy wait on the way back landed after the sprite copy of its frame
            MainLine::Bonus(crate::bonus::BonusPhase::DecompBack(0)) if !self.bonus_ran && !self.bonus_phase_is_last(&crate::bonus::BonusPhase::DecompBack(0)) => {
                self.bonus_ran = true;
                self.bonus_timed(crate::bonus::BonusPhase::DecompBack(0));
            }
            // the end of the final wait lands after the interrupt chain of its frame
            MainLine::Bonus(crate::bonus::BonusPhase::Fin(0)) if !self.bonus_ran && !self.bonus_phase_is_last(&crate::bonus::BonusPhase::Fin(0)) => {
                self.bonus_ran = true;
                self.bonus_timed(crate::bonus::BonusPhase::Fin(0));
            }
            _ => {}
        }
        self.main_timed(false);
    }

    /// Time-based waits (busy loops): `pre` selects the position, `tail_pre` says where they end.
    fn main_timed(&mut self, pre: bool) {
        match self.main.clone() {
            MainLine::DeathArc(arc) if matches!(arc.phase, ArcPhase::Wait(_)) => {
                let front = self.restart_timing.front();
                let tail_pre = front.map_or(true, |t| t.tail_pre);
                let mid = front.map_or(false, |t| t.tail_mid);
                // with `tail_mid` the last step runs in main_pass; the countdown before it runs here
                if tail_pre == pre && !self.bonus_ran && !(mid && arc.phase == ArcPhase::Wait(0)) {
                    self.bonus_ran = true;
                    self.death_arc_step(arc);
                }
            }
            MainLine::Restart(p) if p != RestartPhase::Sync => {
                if self.timing.tail_pre == pre && !self.bonus_ran && p != RestartPhase::Setup(0) {
                    self.bonus_ran = true;
                    self.restart_step(p);
                }
            }
            MainLine::Bonus(p) if !matches!(p, crate::bonus::BonusPhase::Rise | crate::bonus::BonusPhase::SlotWait | crate::bonus::BonusPhase::Sync | crate::bonus::BonusPhase::FinSync | crate::bonus::BonusPhase::Fin(0) | crate::bonus::BonusPhase::DecompBack(0)) => {
                if (if self.bonus_swap_ends_early(&p) { pre } else { self.timing.tail_pre == pre }) && !self.bonus_ran && !self.bonus_phase_is_last(&p) && p != crate::bonus::BonusPhase::DecompBack(0) {
                    self.bonus_ran = true;
                    self.bonus_timed(p);
                }
            }
            // the end of the busy wait on the way back before the first handler of its frame
            MainLine::Bonus(crate::bonus::BonusPhase::DecompBack(0)) => {
                if pre && !self.bonus_ran && self.bonus_timing_front().ret_early {
                    self.bonus_ran = true;
                    self.bonus_timed(crate::bonus::BonusPhase::DecompBack(0));
                }
            }
            MainLine::Timeout(st) => {
                // the text itself is drawn between the interrupts $12A3 and $1393 (main_pass)
                if self.timing.tail_pre == pre && !self.bonus_ran && st.is_some() && !self.timeout_is_last(st) {
                    self.bonus_ran = true;
                    self.timeout_step(st);
                }
            }
            MainLine::Ending(st) => {
                if self.timing.tail_pre == pre && !self.bonus_ran {
                    self.bonus_ran = true;
                    self.ending_step(st);
                }
            }
            MainLine::SpriteSwap { .. } => {
                if self.timing.tail_pre == pre && !self.bonus_ran && !self.swap_step_is_last() {
                    self.bonus_ran = true;
                    self.swap_step();
                }
            }
            MainLine::Warp(p) => {
                // the clear after the third busy wait lands between the interrupts $12A3 and $1393 (main_pass)
                if self.timing.tail_pre == pre && !self.bonus_ran && p != WarpPhase::Wait(0) {
                    self.bonus_ran = true;
                    self.warp_step(p);
                }
            }
            MainLine::StageExit(p) => {
                // The end of the wait depends on when fire is read: its position was measured (`final_pre`)
                // the hand-over to the next stage (Final(0)) runs between the interrupts, in main_pass
                if self.timing.tail_pre == pre && p != ExitPhase::Final(0) && !self.bonus_ran {
                    self.bonus_ran = true;
                    self.exit_step(p);
                }
            }
            _ => {}
        }
    }

    /// L0ABF..L0C96: one pass of the main loop
    fn main_loop_pass(&mut self) {
        // L0ABF: LDA $157E; BNE L0AC7 (the ending, single player; other sequences: not ported, see ledger)
        if self.ending_hook_l0abf() {
            return;
        }
        // L0B6B: LDA $2169; BEQ L0BDF
        if self.st_2169 != 0 {
            self.start_warp_l0b6b();
            return;
        }
        // L0BDF: LDA $046D; BEQ L0BF6
        if self.st_046d != 0 {
            self.start_stage_exit_l0bdf();
            return;
        }
        // L0BF6: LDA $0465; BNE L0C03; LDA $154D; BNE L0C03; JSR sub_1CD7 (status bar)
        // The interrupt that sets $046B (pick-up) hit a pass of the original that was already past $0BF6: that pass starts the
        // sprite swap at $0C89 and the status bar is not redrawn until the swap is over (28 frames later).
        if self.hazard_hit == 0 && self.music_paused == 0 && !self.sprite_swap_would_begin() {
            self.status_bar_sub_1cd7();
        }
        // L0C03: LDA $0465; BEQ L0C81
        if self.hazard_hit != 0 {
            // L0C08: LDA $0457; CLC; ADC $0464; BNE L0C81
            if self.st_0457.wrapping_add(self.st_0464) == 0 {
                self.start_death_sub_2041();
                return;
            }
        }
        // L0C81: LDA $0457; BEQ L0C89; JSR sub_1E19
        if self.st_0457 != 0 {
            self.sub_1e19();
            if self.main != MainLine::Loop {
                return;
            }
        }
        // L0C89: LDA $046B; BEQ L0C96; JSR sub_14DC; LDA #0; STA $0431
        self.sprite_check_l0c89();
    }

    /// The swap began in the gap before the first interrupt of this frame (main loop noticed a flag set between frames):
    /// the pass of this frame is chain 0 of the plan.
    pub fn swap_began_before_chain(&mut self) {
        if let MainLine::SpriteSwap { n, .. } = &mut self.main {
            *n = usize::MAX;
        }
    }

    /// One chain of the exchange loop `$14B7` of a swap from the main loop (chain n after the start)
    fn swap_step(&mut self) {
        let MainLine::SpriteSwap { n, plan } = self.main.clone() else { return };
        let n = n.wrapping_add(1);
        let done = plan.progress.get(n).copied().unwrap_or(crate::state::SWAP_HALVES);
        self.swap_copy_to(done);
        if done >= crate::state::SWAP_HALVES {
            self.swap_sets_finish();
            self.flag_0431 = 0; // $0C8C
            self.main = MainLine::Loop;
            // the loop comes round to $0BF6 at once and draws what the interrupt changed during the swap
            if self.hazard_hit == 0 && self.music_paused == 0 {
                self.status_bar_sub_1cd7();
            }
        } else {
            self.main = MainLine::SpriteSwap { n, plan };
        }
    }

    /// the step that ends the loop: the original ends it after the last interrupt of its frame ($1D and $0431 stay set in the
    /// chains of that frame: gate of `$12F1`, enemy colour cycling), so it runs in `main_resume`
    fn swap_step_is_last(&self) -> bool {
        let MainLine::SpriteSwap { n, plan } = &self.main else { return false };
        plan.progress.get(n.wrapping_add(1)).map_or(true, |&d| d >= crate::state::SWAP_HALVES)
    }

    /// Side-effect free look at `sprite_set_check_decide` + `swap_sets_begin` (`$14DC..$1474`)
    fn sprite_swap_would_begin(&self) -> bool {
        if self.redraw_pending == 0 || self.zp_1d != 0 {
            return false;
        }
        let cc03 = self.sprite_mem[0xCC03 - 0xC000];
        if self.anim_counter != 0 { cc03 == 3 } else { cc03 != 3 }
    }

    /// `$0C89..$0C96`: LDA $046B; BEQ; JSR sub_14DC; LDA #0; STA $0431
    pub fn sprite_check_l0c89(&mut self) {
        if self.redraw_pending != 0 {
            if self.sprite_set_check_decide() {
                if self.swap_sets_begin() {
                    let plan = self.swap_timing.pop_front().unwrap_or_default();
                    if let Some(v) = plan.score_clobber {
                        self.text_cell(0x2E, ((v >> 4) & 0x0F) | 0x30);
                        self.text_cell(0x2F, (v & 0x0F) | 0x30);
                    }
                    self.main = MainLine::SpriteSwap { n: 0, plan };
                    return;
                }
            }
            self.flag_0431 = 0;
        }
    }

    /// $0C11..$0C1A and the start of sub_2041 ($2041..$2054)
    fn start_death_sub_2041(&mut self) {
        // $0C11: LDA $0465; STA $157F
        self.death_kind_157f = self.hazard_hit;
        // $2041: DEC $043C
        // Debug cheat F1 (infinite lives): the counter stays
        if !self.infinite_lives {
            self.lives_c64 = self.lives_c64.wrapping_sub(1);
        }
        if let Some(steps) = self.restart_timing.front().and_then(|t| t.death_late) {
            // $2044: the sample line cuts the music request: the rest runs in the next frame
            self.request_music_cut(7, steps);
            self.main = MainLine::DeathLate;
            self.death_late_pre = self.main_gap == 2;
            return;
        }
        self.start_death_rest_sub_2041(true);
    }

    /// The cut music request goes on in the next frame: between the interrupts `$1249` and `$12A3` when the interrupt
    /// `$1393` cut it (`$0431` is still `$FF` when the animated tiles run), before `$1249` when the sample line did.
    pub fn death_late_continue(&mut self, pre: bool) {
        if self.main == MainLine::DeathLate && self.death_late_pre == pre {
            self.bonus_ran = true;
            self.sound.resume();
            self.start_death_rest_sub_2041(false);
        }
    }

    /// `$2044..$2054`: music request, `$0431 = $80`, sprite pointers, and the first part of the death jump
    fn start_death_rest_sub_2041(&mut self, request: bool) {
        // $2044: LDA #$07; JSR sub_150C (music request)
        if request {
            self.request_music_sub_150c(7);
        }
        // $2049: LDA #$80; STA $0431
        self.flag_0431 = 0x80;
        // $204E: LDA #$2A; JSR sub_10ED
        self.set_sprite_pointers(0x2A);
        // $2053: LDY #$00
        let arc = DeathArc { phase: ArcPhase::Rise, y: 0, awaiting_sync: false };
        self.death_arc_step(arc);
    }

    /// `sub_150C` whose driver call is cut after `steps` instructions (the sound driver pauses; `resume` finishes it)
    fn request_music_cut(&mut self, track: u8, steps: u32) {
        if track != self.music_track_1527 {
            self.music_track_1527 = track;
            self.sound.call_limited(0x49E6, track, 0, 0, false, steps);
            self.sound_requests.push((track, self.sound_ticks));
        }
    }

    /// sub_150C ($150C..$1518): hand a music track to the sound driver unless it is already playing
    pub fn request_music_sub_150c(&mut self, track: u8) {
        if track != self.music_track_1527 {
            self.music_track_1527 = track;
            // $1514: CLC; JSR sub_41C9 (L49E6: start the track)
            self.sound.call(0x49E6, track, 0, 0, false);
            self.sound_requests.push((track, self.sound_ticks));
        }
    }

    /// Runs the death jump until it has to wait for the video chip
    fn death_arc_step(&mut self, mut arc: DeathArc) {
        loop {
            if arc.awaiting_sync {
                // JSR sub_20A3 returned only when the chip shows the new Y
                if !self.sprite_y_synced() {
                    self.main = MainLine::DeathArc(arc);
                    return;
                }
                arc.awaiting_sync = false;
                match arc.phase {
                    ArcPhase::Rise => {
                        // $2065: INY; BNE L2055
                        arc.y += 1;
                    }
                    ArcPhase::Fall => {
                        // $2074: LDA $05F5; CMP #$FA; BCS L2090
                        if self.player_y_c64 >= 0xFA {
                            arc.phase = ArcPhase::Wait(self.restart_timing.front().map_or(LONG_WAIT_CHAINS, |t| t.death_wait as u32));
                            continue;
                        }
                        // $207B: DEY; BPL L2068 (else fall through to $207E)
                        if arc.y == 0 {
                            arc.phase = ArcPhase::Drop;
                            self.move_delta = 3; // $207E: LDA #$03; STA $0428
                        } else {
                            arc.y -= 1;
                        }
                    }
                    ArcPhase::Drop => {
                        // $2089: LDA $05F5; CMP #$FA; BCC L2083
                        if self.player_y_c64 >= 0xFA {
                            arc.phase = ArcPhase::Wait(self.restart_timing.front().map_or(LONG_WAIT_CHAINS, |t| t.death_wait as u32));
                        }
                    }
                    ArcPhase::Wait(_) => {}
                }
                continue;
            }
            match arc.phase {
                ArcPhase::Rise => {
                    // L2055: LDA $9AEC,Y; CMP #$FF; BEQ L2068
                    let a = img(0x9AEC + arc.y);
                    if a == 0xFF {
                        arc.phase = ArcPhase::Fall;
                        continue;
                    }
                    // STA $0428; JSR sub_9F2C; JSR sub_20A3
                    self.move_delta = a;
                    self.sub_player_y_sub_9f2c(a);
                    arc.awaiting_sync = true;
                }
                ArcPhase::Fall => {
                    // L2068: LDA $9AEC,Y; STA $0428; JSR sub_9F1F; JSR sub_20A3
                    let a = img(0x9AEC + arc.y);
                    self.move_delta = a;
                    self.add_player_y_sub_9f1f(a);
                    arc.awaiting_sync = true;
                }
                ArcPhase::Drop => {
                    // L2083: JSR sub_9F1F; JSR sub_20A3
                    self.add_player_y_sub_9f1f(3);
                    arc.awaiting_sync = true;
                }
                ArcPhase::Wait(n) => {
                    if n > 0 {
                        arc.phase = ArcPhase::Wait(n - 1);
                        self.main = MainLine::DeathArc(arc);
                        return;
                    }
                    // L2090 tail: LDA #0; STA $0431; STA $05F5; STA $0465; STA $22
                    self.flag_0431 = 0;
                    self.player_y_c64 = 0;
                    self.hazard_hit = 0;
                    self.anim_counter = 0;
                    self.main = MainLine::Loop;
                    self.after_death_l0c1a();
                    return;
                }
            }
        }
    }

    /// $0C1A..$0C4B after sub_2041 returned
    fn after_death_l0c1a(&mut self) {
        // $0C1A: LDA $157F; CMP #$02; BNE L0C46: death by time shows a text first
        if self.death_kind_157f == 2 {
            self.main = MainLine::Timeout(None);
            return;
        }
        self.after_death_l0c46();
    }

    /// $0C46: LDA $043C; BMI L0C4E; JMP L09B6 -> restart the stage, or the game is over
    fn after_death_l0c46(&mut self) {
        if self.lives_c64 & 0x80 == 0 {
            self.restart_stage_l09b6();
        } else {
            // $0C4E: $1575,Y = 1, high score entry (sub_325D, disk loader: not ported), then L09B6 goes on with the other
            // player, or jumps to L084F, the title, when both are finished ($1575 + $1576 = 2)
            self.player_save[12 + self.player as usize] = 1;
            if self.player_save[12] != 0 && self.player_save[13] != 0 {
                self.game_over = true;
            } else {
                self.restart_stage_l09b6();
            }
        }
    }

    /// L09B6: restart the current stage after a lost life (the stage setup repeats the stage start timeline)
    pub(crate) fn restart_stage_l09b6(&mut self) {
        let stage = self.stage;
        // The waits of the restart last as long as the recording says (harness) or as long as a plain restart
        if let Some(t) = self.restart_timing.pop_front() {
            self.timing = t;
        } else {
            self.timing = crate::state::StageTiming { swap_wait: 0, ..crate::state::StageTiming::default() };
        }
        self.timing.swap_wait = 0;
        // $09BB: JSR sub_20AC; $09C6: JSR sub_9F45 clears screen and colour RAM; $09C9: JSR sub_0D1F
        self.start_stage_head(stage);
        self.skip_h4 = self.main_gap == 1;
        self.irq_off_left = self.timing.irq_off_frames;
        self.skip_subpixel_inc = self.timing.skip_inc;
        // sub_0E39 (`$0A5E`) resets the raster chain: the chains up to the banner texts lose their sound tick
        self.lost_ticks = 1 + self.timing.text_after as u8;
        self.confirm_armed = false;
        // L09DA sets the stage variables before the banner texts are drawn
        if self.timing.vars_after == 0 {
            self.stage_variables_after_restart();
        }
        if self.timing.clear_after == 0 {
            self.restart_clear_screen();
            if self.timing.text_after == 0 {
                self.banner_text_l0a61();
            }
        }
        self.restart_step(RestartPhase::PostFire(self.timing.post_fire));
    }

    /// $09C6/$09C9: sub_9F45 clears screen and colour RAM, sub_0D1F the tile animation slots
    fn restart_clear_screen(&mut self) {
        self.screen_ram = [0; 1000];
        self.color_ram = [0; 1000];
        self.clear_entity_slots_sub_0d1f();
    }

    /// One chain of the restart sequence (the waits of `start_stage`, spread over `update` calls)
    fn restart_step(&mut self, phase: RestartPhase) {
        let stage = self.stage;
        let mut phase = phase;
        loop {
            match phase {
                RestartPhase::PostFire(n) => {
                    let at = self.timing.post_fire.saturating_sub(n);
                    if self.timing.clear_after > 0 && at == self.timing.clear_after {
                        self.restart_clear_screen();
                        if self.timing.text_after == 0 {
                            self.banner_text_l0a61();
                        }
                    }
                    if self.timing.text_after > 0 && at == self.timing.clear_after + self.timing.text_after {
                        self.banner_text_l0a61();
                    }
                    if self.timing.post_fire.saturating_sub(n) == self.timing.vars_after {
                        self.stage_variables_after_restart();
                    }
                    if n > 0 {
                        self.main = MainLine::Restart(RestartPhase::PostFire(n - 1));
                        return;
                    }
                    self.start_stage_after_post_fire();
                    phase = RestartPhase::Banner { step: 0, left: self.timing.banner_steps.first().copied().unwrap_or(0) };
                }
                RestartPhase::Banner { step, left } => {
                    if self.confirm_banner {
                        // Not in the original: stay until a new press. A key still held from the death does not count.
                        self.confirm_armed |= !self.confirm_down;
                        if !(self.confirm_armed && self.confirm_down) {
                            self.main = MainLine::Restart(RestartPhase::Banner { step, left });
                            return;
                        }
                        self.start_stage_after_banner_timed(self.timing.setup_wait);
                        phase = RestartPhase::Setup(self.timing.setup_wait);
                    } else if step >= self.timing.banner_steps.len() {
                        self.start_stage_after_banner_timed(self.timing.setup_wait);
                        phase = RestartPhase::Setup(self.timing.setup_wait);
                    } else if left > 0 {
                        self.main = MainLine::Restart(RestartPhase::Banner { step, left: left - 1 });
                        return;
                    } else {
                        self.banner_delay = self.banner_delay.wrapping_sub(1);
                        let next = step + 1;
                        phase = RestartPhase::Banner { step: next, left: self.timing.banner_steps.get(next).copied().unwrap_or(0) };
                    }
                }
                RestartPhase::Setup(n) => {
                    if n > 0 {
                        if self.timing.setup_wait >= 2 && n == 1 {
                            self.load_stage_tiles_step(1, self.timing.setup_wait);
                        }
                        self.main = MainLine::Restart(RestartPhase::Setup(n - 1));
                        return;
                    }
                    if self.timing.setup_wait >= 2 {
                        self.load_stage_tiles_step(2, self.timing.setup_wait);
                    }
                    self.start_stage_sprites();
                    self.main = MainLine::Restart(RestartPhase::Sync);
                    return;
                }
                RestartPhase::Sync => {
                    self.start_stage_finish(stage);
                    self.main = MainLine::Loop;
                    return;
                }
            }
        }
    }

    /// The banner after a lost life is waiting for a press.
    pub fn waiting_for_confirm(&self) -> bool {
        self.confirm_banner && matches!(self.main, MainLine::Restart(RestartPhase::Banner { .. }))
    }

    /// the step that ends the wait and starts the restart (it runs between the interrupts of its frame)
    fn timeout_is_last(&self, st: Option<(usize, usize)>) -> bool {
        let n = self.timeout_timing.front().map_or(crate::state::TimeoutTiming::default().steps.len(), |t| t.steps.len());
        matches!(st, Some((step, 0)) if step + 1 >= n)
    }

    /// One chain of the time-out text: clear and text first, then the steps of the banner wait (`sub_154F`, A = 4)
    fn timeout_step(&mut self, st: Option<(usize, usize)>) {
        let steps = self.timeout_timing.front().map(|t| t.steps.clone()).unwrap_or_else(|| crate::state::TimeoutTiming::default().steps);
        let (step, left) = match st {
            None => {
                // $0C21: JSR sub_9F45; JSR sub_0D1F; $0C27: $0431 = $FF; JSR sub_0C99
                self.screen_ram = [0; 1000];
                self.color_ram = [0; 1000];
                self.clear_entity_slots_sub_0d1f();
                self.flag_0431 = 0xFF;
                self.en.vic_bg0 = 0;
                self.en.vic_me = 0;
                // $0C2F: the text at $1622 (row 9, column $0F)
                let mut p = 0x1622;
                self.print_string_at_sub_9881(&mut p, 0x0F, 9);
                // $0C3F: LDA #$04; LDY #$00; JSR sub_154F
                self.banner_delay = 0x04;
                (0, steps.first().copied().unwrap_or(0))
            }
            Some(v) => v,
        };
        // $1558: fire ends the wait
        if self.joystick_enabled && self.main_fire && st.is_some() {
            self.timeout_timing.pop_front();
            self.main = MainLine::Loop;
            self.after_death_l0c46();
            return;
        }
        if left > 0 {
            self.main = MainLine::Timeout(Some((step, left - 1)));
            return;
        }
        if st.is_none() {
            self.main = MainLine::Timeout(Some((step, left)));
            return;
        }
        // $1562: DEC $157C
        self.banner_delay = self.banner_delay.wrapping_sub(1);
        let next = step + 1;
        if next >= steps.len() {
            self.timeout_timing.pop_front();
            self.main = MainLine::Loop;
            self.after_death_l0c46();
            return;
        }
        // the chain of the decrement is the first one of the next step
        self.main = MainLine::Timeout(Some((next, steps[next].saturating_sub(1))));
    }

    /// L0B6B..L0B75: music request to the sound driver, then three busy waits
    fn start_warp_l0b6b(&mut self) {
        // $0B70: LDA #$06; CLC; JSR sub_41C9 (the driver is called directly, `$1527` stays)
        self.sound.call(0x49E6, 6, 0, 0, false);
        // $0B76: LDY #$00; JSR sub_9A4F (three times)
        let wait = self.warp_timing.front().map_or(50, |t| t.wait);
        self.main = MainLine::Warp(WarpPhase::Wait(wait));
    }

    /// One chain of the pipe warp
    fn warp_step(&mut self, phase: WarpPhase) {
        let mut phase = phase;
        loop {
            match phase {
                WarpPhase::Wait(n) => {
                    if n > 0 {
                        self.main = MainLine::Warp(WarpPhase::Wait(n - 1));
                        return;
                    }
                    // $0B81: $2169 = 0, $0417 = 0, $0431 = $FF, sub_9F45, sub_0D1F, sub_0C99
                    self.st_2169 = 0;
                    self.flag_0431 = 0xFF;
                    self.screen_ram = [0; 1000];
                    self.color_ram = [0; 1000];
                    self.clear_entity_slots_sub_0d1f();
                    // the clearing takes the rest of the chain
                    self.main = MainLine::Warp(WarpPhase::ToBanner(self.warp_timing.front().map_or(1, |t| t.to_banner)));
                    return;
                }
                WarpPhase::ToBanner(n) => {
                    if n > 1 {
                        self.main = MainLine::Warp(WarpPhase::ToBanner(n - 1));
                        return;
                    }
                    // $0B94: JSR sub_0C99; $0B97: $20 += 2
                    self.en.vic_bg0 = 0;
                    self.en.vic_me = 0;
                    self.stage += 2;
                    // $0B9E..$0BB6: the texts (string at $162B, string `$1568`)
                    let mut p = 0x162B;
                    self.print_string_at_sub_9881(&mut p, 0x0E, 9);
                    self.print_string_sub_9881(self.player as usize, 7, 9);
                    // $0BB9: LDX $20; INX; LDA $9FAE,X; STA $043D; sub_98D9
                    self.warp_stage_id = crate::image::img(0x9FAE + self.stage as usize + 1);
                    self.print_bcd_sub_98d9(&[self.warp_stage_id], 9, 0x1C);
                    // $0BD3: LDA #$06; LDY #$00; JSR sub_154F: STA $157C
                    self.banner_delay = 0x06;
                    let first = self.warp_timing.front().and_then(|t| t.steps.first().copied()).unwrap_or(0);
                    phase = WarpPhase::Banner { step: 0, left: first };
                }
                WarpPhase::Banner { step, left } => {
                    let steps = self.warp_timing.front().map(|t| t.steps.clone()).unwrap_or_default();
                    // $1558: LDA $0401; BNE L1567: fire ends the wait
                    if self.joystick_enabled && self.main_fire {
                        self.warp_timing.pop_front();
                        self.st_046d = 1;
                        self.main = MainLine::Loop;
                        self.start_stage_exit_l0bdf();
                        return;
                    }
                    if left > 0 {
                        self.main = MainLine::Warp(WarpPhase::Banner { step, left: left - 1 });
                        return;
                    }
                    // $1562: DEC $157C
                    self.banner_delay = self.banner_delay.wrapping_sub(1);
                    let next = step + 1;
                    if next >= steps.len() {
                        // $0BDA: LDA #$01; STA $046D, then the stage exit follows at once
                        self.warp_timing.pop_front();
                        self.st_046d = 1;
                        self.main = MainLine::Loop;
                        self.start_stage_exit_l0bdf();
                        return;
                    }
                    phase = WarpPhase::Banner { step: next, left: steps[next] };
                }
            }
        }
    }

    /// L0BDF..L0BE9: music request and the start of `sub_0CA2` (`sub_9F45` clears screen and colour RAM)
    fn start_stage_exit_l0bdf(&mut self) {
        // $0BE4: LDA #$07; JSR sub_150C
        self.request_music_sub_150c(7);
        // $0CA2: JSR sub_9F45
        self.screen_ram = [0; 1000];
        self.color_ram = [0; 1000];
        self.main = MainLine::StageExit(ExitPhase::Cleared);
    }

    /// One chain of the stage exit
    fn exit_step(&mut self, phase: ExitPhase) {
        let mut phase = phase;
        if phase == ExitPhase::Bonus {
            self.exit_acc += self.exit_timing_front().bonus_budget;
        }
        loop {
            match phase {
                ExitPhase::Cleared => {
                    // $0CA5: JSR sub_0D1F; $0CA8: LDA #$FF; STA $0431; $0CAD: JSR sub_0C99
                    self.clear_entity_slots_sub_0d1f();
                    self.flag_0431 = 0xFF;
                    self.en.vic_bg0 = 0;
                    self.en.vic_me = 0;
                    // $0CB0..$0CCD: the two texts of the bonus screen (strings at $15FA and $1612)
                    let mut p = 0x15FA;
                    self.print_string_at_sub_9881(&mut p, 7, 7);
                    let mut p = 0x1612;
                    self.print_string_at_sub_9881(&mut p, 7, 9);
                    // $0CD0: JMP L0CF3: draw the numbers, then the busy wait of sub_9A4F (Y = $0A)
                    self.exit_render_counts();
                    self.exit_acc = self.exit_timing_front().bonus_carry;
                    self.exit_sub = 0;
                    phase = ExitPhase::Bonus;
                    self.exit_acc += self.exit_timing_front().bonus_budget;
                }
                ExitPhase::Bonus => {
                    // the main line has `bonus_budget` cycles per chain; the busy waits and the loop eat them
                    // (measured: 12_poke_warp, one slow pass per chain, 9 per 8 chains, 17 passes while fire is held)
                    // exit_pending: 0 = wait (then the test of L0CD3), 1 = fast pass pause, 2 = count down, 3 = timer
                    // drawn next, 4 = score drawn next
                    let (cost, step) = match self.exit_sub {
                        0 => (BONUS_COST_WAIT, 0),
                        1 => (BONUS_FAST_COST, 1),
                        2 => (BONUS_COST_DEC, 2),
                        3 => (BONUS_COST_TIMER, 3),
                        _ => (BONUS_COST_SCORE, 4),
                    };
                    if cost > self.exit_acc {
                        self.main = MainLine::StageExit(ExitPhase::Bonus);
                        return;
                    }
                    self.exit_acc -= cost;
                    match step {
                        0 | 1 => {
                            // L0CD3: LDA $0449; BNE L0CD9; RTS
                            if self.timer_seconds_low == 0 {
                                phase = ExitPhase::Count(self.exit_timing_front().count_wait);
                                continue;
                            }
                            self.exit_sub = 2;
                        }
                        2 => {
                            // L0CD9: SED; $0449 = $0449 - 1; CLD
                            self.timer_seconds_low = bcd_sub_one(self.timer_seconds_low);
                            // $0CE4: LDA #$0A; LDX #$00; JSR sub_991D
                            self.add_score_sub_991d(0x0A, 0x00);
                            // $0CEB: JSR sub_9F77
                            if self.joystick_enabled {
                                let mut pad = ControllerInput::new();
                                pad.fire = self.main_fire;
                                self.read_joystick_sub_9f77(&pad);
                            }
                            // $0CEE: LDA $0401; BNE L0CD3 (fire: count on without redrawing)
                            self.exit_sub = if self.joystick_fire != 0 { 1 } else { 3 };
                        }
                        3 => {
                            // L0CF3: the timer
                            self.print_bcd_sub_98d9(&[self.timer_seconds_low], 9, 8);
                            self.exit_sub = 4;
                        }
                        _ => {
                            // L0D04: the score, then the busy wait of sub_9A4F (Y = $0A)
                            let score = [self.score_bcd[2], self.score_bcd[1], self.score_bcd[0]];
                            self.print_bcd_sub_98d9(&score, 9, 0x19);
                            self.exit_sub = 0;
                        }
                    }
                    phase = ExitPhase::Bonus;
                }
                ExitPhase::Count(n) => {
                    if n > 0 {
                        self.main = MainLine::StageExit(ExitPhase::Count(n - 1));
                        return;
                    }
                    // $0BEE: LDY #$00; LDA #$06; JSR sub_154F: STA $157C
                    self.banner_delay = 0x06;
                    let first = self.exit_timing_front().banner_steps.first().copied().unwrap_or(0);
                    phase = ExitPhase::Banner { step: 0, left: first };
                }
                ExitPhase::Banner { step, left } => {
                    let steps = self.exit_timing_front().banner_steps;
                    // $1558: LDA $0401; BNE L1567: fire ends the wait
                    if self.joystick_enabled && self.main_fire && step < steps.len() {
                        phase = ExitPhase::Final(0);
                        continue;
                    }
                    if step >= steps.len() {
                        phase = ExitPhase::Final(self.exit_timing_front().final_wait);
                    } else if left > 0 {
                        self.main = MainLine::StageExit(ExitPhase::Banner { step, left: left - 1 });
                        return;
                    } else {
                        // $1562: DEC $157C
                        self.banner_delay = self.banner_delay.wrapping_sub(1);
                        let next = step + 1;
                        phase = ExitPhase::Banner { step: next, left: steps.get(next).copied().unwrap_or(0) };
                    }
                }
                ExitPhase::Final(n) => {
                    if n > 0 {
                        self.main = MainLine::StageExit(ExitPhase::Final(n - 1));
                        return;
                    }
                    self.exit_timing.pop_front();
                    self.stage_exit_l09a3();
                    return;
                }
            }
        }
    }

    fn exit_timing_front(&self) -> crate::state::ExitTiming {
        self.exit_timing.front().cloned().unwrap_or_default()
    }

    /// L0CF3..L0D1C: timer left in `$0449` and score `$0437..$0435` on the bonus screen (`sub_98D9`)
    fn exit_render_counts(&mut self) {
        self.print_bcd_sub_98d9(&[self.timer_seconds_low], 9, 8);
        let score = [self.score_bcd[2], self.score_bcd[1], self.score_bcd[0]];
        self.print_bcd_sub_98d9(&score, 9, 0x19);
    }

    /// L09A3 and `$09B8`: the next stage starts after the exit
    fn stage_exit_l09a3(&mut self) {
        // $09A3: LDA #$00; STA $0456; STA $2169; STA $0417; STA $046D; INC $20
        self.spawn_variant = 0;
        self.st_2169 = 0;
        self.st_046d = 0;
        if self.stage >= 33 {
            // The original continues with code from the disk (ending, high score table: `sub_325D`, `L3348`), which is not
            // part of the program image: the port returns to the title. `$0AD1` marks the player as finished ($1575,Y = $FF),
            // the other one plays on.
            self.main = MainLine::Loop;
            self.player_save[12 + self.player as usize] = 0xFF;
            if self.player_save[12] != 0 && self.player_save[13] != 0 {
                self.game_over = true;
            } else {
                self.restart_stage_l09b6();
            }
            return;
        }
        self.stage += 1;
        // $09B3: LDA #$01 falls into $09B8: STA $1579
        self.exit_1579 = true;
        self.main = MainLine::Loop;
        self.restart_stage_l09b6();
    }
}

/// SED; SEC; SBC #$01 for a valid BCD value above zero
fn bcd_sub_one(v: u8) -> u8 {
    if v & 0x0F == 0 { v - 0x07 } else { v - 1 }
}
