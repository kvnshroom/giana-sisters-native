//! Hülsbeck Sound & Music Driver Synthesizer for The Great Giana Sisters.

pub mod mixer;
pub mod player;
pub mod split;
pub mod voice;

#[cfg(test)]
mod tests {
    use super::player::{AudioEngine, MusicTrack};

    #[test]
    fn test_generate_title_audio() {
        giana_core::skip_without_data!();
        let mut audio = AudioEngine::new();
        audio.load_track(MusicTrack::Title);

        let mut samples = vec![0i16; 44100]; // 1 second of audio
        audio.fill_audio_buffer(&mut samples);

        // Verify that audio is not completely silent
        let max_amp = samples.iter().map(|&s| s.abs()).max().unwrap_or(0);
        assert!(max_amp > 100, "Audio output should have audible amplitude");
    }

    #[test]
    fn test_generate_stage1_audio() {
        giana_core::skip_without_data!();
        let mut audio = AudioEngine::new();
        audio.load_track(MusicTrack::Stage1);

        let mut samples = vec![0i16; 44100]; // 1 second of audio
        audio.fill_audio_buffer(&mut samples);

        let max_amp = samples.iter().map(|&s| s.abs()).max().unwrap_or(0);
        assert!(max_amp > 100, "Audio output should have audible amplitude");
    }

    /// Timed writes: a square wave written as volume steps (the way the sample plays) is heard.
    #[test]
    fn test_timed_volume_writes_are_audible() {
        let mut audio = AudioEngine::new_live();
        let mut samples = Vec::new();
        for _ in 0..10 {
            // 985248 / 7000 = about 140 cycles per volume step
            let mut w = vec![(0u32, 0x18u8, 0x13u8)];
            for i in 0..140u32 {
                w.push((i * 140, 0x18, if i % 2 == 0 { 0x13 } else { 0x1A }));
            }
            audio.queue_timed_writes(w);
            let mut buf = vec![0i16; 882];
            audio.fill_audio_buffer(&mut buf);
            samples.extend(buf);
        }
        let tail = &samples[samples.len() / 2..];
        let swing = tail.iter().map(|&v| v as i32).max().unwrap() - tail.iter().map(|&v| v as i32).min().unwrap();
        assert!(swing > 100, "volume steps are audible, swing {swing}");
    }
}
