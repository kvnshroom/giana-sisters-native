//! The high score table. The original keeps one on its disk; this one is new and lives in the settings folder.

use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 10;
pub const NAME_LEN: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub score: u32,
    pub stage: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scores {
    pub entries: Vec<Entry>,
}

/// The score of the game, three BCD bytes with the lowest first.
pub fn score_value(bcd: [u8; 3]) -> u32 {
    bcd.iter().rev().fold(0, |v, &b| v * 100 + (b >> 4) as u32 * 10 + (b & 15) as u32)
}

impl Scores {
    /// The place a score would get; `None` if it does not make the table.
    pub fn rank(&self, score: u32) -> Option<usize> {
        if score == 0 {
            return None;
        }
        let place = self.entries.iter().position(|e| e.score < score).unwrap_or(self.entries.len());
        (place < MAX_ENTRIES).then_some(place)
    }

    /// Returns the place of the new entry.
    pub fn insert(&mut self, entry: Entry) -> Option<usize> {
        let place = self.rank(entry.score)?;
        self.entries.insert(place, entry);
        self.entries.truncate(MAX_ENTRIES);
        Some(place)
    }

    pub fn to_text(&self) -> String {
        self.entries.iter().map(|e| format!("{} {} {}\n", e.score, e.stage, e.name)).collect()
    }

    /// Lines that cannot be read are left out.
    pub fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let mut p = line.splitn(3, ' ');
            let (Some(score), Some(stage), Some(name)) = (p.next(), p.next(), p.next()) else { continue };
            let (Ok(score), Ok(stage)) = (score.parse(), stage.parse()) else { continue };
            // The table only shows letters and digits: anything else in the file becomes a space
            let name = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { ' ' }).take(NAME_LEN).collect();
            s.insert(Entry { name, score, stage });
        }
        s
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map_or_else(|_| Self::default(), |t| Self::from_text(&t))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::fsutil::write_atomic(path, &self.to_text())
    }
}

pub fn scores_path() -> Option<PathBuf> {
    crate::bindings::config_path().map(|p| p.with_file_name("scores.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(score: u32) -> Entry {
        Entry { name: "GIANA".into(), score, stage: 1 }
    }

    #[test]
    fn score_comes_from_the_bcd_bytes() {
        assert_eq!(score_value([0x56, 0x34, 0x12]), 123456);
        assert_eq!(score_value([0, 0, 0]), 0);
    }

    #[test]
    fn the_table_is_sorted_and_keeps_ten() {
        let mut s = Scores::default();
        for v in 1..=12 {
            s.insert(e(v * 100));
        }
        assert_eq!(s.entries.len(), MAX_ENTRIES);
        assert_eq!(s.entries[0].score, 1200);
        assert_eq!(s.entries[9].score, 300);
        assert_eq!(s.rank(100), None);
        assert_eq!(s.rank(350), Some(9));
    }

    #[test]
    fn zero_does_not_count_and_equal_scores_go_behind() {
        let mut s = Scores::default();
        assert_eq!(s.rank(0), None);
        s.insert(e(500));
        assert_eq!(s.rank(500), Some(1));
    }

    #[test]
    fn file_round_trip_and_damaged_lines() {
        let mut s = Scores::default();
        s.insert(Entry { name: "AB CD".into(), score: 900, stage: 33 });
        s.insert(e(100));
        assert_eq!(Scores::from_text(&s.to_text()), s);
        assert_eq!(Scores::from_text("x y z\n\n12\n").entries.len(), 0);
    }
}
