//! The SID chip model behind the sound: reSIDfp (libresidfp 1.2.3, the model of sidplayfp and of VICE from 3.11 on) or
//! resid-rs (a Rust port of reSID 0.16, what the port used up to 0.4.x). Both run as a 6581 at the PAL clock and give
//! 44.1 kHz band-limited samples.
//!
//! The choice: [`SidEngine::current`], which is the build's default unless the environment variable `GIANA_SID` names
//! another engine (`resid`, `residfp`, `residfp-default`, `residfp-huelsbeck` or `residfp:<curve>,<range>`). A build without the `residfp`
//! feature has no C++ in it and always plays resid-rs.

use std::sync::OnceLock;

/// C64 PAL clock in Hz.
pub const CLOCK_HZ: u32 = 985_248;
/// Output rate of the chips.
pub const RATE_HZ: u32 = 44_100;

/// One SID chip that turns register writes and clock cycles into samples.
pub trait SidChip: Send {
    fn write(&mut self, reg: u8, value: u8);
    fn reset(&mut self);
    /// Runs the chip for up to `cycles` cycles and writes the samples into `buf`. Returns how many samples were written
    /// and how many of the cycles were not run because `buf` is full (the contract of resid-rs's `Sid::sample`).
    fn sample(&mut self, cycles: u32, buf: &mut [i16]) -> (usize, u32);
}

/// Settings of a reSIDfp 6581 (see the sidplayfp manual, `sidplayfp.ini`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FpProfile {
    /// `Filter6581Curve`: 0 = light, 1 = dark (library default 0.5)
    pub curve: f64,
    /// `Filter6581Range`: 0 = dark, 1 = bright (library default 0.5)
    pub range: f64,
    /// `CombinedWaveforms` STRONG instead of the library's AVERAGE
    pub strong_combined: bool,
    /// the DC blocker of the output at about 16 Hz (1 kOhm, like reSID's external filter) instead of 1.6 Hz
    pub dc_blocker_16hz: bool,
}

impl FpProfile {
    /// The library's defaults.
    pub const LIBRARY: Self = Self { curve: 0.5, range: 0.5, strong_combined: false, dc_blocker_16hz: false };
    /// sidplayfp's per-composer table (`--fcurve=auto --frange=auto`) for Chris Hülsbeck.
    pub const HUELSBECK: Self = Self { curve: 0.6, range: 0.9, strong_combined: false, dc_blocker_16hz: false };
    /// What the game uses: the library's filter curve and range, strong combined waveforms, the 16 Hz DC blocker. Chosen
    /// by measurement: of the settings tried, the closest to VICE 3.10's reSID 1.0 on Giana's music and effects
    /// (`.claude/handoff/sid-research.md`, section 7). The 16 Hz DC blocker changes nothing audible; it lets the
    /// start-up click of a chip settle ten times faster, which keeps the two chips of the split mix from clipping.
    pub const GIANA: Self = Self { curve: 0.5, range: 0.5, strong_combined: true, dc_blocker_16hz: true };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidEngine {
    /// resid-rs 1.1.1 (reSID 0.16), resampling
    Resid,
    /// libresidfp 1.2.3, 6581, sinc resampling, with these settings
    ResidFp(FpProfile),
}

impl SidEngine {
    /// What a build plays by default.
    pub const DEFAULT: Self = if cfg!(feature = "residfp") { SidEngine::ResidFp(FpProfile::GIANA) } else { SidEngine::Resid };

    /// The engine every new chip uses: [`SidEngine::DEFAULT`] or what `GIANA_SID` asks for (read once).
    pub fn current() -> Self {
        static CURRENT: OnceLock<SidEngine> = OnceLock::new();
        *CURRENT.get_or_init(|| std::env::var("GIANA_SID").ok().and_then(|v| Self::parse(&v)).unwrap_or(Self::DEFAULT))
    }

    /// `resid`, `residfp` (the game's profile), `residfp-default` (the library's), `residfp-huelsbeck` (sidplayfp's) or
    /// `residfp:<curve>,<range>` (with the library's other settings).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "resid" | "resid-rs" => Some(Self::Resid),
            "residfp" => Some(Self::ResidFp(FpProfile::GIANA)),
            "residfp-default" => Some(Self::ResidFp(FpProfile::LIBRARY)),
            "residfp-huelsbeck" => Some(Self::ResidFp(FpProfile::HUELSBECK)),
            _ => {
                let (curve, range) = s.strip_prefix("residfp:")?.split_once(',')?;
                let (curve, range) = (curve.trim().parse().ok()?, range.trim().parse().ok()?);
                Some(Self::ResidFp(FpProfile { curve, range, ..FpProfile::LIBRARY }))
            }
        }
    }

    /// A new 6581 of this engine (resid-rs when the build has no reSIDfp).
    pub fn chip(self) -> Box<dyn SidChip> {
        match self {
            #[cfg(feature = "residfp")]
            SidEngine::ResidFp(f) => Box::new(FpChip::new(f)),
            _ => Box::new(ResidChip::new()),
        }
    }
}

/// resid-rs, a 6581 that samples like VICE's reSID setting "resampling": band-limited to 20 kHz instead of picking every
/// 22nd cycle, which folds the chip's high overtones back into the audible range (a harsh, "digital" top end).
pub struct ResidChip(resid::Sid);

impl ResidChip {
    pub fn new() -> Self {
        let mut sid = resid::Sid::new(resid::ChipModel::Mos6581);
        sid.set_sampling_parameters(resid::SamplingMethod::Resample, CLOCK_HZ, RATE_HZ);
        Self(sid)
    }
}

impl Default for ResidChip {
    fn default() -> Self {
        Self::new()
    }
}

impl SidChip for ResidChip {
    fn write(&mut self, reg: u8, value: u8) {
        self.0.write(reg, value);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn sample(&mut self, cycles: u32, buf: &mut [i16]) -> (usize, u32) {
        self.0.sample(cycles, buf, 1)
    }
}

/// libresidfp as a [`SidChip`]. The library runs whole cycle counts at once; samples that do not fit into the caller's
/// buffer wait in `spill` for the next call (the chip's time is not affected, only when the samples are handed out).
#[cfg(feature = "residfp")]
pub struct FpChip {
    sid: residfp_sys::ResidFp,
    spill: Vec<i16>,
    spill_pos: usize,
}

#[cfg(feature = "residfp")]
impl FpChip {
    pub fn new(p: FpProfile) -> Self {
        use residfp_sys::{ChipModel, CombinedWaveforms, ResidFp, SamplingMethod};
        let mut sid = ResidFp::new(ChipModel::Mos6581, CLOCK_HZ as f64, SamplingMethod::Resample, RATE_HZ as f64);
        // the range is a setting of the library's shared filter tables, the same for every chip of the process: all
        // chips of the game are made with the same profile
        sid.set_filter_6581_curve(p.curve);
        sid.set_filter_6581_range(p.range);
        sid.set_combined_waveforms(if p.strong_combined { CombinedWaveforms::Strong } else { CombinedWaveforms::Average });
        sid.set_dc_blocker(if p.dc_blocker_16hz { 1.0 } else { 0.0 });
        Self { sid, spill: Vec::new(), spill_pos: 0 }
    }

    fn drain(&mut self, buf: &mut [i16]) -> usize {
        let n = (self.spill.len() - self.spill_pos).min(buf.len());
        buf[..n].copy_from_slice(&self.spill[self.spill_pos..self.spill_pos + n]);
        self.spill_pos += n;
        if self.spill_pos == self.spill.len() {
            self.spill.clear();
            self.spill_pos = 0;
        }
        n
    }
}

#[cfg(feature = "residfp")]
impl SidChip for FpChip {
    fn write(&mut self, reg: u8, value: u8) {
        self.sid.write(reg, value);
    }
    fn reset(&mut self) {
        self.sid.reset();
        self.spill.clear();
        self.spill_pos = 0;
    }
    fn sample(&mut self, cycles: u32, buf: &mut [i16]) -> (usize, u32) {
        let mut n = self.drain(buf);
        if n == buf.len() {
            return (n, cycles);
        }
        self.sid.clock(cycles, &mut self.spill);
        n += self.drain(&mut buf[n..]);
        (n, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_engine_names() {
        assert_eq!(SidEngine::parse("resid"), Some(SidEngine::Resid));
        assert_eq!(SidEngine::parse("ResidFP"), Some(SidEngine::ResidFp(FpProfile::GIANA)));
        assert_eq!(SidEngine::parse("residfp-default"), Some(SidEngine::ResidFp(FpProfile::LIBRARY)));
        assert_eq!(SidEngine::parse("residfp:0.4, 0.7"), Some(SidEngine::ResidFp(FpProfile { curve: 0.4, range: 0.7, ..FpProfile::LIBRARY })));
        assert_eq!(SidEngine::parse("vice"), None);
    }

    /// The same cycles give the same samples, whatever the sizes of the buffers they are asked for in.
    #[test]
    fn samples_do_not_depend_on_the_buffer_sizes() {
        for engine in [SidEngine::Resid, SidEngine::ResidFp(FpProfile::GIANA)] {
            let run = |sizes: &[usize]| {
                std::thread::scope(|s| {
                    s.spawn(|| {
                        let mut chip = engine.chip();
                        for (r, v) in [(0x18u8, 0x1Fu8), (0x17, 0xF1), (0x16, 0x40), (0x05, 0x09), (0x06, 0xF0), (0x01, 0x10), (0x04, 0x41), (0x03, 0x08)] {
                            chip.write(r, v);
                        }
                        let mut out = Vec::new();
                        let mut cycles = 19_656u32 * 5;
                        let mut i = 0;
                        while cycles > 0 || out.len() < 4000 {
                            let mut buf = vec![0i16; sizes[i % sizes.len()]];
                            i += 1;
                            let (n, left) = chip.sample(cycles, &mut buf);
                            out.extend_from_slice(&buf[..n]);
                            cycles = left;
                            if n == 0 && left == 0 {
                                break;
                            }
                        }
                        out.truncate(4000);
                        out
                    })
                    .join()
                    .unwrap()
                })
            };
            let a = run(&[4096]);
            let b = run(&[1, 7, 300, 2]);
            assert_eq!(a.len(), 4000, "{engine:?}");
            assert!(a == b, "{engine:?}: different samples with other buffer sizes");
        }
    }
}
