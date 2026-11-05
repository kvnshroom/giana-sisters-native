use giana_core::state::{ControllerInput, GameScreen, GameState};

#[test]
fn test_player_jump_and_gravity() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // Start in open space without overhead blocks (X=100)
    game.player_x = 100;
    let initial_y = game.player_y;
    assert_eq!(initial_y, 115);

    // Press jump
    let mut input = ControllerInput::new();
    input.jump = true;
    game.update(&input);

    // Giana should ascend
    assert!(game.is_jumping);
    assert!(game.player_y < initial_y);

    // Release jump and simulate 50 frames
    input.jump = false;
    for _ in 0..50 {
        game.update(&input);
    }

    // Giana should have landed back on the ground
    assert_eq!(game.player_y, initial_y);
    assert!(!game.is_jumping);
    assert!(!game.is_falling);
}

#[test]
fn test_enemy_stomp_kill() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // Explicitly add an active monster for the test
    game.enemies.push(giana_core::entities::Enemy::new(giana_core::entities::EnemyType::Monster, 280.0, 115.0));

    // Position player right above enemy 0 (Monster at X=280, Y=115)
    game.player_x = 280;
    game.player_y = 100;
    game.is_falling = true;
    game.fall_speed = 3;

    let initial_score = game.score;
    let input = ControllerInput::new();
    game.update(&input);

    // Enemy should be stomped; the original adds +50 (oracle 04_enemy: $0435 = 0x50), not 200.
    // Red until the enemy routines (sub_2194, sub_288A, sub_29B2) are ported.
    assert!(game.enemies[0].is_dying || !game.enemies[0].active);
    assert_eq!(game.score, initial_score + 50);
}

#[test]
fn test_hit_bonus_block() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // Place an authentic question bonus block ($A8) at Row 11, Col 5 (X=40, Y=88) with air underneath
    let idx = 11 * 256 + 5;
    game.current_map[idx] = 0xA8;

    // Position Giana underneath at X=40 on the ground (Y=115)
    game.player_x = 40;
    game.player_y = 115;
    let initial_diamonds = game.diamonds;

    // Jump into the block
    let mut input = ControllerInput::new();
    input.jump = true;
    for _ in 0..20 {
        game.update(&input);
        if game.diamonds > initial_diamonds {
            break;
        }
    }

    // Diamond should pop out and diamond count should increase
    assert_eq!(game.diamonds, initial_diamonds + 1);
    assert!(game.score >= 100);
}

#[test]
fn test_timer_countdown() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    // Original (oracle frame 70): $0449/$044A = BCD 401
    assert_eq!(game.time, 401);

    let input = ControllerInput::new();
    // Original timer: $0448 reloads 40 -> one second every 41 ticks (sub_1D0A); 50 ticks cross exactly one second
    for _ in 0..50 {
        game.update(&input);
    }

    assert_eq!(game.time, 400);
}

#[test]
fn test_walk_past_bushes_to_stage1_bricks() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);
    game.god_mode = true; // Test environment: invulnerable to enemies while testing map collision

    // Initial position
    assert_eq!(game.player_x, 20);

    // Hold right and simulate walking for 160 frames (160 * 2 px/frame = 320 px)
    let mut input = ControllerInput::new();
    input.right = true;

    for _ in 0..160 {
        game.update(&input);
    }

    // Player should smoothly walk past bushes (at X=56, X=170, etc.) and reach Col 40 brick area (X >= 320)
    assert!(game.player_x >= 320, "Player got blocked by a bush! player_x = {}", game.player_x);
    // Camera scroll should follow player
    assert!(game.scroll_x > 0);

    // Now test hitting the authentic Stage 1 brick platform at Col 41 (X=328)
    game.player_x = 328;
    input.right = false;
    input.jump = true;

    let pre_jump_score = game.score;
    let mut hit_brick = false;
    for _ in 0..20 {
        game.update(&input);
        if game.score > pre_jump_score || game.is_falling {
            hit_brick = true;
            break;
        }
    }
    assert!(hit_brick, "Player jumped under authentic bricks but failed to interact!");
}

#[test]
fn test_stage1_enemies_are_ground_monsters() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);

    // Verify all Stage 1 enemies are ground monsters (Owls), no RollingEye in Stage 1
    for &(_col, _x, y, enemy_type) in &game.enemy_spawns {
        assert_eq!(enemy_type, giana_core::entities::EnemyType::Monster, "Stage 1 enemies must be Owls!");
        assert_eq!(y, 115.0, "Stage 1 enemies must be on the ground!");
    }
}

#[test]
fn test_mushroom_cap_is_not_solid() {
    giana_core::skip_without_data!();
    let game = GameState::new();
    // Mushroom tiles $B4..$BD at Col 64..67
    let is_cap_solid = game.is_tile_solid(64 * 8 + 4, 14 * 8 + 4);
    assert!(!is_cap_solid, "Mushroom cap should be non-solid background decoration!");
}

#[test]
fn test_hit_brick_reveals_diamond() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);

    // Col 41, Row 11 is brick ($44) and Row 10 is diamond ($EB)
    game.player_x = 41 * 8; // X=328
    game.player_y = 115;
    let initial_diamonds = game.diamonds;

    let mut input = ControllerInput::new();
    input.jump = true;

    for _ in 0..25 {
        game.update(&input);
        if game.diamonds > initial_diamonds {
            break;
        }
    }

    assert_eq!(game.diamonds, initial_diamonds + 1, "Hitting brick under $EB must pop a diamond!");
    assert!(!game.items.is_empty(), "PowerupItem must pop up!");
}

#[test]
fn test_cannot_stand_on_diamonds() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // A spawned diamond item in the air at (X=100, Y=80)
    game.items.push(giana_core::entities::PowerupItem::new(100.0, 80.0, true));

    // Position player touching the diamond item
    game.player_x = 100;
    game.player_y = 80;
    let pre_diamonds = game.diamonds;

    let input = ControllerInput::new();
    game.update(&input);

    assert_eq!(game.diamonds, pre_diamonds + 1, "Diamond item must be collected when touched!");
}

#[test]
fn test_pits_are_not_solid() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);

    // In Stage 1, find the first pit column where Row 17 is 0
    let mut pit_col = 0;
    for c in 0..256 {
        if game.current_map[17 * 256 + c] == 0 {
            pit_col = c;
            break;
        }
    }

    assert!(pit_col > 0, "Stage 1 must contain pits!");
    let pit_x = (pit_col * 8 + 4) as i32;

    // Pit ground row must be non-solid!
    assert!(!game.is_tile_solid(pit_x, 17 * 8 + 4), "Pit must be non-solid (open air)!");
}

#[test]
fn test_brick_platforms_are_solid_and_walkable() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);

    // Platform at Col 41, Row 10 (which contains 0xEB)
    let px = 41 * 8 + 4;
    let py = 10 * 8 - 21; // Standing on top of Row 10
    game.player_x = px;
    game.player_y = py;

    let input = ControllerInput::new();
    game.update(&input);

    // Giana should remain standing on top of the brick platform, not fall through!
    assert!(!game.is_falling, "Giana should be standing solidly on top of brick platform!");
    assert_eq!(game.player_y, py);
}

#[test]
fn test_enemy_wall_bounce() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // Place a solid wall at Col 20, Row 14..17
    for r in 14..=17 {
        game.current_map[r * 256 + 20] = 0x4C; // Solid pipe wall
    }

    // Spawn an enemy walking right towards the wall (X=140, wall is at X=160)
    let mut enemy = giana_core::entities::Enemy::new(giana_core::entities::EnemyType::Monster, 140.0, 115.0);
    enemy.vel_x = 1.0;
    game.enemies.push(enemy);

    let input = ControllerInput::new();
    // Simulate until enemy hits wall
    for _ in 0..30 {
        game.update(&input);
        if game.enemies[0].vel_x < 0.0 {
            break;
        }
    }

    assert!(game.enemies[0].vel_x < 0.0, "Enemy must bounce and reverse direction upon hitting wall!");
}

#[test]
fn test_crumbling_bridge_decay() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick

    // Place a crumbling bridge tile ($28) at Col 10, Row 17
    let idx = 17 * 256 + 10;
    game.current_map[idx] = 0x28;

    game.player_x = 10 * 8;
    game.player_y = 17 * 8 - 21;

    let input = ControllerInput::new();
    game.update(&input);

    // Standing on the bridge should have triggered crumbling decay
    assert!(game.current_map[idx] > 0x28, "Crumbling bridge tile should decay when stood upon!");
}

#[test]
fn test_giana_projectile_shoot_and_pop() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.has_punker = true;
    game.has_shot = true; // Armed with shooting ability!

    // Place a solid wall at Col 30, Row 14..17
    for r in 14..=17 {
        game.current_map[r * 256 + 30] = 0x4C;
    }

    game.player_x = 20 * 8;
    game.player_y = 115;
    game.facing_right = true;

    let mut input = ControllerInput::new();
    input.fire = true;
    game.update(&input);

    assert_eq!(game.projectiles.len(), 1, "Projectile should be spawned!");
    assert_eq!(game.projectiles[0].sprite_pointer, 0xF7, "Authentic C64 bubble sprite 0xF7!");

    // Advance until bubble hits wall at X=240
    input.fire = false;
    for _ in 0..20 {
        game.update(&input);
        if game.projectiles.is_empty() || game.projectiles[0].popping {
            break;
        }
    }

    assert!(game.projectiles.is_empty() || game.projectiles[0].popping, "Projectile should pop against wall!");
}

#[test]
fn test_player_death_sequence_and_respawn() {
    giana_core::skip_without_data!();
    let mut game = GameState::new();
    game.screen = GameScreen::InGame;
    game.first_in_game_tick = false; // frame 70 is the initial state in the original; start at the first real tick
    game.load_stage(1);
    game.lives = 3;
    game.player_x = 200;
    game.player_y = 165;

    let input = ControllerInput::new();
    game.update(&input);

    assert!(game.is_dying, "Player should enter dying state");
    assert_eq!(game.get_giana_sprites(), giana_core::physics::GIANA_DEATH, "Death sprite should be active");

    for _ in 0..100 {
        game.update(&input);
        if !game.is_dying {
            break;
        }
    }

    assert!(!game.is_dying, "Player should finish dying sequence");
    assert_eq!(game.lives, 2, "Lives should be decremented by 1");
    assert_eq!(game.player_x, 20, "Player should respawn at stage start");
    assert_eq!(game.player_y, 115, "Player should respawn at ground level");
}

/// Hold right and note every change of (player, lives, finished flags) up to `frames`.
fn turns(mut game: GameState, frames: usize) -> Vec<(u8, u8, u8, u8, bool)> {
    let mut seen = Vec::new();
    let mut input = ControllerInput::new();
    input.right = true;
    for f in 0..frames {
        // Fire ends the banner of a restart; let go of it so that it can be pressed again
        input.fire = f % 60 == 0;
        game.update(&input);
        let now = (game.player, game.lives_c64, game.player_save[12], game.player_save[13], game.game_over);
        if seen.last() != Some(&now) {
            seen.push(now);
        }
        if game.game_over {
            break;
        }
    }
    seen
}

#[test]
fn test_two_players_take_turns_after_every_life() {
    giana_core::skip_without_data!();
    let t = turns(GameState::new_port_game_two(), 20000);
    println!("{t:?}");
    // the original (18_two_players): Giana 3, lives 2, Maria 3, lives 2, Giana 2 ...
    assert_eq!(&t[..5], &[(0, 3, 0, 0, false), (0, 2, 0, 0, false), (1, 3, 0, 0, false), (1, 2, 0, 0, false), (0, 2, 0, 0, false)]);
    assert!(t.last().unwrap().4, "the game ends when both are finished");
    assert!(t.iter().filter(|s| s.0 == 1).count() > 0);
}

#[test]
fn test_a_game_of_its_own_for_each_player() {
    giana_core::skip_without_data!();
    for player in [0u8, 1] {
        let mut t = turns(GameState::new_port_game_as(player), 20000);
        assert!(t.iter().all(|s| s.0 == player), "the turn never changes: {t:?}");
        assert!(t.pop().unwrap().4, "the game ends with the last life");
    }
    let mut maria = GameState::new_port_game_as(1);
    let idle = ControllerInput::new();
    for _ in 0..300 {
        maria.update(&idle);
    }
    assert_eq!(maria.en.vic_col[0], 0x0D, "Maria's colour from $146E");
    assert_eq!((maria.screen_ram[2], maria.screen_ram[3]), (0x0D, 0x01), "the status bar says MARIA");
}

#[test]
fn test_one_player_stays_on_after_a_life() {
    giana_core::skip_without_data!();
    let t = turns(GameState::new_port_game(), 20000);
    assert!(t.iter().all(|s| s.0 == 0));
    assert!(t.last().unwrap().4);
}

#[test]
fn test_each_player_keeps_their_own_score_and_colours() {
    giana_core::skip_without_data!();
    let mut game = GameState::new_port_game_two();
    let mut input = ControllerInput::new();
    input.right = true;
    game.score_bcd = [0x12, 0x34, 0x56];
    let mut f = 0;
    while game.player == 0 && f < 20000 {
        input.fire = f % 60 == 0;
        game.update(&input);
        f += 1;
    }
    assert_eq!(game.score_bcd, [0, 0, 0], "Maria starts at zero");
    assert_eq!(game.player_save[0], 0x12);
    // Maria can lose her life again in this time and Giana is back: watch for both on the way
    let (mut f, mut printed, mut coloured) = (0, false, false);
    while f < 400 {
        input.fire = f % 60 == 0;
        game.update(&input);
        f += 1;
        printed |= game.screen_ram[1] == 0 && game.screen_ram[2] == 0x0D && game.screen_ram[3] == 0x01;
        coloured |= game.player == 1 && game.en.vic_col[0] == 0x0D;
    }
    assert!(printed, "the status bar says MARIA");
    assert!(coloured, "Maria's colour from $146E");
}

#[test]
fn test_the_banner_after_a_death_waits_for_a_new_press() {
    giana_core::skip_without_data!();
    let mut game = GameState::new_port_game();
    let mut input = ControllerInput::new();
    input.right = true;
    input.fire = true; // held all the way through the death
    let mut f = 0;
    while !game.waiting_for_confirm() && f < 5000 {
        game.update(&input);
        f += 1;
    }
    assert!(game.waiting_for_confirm(), "Giana lost a life");
    for _ in 0..1000 {
        game.update(&input);
    }
    assert!(game.waiting_for_confirm(), "a key held from before does not confirm, and no timer does");
    input = ControllerInput::new();
    game.update(&input);
    assert!(game.waiting_for_confirm(), "letting go does not confirm");
    input.jump = true;
    game.update(&input);
    assert!(!game.waiting_for_confirm(), "a new press does");
    input = ControllerInput::new();
    for _ in 0..200 {
        game.update(&input);
    }
    assert_eq!(game.flag_0431, 0, "the game runs again");
}
