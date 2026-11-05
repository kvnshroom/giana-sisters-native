//! Scripted input, traces and lockstep comparison of two machines.

use crate::machine::Machine;
use crate::snapshot::Diff;

/// Joystick input per frame, written like the scripts of `tools/c30/emu.c`: tokens `<keys>:<frames>` separated by blanks,
/// keys `U D L R F` (or `-` for none), e.g. `"R:200 RU:20 R:200"`. After the last token nothing is pressed.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pressed: Vec<u8>,
}

impl Script {
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut pressed = Vec::new();
        for tok in text.split_whitespace() {
            let (keys, n) = tok.split_once(':').ok_or_else(|| format!("bad token '{tok}'"))?;
            let n: usize = n.parse().map_err(|_| format!("bad count in '{tok}'"))?;
            let mut k = 0u8;
            for c in keys.chars() {
                k |= match c {
                    'U' => crate::joy::UP,
                    'D' => crate::joy::DOWN,
                    'L' => crate::joy::LEFT,
                    'R' => crate::joy::RIGHT,
                    'F' => crate::joy::FIRE,
                    '-' => 0,
                    _ => return Err(format!("bad key '{c}' in '{tok}'")),
                };
            }
            pressed.extend(std::iter::repeat_n(k, n));
        }
        Ok(Script { pressed })
    }

    /// Pressed joystick bits for frame `i`.
    pub fn at(&self, i: u64) -> u8 {
        self.pressed.get(i as usize).copied().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.pressed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pressed.is_empty()
    }
}

/// Values of memory cells sampled once per frame (at [`Machine::sync_pc`]), like `tools/c30/vtrace.py`: row `i` is the
/// state *before* frame `i` runs; the joystick for frame `i` is set after sampling.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    pub addrs: Vec<u16>,
    pub rows: Vec<Vec<u8>>,
}

impl Trace {
    /// Runs `frames` frames of `script` and samples `addrs` (`frames` rows). The machine should be at its frame start.
    pub fn run(m: &mut Machine, script: &Script, frames: u64, addrs: &[u16]) -> Result<Trace, String> {
        let mut t = Trace { addrs: addrs.to_vec(), rows: Vec::new() };
        for i in 0..frames {
            t.rows.push(addrs.iter().map(|&a| m.peek(a)).collect());
            m.set_joy(script.at(i));
            m.frame()?;
        }
        Ok(t)
    }

    /// `n,v1,v2,...` per line (the format of the VICE reference traces).
    pub fn to_csv(&self) -> String {
        let mut s = String::new();
        for (i, r) in self.rows.iter().enumerate() {
            s += &i.to_string();
            for v in r {
                s += &format!(",{v}");
            }
            s += "\n";
        }
        s
    }

    /// Reads a CSV trace (`n,v1,v2,...` per line) for the given addresses.
    pub fn from_csv(text: &str, addrs: &[u16]) -> Trace {
        let rows = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split(',').skip(1).map(|v| v.trim().parse().unwrap()).collect())
            .collect();
        Trace { addrs: addrs.to_vec(), rows }
    }

    /// The same trace without the columns of the given addresses.
    pub fn without(&self, drop: &[u16]) -> Trace {
        let keep: Vec<usize> = (0..self.addrs.len()).filter(|&i| !drop.contains(&self.addrs[i])).collect();
        Trace {
            addrs: keep.iter().map(|&i| self.addrs[i]).collect(),
            rows: self.rows.iter().map(|r| keep.iter().map(|&i| r[i]).collect()).collect(),
        }
    }

    /// First frame and cell index where the traces differ (compared over the common length).
    pub fn first_difference(&self, other: &Trace) -> Option<(usize, usize)> {
        for (i, (a, b)) in self.rows.iter().zip(&other.rows).enumerate() {
            if let Some(j) = a.iter().zip(b).position(|(x, y)| x != y) {
                return Some((i, j));
            }
        }
        None
    }
}

/// Runs two machines (typically the same state, one with hooks) for `frames` frames of the same input and returns the
/// first frame at which they differ in the `ranges` (RAM), the VIC/SID/colour state or the `$D01E` reads.
/// `Err((frame, diff))`: `frame` counts from 0 for the first frame run here; `diff` has `left` = `a`, `right` = `b`.
pub fn lockstep(a: &mut Machine, b: &mut Machine, script: &Script, frames: u64, ranges: &[(u16, u16)]) -> Result<(), (u64, Diff)> {
    for i in 0..frames {
        for m in [&mut *a, &mut *b] {
            m.set_joy(script.at(i));
            m.frame().expect("CPU jam while running in lockstep");
        }
        let d = a.snapshot().diff(&b.snapshot(), ranges);
        if !d.is_empty() {
            return Err((i, d));
        }
    }
    Ok(())
}
