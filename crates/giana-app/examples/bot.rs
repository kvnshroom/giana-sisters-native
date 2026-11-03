//! Searches inputs that reach the statue ($0457 == $FF) in stage 1: `bot <seeds> <frames>`; prints the script of a hit.
use giana_core::state::{ControllerInput, GameState};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: u64 = a[1].parse().unwrap();
    let frames: usize = a[2].parse().unwrap();
    for seed in 0..seeds {
        let mut g = GameState::new_port_game();
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut jump_left = 0usize;
        let mut log: Vec<u8> = Vec::new();
        let mut hit = None;
        for f in 0..frames {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            if jump_left == 0 && (s >> 33) % 25 == 0 { jump_left = 8 + ((s >> 45) % 22) as usize; }
            let mut i = ControllerInput::new();
            i.right = true;
            i.up = jump_left > 0; i.jump = i.up; i.fire = (s >> 50) % 4 == 0;
            if jump_left > 0 { jump_left -= 1; }
            log.push((i.right as u8) | (i.up as u8) << 1 | (i.fire as u8) << 2);
            g.update(&i);
            if g.st_0457 == 0xFF { hit = Some(f); break; }
            if g.game_over || g.stage != 1 { break; }
        }
        if let Some(f) = hit {
            println!("seed {seed} hit at frame {f}");
            // run-length script ("R", "RU", "RF", "RUF") from frame 0 of the game (the oracle script prefix adds 70)
            let name = |k: u8| { let mut n = String::new(); if k & 1 != 0 { n.push('R'); } if k & 2 != 0 { n.push('U'); } if k & 4 != 0 { n.push('F'); } if n.is_empty() { n.push('-'); } n };
            let mut out = String::new(); let mut i = 0;
            while i < log.len() { let mut j = i; while j < log.len() && log[j] == log[i] { j += 1; } out += &format!("{}:{} ", name(log[i]), j - i); i = j; }
            println!("{out}");
            break;
        }
    }
}
