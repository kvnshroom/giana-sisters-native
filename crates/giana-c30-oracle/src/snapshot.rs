//! State snapshots, diffs and the picture of a frame.

use crate::hw::VicWrite;
use crate::machine::Machine;
use giana_engine_c::render::FrameState;

/// All state of the machine at one moment plus the logs of the frame that led to it.
#[derive(Clone)]
pub struct Snapshot {
    pub ram: Box<[u8; 65536]>,
    pub color: [u8; 1024],
    /// VIC register image (`$D000..$D02E`)
    pub vic: [u8; 47],
    /// SID register image (`$D400..$D418`)
    pub sid: [u8; 25],
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub flags: u8,
    pub frame: u64,
    pub vic_log: Vec<VicWrite>,
    pub sid_log: Vec<(u16, u8, u8)>,
    /// `(line, value)` of every `$D01E` read of the frame: the collision bits the game got
    pub coll_reads: Vec<(u16, u8)>,
}

/// A sprite as the VIC registers describe it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpriteInfo {
    pub enabled: bool,
    pub x: u16,
    pub y: u8,
    pub color: u8,
    /// shape pointer (`ram[$CBF8 + n]` in the game's bank)
    pub pointer: u8,
    pub multicolor: bool,
    pub expand_x: bool,
    pub expand_y: bool,
    pub behind_chars: bool,
}

impl Snapshot {
    pub(crate) fn of(m: &Machine) -> Snapshot {
        let mut vic = [0u8; 47];
        vic.copy_from_slice(&m.hw.vic[..47]);
        let mut sid = [0u8; 25];
        sid.copy_from_slice(&m.hw.sid[..25]);
        Snapshot {
            ram: m.hw.ram.clone(),
            color: m.hw.color,
            vic,
            sid,
            a: m.cpu.a,
            x: m.cpu.x,
            y: m.cpu.y,
            sp: m.cpu.sp,
            pc: m.cpu.pc,
            flags: m.cpu.flags(false),
            frame: m.frame_no,
            vic_log: m.hw.vic_log.clone(),
            sid_log: m.hw.sid_log.clone(),
            coll_reads: m.hw.coll_reads.clone(),
        }
    }

    pub fn byte(&self, a: u16) -> u8 {
        self.ram[a as usize]
    }

    pub fn sprite(&self, i: usize) -> SpriteInfo {
        let v = &self.vic;
        let bit = |r: usize| v[r] & (1 << i) != 0;
        SpriteInfo {
            enabled: bit(0x15),
            x: v[2 * i] as u16 | if bit(0x10) { 256 } else { 0 },
            y: v[2 * i + 1],
            color: v[0x27 + i] & 15,
            pointer: self.ram[0xCBF8 + i],
            multicolor: bit(0x1C),
            expand_x: bit(0x1D),
            expand_y: bit(0x17),
            behind_chars: bit(0x1B),
        }
    }

    /// Differences to `other` in the given RAM ranges (inclusive, `(lo, hi)`), the VIC and SID images, colour RAM and the
    /// collision reads of the frame. `self` is the "left" side (usually the hooked machine), `other` the "right" (the
    /// unhooked oracle); an entry is `(address, left, right)`.
    pub fn diff(&self, other: &Snapshot, ranges: &[(u16, u16)]) -> Diff {
        let mut d = Diff::default();
        for &(lo, hi) in ranges {
            for a in lo..=hi {
                if self.ram[a as usize] != other.ram[a as usize] {
                    d.ram.push((a, self.ram[a as usize], other.ram[a as usize]));
                }
            }
        }
        for r in 0..47 {
            if self.vic[r] != other.vic[r] {
                d.vic.push((r as u8, self.vic[r], other.vic[r]));
            }
        }
        for r in 0..25 {
            if self.sid[r] != other.sid[r] {
                d.sid.push((r as u8, self.sid[r], other.sid[r]));
            }
        }
        for i in 0..1000 {
            if self.color[i] != other.color[i] {
                d.color.push((i as u16, self.color[i], other.color[i]));
            }
        }
        d.coll_reads = self.coll_reads != other.coll_reads;
        d.vic_log = self.vic_log != other.vic_log;
        d
    }

    /// RAM that differs anywhere (all 64 KB).
    pub fn diff_all(&self, other: &Snapshot) -> Diff {
        self.diff(other, &[(0, 0xFFFF)])
    }
}

/// Result of [`Snapshot::diff`]. Empty means identical in everything compared.
#[derive(Default, Debug, Clone)]
pub struct Diff {
    pub ram: Vec<(u16, u8, u8)>,
    pub vic: Vec<(u8, u8, u8)>,
    pub sid: Vec<(u8, u8, u8)>,
    pub color: Vec<(u16, u8, u8)>,
    /// the `$D01E` reads of the frame differ (line or value)
    pub coll_reads: bool,
    /// the VIC write logs of the frame differ (line, cycle, register or value)
    pub vic_log: bool,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.ram.is_empty() && self.vic.is_empty() && self.sid.is_empty() && self.color.is_empty() && !self.coll_reads
    }

    /// Drops RAM differences at the given addresses (scratch cells a port is not expected to reproduce).
    pub fn ignore_ram(mut self, cells: &[u16]) -> Diff {
        self.ram.retain(|(a, _, _)| !cells.contains(a));
        self
    }

    /// Short human-readable summary (first few entries of each kind).
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let mut part = |name: &str, n: usize, first: String| {
            if n > 0 {
                s += &format!("{name}: {n} differ, first {first}\n");
            }
        };
        part("ram", self.ram.len(), format!("{:04X?}", &self.ram[..self.ram.len().min(8)]));
        part("vic", self.vic.len(), format!("{:02X?}", &self.vic[..self.vic.len().min(8)]));
        part("sid", self.sid.len(), format!("{:02X?}", &self.sid[..self.sid.len().min(8)]));
        part("color", self.color.len(), format!("{:03X?}", &self.color[..self.color.len().min(8)]));
        if self.coll_reads {
            s += "$D01E reads differ\n";
        }
        s
    }
}

/// The finished picture of a frame, ready for `giana_engine_c::render::FrameState`.
pub struct Picture {
    pub ram: Vec<u8>,
    pub vic: [u8; 47],
    pub color: Vec<u8>,
}

impl Picture {
    pub fn state(&self) -> FrameState<'_> {
        FrameState { ram: &self.ram, vic: self.vic, color: &self.color }
    }
}
