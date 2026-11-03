use giana_core::state::GameState;
fn main() {
    for st in [1u32, 2, 3] {
        let mut g = GameState::new_boot();
        g.stage = st;
        g.load_map_for_stage();
        let mut hits = vec![];
        for row in 0..20 { for col in 0..256 { if g.current_map[row * 256 + col] == 0x8F { hits.push((row, col)); } } }
        println!("stage {st}: tile $8F at {:?}", hits);
    }
}
