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
