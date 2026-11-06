//! Fullscreen at 50 Hz, or at a multiple of it.
//!
//! The game logic runs at 50 Hz (the PAL frame rate of the C64) and draws one picture per step. On a 60 Hz display some
//! steps would be shown twice and a picture every sixth frame is repeated: the scrolling judders, which is most visible in
//! a game that scrolls two pixels every frame. If the display offers a 50 Hz mode, an exclusive fullscreen window uses it and
//! every logic step becomes exactly one shown frame. 100, 150 and 200 Hz work the same way: every logic step is shown 2, 3
//! or 4 times, evenly, and the logic stays at 50 Hz. The highest such mode wins. Without such a mode the desktop fullscreen
//! stays, as it was.

use giana_render::framebuffer::HEIGHT;
use giana_render::wide::WIDE_WIDTH;

/// The rate of the game logic, in Hz.
pub const WANTED_HZ: i32 = 50;

/// The highest multiple of 50 Hz that is used (200 Hz: every step is shown four times).
pub const MAX_FACTOR: i32 = 4;

/// One display mode: width, height, refresh rate.
pub type Mode = (i32, i32, i32);

/// How many frames of this refresh rate one logic step takes (1 for 50 Hz, 2 for 100 Hz, ...), or `None` when the rate is
/// not an exact multiple of 50 Hz (up to `MAX_FACTOR`).
pub fn refresh_factor(hz: i32) -> Option<u32> {
    (hz >= WANTED_HZ && hz % WANTED_HZ == 0 && hz / WANTED_HZ <= MAX_FACTOR).then_some((hz / WANTED_HZ) as u32)
}

/// The index of the mode to use for fullscreen, and how many frames of it one logic step takes; `None` when the display has
/// no mode that suits.
///
/// Suitable: a refresh rate that is 50, 100, 150 or 200 Hz, at least as big as the picture, and not bigger than the desktop
/// (the picture is scaled up to fill it). The highest refresh rate wins; among those the biggest size, where a mode with the
/// proportions of the desktop is preferred, so that the bars stay the same.
pub fn pick_mode(modes: &[Mode], desktop: (i32, i32)) -> Option<(usize, u32)> {
    let same_shape = |w: i32, h: i32| w as i64 * desktop.1 as i64 == h as i64 * desktop.0 as i64;
    modes
        .iter()
        .enumerate()
        .filter(|&(_, &(w, h, hz))| refresh_factor(hz).is_some() && w >= WIDE_WIDTH as i32 && h >= HEIGHT as i32 && w <= desktop.0 && h <= desktop.1)
        .max_by_key(|&(_, &(w, h, hz))| (hz, same_shape(w, h), w as i64 * h as i64))
        .map(|(i, &(_, _, hz))| (i, refresh_factor(hz).unwrap_or(1)))
}

/// Whether the display runs at a rate that is not a multiple of 50 Hz (60, 120, 144 ...): there the picture between two logic
/// steps can be interpolated (`SMOOTH MOTION`). A rate that is unknown (0) counts as one that fits.
pub fn is_odd_rate(hz: i32) -> bool {
    hz > 0 && refresh_factor(hz).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_matching_mode_keeps_the_desktop() {
        assert_eq!(pick_mode(&[(1920, 1080, 60), (1280, 720, 60), (1920, 1080, 59), (1920, 1080, 144)], (1920, 1080)), None);
        assert_eq!(pick_mode(&[], (1920, 1080)), None);
    }

    #[test]
    fn the_desktop_size_at_50hz_is_the_best() {
        let modes = [(1920, 1080, 60), (1280, 720, 50), (1920, 1080, 50), (1024, 768, 50)];
        assert_eq!(pick_mode(&modes, (1920, 1080)), Some((2, 1)));
    }

    #[test]
    fn the_highest_multiple_of_50_wins() {
        let modes = [(1920, 1080, 50), (1920, 1080, 100), (1920, 1080, 60), (1920, 1080, 144)];
        assert_eq!(pick_mode(&modes, (1920, 1080)), Some((1, 2)));
        let modes = [(1920, 1080, 100), (1920, 1080, 150), (1920, 1080, 200), (1920, 1080, 120), (1920, 1080, 165)];
        assert_eq!(pick_mode(&modes, (1920, 1080)), Some((2, 4)));
        // 250 Hz is not used: a step would be shown five times, which is more than the cap
        assert_eq!(pick_mode(&[(1920, 1080, 250), (1920, 1080, 150)], (1920, 1080)), Some((1, 3)));
        assert_eq!(pick_mode(&[(1920, 1080, 250), (1920, 1080, 60)], (1920, 1080)), None);
    }

    #[test]
    fn the_rate_comes_before_the_size() {
        // 100 Hz only at a smaller size than the desktop: still preferred over 50 Hz at the full size
        let modes = [(1920, 1080, 50), (1280, 720, 100)];
        assert_eq!(pick_mode(&modes, (1920, 1080)), Some((1, 2)));
    }

    #[test]
    fn a_smaller_mode_with_the_same_proportions_beats_a_bigger_one_with_other_proportions() {
        let modes = [(1600, 1200, 50), (1280, 720, 50)];
        assert_eq!(pick_mode(&modes, (1920, 1080)), Some((1, 1)));
        // without a fitting shape the biggest one is taken
        assert_eq!(pick_mode(&[(800, 600, 50), (1024, 768, 50)], (1920, 1080)), Some((1, 1)));
    }

    #[test]
    fn modes_that_are_too_small_or_too_big_do_not_count() {
        assert_eq!(pick_mode(&[(320, 200, 50)], (1920, 1080)), None);
        assert_eq!(pick_mode(&[(3840, 2160, 50)], (1920, 1080)), None);
        assert_eq!(pick_mode(&[(3840, 2160, 200), (1920, 1080, 100)], (1920, 1080)), Some((1, 2)));
    }

    #[test]
    fn refresh_factors() {
        assert_eq!([50, 100, 150, 200, 60, 120, 144, 0, 250].map(refresh_factor), [Some(1), Some(2), Some(3), Some(4), None, None, None, None, None]);
        assert!(is_odd_rate(60) && is_odd_rate(144) && is_odd_rate(120));
        assert!(!is_odd_rate(50) && !is_odd_rate(100) && !is_odd_rate(200) && !is_odd_rate(0));
    }
}
