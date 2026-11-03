//! Addresses of the run-time state the game's own start-up writes (variables, buffers, sound driver and IRQ state).
//! A module never carries these: [`crate::make::clean_boot`] takes the base byte there. The original game and
//! its simple hacks use [`RUNTIME_RANGES`], hacks of the 7A3 construction-set engine [`RUNTIME_RANGES_B`].

include!("runtime_ranges_table.rs");
// family B (7A3 engine): other layout, own table (tools/modules/gen_runtime_ranges_b.py)
include!("runtime_ranges_b_table.rs");
// Giana Sisters II (Gem Nightmare): the older variant of the 7A3 engine keeps the original's resident stage data and
// tables, so it needs its own table (tools/modules/gen_runtime_ranges_gem.py)
include!("runtime_ranges_gem_table.rs");
