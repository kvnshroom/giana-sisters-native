//! libresidfp 1.2.3 (reSIDfp), the SID chip model of VICE (trunk) and sidplayfp, built from the vendored sources.
//!
//! [`ResidFp`] owns one chip. The C++ class is reached through the `extern "C"` shim in `shim/residfp_shim.cpp`.

use std::os::raw::{c_char, c_double, c_int, c_short, c_uchar, c_uint};

#[repr(C)]
struct RawSid {
    _private: [u8; 0],
}

extern "C" {
    fn giana_residfp_new(model: c_int) -> *mut RawSid;
    fn giana_residfp_delete(sid: *mut RawSid);
    fn giana_residfp_reset(sid: *mut RawSid);
    fn giana_residfp_write(sid: *mut RawSid, offset: c_int, value: c_uchar);
    fn giana_residfp_peek(sid: *const RawSid, offset: c_int) -> c_uchar;
    fn giana_residfp_set_sampling(sid: *mut RawSid, clock_hz: c_double, method: c_int, rate_hz: c_double) -> c_int;
    fn giana_residfp_clock(sid: *mut RawSid, cycles: c_uint, buf: *mut c_short) -> c_int;
    fn giana_residfp_set_filter6581_curve(sid: *mut RawSid, curve: c_double);
    fn giana_residfp_set_filter6581_range(sid: *mut RawSid, range: c_double);
    fn giana_residfp_set_filter8580_curve(sid: *mut RawSid, curve: c_double);
    fn giana_residfp_set_combined_waveforms(sid: *mut RawSid, strength: c_int) -> c_int;
    fn giana_residfp_enable_filter(sid: *mut RawSid, enable: c_int);
    fn giana_residfp_enable_old_6581_caps(sid: *mut RawSid, enable: c_int);
    fn giana_residfp_set_dac_leakage(sid: *mut RawSid, level: c_double);
    fn giana_residfp_set_offset6581(sid: *mut RawSid, offset: c_double);
    fn giana_residfp_set_dcb_res(sid: *mut RawSid, res: c_double);
    fn giana_residfp_version() -> *const c_char;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipModel {
    Mos6581,
    Csg8580,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingMethod {
    /// linear interpolation (fast, aliases)
    Decimate,
    /// two-pass sinc resampler, band-limited (what sidplayfp and VICE use for quality)
    Resample,
    /// no resampling: one sample per cycle (the rate is ignored); for measurements
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinedWaveforms {
    Average,
    Weak,
    Strong,
}

/// One SID chip (reSIDfp).
pub struct ResidFp {
    raw: *mut RawSid,
    clock_hz: f64,
    rate_hz: f64,
}

// SAFETY: the chip is a plain C++ object without thread affinity or shared state (the filter tables it shares with
// other chips are built once, under the library's own lock, and only read afterwards); `ResidFp` owns it alone and
// all access goes through `&mut self` (or `&self` for the read-only `peek`).
unsafe impl Send for ResidFp {}

impl ResidFp {
    /// A chip of `model`, sampling at `rate_hz` from a `clock_hz` system clock (985248 Hz for a PAL C64).
    pub fn new(model: ChipModel, clock_hz: f64, method: SamplingMethod, rate_hz: f64) -> Self {
        // SAFETY: the shim returns either a new object or null (on a C++ exception)
        let raw = unsafe { giana_residfp_new(if model == ChipModel::Csg8580 { 1 } else { 0 }) };
        assert!(!raw.is_null(), "libresidfp: cannot create the chip");
        let mut sid = Self { raw, clock_hz, rate_hz };
        let m = match method {
            SamplingMethod::Decimate => 0,
            SamplingMethod::Resample => 1,
            SamplingMethod::None => 2,
        };
        if method == SamplingMethod::None {
            sid.rate_hz = clock_hz;
        }
        // SAFETY: `raw` is valid
        let ok = unsafe { giana_residfp_set_sampling(sid.raw, clock_hz, m, rate_hz) };
        assert!(ok == 1, "libresidfp: sampling parameters rejected ({clock_hz} Hz -> {rate_hz} Hz)");
        sid.reset();
        sid
    }

    pub fn version() -> &'static str {
        // SAFETY: a static NUL-terminated string in the library
        unsafe { std::ffi::CStr::from_ptr(giana_residfp_version()) }.to_str().unwrap_or("?")
    }

    pub fn reset(&mut self) {
        // SAFETY: `raw` is valid for the lifetime of `self`
        unsafe { giana_residfp_reset(self.raw) }
    }

    pub fn write(&mut self, reg: u8, value: u8) {
        // SAFETY: as above; the chip masks the register number itself
        unsafe { giana_residfp_write(self.raw, (reg & 0x1F) as c_int, value) }
    }

    /// A register as the chip would read it, without side effects.
    pub fn peek(&self, reg: u8) -> u8 {
        // SAFETY: as above
        unsafe { giana_residfp_peek(self.raw, (reg & 0x1F) as c_int) }
    }

    /// The most samples `cycles` can produce.
    pub fn max_samples(&self, cycles: u32) -> usize {
        (cycles as f64 * self.rate_hz / self.clock_hz).ceil() as usize + 1
    }

    /// Runs the chip `cycles` cycles and appends the samples to `out`.
    pub fn clock(&mut self, cycles: u32, out: &mut Vec<i16>) {
        let start = out.len();
        out.resize(start + self.max_samples(cycles), 0);
        // SAFETY: `out` has room for `max_samples(cycles)` samples after `start`, the most the chip writes
        let n = unsafe { giana_residfp_clock(self.raw, cycles as c_uint, out[start..].as_mut_ptr()) };
        out.truncate(start + n.max(0) as usize);
    }

    /// 0 = light, 1 = dark (default 0.5)
    pub fn set_filter_6581_curve(&mut self, curve: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_filter6581_curve(self.raw, curve.clamp(0.0, 1.0)) }
    }

    /// 0 = dark, 1 = bright (default 0.5)
    pub fn set_filter_6581_range(&mut self, range: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_filter6581_range(self.raw, range.clamp(0.0, 1.0)) }
    }

    pub fn set_filter_8580_curve(&mut self, curve: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_filter8580_curve(self.raw, curve.clamp(0.0, 1.0)) }
    }

    pub fn set_combined_waveforms(&mut self, strength: CombinedWaveforms) {
        let s = match strength {
            CombinedWaveforms::Average => 0,
            CombinedWaveforms::Weak => 1,
            CombinedWaveforms::Strong => 2,
        };
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_combined_waveforms(self.raw, s) };
    }

    pub fn enable_filter(&mut self, enable: bool) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_enable_filter(self.raw, enable as c_int) }
    }

    /// The 2200 pF filter capacitors of the early boards (ASSY 326298): lower cutoff.
    pub fn enable_old_6581_caps(&mut self, enable: bool) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_enable_old_6581_caps(self.raw, enable as c_int) }
    }

    /// 0 = no leakage, 1 = standard (default)
    pub fn set_dac_leakage(&mut self, level: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_dac_leakage(self.raw, level.clamp(0.0, 1.0)) }
    }

    /// 0 = average digis, 1 = loud digis (default)
    pub fn set_offset_6581(&mut self, offset: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_offset6581(self.raw, offset.clamp(0.0, 1.0)) }
    }

    /// DC blocker: 0 = about 1.6 Hz (default), 1 = about 16 Hz
    pub fn set_dc_blocker(&mut self, res: f64) {
        // SAFETY: `raw` is valid
        unsafe { giana_residfp_set_dcb_res(self.raw, res.clamp(0.0, 1.0)) }
    }
}

impl Drop for ResidFp {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `giana_residfp_new` and is deleted once
        unsafe { giana_residfp_delete(self.raw) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sid: &mut ResidFp) -> Vec<i16> {
        sid.write(0x18, 0x0F);
        sid.write(0x05, 0x00);
        sid.write(0x06, 0xF0);
        sid.write(0x00, 0x00);
        sid.write(0x01, 0x1C); // about 440 Hz
        sid.write(0x04, 0x21); // saw, gate
        let mut out = Vec::new();
        sid.clock(985_248 / 10, &mut out);
        out
    }

    #[test]
    fn makes_a_tone_with_the_expected_number_of_samples() {
        let mut sid = ResidFp::new(ChipModel::Mos6581, 985_248.0, SamplingMethod::Resample, 44_100.0);
        let out = tone(&mut sid);
        assert!((4405..=4411).contains(&out.len()), "{} samples for 0.1 s", out.len());
        let tail = &out[out.len() / 2..];
        let swing = *tail.iter().max().unwrap() as i32 - *tail.iter().min().unwrap() as i32;
        assert!(swing > 2000, "the saw is audible, swing {swing}");
        assert_eq!(ResidFp::version(), "1.2.3");
    }

    fn filtered() -> ResidFp {
        let mut s = ResidFp::new(ChipModel::Mos6581, 985_248.0, SamplingMethod::Resample, 44_100.0);
        s.set_filter_6581_curve(0.6);
        s.set_filter_6581_range(0.9);
        s.write(0x17, 0xF1);
        s
    }

    /// A chip's samples depend only on what that chip is given: not on other chips run in between, nor on the thread
    /// (each chip has its own position in the dither of the filter model, see the patch in `vendor/README.md`).
    #[test]
    fn is_deterministic() {
        let alone = tone(&mut filtered());
        let (mut a, mut b) = (filtered(), filtered());
        b.write(0x18, 0x1F);
        b.write(0x04, 0x81);
        let mut other = Vec::new();
        b.clock(5000, &mut other);
        let first = tone(&mut a);
        b.clock(5000, &mut other);
        assert_eq!(alone, first);
        let on_thread = std::thread::spawn(|| tone(&mut filtered())).join().unwrap();
        assert_eq!(alone, on_thread);
    }
}
