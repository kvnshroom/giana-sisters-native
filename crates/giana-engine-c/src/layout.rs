//! The one address table of the 30A preview (`docs/rom-hack-family-c.md`, 7.4): every address a native routine of this
//! engine reads or writes, in the game's own memory map, so that another version of the preview only needs another
//! table and no constant is scattered through the code.
//!
//! Rules for whoever extends it (every work package does): one field per line, **sorted by address** (the field list
//! and the value list of [`PREVIEW`] in the same order), a short role in the doc comment and the routine that owns
//! the address. Zero-page fields are `u16` like the rest so that they index the 64 KB image directly.
//!
//! [`PREVIEW`] is "Giana 30A Demo 2.0" (CSDb release 168405); the addresses were read from the unpacked image and
//! checked against the game running in the oracle machine (`crates/giana-c30-oracle`).

/// Addresses of one version of the preview. See the module documentation for the ordering rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C30aLayout {
    /// `$02`: the game's pointer `$02/$03` (the status bar's item copies, `$2671`, `$269D`) [B]
    pub pointer2: u16,
    /// `$04`: print pointer `$04/$05`: the row address of the print position (`$2392`) [B, F]
    pub print_ptr: u16,
    /// `$06`: the general pointer `$06/$07`: the text printer's screen pointer (`$08/$09` its colour pointer, `$22C5`) [F],
    /// the treasure-room door's second row (`$40E2`) and the screen copy (`$4152`) [O3]
    pub text_ptr: u16,
    /// `$0A`: the object probe saves its slot here (`$7B11`) [O1]
    pub obj_probe_slot: u16,
    /// `$0B`: screen column of the object probe (`$7B11`) [O1]
    pub obj_probe_col: u16,
    /// `$0C`: object probe row pointer 1 (`$E6`, `$E8`: rows 2, 3) [O1]
    pub obj_probe_ptr1: u16,
    /// `$0E`: the game's 16-bit scratch pointer `$0E/$0F` (many routines; the probe leaves its row 3 pointer there)
    pub scratch_ptr: u16,
    /// `$10`: colour RAM pointer `$10/$11` matching the screen pointer `$0E/$0F` (`$3E72`) [S]
    pub colour_ptr: u16,
    /// `$12`: column offset of a falling item's last move: `1` straight down, `0` slid right, `2` slid left (`$3991`) [S]
    pub fall_column: u16,
    /// `$13`: `0` when a landed item changed the ground under it this frame (`$3A3C`) [S]
    pub fall_landed: u16,
    /// `$14`: column step request/state: `$80` = step requested (`$2A59`), `$7F` while the step runs (the IRQs skip the logic), `0` idle [S]
    pub scroll_step: u16,
    /// `$15`: horizontal direction of travel: `$FF` right, `$80` left, `0` standing (`BIT`: V = moving right) [P]
    pub walk_dir: u16,
    /// `$16`: `$FF` when walking right at or beyond the scroll edge (x >= `$A4`): the move scrolls instead [P]
    pub walk_scroll: u16,
    /// `$17`: vertical direction: `1` rising, `$FF` falling, `0` on the ground [P]
    pub vert_dir: u16,
    /// `$18`: spring/bounce phase: `0` none, positive = rising part, negative = falling part [P]
    pub bounce: u16,
    /// `$19`: swimming flag (`0` on land) [P]
    pub swim: u16,
    /// `$1A`: Giana's animation index (walk/swim cycle 3..0 counting down, `$FF` after a pose) (`$2B6E`) [P]
    pub anim_index: u16,
    /// `$1B`: `$FF` while braking against the direction of travel [P]
    pub braking: u16,
    /// `$1C`: frames spent falling (feeds the swim entry depth) [P]
    pub fall_frames: u16,
    /// `$1D`: `0` = Giana's sprite 2 is multicolour (the sprite routine sets bit 2 of `$D01C`, `$3575`) [W]
    pub layer_hires: u16,
    /// `$1E`: running `$389D` animations [S, B]
    pub anim_389d_count: u16,
    /// `$1F`: non-zero = Giana stands on an object (set by the object code), counts as floor [P]
    pub on_object: u16,
    /// `$20`: game state, `0` = playing, anything else freezes the logic [P]
    pub game_state: u16,
    /// `$21`: screen column of the scroll (0 = start of the stage) [S]
    pub scroll_column: u16,
    /// `$22`: slot of the object Giana stands on (written by the object code `$82CE`) [P, O1]
    pub object_index: u16,
    /// `$23`: how deep the feet are in their tile row, `(y - 3) & 7` (written by the sprite routine `$333A`) [P]
    pub feet_depth: u16,
    /// `$24`: print column (`$25`: print row, through `$8DE3/$8E01`) of the status bar and text printer (`$23A1`) [B, F]
    pub print_col: u16,
    /// `$26`: bobbing timer on the water surface [P]
    pub bob_timer: u16,
    /// `$27`: bobbing direction (`$80` = up) [P]
    pub bob_dir: u16,
    /// `$28`: phase of the title's and the high-score screen's IRQ chain (2, 1, 0) [F]
    pub title_phase: u16,
    /// `$29`: index into the title's upper scrolling text (`$2E25`) [F]
    pub title_text_index: u16,
    /// `$2A/$2B`: the raster bar table of `$1890` [F]
    pub bar_table: u16,
    /// `$2C`: index into the title's lower scrolling text (`$2E74`) [F]
    pub title_text_index2: u16,
    /// `$2D`: low byte of the screen address where the last `$38F8` block animation started (moves left with each column step) [S, B]
    pub anim_38f8_last: u16,
    /// `$2E`: running `$38F8` animations [S, B]
    pub anim_38f8_count: u16,
    /// `$2F`: non-zero = the probe's bottom row reads as empty (set by the object code at `$83D9`) [P]
    pub probe_skip_bottom: u16,
    /// `$30`: the register offset (`$D001,Y`) of the object sprite the multiplexer moves down by 21 lines (`$8670`) [O1, W]
    pub mux_slot: u16,
    /// `$31`: joystick directions pressed (bit 0 up .. 3 right), bit 7 = up already used for a jump (`$8F95`) [P]
    pub joy: u16,
    /// `$32`: fire (`$10` pressed) (`$8F95`) [P]
    pub fire: u16,
    /// `$33`: walking speed counter (0..31 on land, 0..24 swimming) [P]
    pub speed: u16,
    /// `$34`: index into the jump/fall or swim table [P]
    pub vert_step: u16,
    /// `$35`: index into the spring table [P]
    pub bounce_step: u16,
    /// `$36`: pixels moved horizontally this frame (`speed_table[$33]`) [P]
    pub dx: u16,
    /// `$37`: facing: `0` right, `1` left [P]
    pub facing: u16,
    /// `$38`: frames until the next animation step (`$2B6E`) [P]
    pub anim_timer: u16,
    /// `$39`: 12 tile codes around Giana from the screen (`$8E9F`): `$39..$3B`, `$42..$44`, `$3C..$3E`, `$3F..$41` are
    /// the rows from top to bottom, 3 columns each [P]
    pub probe_tiles: u16,
    /// `$3F/$40` old letter, `$42/$43` new letter, `$44` scroll direction, `$45` row of the high-score name entry's big letter (`$5103`) [F]
    pub entry_letter: u16,
    /// `$45`: the 12 tile properties of `probe_tiles` (`$BE00[tile]`, same order) [P]
    pub probe_props: u16,
    /// `$46`: colour index of the high-score screen's flashing background, `$47` its delay [F]
    pub hiscore_flash: u16,
    /// `$48`: cursor position in the high-score name (0..11) [F]
    pub entry_cursor: u16,
    /// `$49`: rank of the record in a high-score table (`$4FED`; `$4A` the copy counter) [F]
    pub entry_rank: u16,
    /// `$4B`: joystick repeat delay of the name entry [F]
    pub entry_delay: u16,
    /// `$51`: slot being filled by the spawn (`$73E8`) [O1]
    pub spawn_slot: u16,
    /// `$52`: the enabled bits shifted through by the update loop (`$7613`) [O1]
    pub update_bits: u16,
    /// `$53`: nine tile properties around an object (`$7B11`): `$53..$55` top row left/mid/right, `$56..$58` middle, `$59..$5B` bottom; `$57` = `$11` is a deadly tile [O1, O2]
    pub obj_probe_props: u16,
    /// `$5C`: pixels an object moves horizontally (`$79F8`) [O1, O2]
    pub obj_dx: u16,
    /// `$5D`: direction for `$79F8` (non-zero = left) [O1, O2]
    pub obj_dir_tmp: u16,
    /// `$5E`: the frame limit of an animation step (`$7A74`) [O1]
    pub anim_limit: u16,
    /// `$5F`: a jumper's rise of this frame from the jump table (`$7747`) [O2]
    pub jump_delta: u16,
    /// `$60`: the contact handler's object bits (`$D01E & $7A & $F8`), shifted left per slot by `$7E04` [O3]
    pub contact_bits: u16,
    /// `$61`: non-zero = the object multiplexer runs this frame (an IRQ at line `$63`, `$31DE`; set by `$8521`) [O1, W]
    pub mux_first: u16,
    /// `$62`: the register offset of the second sprite the multiplexer checks against line `$63` (`$318C`) [O1, W]
    pub mux_other: u16,
    /// `$63`: raster line of the next multiplexer IRQ (`$3183`); `$64` is its start value for the next frame [O1, W]
    pub mux_line: u16,
    /// `$64`: the multiplexer's first line, copied to `$63` when the last multiplexer IRQ of a frame hands over to `$FB` [O1, W]
    pub mux_line_start: u16,
    /// `$65`: x high bit of the shot (sprite 3) for `$7E6D` [O3]
    pub contact_shot_msb: u16,
    /// `$66`: x high bit of the object being compared (`$7E04`, `$7E6D`, `$7DBE`, `$8146`) [O3]
    pub contact_msb: u16,
    /// `$67`: fine scroll of the playfield (`$D016` low bits) [S]
    pub fine_scroll: u16,
    /// `$68`: fine scroll of the cloud band, 0..15 (`$D016` gets `$68 >> 1`; bit 4 of that = multicolour text) [S]
    pub sky_scroll: u16,
    /// `$69`: `$3991` animations not yet past their third frame [S, B]
    pub anim_3991_count: u16,
    /// `$6A`: low byte of the screen address where the last `$3991` block animation started [S, B]
    pub anim_3991_last: u16,
    /// `$6B`: running `$3B3E` animations [S, B]
    pub anim_3b3e_count: u16,
    /// `$6C`: low byte of the screen address where the last `$3B3E` block animation started [S, B]
    pub anim_3b3e_last: u16,
    /// `$6D`: object bits shifted left per slot (`$7E6D` shot targets, `$7DBE`, `$8146`) [O3]
    pub contact_mask: u16,
    /// `$6E`: the object Giana rode last frame: it does not turn once (`$7309`, `$79D0`) [O1]
    pub ride_slot: u16,
    /// `$70`: x of Giana's sprites, low byte (sprite 0..2 share it) [P]
    pub player_x: u16,
    /// `$71`: x of Giana's sprites 0..2 as drawn (`$70` plus the swim offset, `$3313`) [P, W]
    pub player_sprite_x: u16,
    /// `$72`: x high bits for `$D010` as drawn (`$88`, flipped by the swim offset, `$3313`) [P, W]
    pub player_sprite_msb: u16,
    /// `$73`: x (low byte) of the object sprites 3..7, `$73..$77` (`$3340`) [O1, W]
    pub object_sprite_x: u16,
    /// `$78`: y of Giana's sprite 0 [P]
    pub player_y: u16,
    /// `$79`: y of Giana's sprite 0 as drawn (copy of `$78`, `$3313`; sprites 1, 2 at +2, +1) [P, W]
    pub player_sprite_y: u16,
    /// `$7A`: sprites switched on (`$D015`, written at `$FB`; bits 0..2 Giana, 3..7 objects) [O1, W]
    pub sprite_enable: u16,
    /// `$7B`: y of the object sprites 3..7, `$7B..$7F` (`$3345`) [O1, W]
    pub object_sprite_y: u16,
    /// `$80`: sprite frames (pointers) of Giana's layers: `$80` sprite 0, `$81` sprite 1, `$82` sprite 2 (`$2BD8`) [P, W]
    pub player_frames: u16,
    /// `$83`: shape pointers of the object sprites 3..7, `$83..$87`, copied to `$CBFB..$CBFF` (`$342A`) [O1, W]
    pub object_frames: u16,
    /// `$88`: x high bits of the sprites; bit 0 = Giana [P]
    pub sprite_x_msb: u16,
    /// `$89`: x (low byte) of the three bump sprites `$89..$8B`, one per `$389D` slot (`$3E0F`) [B, W]
    pub bump_x: u16,
    /// `$8C`: y of the three bump sprites `$8C..$8E` (`$3DF5`) [B, W]
    pub bump_y: u16,
    /// `$8F`: screen row of the probe (`$8E9F`) [P]
    pub probe_row: u16,
    /// `$90`: x high bits of the bump sprites, bit n = slot n (`$3E0A`) [B, W]
    pub bump_msb: u16,
    /// `$91`: frames left of the three bump sprites `$91..$93` (`$3E13`) [B, W]
    pub bump_timer: u16,
    /// `$94`: non-zero = the IRQ splits the sprites this frame (set by a bump, `$3DDB`; `$353A`) [B, W]
    pub sprite_split: u16,
    /// `$95`: non-zero stops channel 3 of the tile animation (set elsewhere; role not yet known) [S]
    pub state_95: u16,
    /// `$96`: pause menu: `$FF` = draw the menu, positive = draw the status bar again (`$2040`, `$20C6`) [F]
    pub pause_state: u16,
    /// `$97`: the pause menu's cursor row (`$FF` = none) [F]
    pub menu_cursor: u16,
    /// `$98`: stage end flag: non-zero = the stage is over (`1` on the end block `$3131`, `$FF` from `$3F87`; read by the
    /// main loop, the lift `$81C3` and the hidden key `$7279`) [B, O1, F]
    pub stage_end: u16,
    /// `$99`: non-zero while Giana is dying (`$3F26`) or the time ran out (`$3EBE`) [P]
    pub dying: u16,
    /// `$9A`: pointer to the stage's spawn list (3-byte entries, `$FF` end) [O1]
    pub spawn_list: u16,
    /// `$9C`: extra columns the object probe scans (`$7B11`) [O1]
    pub obj_probe_width: u16,
    /// `$9D`: the KERNAL's message flag, 0 in the intro [F]
    pub intro_mode: u16,
    /// `$9E`: pointer `$9E/$9F` to the characters of the tile shimmer's second part (`$33D0`) [W]
    pub shimmer_ptr: u16,
    /// `$A0`: tile animation pointers: `$A0/$A1` the channel's frame table, `$A2/$A3` the frame (`$3725`) [S]
    pub tile_anim_ptr: u16,
    /// `$B4`: probe row pointer 1 (`$C0`, `$0E` and `$E4` hold rows 2..4; `$0E` is scratch) [P]
    pub probe_ptr1: u16,
    /// `$C0`: probe row pointer 2 [P]
    pub probe_ptr2: u16,
    /// `$C5`: free-running frame counter, incremented every frame; the game looks only at its low bits (`& 7` for the
    /// flashing colours `$4C5B`, `& 31` for the time's blinking `$3103`) [B, O1]
    pub frame_counter: u16,
    /// `$C6`: free-running frame counter 0..3 (sub-pixel pattern at full speed) [P]
    pub frame4: u16,
    /// `$C8`: `$FF` in a treasure room (no death below the screen, no scroll) [P]
    pub in_room: u16,
    /// `$C9`: frames to the next second of the time, 40..0 (`$3E92`; held by the lift `$8188`) [B, O1]
    pub time_frames: u16,
    /// `$CA`: the time, BCD, `$CA` low and `$CB` high byte (`$3E92`) [B]
    pub time: u16,
    /// `$CB`: the KERNAL's key held (matrix code, `$40` none; `$C5` the last one) [F]
    pub kernal_key: u16,
    /// `$CC`: the shot's horizontal direction (0 = right, else left; from `$37`) [O3]
    pub shot_dir: u16,
    /// `$CD`: the shot's vertical direction (0 = down, 1 = up; homing: 0 = up, `$FF` = down) [O3]
    pub shot_vdir: u16,
    /// `$CE`: frames the shot still rises after a bounce [O3]
    pub shot_vstep: u16,
    /// `$CF`: the shot bounced (power level >= 3) and can be caught again [O3]
    pub shot_bounce: u16,
    /// `$D0`: the slot the homing shot (power level >= 4) flies at, 0 = none [O3]
    pub shot_target: u16,
    /// `$D1`: homing shot: horizontal step part (`$805A`) [O3]
    pub shot_dx: u16,
    /// `$D2`: homing shot: vertical step (`$805A`) [O3]
    pub shot_dy: u16,
    /// `$D3`: homing shot: half the vertical distance to the target [O3]
    pub shot_dist: u16,
    /// `$D4`: Giana's x / 2 + 7, the target of a script object that follows her (`$84B3`) [O2]; the homing shot's target x / 2 (`$8085`) [O3]
    pub follow_x: u16,
    /// `$D5`: sound effect request (`$FF` = none; the IRQ starts it) [P]
    pub effect: u16,
    /// `$D6`: how deep the feet may be in a floor to land this frame (3 or 4) [P]
    pub land_depth: u16,
    /// `$D7`: `$D011` value of the frame (screen on/off, vertical scroll), written at `$FB` (`$3305`) [W, F]
    pub screen_mode: u16,
    /// `$D8`: tile shimmer: bit 7 toggles each frame, bits 0..6 = the shimmer frame 1..9 (`$3387`) [W]
    pub shimmer_phase: u16,
    /// `$D9`: state a new `$3991` animation starts with (`$394E`) [S, B]
    pub fall_start_state: u16,
    /// `$DA`: low byte of the screen address where the last `$389D` block animation started [S, B]
    pub anim_389d_last: u16,
    /// `$DB`: screen column of the probe (`$8E9F`) [P]
    pub probe_col: u16,
    /// `$DC`: colour of Giana's first sprite layer (`$D027`, `$356B`) [P, W]
    pub layer0_colour: u16,
    /// `$DD`: colour of Giana's second sprite layer (0 in the water) [P]
    pub sprite_color: u16,
    /// `$DE..$E0`: the mirrored row of the left-facing sprite shapes (`$36CA`) [F]
    pub mirror_bytes: u16,
    /// `$DF`: the pause menu's last row (2 or 3: "restart here" only away from the restart column, `$2072`) [F]
    pub menu_last: u16,
    /// `$E0`: slot of the script object that spawns another one (`$84D3`) [O2]
    pub script_slot: u16,
    /// `$E1`: screen columns of the three clouds of the cloud band (`$E1..$E3`, right ends) [S]
    pub cloud_columns: u16,
    /// `$E4`: probe row pointer 4 [P]
    pub probe_ptr4: u16,
    /// `$E6`: object probe row pointer 2 [O1]
    pub obj_probe_ptr2: u16,
    /// `$E8`: object probe row pointer 3 [O1]
    pub obj_probe_ptr3: u16,
    /// `$EC`: pointer to an object's script (`$022C/$0231`) [O1, O2]
    pub obj_script: u16,
    /// `$EE`: pointer to the descriptor being spawned (`$7349`, `$83ED`) [O1]
    pub descriptor_ptr: u16,
    /// `$F0`: direction (`$0268`) of an object spawned by a script (`$84D3`) [O2]
    pub child_dir: u16,
    /// `$F1`: slot picked for a spawn (`$739A`); a jumper's extra vertical speed, from `$022C` (`$76E7`); the shot's x high difference to a target (`$7E8D`) [O1, O2, O3]
    pub spawn_pick: u16,
    /// `$F2`: x high bit of the script object that spawns another one (`$84D3`) [O2]
    pub child_msb: u16,
    /// `$F3`: leader of the group being spawned; the x of a script object that spawns another one (`$84DB`) [O1, O2]
    pub group_leader_tmp: u16,
    /// `$F4`: members of the group spawned so far (`$FF` = none yet) [O1]
    pub group_count: u16,
    /// `$F5`: scratch of the bump sprites: Giana's y minus 29, the lowest a bump sprite may reach (`$34BB`) [W]
    pub bump_limit: u16,
    /// `$F6`: `$FF` stage start (objects at their columns), `$80` spawned by a script (`$83ED`), else normal [O1]
    pub spawn_mode: u16,
    /// `$F7`: smallest x while picking a slot [O1]
    pub spawn_min_x: u16,
    /// `$F8`: scroll request in pixels for `$2A59` (playfield moves left by this many) [P, S]
    pub scroll_px: u16,
    /// `$F9`: the object Giana stands on, for the column-step frame (`$7301`) [O3, F]
    pub land_slot: u16,
    /// `$FA`: the playfield's fine scroll change of the frame (`$3149`) [F]
    pub fine_delta: u16,
    /// `$FB`: lives, BCD (`$2164`) [P, B]
    pub lives: u16,
    /// `$FC`: stage number (`$65..` in a treasure room) [P, F]
    pub stage: u16,
    /// `$FD`: power-up level (0 plain, 2.. shooting, 7.. immune to some deadly tiles) [P, O3]
    pub power: u16,
    /// `$FE`: diamond counter, BCD (cleared on a death, `$213F`) [P, B]
    pub diamonds: u16,
    /// `$FF`: non-zero = the column step skips the block animations (a stacked item moved, `$3B1E`; cleared at line `$42`) [S]
    pub blocks_skip: u16,
    /// `$0100`: copy of the stage record's 16 header bytes [stage]
    pub stage_header: u16,
    /// `$010A`: `$D022/$D023` of the cloud band and playfield (stage header bytes 10, 11; written at line `$42`) [S, W]
    pub band_colours: u16,
    /// `$0110`: the sprite variables `$70..$88` saved on entering a treasure room (`$41D1`) [B, O3]
    pub room_sprites: u16,
    /// `$0129`: the stage to go back to from a treasure room (`$3F6C`) [P, O3]
    pub room_return: u16,
    /// `$012B`: `$D01B`, `$D01C`, `$D02B..$D02E` saved on entering a treasure room (`$41D1`) [B, O3]
    pub room_vic: u16,
    /// `$0200`: per slot (`$0200 + n`, n = 4..7): `$10` = sprite behind the background [O1]. The same base address is used
    /// once without an index (`$0200` itself, slot 0 of the array, which the object code never uses): the treasure room code
    /// (`$3F79`, `$40ED`) saves the probe column `$DB` there while Giana is inside the room [B, O3]
    pub obj_behind: u16,
    /// `$0201`: `$FF` = the head hit a ceiling this frame (read by the block code) [P, B]
    pub head_bump: u16,
    /// `$0202`: counter of the repeated-hit block (`$3DB6..$3DC5`, `$3EB1`, `$389D`) [S, B]
    pub repeat_hits: u16,
    /// `$0203`: stages started (`$1E5B`; the next-stage key adds one) [F]
    pub stage_counter: u16,
    /// `$0204`: per slot: descriptor byte 1 (bit 0 hi-res, bit 1 behind, bit 7 ping-pong animation) [O1]
    pub obj_flags: u16,
    /// `$0209`: per slot: the 18 descriptor fields, `$0209 + 5 * k` (`objects::Field`) [O1, O2]
    pub obj_record: u16,
    /// `$02B8`: `$02B8 + i` = the group leader (`$02B4 + n`, `group_leader`) of object slot `n = 4 + i`, read by tile animation channel 3 [S, O1]
    /// `$020E`: class byte of the object slots (`$AC` = power-up object, `$7A3A`; `$0213`, `$0218` are two more) [B, O1]
    pub object_class: u16,
    /// `$0222`: state/animation of the object slots (`$12` = pickup) [B, O1]
    pub object_anim: u16,
    /// `$0226`: `$0226 + i` = the kind (`objects::Field::Kind`, `$0222 + n`) of object slot `n = 4 + i`; tile animation channel 3 watches kind `$7F` [S, O1]
    pub object_type: u16,
    /// `$0263`: per slot: animation delay counter [O1]
    pub obj_anim_timer: u16,
    /// `$0268`: per slot: direction (0 = A frames, 1 = B frames) [O1, O2]
    pub obj_dir: u16,
    /// `$026D`: per slot: step index (jump table, script) [O1, O2]
    pub obj_step: u16,
    /// `$0277`: per slot (`$0277 + n`, n = 4..7): sprite colour (bit 7 = flashing; written by the spawn `$7A52`) [B, O1]
    pub obj_colour: u16,
    /// `$027F`: the probe's row-4 pointer `$E4/$E5` saved on entering a treasure room (`$3F7C`) [B, O3]
    pub room_pointer: u16,
    /// `$02A4`: per slot: the members of its group at `$02A4/$02A8/$02AC/$02B0` [O1]
    pub group_members: u16,
    /// `$02B4`: per slot: the leader of its group (0 = none) [O1]
    pub group_leader: u16,
    /// `$02B8`: per slot (`$02B8 + n`, n = 4..7, i.e. `$02BC..$02BF`): saved y (`$FF` after a flip) [O1, O2]. The bytes
    /// `$02B8..$02BB` are also the group leaders of slots 4..7 (`group_leader + n`, read by tile animation channel 3 as
    /// `group_leader + 4 + i`): the same base address, two arrays that overlap by index range
    pub obj_saved_y: u16,
    /// `$02BC`: per slot: animation phase (ping-pong direction) [O1]
    pub anim_phase: u16,
    /// `$02C1`: per-object state (bit 7 = an object whose top kills like the body row) [P, O1]
    pub object_state: u16,
    /// `$02C6`: per slot: the script's first byte [O1, O2]
    pub obj_script_byte: u16,
    /// `$02CB`: per slot: script position [O1, O2]
    pub obj_script_pos: u16,
    /// `$02D3`: `$FF` once the stage-end block was bumped (`$3B9F`; then a bump draws a used block, `$3DCC`) [B, F]
    pub end_bump: u16,
    /// `$02D5`: lifetime of the player's shot (`$7F29`, `$7FA0`); 7 at a death (`$2102`) [P, O3]
    pub shot_timer: u16,
    /// `$02D6`: the game is over after this stage (the last stage was cleared): the stage to go on with, or 0 [O3, F]
    pub pickup_end: u16,
    /// `$02D7`: fine scroll of the title's upper text row (`$02D8` the lower one) [F]
    pub title_scroll: u16,
    /// `$02D9`: the joystick byte of the pause menu's last pass (a move needs a change, `$207A`) [F]
    pub menu_joy: u16,
    /// `$02DA`: status bar redraw: `0` all (`$26EF`), `$FF` the lives too, `1` score, diamonds, time (`$26E8`) [B]
    pub hud_redraw: u16,
    /// `$02DB`: a power-up was taken: the main loop changes Giana's sprites (`$20E1`, `$360A`) [O3, F]
    pub pickup_taken: u16,
    /// `$02DC`: bonus-object switch: `$FF` after the `$81` block was hit (`$3D83`); the power-up object (kind 2, `$766A`) takes
    /// it and clears it, so does the spawn (`$745D`) [B, O1, O2, O3]
    pub bonus_flag: u16,
    /// `$02DD`: index of the next spawn list entry [O1]
    pub spawn_index: u16,
    /// `$02DE`: frames the objects stay frozen (the update loop only counts it down) [O1, O3]
    pub freeze_timer: u16,
    /// `$02DF`: frames left of the power-up level 6 flash that kills the objects (`$72CC`) [O3]
    pub power_flash: u16,
    /// `$02E0`: non-zero = channel 3 of the tile animation counts up (also read by the frozen-logic path `$3118`; role not yet known) [S]
    pub state_02e0: u16,
    /// `$02E1`: which player plays (index into `colour_table`) [P, F]
    pub player_number: u16,
    /// `$02E2`: the two players' save slots, two bytes per value (`$1E72`) [F]
    pub player_slots: u16,
    /// `$02F4`: per player: the game is over (`$02F5` is also set at the start of a one-player game) [F]
    pub game_over_flags: u16,
    /// `$02F8`: `1` = the next stage start keeps the player (stage clear), `0` = the other one may be next [F]
    pub same_player: u16,
    /// `$02F9`: frame counters of the 6 tile animation channels [S]
    pub tile_anim_frames: u16,
    /// `$02FC`: phase of tile animation channel 3 (low 2 bits used) [S]
    pub tile_anim_phase: u16,
    /// `$0310`: screen address of the cell above the probe (row 0, column `$DB`) [P, B]
    pub probe_above: u16,
    /// `$0313`: the stage chosen in the stage select (0 = none) [F]
    pub select_cursor: u16,
    /// `$0314`: the KERNAL's IRQ vector (the high-score screen's handler `$5098`) [F]
    pub kernal_irq_vector: u16,
    /// `$0400`: level buffer, 256 columns x 20 rows [stage]
    pub level: u16,
    /// `$080C`: units of the intro's last delay (`$1052`) [F]
    pub intro_count: u16,
    /// `$0FD8`: the intro's texts of the loading page (`$0FD8`, `$0FEB`, `$0FB6`) [F]
    pub intro_texts: u16,
    /// `$1000`: the intro's line for a PAL machine [F]
    pub intro_pal: u16,
    /// `$1800`: per slot: phase (0 = alive; dying, collected ...) [O1, O2]
    pub obj_phase: u16,
    /// `$180D`: the tile shimmer's source bytes (`$180D` and `$181D`, masked by the moving operands `$33B8`/`$33C1`) [W]
    pub shimmer_source: u16,
    /// `$185E`: raster bar tables of the title (`$185E`, `$1867`, `$1870`) [F]
    pub bars_title: u16,
    /// `$1878`: raster bar tables of the menu IRQ (`$1878`, `$1880`, `$1888`) [F]
    pub bars_menu: u16,
    /// `$18B8`: the 3 x 4 patterns rotated into `$C738..$C74F` by channel 3 [S]
    pub tile_anim_rot: u16,
    /// `$18C4`: column step: shifts the playfield, draws the new column, runs the logic again, then the block animations [S]
    pub scroll_shift: u16,
    /// `$1AD8`: bit patterns of tile animation channel 1 (word per frame, read at `$1AD7 + 2f + 1` and `$1AD8 + 2f + 1`) [S]
    pub tile_anim_bits: u16,
    /// `$1D10`: the game's start (title), also the NMI target [F]
    pub game_start: u16,
    /// `$1E56`: stage start after a lost life (and for the next stage) [P, F]
    pub stage_restart: u16,
    /// `$20F3`: main loop: the death sequence starts (`$99` set) [P]
    pub death_sequence: u16,
    /// `$2114`: main loop: the death jump, one step per frame [P]
    pub death_jump: u16,
    /// `$213D`: main loop: Giana's state after the death jump [P]
    pub death_reset: u16,
    /// `$2164`: main loop: a life less, restart or game over [P]
    pub lose_life: u16,
    /// `$23C9`: low bytes of the text pointers (`$23E1` the high bytes, 24 texts) [F]
    pub text_lo: u16,
    /// `$23E1`: high bytes of the text pointers [F]
    pub text_hi: u16,
    /// `$2423`: "LIVES" of the end-of-game count (`$486B`, to `$CA86`) [F]
    pub end_word_lives: u16,
    /// `$245B`: "BONUS" (5 screen codes) of the end-of-game count (`$47DE`, to `$C8F6`) [F]
    pub end_word_bonus: u16,
    /// `$25F2`: "EXTRA" of the end-of-game count (`$4823`, to `$C9BE`) [F]
    pub end_word_extra: u16,
    /// `$26CE`: low bytes of the item icon characters per power-up level (`$26CF`: of the item sprite shapes) [B]
    pub item_ptr_lo: u16,
    /// `$26D7`: high bytes of the item icon characters per power-up level (`$26D8`: of the item sprite shapes) [B]
    pub item_ptr_hi: u16,
    /// `$26E0`: colour of the item icon per power-up level [B]
    pub item_colours: u16,
    /// `$26E8`: the status bar of a frame (main loop) [B]
    pub hud: u16,
    /// `$27A7`: colours of the time's blinking below 20 seconds (8 phases) [B]
    pub time_blink: u16,
    /// `$2880`: player movement: vertical (jump, fall, spring, swim) and horizontal (walk, brake) [P]
    pub player_move: u16,
    /// `$2A59`: applies `$F8` to the fine scrolls and requests a column step [S]
    pub scroll_apply: u16,
    /// `$2ADD`: walking frames facing left, by animation index (4 bytes) [P]
    pub walk_frames_left: u16,
    /// `$2AE1`: walking frames facing right (4 bytes) [P]
    pub walk_frames_right: u16,
    /// `$2AE5`: swimming frames facing left (4 bytes) [P]
    pub swim_frames_left: u16,
    /// `$2AE9`: swimming frames facing right (4 bytes) [P]
    pub swim_frames_right: u16,
    /// `$2B6E`: Giana's sprite frames for the next picture [P]
    pub frame_select: u16,
    /// `$2BD8`: sets the three frames `$80..$82` from `A` [P]
    pub set_frames: u16,
    /// `$2BEE`: what the probe's tiles do to Giana (water, ceiling, landing, edges, bobbing) [P]
    pub vertical: u16,
    /// `$2D89`: bobbing delays by feet depth [P]
    pub bob_delays: u16,
    /// `$2D91`: walls [P]
    pub walls: u16,
    /// `$2E25`: the title's scrolling texts (`$2E25`, the lower one at `$2E74`) [F]
    pub title_text: u16,
    /// `$2F54`: raster IRQ of the menus and text screens (line `$69`): colour bands, tile animation, music, input [F]
    pub irq_menu: u16,
    /// `$2FC8`: the title's raster IRQ (three phases) [F]
    pub irq_title: u16,
    /// `$2FEF`: operand of the title IRQ's `JMP` to its phase [F]
    pub title_jump_op: u16,
    /// `$309B`: lines (`$309B`) and phase addresses (`$309E`/`$30A1`) of the title IRQ [F]
    pub title_lines: u16,
    /// `$30A4`: the raster IRQ at line `$42`: cloud band `$D016` (cycle exact), `$D022/$D023`, next IRQ `$5A` (up to `$30D7`; `$30D8` runs the block animations) [W, S]
    pub irq_42: u16,
    /// `$30B2`: branch offset of the `$42` IRQ's delay (more sprites on its lines steal more cycles), set at `$3450` [W]
    pub cloud_delay_op: u16,
    /// `$3183`: the object multiplexer's raster IRQ (moves an object sprite 21 lines down) [W]
    pub irq_mux: u16,
    /// `$31DE`: schedules the next multiplexer IRQ at line `$63` (carry set: none, go on to `$FB`) [W]
    pub mux_next: u16,
    /// `$31F4`: the raster IRQ at line `$5A`: playfield `$D016`, Giana's sprites or the split at `y - 4`, the next IRQ (up to `$323D`; `$323E` runs the frame logic) [W]
    pub irq_5a: u16,
    /// `$323E`: the frame logic inside the `$5A` IRQ (`$30FE`, or the column step) [S, W]
    pub frame_logic: u16,
    /// `$3249`: the column step's bookkeeping inside the `$5A` IRQ (up to `$32A6`; `$32A7` calls `scroll_shift`) [S]
    pub scroll_counters: u16,
    /// `$32B4`: the raster IRQ at line `y - 4` that moves Giana's sprites while the bump sprites are shown above her [W]
    pub irq_split: u16,
    /// `$32DC`: the raster IRQ at line `$FB` (up to `$330F`; `$3310` calls the object code with IRQs on) [W]
    pub irq_fb: u16,
    /// `$32E8`: code operand: the line of the next frame's first IRQ (`$41`; rewritten by `$27F1`/`$281F`) [W, F]
    pub irq_fb_line_op: u16,
    /// `$32ED`: code operand: low byte of the next frame's first IRQ vector (`$3488`; `$27F6`/`$2824`) [W, F]
    pub irq_fb_vector_lo_op: u16,
    /// `$32F2`: code operand: high byte of that vector (`$27FB`/`$2829`) [W, F]
    pub irq_fb_vector_hi_op: u16,
    /// `$32F7`: code operand: `$D016` of the status bar (`$17` multicolour, `$07` hi-res text), set by the power-up level (`$270D`) [B, W]
    pub hud_d016_op: u16,
    /// `$32FC`: code operand: `$D022` of the status bar [W]
    pub hud_d022_op: u16,
    /// `$3301`: code operand: `$D023` of the status bar (`$270A`) [B, W]
    pub hud_d023_op: u16,
    /// `$3313`: Giana's sprite position for the picture (inside the IRQ at line `$5A`, up to `$333F`) [P]
    pub sprite_position: u16,
    /// `$3340`: the object sprites' registers at `$FB`, then the bump sprites or Giana's (up to `$3383`) [W]
    pub object_sprites: u16,
    /// `$3387`: the tile shimmer inside the `$FB` IRQ (up to `$3415`) [W]
    pub shimmer: u16,
    /// `$33B8`: code operand (address `$33B8/$33B9`) of the shimmer's first mask, moved by one each other frame [W]
    pub shimmer_op1: u16,
    /// `$33C1`: code operand (address `$33C1/$33C2`) of the shimmer's second mask [W]
    pub shimmer_op2: u16,
    /// `$3416`: the `$FB` IRQ after the shimmer: sound effect request, object pointers, the `$42` delay (up to `$3452`) [W]
    pub fb_tail: u16,
    /// `$342A`: object sprite pointers and the `$42` IRQ's delay (up to `$3452`) [W]
    pub object_pointers: u16,
    /// `$345F`: the `$42` IRQ's branch offsets per mask of object sprites 4..7 on lines `$2D..$41` (16 bytes) [W]
    pub cloud_delays: u16,
    /// `$346F`: the shimmer's second part: first entry of this frame (5 or 2) [W]
    pub shimmer_cursor: u16,
    /// `$3470`: shimmer entries: low bytes of the character pointer `$9E` (6 bytes); `$3476`, `$347C`: the two XOR masks [W]
    pub shimmer_lo: u16,
    /// `$3482`: shimmer entries: row (bits 0..6) and direction (bit 7) (6 bytes) [W]
    pub shimmer_state: u16,
    /// `$3488`: the raster IRQ at line `$41`: next IRQ at `$42`, `CLI` and a NOP slide (the `$42` IRQ lands in it) [W]
    pub irq_41: u16,
    /// `$34B1`: sprites 0..2 as the bump sprites (`$89..$93`, arc `$3582`); when all are done `$3538` clears `$94` and goes on to `$353C` [W]
    pub bump_sprites: u16,
    /// `$353C`: Giana's sprite registers: y, x, pointers `$80..$82`, colours, multicolour bit [W]
    pub giana_sprites: u16,
    /// `$3543`: code operand: y offset of sprite 2 from sprite 0 (`ADC #`; the one of sprite 1 is at `$3548`, added on top; rewritten at the stage start, `$3640`/`$36C4`) [W]
    pub layer_offset_ops: u16,
    /// `$3582`: rise of a bump sprite per frame left (24 bytes) [W]
    pub bump_arc: u16,
    /// `$3608`: colour of the second sprite layer per player (`$3608` itself is rewritten at the stage start, `$2863`) [P]
    pub colour_table: u16,
    /// `$3705`: non-zero = automatic scroll (stage end walk): the player routine only requests 3 pixels [P]
    pub auto_scroll: u16,
    /// `$3725`: the animated tiles, one channel per frame [S]
    pub tile_anim: u16,
    /// `$3729`: branch opcode of the tile animation's cursor wrap, rewritten per stage (`$10` `BPL`: wrap below 0; `$D0` `BNE`: wrap at 0) [S]
    pub tile_anim_wrap: u16,
    /// `$372C`: operand of `LDX #`: the channel the tile animation's cursor wraps to (5; 6 in stage 12) [S]
    pub tile_anim_last: u16,
    /// `$389D`: the bumped-block animation [S]
    pub anim_389d: u16,
    /// `$38F8`: the breaking-block animation [S]
    pub anim_38f8: u16,
    /// `$3991`: the falling-item animation [S]
    pub anim_3991: u16,
    /// `$3B3E`: the dissolving-ground animation [S]
    pub anim_3b3e: u16,
    /// `$3B80`: the block code of a frame (stage end, doors, dissolving ground, then `$3C1B`) [B]
    pub block_hits: u16,
    /// `$3C1B`: diamonds at Giana's sides and blocks hit from below [B]
    pub pickups: u16,
    /// `$3E92`: one frame of the time [B]
    pub time_tick: u16,
    /// `$3EC1`: deadly tiles [P]
    pub hazards: u16,
    /// `$3F26`: Giana dies (entered by `JMP` from the movement when she falls below the screen) [P]
    pub player_die: u16,
    /// `$3F28`: second entry of `$3F26` with `A` = the value for `$99`/`$20` (`$FE` from the pause menu) [P]
    pub player_die_with: u16,
    /// `$3FA1`: the treasure room's main-loop part [O3]
    pub room_logic: u16,
    /// `$3FBC`: `$C8 = $80` while the room is built (`rooms::room_mark`) [O3]
    pub room_mark: u16,
    /// `$3FC3`: the next room by the counter `$CFFF` (`rooms::room_select`) [O3]
    pub room_select: u16,
    /// `$3FDC`: fine scroll 7, sprites off (`rooms::room_hide`) [O3]
    pub room_hide: u16,
    /// `$3FEC`: the lift and Giana's sprites in the room (`rooms::room_place`) [O3]
    pub room_place: u16,
    /// `$4054`: x high bits of the lift sprites 5..7 (`$80`, `$C0`, `$E0`) [O3]
    pub room_lift_msb: u16,
    /// `$4057`: Giana on the lift starts the way back (`rooms::room_exit_check`) [O3]
    pub room_exit_check: u16,
    /// `$40A1`: the lift rises (`rooms::room_rise`) [O3]
    pub room_rise: u16,
    /// `$40C9`: the stage's fine scroll back (`rooms::room_scroll_back`) [O3]
    pub room_scroll_back: u16,
    /// `$40DD`: the door is closed (`rooms::room_door`) [O3]
    pub room_door: u16,
    /// `$412C`: sprites, VIC and input state back in the stage (`rooms::room_back`) [O3]
    pub room_back: u16,
    /// `$414C`: input on, `$C8 = 0` (`rooms::room_done`) [O3]
    pub room_done: u16,
    /// `$41C7`: `$14..$1F` cleared (`rooms::clear_motion`; also from `$35A1`) [O3, F]
    pub clear_motion: u16,
    /// `$4296`: the script a class-3 object runs once it is hit (phase 5, `$7852`) [O2]
    pub hit_script: u16,
    /// `$42A8`: word pointers to the 21-byte object descriptors, by type [O1]
    pub descriptor_table: u16,
    /// `$449F`: `$FF` in stage 13 (written by the stage setup `$4A5B`) [F]
    pub boss_flag: u16,
    /// `$44E5`: 6 when the header's colour nibble is 0, else 0 (`$4AE7`) [F]
    pub palette_patch: u16,
    /// `$49A2`: the 18 characters of the stage select's line (`$4947`, to `$C9EB`) [F]
    pub select_text: u16,
    /// `$49EA`: tile colours (`$49EA`) and properties (`$49FA`) of tiles `$D0..$D3` per stage group; `$49B4/$49B6` stage numbers with their `$BF8F` colour [F]
    pub stage_tiles: u16,
    /// `$4C19`: y of a spawned object by the spawn entry's parameter [O1]
    pub spawn_y_table: u16,
    /// `$4C1B`: sprite y of each screen row (the bump sprites, `$3DF2`) [B]
    pub row_sprite_y: u16,
    /// `$4C36`: parameter of the spawn entry being spawned; the y of a script object that spawns another one (`$84E6`) [O1, O2]
    pub spawn_param: u16,
    /// `$4C37`: `1 << n` for n = 0..7 (table at `$4C37`, `ORA`ed in to set a bit) [B, O1]
    pub bit_set: u16,
    /// `$4C3F`: `!(1 << n)` for n = 0..7 (`AND`ed in to clear a bit) [B, O1]
    pub bit_clear: u16,
    /// `$4C47`: background colours of the power-up flash (`$4C47` gets the old `$D021` first) [O3]
    pub power_colours: u16,
    /// `$4C4C`: homing shot: step size by distance / 16 (`$80C2`) [O3]
    pub shot_steps: u16,
    /// `$4C5B`: flashing colours by `$C5` [O1]
    pub flash_colours: u16,
    /// `$4C81`: the map record of the high-score screen [F]
    pub hiscore_map: u16,
    /// `$4D8C`: the 88 + 16 characters the font switch `$4D40` swaps in [F]
    pub charset_save: u16,
    /// `$4E51`: keys of the name entry (`$4E51`) and their characters (`$4E5C`) [F]
    pub entry_keys: u16,
    /// `$508B`: lines, bar tables, flash offsets (`$5094`) and `$D022` colours (`$5096`) of the high-score IRQ [F]
    pub hiscore_lines: u16,
    /// `$5098`: the high-score screen's IRQ (through `$0314`) [F]
    pub irq_hiscore: u16,
    /// `$5133`: the high-score screen's bottom texts (`$5133`, `$5153`) [F]
    pub hiscore_prompt: u16,
    /// `$516F`: the 11 characters of the save screen (`$4CCD`, to `$C9C6`) [F]
    pub saving_text: u16,
    /// `$5192`: start of the high-score file data (150 bytes; the second table) [F]
    pub score_file: u16,
    /// `$520F`: the highest stage reached (part of the high-score file), the stage select needs 2 [F]
    pub highest_stage: u16,
    /// `$5210`: a saved game exists (the title offers to go on with it) [F]
    pub saved_game: u16,
    /// `$523E`: the first high-score table [F]
    pub score_table: u16,
    /// `$52BB`: offsets of the five entries in a high-score table [F]
    pub entry_offsets: u16,
    /// `$52FE`: characters swapped with `$C550..$C5BF` by header bit 4 (`$4B42`) [F]
    pub swap_b_store: u16,
    /// `$536E`: characters swapped with `$C5C0..$C66F` by header bit 7 (`$4B7B`) [F]
    pub swap_c_store: u16,
    /// `$541E`: characters swapped with `$C6D0..$C71F` by header bit 5 (`$4BB0`) [F]
    pub swap_d_store: u16,
    /// `$546F`: 32 bytes swapped with `$C3F8` by header bit 6 (`$4B07`) [F]
    pub swap_a_store: u16,
    /// `$5609`: music driver: initialise [music]
    pub music_init: u16,
    /// `$5650`: music driver: one tick [music]
    pub music_tick: u16,
    /// `$5C35`: music driver: start effect `Y` [music]
    pub music_start_effect: u16,
    /// `$5C93`: music driver: start song `Y` unless it plays [music]
    pub music_start_song: u16,
    /// `$5C9B`: music driver: start song `Y` unconditionally [music]
    pub music_restart_song: u16,
    /// `$72CC`: one frame of the power-up level 6 flash (`pickups::power_flash`) [O3]
    pub flash_step: u16,
    /// `$7349`: spawn by scroll column [O1]
    pub spawn: u16,
    /// `$73E8`: one object from the descriptor `$EE` into slot `X` [O1]
    pub instantiate: u16,
    /// `$750F`: operand of `STA $02A4,Y` in the spawn, rewritten per group member [O1]
    pub group_link_operand: u16,
    /// `$7527`: objects that left the screen [O1]
    pub despawn: u16,
    /// `$75D6`: frees a group's members (`Y` = leader) [O1]
    pub release_group: u16,
    /// `$75F4`: object sprites off, group links cleared [O1]
    pub clear_groups: u16,
    /// `$7613`: per-frame object update loop [O1]
    pub update_objects: u16,
    /// `$7622`: the update loop's next slot (where the class code returns) [O1]
    pub update_next: u16,
    /// `$7822`: after the class code: flashing, deadly tile [O1]
    pub update_tail: u16,
    /// `$79D0`: object turns around [O1]
    pub obj_turn: u16,
    /// `$79F8`: object moves by `$5C` in direction `$5D` (`$79FA`: direction in `A`) [O1]
    pub obj_move: u16,
    /// `$7A17`: object state flips [O1]
    pub obj_flip: u16,
    /// `$7A3A`: power-up frames and colour by the power level [O1]
    pub powerup_colour: u16,
    /// `$7A6C`: sprite colour of the power-up object by power level (bit 7 = hi-res) [B, O1, O3]
    pub powerup_colours: u16,
    /// `$7A74`: one animation step [O1]
    pub obj_animate: u16,
    /// `$7B08`: `$53..$5B` = `A` (`$7B06`: = 0) [O1]
    pub obj_probe_clear: u16,
    /// `$7B11`: tile probe around an object [O1]
    pub obj_probe: u16,
    /// `$7B19`: operand of `SBC #` in the object probe: y offset (`$33`, rewritten by the spawn and classes) [O1]
    pub probe_y_offset: u16,
    /// `$7B36`: operand of `SBC #` in the object probe: x offset (`$15`, `$11`, `$17`) [O1]
    pub probe_x_offset: u16,
    /// `$7CB7`: Giana against the objects (reads `$D01E`) [O3]
    pub contact: u16,
    /// `$7D8F`: an object destroyed [O3]
    pub obj_smash: u16,
    /// `$7D9E`: an object killed on a deadly tile [O3]
    pub obj_hit: u16,
    /// `$7DBE`: contacts in a column-step frame (reads `$D01E`) [O3]
    pub step_contact: u16,
    /// `$7E04`: the next object touching Giana (`pickups::touching`; also from `$E597`, `$E5E6`) [O3]
    pub touching: u16,
    /// `$7E6D`: the object the shot touches (`pickups::shot_target`) [O3]
    pub shot_contact: u16,
    /// `$7F11`: Giana's shot [O3]
    pub shot_fire: u16,
    /// `$7F94`: the shot's movement [O3]
    pub shot_move: u16,
    /// `$8146`: the homing shot's target (`pickups::shot_aim`) [O3]
    pub shot_aim: u16,
    /// `$8188`: the lift at the stage end carries Giana down [O1]
    pub lift: u16,
    /// `$81C6`: Giana stands on an object [O1]
    pub ride: u16,
    /// `$8277`: `$1F` = Giana over object `X` [O1]
    pub near: u16,
    /// `$828C`: Giana meets a platform object (`pickups::land`) [O3]
    pub land: u16,
    /// `$8521`: the members of object groups placed after their leaders (leaves `X` = 3) [O4]
    pub group_place: u16,
    /// `$873B`: colours of the tile sets (`$873B`, `$8744`, `$874D` by `$0101`) [F]
    pub tileset_colours: u16,
    /// `$8757`: word pointers to the map shapes [stage]
    pub shape_table: u16,
    /// `$897A`: the 5-character cloud, top row (`$897F`: bottom row) [S]
    pub cloud_shape_top: u16,
    /// `$897F`: the 5-character cloud, bottom row [S]
    pub cloud_shape_bottom: u16,
    /// `$8BDB`: adds the BCD amount in `A` to the score (`$8BD9`: 50 points) [B]
    pub score_add: u16,
    /// `$8C03`: one life more [B]
    pub extra_life: u16,
    /// `$8D1B`: horizontal pixels per frame, indexed by the speed counter (+ `$C6` at full speed) [P]
    pub speed_table: u16,
    /// `$8D1F`: swim table (vertical pixels per frame while swimming, index `$34`, 26 entries) [P]
    pub swim_table: u16,
    /// `$8D5C`: jump table (rise per frame, index `$34` 0..27; read backwards for the fall) [P]
    pub jump_table: u16,
    /// `$8D69`: steps of the death jump (up: `$8D6A + i`, down: `$8D69 + i`) [P]
    pub death_table: u16,
    /// `$8D77`: spring table (index `$35` 0..7) [P]
    pub spring_table: u16,
    /// `$8D80`: states of the 8 `$3991` slots (0 free) [S, B]
    pub anim_3991_state: u16,
    /// `$8D88`: screen address low bytes of the 8 `$3991` animation slots [S, B]
    pub anim_3991_lo: u16,
    /// `$8D90`: screen address high bytes of the 8 `$3991` animation slots [S, B]
    pub anim_3991_hi: u16,
    /// `$8D98`: states of the 16 `$3B3E` slots (0 free) [S, B]
    pub anim_3b3e_state: u16,
    /// `$8DA8`: screen address low bytes of the 16 `$3B3E` animation slots [S, B]
    pub anim_3b3e_lo: u16,
    /// `$8DB8`: screen address high bytes of the 16 `$3B3E` animation slots [S, B]
    pub anim_3b3e_hi: u16,
    /// `$8DC8`: screen address low bytes of the 4 `$38F8` animation slots [S, B]
    pub anim_38f8_lo: u16,
    /// `$8DCC`: screen address high bytes of the 4 `$38F8` animation slots [S, B]
    pub anim_38f8_hi: u16,
    /// `$8DD0`: states of the 4 `$38F8` slots (0 free) [S, B]
    pub anim_38f8_state: u16,
    /// `$8DD4`: states of the 3 `$389D` slots (0 free) [S, B]
    pub anim_389d_state: u16,
    /// `$8DD8`: screen address low bytes of the 3 `$389D` animation slots [S, B]
    pub anim_389d_lo: u16,
    /// `$8DDB`: screen address high bytes of the 3 `$389D` animation slots [S, B]
    pub anim_389d_hi: u16,
    /// `$8DDE`: kind of each `$389D` block (0 = turns into a used block) [S, B]
    pub anim_389d_kind: u16,
    /// `$8DE2`: low bytes of the screen row addresses (`$8E00`: high bytes), indexed by screen row [P]
    pub row_lo: u16,
    /// `$8E00`: high bytes of the screen row addresses [P]
    pub row_hi: u16,
    /// `$8E1C`: colours of the bumped block per state [S]
    pub anim_389d_colours: u16,
    /// `$8E24`: channel cursor of the tile animation (5 down to 0) [S]
    pub tile_anim_cursor: u16,
    /// `$8E25`: last frame number of each tile animation channel [S]
    pub tile_anim_lengths: u16,
    /// `$8E2C`: low bytes of each channel's frame table address [S]
    pub tile_anim_tables_lo: u16,
    /// `$8E33`: high bytes of each channel's frame table address [S]
    pub tile_anim_tables_hi: u16,
    /// `$8E70`: second frame table of the unreachable channel 6 [S]
    pub tile_anim_alt: u16,
    /// `$8E87`: first character of the top row of each animation frame (`$396C`) [S]
    pub anim_frames_top: u16,
    /// `$8E8D`: first character of the bottom row of each animation frame [S]
    pub anim_frames_bottom: u16,
    /// `$8E9F`: the tile probe around Giana [P]
    pub probe: u16,
    /// `$8F95`: joystick read; its first byte doubles as a switch (`$A9` = input live, `$60` = `RTS`, input off) [P]
    pub read_input: u16,
    /// `$8FB0`: word pointers of the special rooms (stage numbers >= 100) [stage]
    pub room_table: u16,
    /// `$8FBC`: word pointers of the 16 stage records [stage]
    pub stage_table: u16,
    /// `$B410`: the item icon without a power-up (48 bytes, `$8F8A`) [F]
    pub item_icon_none: u16,
    /// `$B5C0`: Giana's sprite shapes of the other power level, swapped in by `$360A` [F]
    pub power_shapes: u16,
    /// `$BE00`: property of each tile code (bit 0 solid, bit 1 .., bit 3 ..: read by the collision code) [P, B]
    pub tile_props: u16,
    /// `$BF00`: colour RAM value of each tile code (the column step draws with it) [S]
    pub tile_colours: u16,
    /// `$C000`: first byte of the character set: `0` = game font, else the inverted menu font (`$4D2E`, `$4D37`) [F]
    pub charset: u16,
    /// `$C0D8`: character animated by tile channel 5 [S]
    pub tile_anim_chars_5: u16,
    /// `$C234`: characters animated by tile channel 2 (`$C234..$C25B`) [S]
    pub tile_anim_chars_2: u16,
    /// `$C3F8`: see `swap_a_store` [F]
    pub swap_a_live: u16,
    /// `$C4D3`: the sparkle bytes `$C4D3..$C4DC` toggled by tile channel 5 [S]
    pub tile_anim_sparkle: u16,
    /// `$C4E0`: the shimmering characters (16 bytes, `$33BA`) [W]
    pub shimmer_chars: u16,
    /// `$C530`: characters built by tile channel 1 (`$C530..$C54F`) [S]
    pub tile_anim_chars_1: u16,
    /// `$C54F`: see `swap_b_store` (`$C54F + 1..$70`) [F]
    pub swap_b_live: u16,
    /// `$C5BF`: see `swap_c_store` [F]
    pub swap_c_live: u16,
    /// `$C680`: characters animated by tile channel 0 (24 bytes) [S]
    pub tile_anim_chars_0: u16,
    /// `$C698`: 8 character bytes set by the stage (`$AA` in stages 5 and 10, else `$FF`; `$C6C8`, `$C7C0` by `$010A`) [F]
    pub water_chars: u16,
    /// `$C6CF`: see `swap_d_store` [F]
    pub swap_d_live: u16,
    /// `$C738`: characters rotated by tile channel 3 (`$C738..$C74F`) [S]
    pub tile_anim_rot_chars: u16,
    /// `$C760`: characters animated by tile channel 4 (48 bytes) [S]
    pub tile_anim_chars_4: u16,
    /// `$C793`: second copy of the middle part of tile channel 2 (5 bytes) [S]
    pub tile_anim_chars_2b: u16,
    /// `$C800`: screen RAM (40 x 25; rows 0-1 status bar, 2-4 cloud band, 5-24 playfield) [S]
    pub screen: u16,
    /// `$C84A`: the time's four cells in the status bar [B]
    pub hud_time: u16,
    /// `$CBD7`: the name being entered (12 characters, `$2E` = empty) [F]
    pub entry_name: u16,
    /// `$CBE0..$CBFF`: cleared by the title (`$4215`) [F]
    pub hud_slots: u16,
    /// `$CBE8`: the time's four digits, printed past the end of the screen first (`$275E`) [B]
    pub time_digits: u16,
    /// `$CBEC`: round-robin slot cursor of `$389D` [S]
    pub anim_389d_cursor: u16,
    /// `$CBED`: round-robin slot cursor of `$3991` [S]
    pub anim_3991_cursor: u16,
    /// `$CBEE`: round-robin slot cursor of `$38F8` [S]
    pub anim_38f8_cursor: u16,
    /// `$CBEF`: round-robin slot cursor of `$3B3E` [S]
    pub anim_3b3e_cursor: u16,
    /// `$CBF0`: the score, 6 BCD digits, low byte first (`$8BDB`) [B]
    pub score: u16,
    /// `$CBF3`: restart column of the stage after a death (0 = the start), chosen by `$3F26` [P, F]
    pub restart_column: u16,
    /// `$CBF4`: rows of the three colour bands of the menu IRQ (`$CBF4..$CBF6`, `$2F5F`) [F]
    pub menu_bands: u16,
    /// `$CBF7`: the cleared stage, counted down by the no-scroll bonus (`$1F84`, `$2298`) [F]
    pub bonus_stage: u16,
    /// `$CBF8`: sprite pointers (screen `$C800` + `$3F8`) [W]
    pub sprite_pointers: u16,
    /// `$CF7F`: tile channels 0-2 wait while non-zero; channel 5 toggles it every other round [S]
    pub tile_anim_flicker: u16,
    /// `$CFFF`: the treasure-room counter `$65..$69` (rooms 101..105, `$3FC3`) [O3]; saved per player and set at the stage
    /// start to `$64 + stage / 4` (`$4915`) [F]
    pub room_counter: u16,
    /// `$EAE9`: every third byte from here gets tile channel 5's frame too (a sprite shape column) [S]
    pub tile_anim_sprite_5: u16,
    /// `$EB0F`: sprite shape of the current item (48 bytes, `$2671`) [B]
    pub item_sprite: u16,
    /// `$ED00`: Giana's sprite shapes (64-byte blocks, byte 63 = flags) [F]
    pub giana_shapes: u16,
    /// `$F540`: the left-facing shapes made from them (`$3676`) [F]
    pub giana_shapes_left: u16,
    /// `$FFFA`: the NMI vector (RESTORE) [F]
    pub nmi_vector: u16,
    /// `$FFFE`: the IRQ vector (RAM under the KERNAL) [W]
    pub irq_vector: u16,
}

/// "Giana 30A Demo 2.0" (CSDb 168405).
pub const PREVIEW: C30aLayout = C30aLayout {
    pointer2: 0x02,
    print_ptr: 0x04,
    text_ptr: 0x06,
    obj_probe_slot: 0x0A,
    obj_probe_col: 0x0B,
    obj_probe_ptr1: 0x0C,
    scratch_ptr: 0x0E,
    colour_ptr: 0x10,
    fall_column: 0x12,
    fall_landed: 0x13,
    scroll_step: 0x14,
    walk_dir: 0x15,
    walk_scroll: 0x16,
    vert_dir: 0x17,
    bounce: 0x18,
    swim: 0x19,
    anim_index: 0x1A,
    braking: 0x1B,
    fall_frames: 0x1C,
    layer_hires: 0x1D,
    anim_389d_count: 0x1E,
    on_object: 0x1F,
    game_state: 0x20,
    scroll_column: 0x21,
    object_index: 0x22,
    feet_depth: 0x23,
    print_col: 0x24,
    bob_timer: 0x26,
    bob_dir: 0x27,
    title_phase: 0x28,
    title_text_index: 0x29,
    bar_table: 0x2A,
    title_text_index2: 0x2C,
    anim_38f8_last: 0x2D,
    anim_38f8_count: 0x2E,
    probe_skip_bottom: 0x2F,
    mux_slot: 0x30,
    joy: 0x31,
    fire: 0x32,
    speed: 0x33,
    vert_step: 0x34,
    bounce_step: 0x35,
    dx: 0x36,
    facing: 0x37,
    anim_timer: 0x38,
    probe_tiles: 0x39,
    entry_letter: 0x3F,
    probe_props: 0x45,
    hiscore_flash: 0x46,
    entry_cursor: 0x48,
    entry_rank: 0x49,
    entry_delay: 0x4B,
    spawn_slot: 0x51,
    update_bits: 0x52,
    obj_probe_props: 0x53,
    obj_dx: 0x5C,
    obj_dir_tmp: 0x5D,
    anim_limit: 0x5E,
    jump_delta: 0x5F,
    contact_bits: 0x60,
    mux_first: 0x61,
    mux_other: 0x62,
    mux_line: 0x63,
    mux_line_start: 0x64,
    contact_shot_msb: 0x65,
    contact_msb: 0x66,
    fine_scroll: 0x67,
    sky_scroll: 0x68,
    anim_3991_count: 0x69,
    anim_3991_last: 0x6A,
    anim_3b3e_count: 0x6B,
    anim_3b3e_last: 0x6C,
    contact_mask: 0x6D,
    ride_slot: 0x6E,
    player_x: 0x70,
    player_sprite_x: 0x71,
    player_sprite_msb: 0x72,
    object_sprite_x: 0x73,
    player_y: 0x78,
    player_sprite_y: 0x79,
    sprite_enable: 0x7A,
    object_sprite_y: 0x7B,
    player_frames: 0x80,
    object_frames: 0x83,
    sprite_x_msb: 0x88,
    bump_x: 0x89,
    bump_y: 0x8C,
    probe_row: 0x8F,
    bump_msb: 0x90,
    bump_timer: 0x91,
    sprite_split: 0x94,
    state_95: 0x95,
    pause_state: 0x96,
    menu_cursor: 0x97,
    stage_end: 0x98,
    dying: 0x99,
    spawn_list: 0x9A,
    obj_probe_width: 0x9C,
    intro_mode: 0x9D,
    shimmer_ptr: 0x9E,
    tile_anim_ptr: 0xA0,
    probe_ptr1: 0xB4,
    probe_ptr2: 0xC0,
    frame_counter: 0xC5,
    frame4: 0xC6,
    in_room: 0xC8,
    time_frames: 0xC9,
    time: 0xCA,
    kernal_key: 0xCB,
    shot_dir: 0xCC,
    shot_vdir: 0xCD,
    shot_vstep: 0xCE,
    shot_bounce: 0xCF,
    shot_target: 0xD0,
    shot_dx: 0xD1,
    shot_dy: 0xD2,
    shot_dist: 0xD3,
    follow_x: 0xD4,
    effect: 0xD5,
    land_depth: 0xD6,
    screen_mode: 0xD7,
    shimmer_phase: 0xD8,
    fall_start_state: 0xD9,
    anim_389d_last: 0xDA,
    probe_col: 0xDB,
    layer0_colour: 0xDC,
    sprite_color: 0xDD,
    mirror_bytes: 0xDE,
    menu_last: 0xDF,
    script_slot: 0xE0,
    cloud_columns: 0xE1,
    probe_ptr4: 0xE4,
    obj_probe_ptr2: 0xE6,
    obj_probe_ptr3: 0xE8,
    obj_script: 0xEC,
    descriptor_ptr: 0xEE,
    child_dir: 0xF0,
    spawn_pick: 0xF1,
    child_msb: 0xF2,
    group_leader_tmp: 0xF3,
    group_count: 0xF4,
    bump_limit: 0xF5,
    spawn_mode: 0xF6,
    spawn_min_x: 0xF7,
    scroll_px: 0xF8,
    land_slot: 0xF9,
    fine_delta: 0xFA,
    lives: 0xFB,
    stage: 0xFC,
    power: 0xFD,
    diamonds: 0xFE,
    blocks_skip: 0xFF,
    stage_header: 0x0100,
    band_colours: 0x010A,
    room_sprites: 0x0110,
    room_return: 0x0129,
    room_vic: 0x012B,
    obj_behind: 0x0200,
    head_bump: 0x0201,
    repeat_hits: 0x0202,
    stage_counter: 0x0203,
    obj_flags: 0x0204,
    obj_record: 0x0209,
    object_class: 0x020E,
    object_anim: 0x0222,
    object_type: 0x0226,
    obj_anim_timer: 0x0263,
    obj_dir: 0x0268,
    obj_step: 0x026D,
    obj_colour: 0x0277,
    room_pointer: 0x027F,
    group_members: 0x02A4,
    group_leader: 0x02B4,
    obj_saved_y: 0x02B8,
    anim_phase: 0x02BC,
    object_state: 0x02C1,
    obj_script_byte: 0x02C6,
    obj_script_pos: 0x02CB,
    end_bump: 0x02D3,
    shot_timer: 0x02D5,
    pickup_end: 0x02D6,
    title_scroll: 0x02D7,
    menu_joy: 0x02D9,
    hud_redraw: 0x02DA,
    pickup_taken: 0x02DB,
    bonus_flag: 0x02DC,
    spawn_index: 0x02DD,
    freeze_timer: 0x02DE,
    power_flash: 0x02DF,
    state_02e0: 0x02E0,
    player_number: 0x02E1,
    player_slots: 0x02E2,
    game_over_flags: 0x02F4,
    same_player: 0x02F8,
    tile_anim_frames: 0x02F9,
    tile_anim_phase: 0x02FC,
    probe_above: 0x0310,
    select_cursor: 0x0313,
    kernal_irq_vector: 0x0314,
    level: 0x0400,
    intro_count: 0x080C,
    intro_texts: 0x0FD8,
    intro_pal: 0x1000,
    obj_phase: 0x1800,
    shimmer_source: 0x180D,
    bars_title: 0x185E,
    bars_menu: 0x1878,
    tile_anim_rot: 0x18B8,
    scroll_shift: 0x18C4,
    tile_anim_bits: 0x1AD8,
    game_start: 0x1D10,
    stage_restart: 0x1E56,
    death_sequence: 0x20F3,
    death_jump: 0x2114,
    death_reset: 0x213D,
    lose_life: 0x2164,
    text_lo: 0x23C9,
    text_hi: 0x23E1,
    end_word_lives: 0x2423,
    end_word_bonus: 0x245B,
    end_word_extra: 0x25F2,
    item_ptr_lo: 0x26CE,
    item_ptr_hi: 0x26D7,
    item_colours: 0x26E0,
    hud: 0x26E8,
    time_blink: 0x27A7,
    player_move: 0x2880,
    scroll_apply: 0x2A59,
    walk_frames_left: 0x2ADD,
    walk_frames_right: 0x2AE1,
    swim_frames_left: 0x2AE5,
    swim_frames_right: 0x2AE9,
    frame_select: 0x2B6E,
    set_frames: 0x2BD8,
    vertical: 0x2BEE,
    bob_delays: 0x2D89,
    walls: 0x2D91,
    title_text: 0x2E25,
    irq_menu: 0x2F54,
    irq_title: 0x2FC8,
    title_jump_op: 0x2FEF,
    title_lines: 0x309B,
    irq_42: 0x30A4,
    cloud_delay_op: 0x30B2,
    irq_mux: 0x3183,
    mux_next: 0x31DE,
    irq_5a: 0x31F4,
    frame_logic: 0x323E,
    scroll_counters: 0x3249,
    irq_split: 0x32B4,
    irq_fb: 0x32DC,
    irq_fb_line_op: 0x32E8,
    irq_fb_vector_lo_op: 0x32ED,
    irq_fb_vector_hi_op: 0x32F2,
    hud_d016_op: 0x32F7,
    hud_d022_op: 0x32FC,
    hud_d023_op: 0x3301,
    sprite_position: 0x3313,
    object_sprites: 0x3340,
    shimmer: 0x3387,
    shimmer_op1: 0x33B8,
    shimmer_op2: 0x33C1,
    fb_tail: 0x3416,
    object_pointers: 0x342A,
    cloud_delays: 0x345F,
    shimmer_cursor: 0x346F,
    shimmer_lo: 0x3470,
    shimmer_state: 0x3482,
    irq_41: 0x3488,
    bump_sprites: 0x34B1,
    giana_sprites: 0x353C,
    layer_offset_ops: 0x3543,
    bump_arc: 0x3582,
    colour_table: 0x3608,
    auto_scroll: 0x3705,
    tile_anim: 0x3725,
    tile_anim_wrap: 0x3729,
    tile_anim_last: 0x372C,
    anim_389d: 0x389D,
    anim_38f8: 0x38F8,
    anim_3991: 0x3991,
    anim_3b3e: 0x3B3E,
    block_hits: 0x3B80,
    pickups: 0x3C1B,
    time_tick: 0x3E92,
    hazards: 0x3EC1,
    player_die: 0x3F26,
    player_die_with: 0x3F28,
    room_logic: 0x3FA1,
    room_mark: 0x3FBC,
    room_select: 0x3FC3,
    room_hide: 0x3FDC,
    room_place: 0x3FEC,
    room_lift_msb: 0x4054,
    room_exit_check: 0x4057,
    room_rise: 0x40A1,
    room_scroll_back: 0x40C9,
    room_door: 0x40DD,
    room_back: 0x412C,
    room_done: 0x414C,
    clear_motion: 0x41C7,
    hit_script: 0x4296,
    descriptor_table: 0x42A8,
    boss_flag: 0x449F,
    palette_patch: 0x44E5,
    select_text: 0x49A2,
    stage_tiles: 0x49EA,
    spawn_y_table: 0x4C19,
    row_sprite_y: 0x4C1B,
    spawn_param: 0x4C36,
    bit_set: 0x4C37,
    bit_clear: 0x4C3F,
    power_colours: 0x4C47,
    shot_steps: 0x4C4C,
    flash_colours: 0x4C5B,
    hiscore_map: 0x4C81,
    charset_save: 0x4D8C,
    entry_keys: 0x4E51,
    hiscore_lines: 0x508B,
    irq_hiscore: 0x5098,
    hiscore_prompt: 0x5133,
    saving_text: 0x516F,
    score_file: 0x5192,
    highest_stage: 0x520F,
    saved_game: 0x5210,
    score_table: 0x523E,
    entry_offsets: 0x52BB,
    swap_b_store: 0x52FE,
    swap_c_store: 0x536E,
    swap_d_store: 0x541E,
    swap_a_store: 0x546F,
    music_init: 0x5609,
    music_tick: 0x5650,
    music_start_effect: 0x5C35,
    music_start_song: 0x5C93,
    music_restart_song: 0x5C9B,
    flash_step: 0x72CC,
    spawn: 0x7349,
    instantiate: 0x73E8,
    group_link_operand: 0x750F,
    despawn: 0x7527,
    release_group: 0x75D6,
    clear_groups: 0x75F4,
    update_objects: 0x7613,
    update_next: 0x7622,
    update_tail: 0x7822,
    obj_turn: 0x79D0,
    obj_move: 0x79F8,
    obj_flip: 0x7A17,
    powerup_colour: 0x7A3A,
    powerup_colours: 0x7A6C,
    obj_animate: 0x7A74,
    obj_probe_clear: 0x7B08,
    obj_probe: 0x7B11,
    probe_y_offset: 0x7B19,
    probe_x_offset: 0x7B36,
    contact: 0x7CB7,
    obj_smash: 0x7D8F,
    obj_hit: 0x7D9E,
    step_contact: 0x7DBE,
    touching: 0x7E04,
    shot_contact: 0x7E6D,
    shot_fire: 0x7F11,
    shot_move: 0x7F94,
    shot_aim: 0x8146,
    lift: 0x8188,
    ride: 0x81C6,
    near: 0x8277,
    land: 0x828C,
    group_place: 0x8521,
    tileset_colours: 0x873B,
    shape_table: 0x8757,
    cloud_shape_top: 0x897A,
    cloud_shape_bottom: 0x897F,
    score_add: 0x8BDB,
    extra_life: 0x8C03,
    speed_table: 0x8D1B,
    swim_table: 0x8D1F,
    jump_table: 0x8D5C,
    death_table: 0x8D69,
    spring_table: 0x8D77,
    anim_3991_state: 0x8D80,
    anim_3991_lo: 0x8D88,
    anim_3991_hi: 0x8D90,
    anim_3b3e_state: 0x8D98,
    anim_3b3e_lo: 0x8DA8,
    anim_3b3e_hi: 0x8DB8,
    anim_38f8_lo: 0x8DC8,
    anim_38f8_hi: 0x8DCC,
    anim_38f8_state: 0x8DD0,
    anim_389d_state: 0x8DD4,
    anim_389d_lo: 0x8DD8,
    anim_389d_hi: 0x8DDB,
    anim_389d_kind: 0x8DDE,
    row_lo: 0x8DE2,
    row_hi: 0x8E00,
    anim_389d_colours: 0x8E1C,
    tile_anim_cursor: 0x8E24,
    tile_anim_lengths: 0x8E25,
    tile_anim_tables_lo: 0x8E2C,
    tile_anim_tables_hi: 0x8E33,
    tile_anim_alt: 0x8E70,
    anim_frames_top: 0x8E87,
    anim_frames_bottom: 0x8E8D,
    probe: 0x8E9F,
    read_input: 0x8F95,
    room_table: 0x8FB0,
    stage_table: 0x8FBC,
    item_icon_none: 0xB410,
    power_shapes: 0xB5C0,
    tile_props: 0xBE00,
    tile_colours: 0xBF00,
    charset: 0xC000,
    tile_anim_chars_5: 0xC0D8,
    tile_anim_chars_2: 0xC234,
    swap_a_live: 0xC3F8,
    tile_anim_sparkle: 0xC4D3,
    shimmer_chars: 0xC4E0,
    tile_anim_chars_1: 0xC530,
    swap_b_live: 0xC54F,
    swap_c_live: 0xC5BF,
    tile_anim_chars_0: 0xC680,
    water_chars: 0xC698,
    swap_d_live: 0xC6CF,
    tile_anim_rot_chars: 0xC738,
    tile_anim_chars_4: 0xC760,
    tile_anim_chars_2b: 0xC793,
    screen: 0xC800,
    hud_time: 0xC84A,
    entry_name: 0xCBD7,
    hud_slots: 0xCBE0,
    time_digits: 0xCBE8,
    anim_389d_cursor: 0xCBEC,
    anim_3991_cursor: 0xCBED,
    anim_38f8_cursor: 0xCBEE,
    anim_3b3e_cursor: 0xCBEF,
    score: 0xCBF0,
    restart_column: 0xCBF3,
    menu_bands: 0xCBF4,
    bonus_stage: 0xCBF7,
    sprite_pointers: 0xCBF8,
    tile_anim_flicker: 0xCF7F,
    room_counter: 0xCFFF,
    tile_anim_sprite_5: 0xEAE9,
    item_sprite: 0xEB0F,
    giana_shapes: 0xED00,
    giana_shapes_left: 0xF540,
    nmi_vector: 0xFFFA,
    irq_vector: 0xFFFE,
};
