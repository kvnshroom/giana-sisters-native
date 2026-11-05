//! Read-only access to the original program image (`ingame.bin`, file offset == C64 address), loaded at run time
//! (see `data`). Static tables and the initial contents of data regions that the original routines read are taken
//! from here instead of being retyped.

/// The 64K program image.
#[inline]
pub fn image() -> &'static [u8; 65536] {
    crate::data::get().ingame.as_slice().try_into().expect("ingame.bin is 64K (checked on load)")
}

/// Byte of the original image at a C64 address.
#[inline]
pub fn img(addr: usize) -> u8 {
    crate::data::get().ingame[addr & 0xFFFF]
}

/// Little-endian word of the original image.
#[inline]
pub fn img_word(addr: usize) -> usize {
    img(addr) as usize | (img(addr + 1) as usize) << 8
}

/// The joystick routine of Super Marioahner Sisters: `$9F7F` calls `$3F00`, which reads `$DC00` and, when the fire bit is
/// clear (pressed), sets bit 0 of `$0400` ("up") before the routine combines the directions, so fire jumps too.
/// Recognised by the call and the 30 bytes of the routine; every other image keeps the original's `$9F77`.
pub fn fire_jumps() -> bool {
    const CALL: [u8; 3] = [0x20, 0x00, 0x3F];
    const ROUTINE: [u8; 30] = [
        0xA9, 0xFF, 0x8D, 0x00, 0xDC, 0xAD, 0x00, 0xDC, 0x8D, 0xF0, 0x3E, 0x29, 0x10, 0xC9, 0x00, 0xF0, 0x04, 0xAD, 0xF0, 0x3E, 0x60, 0xAD, 0x00, 0x04,
        0x09, 0x01, 0x8D, 0x00, 0x04, 0x60,
    ];
    let img = &crate::data::get().ingame;
    img[0x9F7F..0x9F82] == CALL && img[0x3F00..0x3F1E] == ROUTINE
}
