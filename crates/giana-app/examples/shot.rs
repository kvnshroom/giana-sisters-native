//! Headless screenshot of the ported game: `shot <frames> <out.ppm> [right_from] [jump_from]`
//!
//! `GIANA_MODULE=<id or its first digits>` plays an installed ROM module (from the modules folder of the configuration
//! directory, so `XDG_CONFIG_HOME` picks another one).
use giana_core::state::{ControllerInput, GameState};
use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::vic::{render_vic, VicView};
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if let Ok(want) = std::env::var("GIANA_MODULE") {
        let store = giana_app::modules::Store::default_location().expect("no configuration folder");
        let found = store.find(&want).unwrap_or_else(|| panic!("no ROM module '{want}' in {}", store.dir().display()));
        let data_dir = giana_app::firstrun::locate(None).expect("no game data");
        let (data, _, meta) = store.load_selected(&found.id, &data_dir).expect("module does not load");
        giana_core::data::install(data);
        eprintln!("ROM module: {} ({})", meta.name, found.id);
    }
    let frames: usize = a[1].parse().unwrap();
    let right_from: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let jump_from: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let mut g = GameState::new_port_game();
    // `script:<file>` replays an oracle script (inputs from frame 70 on, the frame the game starts)
    let keys: Vec<(String, usize)> = if a.get(3).map_or(false, |s| s.starts_with("script:")) {
        std::fs::read_to_string(&a[3][7..]).unwrap().split_whitespace()
            .filter(|t| !t.starts_with("POKE")).map(|t| { let (k, n) = t.split_once(':').unwrap(); (k.to_string(), n.parse().unwrap()) }).collect()
    } else { Vec::new() };
    let mut per_frame: Vec<String> = Vec::new();
    for (k, n) in &keys { for _ in 0..*n { per_frame.push(k.clone()); } }
    let total = if keys.is_empty() { frames } else { per_frame.len().saturating_sub(70) };
    for f in 0..total {
        let mut i = ControllerInput::new();
        if keys.is_empty() {
            i.right = f >= right_from;
            i.jump = f >= jump_from && f % 40 < 20;
            i.fire = i.jump;
        } else if let Some(k) = per_frame.get(f + 70) {
            i.right = k.contains('R'); i.left = k.contains('L'); i.down = k.contains('D');
            i.up = k.contains('U'); i.jump = i.up; i.fire = k.contains('F');
        }
        g.update(&i);
    }
    if std::env::var("SHOT_DEBUG").is_ok() {
        println!("draw_xy {:?} draw_scroll {}", g.en.vic_frame.draw_xy, g.en.vic_frame.draw_xscroll);
    }
    let view = VicView::from_game(&g);
    let mut fb = Framebuffer::new();
    render_vic(&mut fb, &view);
    let mut f = std::fs::File::create(&a[2]).unwrap();
    writeln!(f, "P6\n{} {}\n255", WIDTH, HEIGHT).unwrap();
    for p in &fb.pixels { f.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]).unwrap(); }
}
