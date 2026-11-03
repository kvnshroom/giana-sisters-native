use resid::{ChipModel, Sid};

use giana_core::sound_driver::Driver;

/// Sound driver tracks (`$49E6`, A = track): the tune the original starts at its title / menu and the music of stage 1.
const TITLE_TRACK: u8 = 3;
const STAGE1_TRACK: u8 = 1;

/// Runs the ported sound driver for `frames` frames (one `$439E` tick each, 50 Hz) and returns the SID register changes.
/// Frame 0 is the state right after the track request, before the first tick.
fn generate_events(track: u8, frames: u16) -> Vec<SidEvent> {
    let mut d = Driver::new();
    d.call(0x4346, 0, 0, 0xF0, false); // init
    d.call(0x49E6, track, 0, 0, false); // start the track
    let mut prev = [0u8; 25];
    let mut events = Vec::new();
    for frame in 0..frames {
        if frame > 0 {
            d.call(0x439E, 0, 0, 0, false);
        }
        for reg in 0..25 {
            if d.sid[reg] != prev[reg] {
                prev[reg] = d.sid[reg];
                events.push(SidEvent { frame, reg: reg as u8, val: prev[reg] });
            }
        }
    }
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicTrack {
    Title,
    Stage1,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SidEvent {
    pub frame: u16,
    pub reg: u8,
    pub val: u8,
}

pub struct AudioEngine {
    pub sid: Sid,
    pub current_track: MusicTrack,
    pub events: Vec<SidEvent>,
    pub max_frames: u16,
    pub loop_start: u16,
    pub cur_frame: u16,
    pub cur_event_idx: usize,
    pub pending_cycles: u32,
    /// SID registers of the ported sound driver (`$D400..$D418`); when set they replace the generated tune
    pub live_regs: Option<[u8; 25]>,
    last_regs: [u8; 25],
}

impl AudioEngine {
    pub fn new() -> Self {
        let sid = Sid::new(ChipModel::Mos6581);

        let mut engine = Self {
            sid,
            current_track: MusicTrack::Title,
            events: Vec::new(),
            max_frames: 0,
            loop_start: 0,
            cur_frame: 0,
            cur_event_idx: 0,
            pending_cycles: 0,
            live_regs: None,
            last_regs: [0; 25],
        };
        engine.load_track(MusicTrack::Title);
        engine
    }

    /// An engine that plays registers it is given (`live_regs`) and no tune of its own.
    pub fn new_live() -> Self {
        Self {
            sid: Sid::new(ChipModel::Mos6581),
            current_track: MusicTrack::Title,
            events: Vec::new(),
            max_frames: 0,
            loop_start: 0,
            cur_frame: 0,
            cur_event_idx: 0,
            pending_cycles: 0,
            live_regs: Some([0; 25]),
            last_regs: [0; 25],
        }
    }

    pub fn load_track(&mut self, track: MusicTrack) {
        self.current_track = track;
        self.sid.reset();
        self.last_regs = [0; 25];
        self.cur_frame = 0;
        self.cur_event_idx = 0;
        self.pending_cycles = 0;

        // The title tune repeats exactly every 3648 frames once its first 100 frames are played; stage 1 plays
        // 4500 frames and starts over.
        let (events, loop_start, end) = match track {
            MusicTrack::Title => (generate_events(TITLE_TRACK, 100 + 3648), 100, 100 + 3648),
            MusicTrack::Stage1 => (generate_events(STAGE1_TRACK, 4500), 0, 4500),
        };
        self.events = events;
        self.loop_start = loop_start;
        self.max_frames = end;
    }

    /// Advances 1 PAL frame (at 50 Hz) and applies all SID register writes
    pub fn advance_frame(&mut self) {
        if let Some(regs) = self.live_regs {
            for r in 0..25 {
                if regs[r] != self.last_regs[r] {
                    self.sid.write(r as u8, regs[r]);
                    self.last_regs[r] = regs[r];
                }
            }
            return;
        }
        while self.cur_event_idx < self.events.len() {
            let ev = self.events[self.cur_event_idx];
            if ev.frame == self.cur_frame {
                self.sid.write(ev.reg, ev.val);
                self.cur_event_idx += 1;
            } else if ev.frame > self.cur_frame {
                break;
            } else {
                self.cur_event_idx += 1;
            }
        }

        self.cur_frame += 1;
        if self.cur_frame >= self.max_frames {
            // Loop track
            self.cur_frame = self.loop_start;
            self.cur_event_idx = self.events.iter().position(|e| e.frame >= self.loop_start).unwrap_or(self.events.len());
        }
    }

    /// Fills an output buffer with 44100 Hz mono 16-bit PCM samples
    pub fn fill_audio_buffer(&mut self, buffer: &mut [i16]) {
        // C64 PAL clock is 985248 Hz. Each 50 Hz PAL frame is 985248 / 50 = 19705 cycles.
        let cycles_per_frame = 19705u32;
        let mut sample_idx = 0;

        while sample_idx < buffer.len() {
            if self.pending_cycles == 0 {
                self.advance_frame();
                self.pending_cycles = cycles_per_frame;
            }

            let slice = &mut buffer[sample_idx..];
            let (produced, next_delta) = self.sid.sample(self.pending_cycles, slice, 1);
            sample_idx += produced;
            self.pending_cycles = next_delta;
        }
    }
}
