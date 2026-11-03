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
}
