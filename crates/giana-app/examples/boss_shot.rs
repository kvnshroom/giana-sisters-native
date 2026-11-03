//! Picture of the stage 4 boss `n` frames after it appears: `boss_shot <n> <out.ppm>`
use giana_core::state::{ControllerInput, GameState};
use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::vic::{render_vic, VicView};
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a[1].parse().unwrap();
    let mut g = GameState::new_port_game_at(4);
    let idle = ControllerInput::new();
    let mut fire = ControllerInput::new();
    fire.fire = true;
    for _ in 0..3 {
        g.update(&fire);
    }
    while g.flag_0431 != 0 {
        g.update(&idle);
    }
    for _ in 0..4 {
        g.update(&idle);
    }
    g.map_column = 0xF4;
    let mut seen = 0;
    loop {
        g.update(&idle);
        if g.en.vic_me & 0xE0 != 0 {
            seen += 1;
            if seen > n {
                break;
            }
        }
    }
    let mut fb = Framebuffer::new();
    render_vic(&mut fb, &VicView::from_game(&g));
    let mut f = std::fs::File::create(&a[2]).unwrap();
    writeln!(f, "P6\n{} {}\n255", WIDTH, HEIGHT).unwrap();
    for p in &fb.pixels {
        f.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]).unwrap();
    }
}
