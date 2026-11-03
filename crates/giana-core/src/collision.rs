pub fn colortable() -> &'static [u8] {
    &crate::data::get().colortable
}
pub fn c64_collision_table() -> &'static [u8] {
    &crate::data::get().collision_table
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileType {
    Air,
    Solid,
    Breakable,
    Bonus,
    Crumbling,
    Hazard,
    Diamond,
}

/// Identifies the collision type of any C64 character code using Armin Gessert's authentic collision table ($9C20).
pub fn get_tile_type(char_idx: u8) -> TileType {
    let raw = c64_collision_table()[char_idx as usize];

    // Collectible diamonds in open air ($F7)
    if char_idx == 0xF7 {
        return TileType::Diamond;
    }

    // Question / Bonus blocks: authentic C64 bitmask 0x80 ($A6..$A9)
    if raw & 0x80 != 0 {
        return TileType::Bonus;
    }

    // Special / Powerup blocks: authentic C64 bitmask 0x40 ($72..$75, $F3..$F6)
    if raw & 0x40 != 0 {
        return TileType::Bonus;
    }

    // Breakable blocks: authentic C64 bitmask 0x60 ($EC..$EF) or Stage brick blocks ($43..$4B, $EA, $EB)
    if (raw & 0x60 == 0x60) || (char_idx >= 0x43 && char_idx <= 0x4B) || char_idx == 0xEA || char_idx == 0xEB {
        return TileType::Breakable;
    }

    // Crumbling bridge tiles in Stage 1 ($28..$2F)
    if char_idx >= 0x28 && char_idx <= 0x2F {
        return TileType::Crumbling;
    }

    // Solid blocks (bit 0 set in C64 table: $3A..$42 ground, $4C..$6B pipes/castle)
    if raw & 0x01 != 0 {
        return TileType::Solid;
    }

    // Non-solid background decoration: bushes, flowers, mushrooms ($B4..$BD), air
    TileType::Air
}

/// Collision bounds for player and entities
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}
