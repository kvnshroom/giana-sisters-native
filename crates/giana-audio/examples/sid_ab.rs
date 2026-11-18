//! Renders SID register streams through the SID models, for the A/B against VICE (`tools/audio/sid_ab.py`).
//!
//!   sid_ab <engine> dump <writes.dump> <out.raw>          VICE's writes (`tools/audio/vice_sid_ref.py`), one signed
//!                                                         16-bit sample per cycle from the first write (like
//!                                                         VICE's -residrawoutput)
//!   sid_ab <engine> dump44 <writes.dump> <out.wav>        the same writes, 44.1 kHz band-limited
//!   sid_ab <engine> track <n> <seconds> <out.wav>         the port's own stream: the ported driver plays track n,
//!                                                         one register image per frame, through `AudioEngine`
//!   sid_ab <engine> effects <seconds> <out.wav> <frame>:<id> ...   the port's stream of effects (no track)
//!
//! engine: `resid` (resid-rs 1.1.1 = reSID 0.16), `residfp-default` (libresidfp 1.2.3 defaults), `residfp-huelsbeck`
//! (sidplayfp's profile: curve 0.6, range 0.9), `residfp` (the game's profile, `FpProfile::GIANA`) or `residfp:<curve>,<range>[,weak|strong][,oldcaps][,dcb16][,offset<x>]` (offset: the 6581
//! wave offset, 0 = average digis .. 1 = loud digis, library default 1; dcb16: the DC blocker at 16 Hz, 1 kOhm,
//! instead of the library's 1.6 Hz).
//! The port streams accept only the engines of `SidEngine::parse` (no options).
use giana_audio::chip::{FpProfile, SidEngine, CLOCK_HZ, RATE_HZ};
use giana_audio::player::AudioEngine;
use giana_core::sound_driver::Driver;
use residfp_sys::{ChipModel, CombinedWaveforms, ResidFp, SamplingMethod};
use std::io::Write;

const FRAME_CYCLES: u64 = 63 * 312;

struct FpSpec {
    profile: FpProfile,
    weak: bool,
    old_caps: bool,
    offset: Option<f64>,
}

fn parse_fp(s: &str) -> Option<FpSpec> {
    let base = SidEngine::parse(s.split(',').take(2).collect::<Vec<_>>().join(",").as_str())
        .or_else(|| SidEngine::parse(s.split(',').next()?))?;
    let SidEngine::ResidFp(profile) = base else { return None };
    let mut spec = FpSpec { profile, weak: false, old_caps: false, offset: None };
    for opt in s.split(',').skip(1) {
        match opt.trim() {
            "weak" => spec.weak = true,
            "strong" => spec.profile.strong_combined = true,
            "oldcaps" => spec.old_caps = true,
            "dcb16" => spec.profile.dc_blocker_16hz = true,
            o if o.starts_with("offset") => spec.offset = o[6..].parse().ok(),
            _ => {}
        }
    }
    Some(spec)
}

fn fp_chip(spec: &FpSpec, method: SamplingMethod) -> ResidFp {
    let mut sid = ResidFp::new(ChipModel::Mos6581, CLOCK_HZ as f64, method, RATE_HZ as f64);
    let p = spec.profile;
    sid.set_filter_6581_curve(p.curve);
    sid.set_filter_6581_range(p.range);
    sid.set_combined_waveforms(if spec.weak {
        CombinedWaveforms::Weak
    } else if p.strong_combined {
        CombinedWaveforms::Strong
    } else {
        CombinedWaveforms::Average
    });
    sid.enable_old_6581_caps(spec.old_caps);
    sid.set_dc_blocker(if p.dc_blocker_16hz { 1.0 } else { 0.0 });
    if let Some(o) = spec.offset {
        sid.set_offset_6581(o);
    }
    sid
}

fn read_dump(path: &str) -> Vec<(i64, u8, u8)> {
    std::fs::read_to_string(path)
        .expect("dump")
        .lines()
        .filter_map(|l| {
            let w: Vec<i64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (w.len() == 3).then(|| (w[0], w[1] as u8, w[2] as u8))
        })
        .collect()
}

fn write_wav(path: &str, rate: u32, samples: &[i16]) {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let bytes = (samples.len() * 2) as u32;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [16u32, 0x0001_0001, rate, rate * 2, 0x0010_0002] {
        f.write_all(&v.to_le_bytes()).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&bytes.to_le_bytes()).unwrap();
    for s in samples {
        f.write_all(&s.to_le_bytes()).unwrap();
    }
}

/// VICE's writes through `engine`, from the first write on; `raw`: one sample per cycle, else 44.1 kHz.
fn render_dump(engine: &str, writes: &[(i64, u8, u8)], raw: bool) -> Vec<i16> {
    let start = writes[0].0;
    let end = writes.last().unwrap().0 + 2 * FRAME_CYCLES as i64;
    let mut out = Vec::new();
    if engine == "resid" {
        let mut sid = resid::Sid::new(resid::ChipModel::Mos6581);
        sid.set_sampling_parameters(resid::SamplingMethod::Resample, CLOCK_HZ, RATE_HZ);
        let mut now = start;
        let mut buf = vec![0i16; 8192];
        let mut run = |sid: &mut resid::Sid, cycles: i64, out: &mut Vec<i16>| {
            if raw {
                for _ in 0..cycles {
                    sid.clock();
                    out.push(sid.output());
                }
            } else {
                let mut left = cycles as u32;
                while left > 0 {
                    let (n, rest) = sid.sample(left, &mut buf, 1);
                    out.extend_from_slice(&buf[..n]);
                    left = rest;
                }
            }
        };
        for &(c, r, v) in writes {
            run(&mut sid, c - now, &mut out);
            now = c;
            sid.write(r, v);
        }
        run(&mut sid, end - now, &mut out);
    } else {
        let spec = parse_fp(engine).expect("engine");
        let mut sid = fp_chip(&spec, if raw { SamplingMethod::None } else { SamplingMethod::Resample });
        let mut now = start;
        for &(c, r, v) in writes {
            sid.clock((c - now) as u32, &mut out);
            now = c;
            sid.write(r, v);
        }
        sid.clock((end - now) as u32, &mut out);
    }
    out
}

/// The port's stream: the ported driver, one call per frame, its register image queued at the start of each frame
/// (what the app does), through `AudioEngine`; exactly one frame of samples per frame.
fn render_port(engine: SidEngine, track: Option<u8>, effects: &[(u32, u8)], seconds: f64) -> Vec<i16> {
    let mut d = Driver::new();
    d.call(0x4346, 0, 0, 0xF0, false);
    if let Some(t) = track {
        d.call(0x49E6, t, 0, 0, false);
    }
    let mut audio = AudioEngine::new_live_with(engine);
    let frames = (seconds * CLOCK_HZ as f64 / FRAME_CYCLES as f64) as u64;
    let mut out: Vec<i16> = Vec::new();
    let mut last = [0u8; 25];
    for frame in 0..frames {
        // like the VICE stub: frame 0 is the image after init and track start, then effect requests and the tick
        if frame > 0 {
            for &(f, id) in effects {
                if f as u64 == frame - 1 {
                    d.call(0x4987, id, 0, 0, false);
                }
            }
            d.call(0x439E, 0, 0, 0, false);
        }
        let mut regs = [0u8; 25];
        regs.copy_from_slice(&d.sid[..25]);
        let writes: Vec<(u32, u8, u8)> = (0..25).filter(|&r| frame == 0 || regs[r] != last[r]).map(|r| (0, r as u8, regs[r])).collect();
        last = regs;
        audio.live_regs = Some(regs);
        audio.queue_timed_writes(writes);
        let want = ((frame + 1) * FRAME_CYCLES * RATE_HZ as u64 / CLOCK_HZ as u64) as usize;
        let mut buf = vec![0i16; want - out.len()];
        audio.fill_audio_buffer(&mut buf);
        out.extend(buf);
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let engine = a[1].as_str();
    match a[2].as_str() {
        "dump" | "dump44" => {
            let raw = a[2] == "dump";
            let out = render_dump(engine, &read_dump(&a[3]), raw);
            if raw {
                let mut f = std::io::BufWriter::new(std::fs::File::create(&a[4]).unwrap());
                for s in &out {
                    f.write_all(&s.to_le_bytes()).unwrap();
                }
            } else {
                write_wav(&a[4], RATE_HZ, &out);
            }
            println!("{engine}: {} samples -> {}", out.len(), a[4]);
        }
        "track" | "effects" => {
            let e = SidEngine::parse(engine).expect("engine of the port: resid, residfp, residfp-default, residfp:c,r");
            let (track, seconds, out_path, fx) = if a[2] == "track" {
                (Some(a[3].parse().unwrap()), a[4].parse().unwrap(), &a[5], Vec::new())
            } else {
                let fx = a[5..]
                    .iter()
                    .map(|s| {
                        let (f, i) = s.split_once(':').unwrap();
                        (f.parse().unwrap(), u8::from_str_radix(i.trim_start_matches("0x"), if i.starts_with("0x") { 16 } else { 10 }).unwrap())
                    })
                    .collect();
                (None, a[3].parse().unwrap(), &a[4], fx)
            };
            let out = render_port(e, track, &fx, seconds);
            write_wav(out_path, RATE_HZ, &out);
            println!("{engine}: {} samples -> {out_path}", out.len());
        }
        m => panic!("unknown mode {m}"),
    }
}
