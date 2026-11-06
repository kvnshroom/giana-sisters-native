//! Random-input soak test of the ported game: `fuzz <seeds> <frames>`; prints stage/lives progress and catches panics.
use giana_core::state::{ControllerInput, GameState};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seeds: u64 = a[1].parse().unwrap();
    let frames: usize = a[2].parse().unwrap();
    for seed in 0..seeds {
        let r = std::panic::catch_unwind(|| {
            let st: u32 = std::env::var("STAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            let mut g = GameState::new_port_game_at(st);
            // `WIDE=<k>`: play by the rules of WIDESCREEN ON with k extra columns per side
            g.set_wide(std::env::var("WIDE").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
            let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut keys = 0u8;
            let mut max_stage = 1; let mut statue = 0u32;
            for f in 0..frames {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                if (s >> 33) % 20 == 0 { keys = ((s >> 40) & 0x1F) as u8 | 0x08; }
                let mut i = ControllerInput::new();
                i.left = keys & 4 != 0 && keys & 8 == 0; i.right = keys & 8 != 0 && keys & 4 == 0 || f % 3 == 0;
                i.up = keys & 1 != 0; i.jump = i.up; i.down = keys & 2 != 0; i.fire = keys & 16 != 0;
                g.update(&i);
                max_stage = max_stage.max(g.stage); if g.st_0457 == 0xFF { statue += 1; }
                if g.game_over { return (f, max_stage + statue * 100, true); }
            }
            eprintln!("end state: main {:?} stage {} lives {} 2169 {} 046d {} 0431 {:#x} 0457 {:#x}", g.main, g.stage, g.lives_c64, g.st_2169, g.st_046d, g.flag_0431, g.st_0457);
            (frames, max_stage, false)
        });
        match r { Ok((f, st, go)) => println!("seed {seed}: frames {f} max stage {st} game_over {go}"), Err(_) => println!("seed {seed}: PANIC") }
    }
}
