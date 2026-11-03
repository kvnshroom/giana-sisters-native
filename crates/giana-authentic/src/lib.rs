//! AUTHENTIC presentation: the screens outside the game (title program, high score table) run the player's own
//! original code on a small C64 slice (`machine`) and are drawn by its video chip model (`vic`).

pub mod cpu;
pub mod machine;
pub mod vic;
pub mod session;
