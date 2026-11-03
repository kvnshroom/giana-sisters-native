/// Armin Gessert's authentic jump curve table from C64 address $9ADE (28 frames)
pub const JUMP_CURVE: [i32; 28] = [
    4, 4, 3, 3, 2, 3, 3, 2, 3, 2, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 2, 1, 2, 1, 1, 1,
];

pub const MAX_FALL_SPEED: i32 = 4;
pub const WALK_SPEED: i32 = 2;
pub const RUN_SPEED: i32 = 3;

/// Sprite pointer offsets for Giana animations:
/// Pointers are stored relative to Bank 3 ($C000):
/// Each frame consists of 3 composite layers:
/// [Sprite 0 (Head/Body, Multicolor), Sprite 1 (Dress, Multicolor), Sprite 2 (Eye/Shoes, HIRES)]
pub const GIANA_STAND_RIGHT: [usize; 3] = [0x4E, 0x4F, 0x50];
pub const GIANA_WALK_RIGHT_1: [usize; 3] = [0x30, 0x31, 0x32];
pub const GIANA_WALK_RIGHT_2: [usize; 3] = [0x33, 0x34, 0x35];
pub const GIANA_WALK_RIGHT_3: [usize; 3] = [0x36, 0x37, 0x38];
pub const GIANA_JUMP_RIGHT: [usize; 3] = [0x48, 0x49, 0x4A];
pub const GIANA_DEATH: [usize; 3] = [0x5A, 0x5B, 0x5C];

pub const GIANA_STAND_LEFT: [usize; 3] = [0x51, 0x52, 0x53];
pub const GIANA_WALK_LEFT_1: [usize; 3] = [0x3C, 0x3D, 0x3E];
pub const GIANA_WALK_LEFT_2: [usize; 3] = [0x3F, 0x40, 0x41];
pub const GIANA_WALK_LEFT_3: [usize; 3] = [0x42, 0x43, 0x44];
pub const GIANA_JUMP_LEFT: [usize; 3] = [0x4B, 0x4C, 0x4D];

pub const PUNK_STAND_RIGHT: [usize; 3] = [0x4E, 0x4F, 0x50];
pub const PUNK_WALK_RIGHT_1: [usize; 3] = [0x30, 0x31, 0x32];
pub const PUNK_WALK_RIGHT_2: [usize; 3] = [0x33, 0x34, 0x35];
pub const PUNK_WALK_RIGHT_3: [usize; 3] = [0x36, 0x37, 0x38];
pub const PUNK_JUMP_RIGHT: [usize; 3] = [0x48, 0x49, 0x4A];

pub const PUNK_STAND_LEFT: [usize; 3] = [0x51, 0x52, 0x53];
pub const PUNK_WALK_LEFT_1: [usize; 3] = [0x3C, 0x3D, 0x3E];
pub const PUNK_WALK_LEFT_2: [usize; 3] = [0x3F, 0x40, 0x41];
pub const PUNK_WALK_LEFT_3: [usize; 3] = [0x42, 0x43, 0x44];
pub const PUNK_JUMP_LEFT: [usize; 3] = [0x4B, 0x4C, 0x4D];
