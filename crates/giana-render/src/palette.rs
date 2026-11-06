/// The 16 authentic Commodore 64 colors in ARGB8888 (0xAARRGGBB) format.
pub const C64_PALETTE: [u32; 16] = [
    0xFF000000, // 0: Black
    0xFFFFFFFF, // 1: White
    0xFF813338, // 2: Red
    0xFF75CEC8, // 3: Cyan
    0xFF8E3C97, // 4: Purple
    0xFF56AC4D, // 5: Green
    0xFF2E2C9B, // 6: Blue
    0xFFEDF171, // 7: Yellow
    0xFF8E5029, // 8: Orange
    0xFF553800, // 9: Brown
    0xFFC46C71, // 10: Light Red
    0xFF4A4A4A, // 11: Dark Grey
    0xFF7B7B7B, // 12: Medium Grey
    0xFFA9FF9F, // 13: Light Green
    0xFF706DEB, // 14: Light Blue
    0xFFB2B2B2, // 15: Light Grey
];

#[inline(always)]
pub fn c64_color(index: u8) -> u32 {
    C64_PALETTE[(index & 0x0F) as usize]
}

/// C64 colour -> the darker neighbour (the dimmed picture behind a menu).
pub const DARKER: [u8; 16] = [0, 11, 0, 6, 0, 11, 0, 8, 9, 0, 2, 0, 11, 5, 6, 12];
