//! Putting the 384 x 272 picture into the window: the scaling modes of the options page.
//!
//! The C64 frame has a border around the 320 x 200 screen. What the game draws never leaves the screen: the status bar is
//! the first two character rows of the screen, and of the screen the 38-column window shows 304 x 200 pixels. So while a
//! game is shown, the frame can be cut down to that `PLAY` rectangle without losing anything. Menus and the title picture
//! use the border too (the version, the controls, the messages of a network game), so they always show the whole frame.

use crate::settings::Scaling;
use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::vic::{ORIGIN_X, ORIGIN_Y};
use giana_render::wide::{self, Strips, EXTRA, EXTRA_CROP, WIDE_WIDTH};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect as SdlRect;
use sdl2::render::{BlendMode, Canvas, RenderTarget, ScaleMode, Texture, TextureCreator};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn sdl(self) -> SdlRect {
        SdlRect::new(self.x as i32, self.y as i32, self.w, self.h)
    }
}

/// The whole frame, border included.
pub const FRAME: Rect = Rect { x: 0, y: 0, w: WIDTH as u32, h: HEIGHT as u32 };
/// What a game fills: the 38 columns x 25 rows of the screen, everything else of the frame is empty border.
pub const PLAY: Rect = Rect { x: ORIGIN_X as u32 + 8, y: ORIGIN_Y as u32, w: 304, h: 200 };

/// The widescreen frame (`WIDESCREEN` is on while a game is shown): the border, `EXTRA` pixels of level on each side of the
/// playing area, the playing area, and the border again: 480 x 272.
pub const WIDE_FRAME: Rect = Rect { x: 0, y: 0, w: WIDE_WIDTH as u32, h: HEIGHT as u32 };
/// The widescreen playing area without the border: `EXTRA_CROP` pixels of level on each side, 352 x 200.
pub const WIDE_PLAY: Rect = Rect { x: (PLAY.x as usize + EXTRA - EXTRA_CROP) as u32, y: ORIGIN_Y as u32, w: (304 + 2 * EXTRA_CROP) as u32, h: 200 };

/// Which part of the frame is drawn where in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub src: Rect,
    pub dst: Rect,
}

impl Layout {
    /// Messages and menus over the game stay inside the playing area (`ui::View::CROP`): in the cut-down pictures, and in
    /// the widescreen frame, where the strips of extra level are put in between the border and the playing area.
    pub fn crops_overlays(&self) -> bool {
        self.src == PLAY || self.src == WIDE_PLAY || self.src == WIDE_FRAME
    }

    /// The picture is not a whole multiple of the part that it shows: its pixels have to be blended at the edges.
    pub fn is_fractional(&self) -> bool {
        !self.dst.w.is_multiple_of(self.src.w) || !self.dst.h.is_multiple_of(self.src.h)
    }

    /// The size of a picture pixel in window pixels (the larger one of both directions).
    pub fn scale(&self) -> f64 {
        (self.dst.w as f64 / self.src.w as f64).max(self.dst.h as f64 / self.src.h as f64)
    }
}

/// Where the picture goes in a window of `win` pixels. `playing`: a game is shown, so the border may be cut off.
///
/// * `Whole`: the whole frame, a whole number of times.
/// * `NoBorder`: while playing the `PLAY` part, a whole number of times (5 x on a 1920 x 1080 screen); otherwise as `Whole`.
/// * `Fill`: while playing the `PLAY` part, otherwise the whole frame, as big as the window allows with its proportions.
pub fn layout(scaling: Scaling, playing: bool, win: (u32, u32)) -> Layout {
    layout_wide(scaling, playing, win, false)
}

/// [`layout`] for the widescreen view: `wide` says that the frame of the game is the widescreen one (it only counts while
/// `playing`). The whole frame is then 480 x 272 (16:9 within 1 %), the playing area 352 x 200.
pub fn layout_wide(scaling: Scaling, playing: bool, win: (u32, u32), wide: bool) -> Layout {
    let wide = wide && playing;
    let src = match (playing && scaling != Scaling::Whole, wide) {
        (true, true) => WIDE_PLAY,
        (true, false) => PLAY,
        (false, true) => WIDE_FRAME,
        (false, false) => FRAME,
    };
    let whole = (win.0 / src.w).min(win.1 / src.h);
    let (w, h) = if scaling != Scaling::Fill && whole >= 1 {
        (src.w * whole, src.h * whole)
    } else {
        // a window smaller than the picture also lands here: the picture shrinks to fit
        let s = (win.0 as f64 / src.w as f64).min(win.1 as f64 / src.h as f64);
        (((src.w as f64 * s).round() as u32).clamp(1, win.0.max(1)), ((src.h as f64 * s).round() as u32).clamp(1, win.1.max(1)))
    };
    Layout { src, dst: Rect { x: (win.0 - w.min(win.0)) / 2, y: (win.1 - h.min(win.1)) / 2, w, h } }
}

/// The picture on its way to the window: the frame as a texture, and for pictures that are not a whole multiple a bigger
/// copy of it that makes the pixels even ("sharp bilinear": the frame is first enlarged by a whole number without blending,
/// and only the rest of the way down to the window is smoothed, so the pixels keep their edges and are of one size within
/// a fraction of a window pixel).
pub struct Presenter<'r, C> {
    creator: &'r TextureCreator<C>,
    frame: Texture<'r>,
    enlarged: Option<(u32, u32, Texture<'r>)>,
    /// `SCREEN: CRT`: soft pixels, scanlines, a shadow mask, glow and a vignette (see `draw_crt`)
    pub crt: bool,
    /// the scanline and mask picture of the CRT look, for one layout
    crt_mask: Option<(Layout, Texture<'r>)>,
    /// a small blurred copy of the frame for the glow of the CRT look
    crt_glow: Option<Texture<'r>>,
    wide_scratch: Vec<u32>,
}

impl<'r, C> Presenter<'r, C> {
    pub fn new(creator: &'r TextureCreator<C>) -> Result<Self, String> {
        let mut frame = creator.create_texture_streaming(PixelFormatEnum::ARGB8888, WIDE_WIDTH as u32, HEIGHT as u32).map_err(|e| e.to_string())?;
        frame.set_scale_mode(ScaleMode::Nearest);
        Ok(Self { creator, frame, enlarged: None, crt: false, crt_mask: None, crt_glow: None, wide_scratch: Vec::new() })
    }

    /// Copy the frame buffer and the strips of the widescreen view to the texture (see `wide::compose`).
    pub fn upload_wide(&mut self, fb: &Framebuffer, strips: &Strips) -> Result<(), String> {
        self.wide_scratch.resize(WIDE_WIDTH * HEIGHT, 0);
        wide::compose(fb, strips, &mut self.wide_scratch);
        let scratch = &self.wide_scratch;
        self.frame.with_lock(None, |buffer: &mut [u8], pitch: usize| {
            for y in 0..HEIGHT {
                let row = &mut buffer[y * pitch..y * pitch + WIDE_WIDTH * 4];
                for (x, &pixel) in scratch[y * WIDE_WIDTH..(y + 1) * WIDE_WIDTH].iter().enumerate() {
                    row[x * 4..x * 4 + 4].copy_from_slice(&pixel.to_ne_bytes());
                }
            }
        })
    }

    /// Copy the frame buffer to the texture.
    pub fn upload(&mut self, fb: &Framebuffer) -> Result<(), String> {
        self.frame.with_lock(None, |buffer: &mut [u8], pitch: usize| {
            for y in 0..HEIGHT {
                let row = &mut buffer[y * pitch..y * pitch + WIDTH * 4];
                for (x, &pixel) in fb.pixels[y * WIDTH..(y + 1) * WIDTH].iter().enumerate() {
                    row[x * 4..x * 4 + 4].copy_from_slice(&pixel.to_ne_bytes());
                }
            }
        })
    }

    /// Clear the window and draw the picture as `layout` says.
    pub fn present<T: RenderTarget<Context = C>>(&mut self, canvas: &mut Canvas<T>, layout: Layout) -> Result<(), String> {
        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();
        let (src, dst) = (layout.src.sdl(), layout.dst.sdl());
        if self.crt && self.draw_crt(canvas, layout, src, dst) {
            return Ok(());
        }
        if layout.is_fractional() && self.draw_enlarged(canvas, layout, src, dst) {
            return Ok(());
        }
        canvas.copy(&self.frame, src, dst)
    }

    /// The CRT look without shaders, only with what every SDL renderer can do: the frame is drawn smoothed (soft
    /// pixels), a small blurred copy is added on top (glow), and a precomputed picture of scanlines (aligned to the C64
    /// rows), an aperture grille and a vignette darkens it (multiplied). False when the renderer cannot do it.
    fn draw_crt<T: RenderTarget<Context = C>>(&mut self, canvas: &mut Canvas<T>, layout: Layout, src: SdlRect, dst: SdlRect) -> bool {
        if self.crt_mask.as_ref().is_none_or(|(l, _)| *l != layout) {
            let Some(mask) = self.make_crt_mask(layout) else { return false };
            self.crt_mask = Some((layout, mask));
        }
        if self.crt_glow.is_none() {
            if let Ok(mut g) = self.creator.create_texture_target(PixelFormatEnum::ARGB8888, WIDTH as u32 / 4, HEIGHT as u32 / 4) {
                g.set_scale_mode(ScaleMode::Linear);
                g.set_blend_mode(BlendMode::Add);
                g.set_alpha_mod(70);
                self.crt_glow = Some(g);
            }
        }
        self.frame.set_scale_mode(ScaleMode::Linear);
        let drawn = canvas.copy(&self.frame, src, dst).is_ok();
        if drawn {
            if let Some(glow) = self.crt_glow.as_mut() {
                let frame = &self.frame;
                if canvas.with_texture_canvas(glow, |c| {
                    let _ = c.copy(frame, src, None);
                })
                .is_ok()
                {
                    let _ = canvas.copy(glow, None, dst);
                }
            }
        }
        self.frame.set_scale_mode(ScaleMode::Nearest);
        if !drawn {
            return false;
        }
        if let Some((_, mask)) = self.crt_mask.as_ref() {
            let _ = canvas.copy(mask, None, dst);
        }
        true
    }

    fn make_crt_mask(&self, layout: Layout) -> Option<Texture<'r>> {
        let (w, h) = (layout.dst.w, layout.dst.h);
        let mut t = self.creator.create_texture_streaming(PixelFormatEnum::ARGB8888, w, h).ok()?;
        t.set_blend_mode(BlendMode::Mod);
        let pixels = crt_mask_pixels(layout);
        t.with_lock(None, |buffer: &mut [u8], pitch: usize| {
            for y in 0..h as usize {
                let row = &mut buffer[y * pitch..y * pitch + w as usize * 4];
                for x in 0..w as usize {
                    row[x * 4..x * 4 + 4].copy_from_slice(&pixels[y * w as usize + x].to_ne_bytes());
                }
            }
        })
        .ok()?;
        Some(t)
    }

    /// The sharp-bilinear way; false when this renderer cannot draw into a texture (then the plain copy is used).
    fn draw_enlarged<T: RenderTarget<Context = C>>(&mut self, canvas: &mut Canvas<T>, layout: Layout, src: SdlRect, dst: SdlRect) -> bool {
        // a whole number of times at least as big as the window picture, but not absurdly big
        let k = (layout.scale().ceil() as u32).clamp(1, 8);
        let (w, h) = (layout.src.w * k, layout.src.h * k);
        if self.enlarged.as_ref().is_none_or(|(ew, eh, _)| (*ew, *eh) != (w, h)) {
            let Ok(mut t) = self.creator.create_texture_target(PixelFormatEnum::ARGB8888, w, h) else { return false };
            t.set_scale_mode(ScaleMode::Linear);
            self.enlarged = Some((w, h, t));
        }
        let Some((_, _, big)) = self.enlarged.as_mut() else { return false };
        let frame = &self.frame;
        if canvas.with_texture_canvas(big, |c| {
            let _ = c.copy(frame, src, None);
        })
        .is_err()
        {
            return false;
        }
        canvas.copy(big, None, dst).is_ok()
    }
}

/// The multiplier picture of the CRT look for `layout` (ARGB, one value per window pixel of the picture): scanlines
/// that follow the rows of the C64 picture, an aperture grille of red, green and blue columns, and darker edges. The
/// effects grow with the size of a C64 pixel in window pixels, so a small window does not turn grey and stripy.
pub fn crt_mask_pixels(layout: Layout) -> Vec<u32> {
    let (w, h) = (layout.dst.w as usize, layout.dst.h as usize);
    let rows = layout.src.h as f64;
    let scale = h as f64 / rows;
    // 0 at 1.5 window pixels per C64 pixel or less, full at 4 or more
    let strength = ((scale - 1.5) / 2.5).clamp(0.0, 1.0);
    let scan_depth = 0.45 * strength;
    let grille = 0.18 * strength;
    let mut out = vec![0u32; w * h];
    for y in 0..h {
        // where in its C64 row this window row lies, 0..1; the middle is brightest
        let phase = ((y as f64 + 0.5) * rows / h as f64).fract();
        let scan = 1.0 - scan_depth * (1.0 - (std::f64::consts::PI * phase).sin());
        let dy = (y as f64 + 0.5) / h as f64 * 2.0 - 1.0;
        for x in 0..w {
            let dx = (x as f64 + 0.5) / w as f64 * 2.0 - 1.0;
            let vignette = 1.0 - 0.22 * (dx * dx * 0.6 + dy * dy).powf(1.5);
            let base = scan * vignette;
            let mut c = [base * (1.0 - grille); 3];
            c[x % 3] = base;
            let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            out[y * w + x] = 0xFF00_0000 | b(c[0]) << 16 | b(c[1]) << 8 | b(c[2]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_crt_mask_follows_the_rows_and_fades_out_in_small_windows() {
        use super::*;
        let big = layout(Scaling::Whole, false, (1920, 1080)); // 3 x
        let m = crt_mask_pixels(big);
        let px = |x: usize, y: usize| m[y * big.dst.w as usize + x];
        let green = |p: u32| (p >> 8) & 0xFF;
        // in the middle of the picture: the middle row of a C64 row is brighter than its edge rows
        let (cx, cy) = (big.dst.w as usize / 2 / 3 * 3 + 1, big.dst.h as usize / 2 / 3 * 3);
        assert!(green(px(cx, cy + 1)) > green(px(cx, cy)), "{:x} {:x}", px(cx, cy + 1), px(cx, cy));
        // edges are darker than the middle
        assert!(green(px(1, cy + 1)) < green(px(cx, cy + 1)));
        // a 1 x picture gets no scanlines or grille, only the vignette
        let small = layout(Scaling::Whole, false, (384, 272));
        let s = crt_mask_pixels(small);
        let mid = s[136 * 384 + 192];
        assert_eq!(((mid >> 16) & 0xFF, (mid >> 8) & 0xFF, mid & 0xFF), (255, 255, 255));
    }

    use super::*;

    #[test]
    fn whole_pixels_keep_the_border_and_a_whole_factor() {
        let l = layout(Scaling::Whole, true, (1920, 1080));
        assert_eq!((l.src, l.dst.w, l.dst.h), (FRAME, 384 * 3, 272 * 3));
        assert_eq!((l.dst.x, l.dst.y), ((1920 - 1152) / 2, (1080 - 816) / 2));
        assert!(!l.is_fractional());
    }

    #[test]
    fn no_border_cuts_the_border_while_playing_and_gets_five_times_on_full_hd() {
        let l = layout(Scaling::NoBorder, true, (1920, 1080));
        assert_eq!(l.src, PLAY);
        assert_eq!((l.dst.w, l.dst.h), (1520, 1000));
        assert_eq!((l.dst.x, l.dst.y), (200, 40));
        assert!(!l.is_fractional());
        // menus and the title keep their border
        let m = layout(Scaling::NoBorder, false, (1920, 1080));
        assert_eq!((m.src, m.dst.w, m.dst.h), (FRAME, 1152, 816));
    }

    #[test]
    fn fill_uses_the_full_height_with_the_proportions_kept() {
        let l = layout(Scaling::Fill, true, (1920, 1080));
        assert_eq!((l.src, l.dst.h, l.dst.y), (PLAY, 1080, 0));
        assert_eq!(l.dst.w, (304.0f64 * 1080.0 / 200.0).round() as u32);
        assert!(l.is_fractional());
        // the proportions of the picture are kept to a pixel
        assert!((l.dst.w as f64 / l.dst.h as f64 - 304.0 / 200.0).abs() < 0.002);
        // a window that is narrower than the picture: the width decides
        let n = layout(Scaling::Fill, true, (600, 1080));
        assert_eq!((n.dst.w, n.dst.x), (600, 0));
        assert!(n.dst.h < 1080 && n.dst.y > 0);
        // not playing: the whole frame
        assert_eq!(layout(Scaling::Fill, false, (1920, 1080)).src, FRAME);
    }

    #[test]
    fn a_window_smaller_than_the_picture_shrinks_it_to_fit() {
        let l = layout(Scaling::Whole, false, (300, 200));
        assert!(l.dst.w <= 300 && l.dst.h <= 200 && l.dst.w > 0);
        let l = layout(Scaling::NoBorder, true, (100, 100));
        assert!(l.dst.w <= 100 && l.dst.h <= 100);
    }

    /// Bars left/right and top/bottom of the picture in a window of `win`.
    fn bars(l: &Layout, win: (u32, u32)) -> (i64, i64, i64, i64) {
        (l.dst.x as i64, win.0 as i64 - (l.dst.x + l.dst.w) as i64, l.dst.y as i64, win.1 as i64 - (l.dst.y + l.dst.h) as i64)
    }

    #[test]
    fn the_picture_is_centred_in_every_window_and_mode() {
        // exact multiples, odd sizes, and drawable sizes of HiDPI windows (1280 x 720 at 1.25 / 1.5 / 2 = 1600 x 900,
        // 1920 x 1080, 2560 x 1440), where `output_size` (pixels) and not the window size (points) is what is passed
        let sizes = [
            (384, 272), (1280, 720), (1366, 768), (1920, 1080), (2560, 1440), (3840, 2160), (1001, 777), (1000, 1000), (1600, 900),
            (1707, 960), (2561, 1441), (800, 600), (1280, 1024), (1921, 1080), (1919, 1079), (500, 300), (200, 100), (1, 1),
        ];
        for scaling in [Scaling::Whole, Scaling::NoBorder, Scaling::Fill] {
            for playing in [false, true] {
                for win in sizes {
                    let l = layout(scaling, playing, win);
                    assert!(l.dst.w >= 1 && l.dst.h >= 1 && l.dst.x + l.dst.w <= win.0.max(1) && l.dst.y + l.dst.h <= win.1.max(1), "{scaling:?} {playing} {win:?}: {l:?}");
                    let (left, right, top, bottom) = bars(&l, win);
                    assert!((left - right).abs() <= 1 && (top - bottom).abs() <= 1, "{scaling:?} playing={playing} {win:?}: bars {left} {right} {top} {bottom}");
                }
            }
        }
    }

    #[test]
    fn the_bars_of_known_windows() {
        // 1366 x 768: 2 x the frame (768 x 544), bars 299 / 299 and 112 / 112
        assert_eq!(bars(&layout(Scaling::Whole, false, (1366, 768)), (1366, 768)), (299, 299, 112, 112));
        // 1001 x 777: 2 x (768 x 544): 116 / 117 and 116 / 117
        assert_eq!(bars(&layout(Scaling::Whole, true, (1001, 777)), (1001, 777)), (116, 117, 116, 117));
        // 1600 x 900 (1280 x 720 at 1.25): 3 x the frame, 1152 x 816 -> 224 / 224, 42 / 42
        assert_eq!(bars(&layout(Scaling::Whole, false, (1600, 900)), (1600, 900)), (224, 224, 42, 42));
        // NO BORDER at 2560 x 1440: 7 x 304 x 200 = 2128 x 1400
        assert_eq!(bars(&layout(Scaling::NoBorder, true, (2560, 1440)), (2560, 1440)), (216, 216, 20, 20));
        // FILL at 1920 x 1080: full height, bars only left and right
        let l = layout(Scaling::Fill, true, (1920, 1080));
        assert_eq!((bars(&l, (1920, 1080)).2, bars(&l, (1920, 1080)).3), (0, 0));
    }

    #[test]
    fn the_play_area_is_centred_in_the_frame() {
        // what a game shows must have equal border on both sides, or cutting the border moves the picture
        assert_eq!(PLAY.x, WIDTH as u32 - (PLAY.x + PLAY.w));
        assert_eq!(PLAY.y, HEIGHT as u32 - (PLAY.y + PLAY.h));
    }

    #[test]
    fn other_screens() {
        // 4K: 10 x the play area, 1440p: 7 x
        assert_eq!(layout(Scaling::NoBorder, true, (3840, 2160)).dst.h, 2000);
        assert_eq!(layout(Scaling::NoBorder, true, (2560, 1440)).dst.h, 1400);
        // 720p: 3 x
        assert_eq!(layout(Scaling::NoBorder, true, (1280, 720)).dst.h, 600);
        // the play area is exactly what the picture shows: 38 columns, 25 rows, nothing of the border
        assert_eq!((PLAY.x, PLAY.y, PLAY.w, PLAY.h), (40, 36, 304, 200));
    }
}
