//! Who wins a game of two players. The original knows no winner: this is an addition.

/// The rule for the winner of a game of two players.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Winner {
    /// the higher score wins, the higher stage breaks a tie
    #[default]
    Score,
    /// the higher stage reached wins, the higher score breaks a tie
    Furthest,
}

impl Winner {
    pub fn label(self) -> &'static str {
        match self {
            Winner::Score => "SCORE",
            Winner::Furthest => "FURTHEST",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Winner::Score => Winner::Furthest,
            Winner::Furthest => Winner::Score,
        }
    }

    /// One byte value for the handshake and `settings.conf`.
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Winner::Score),
            1 => Some(Winner::Furthest),
            _ => None,
        }
    }

    /// The rule in a line for the result page.
    pub fn rule_text(self) -> &'static str {
        match self {
            Winner::Score => "THE HIGHER SCORE WINS",
            Winner::Furthest => "THE FURTHEST STAGE WINS",
        }
    }
}

/// What a player reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub name: String,
    pub score: u32,
    pub stage: u32,
}

/// The end of a game of two players.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub rule: Winner,
    pub players: [Standing; 2],
    /// the index in `players` of the winner, `None` for a draw
    pub winner: Option<usize>,
    /// the result in one line (36 characters at most)
    pub text: String,
}

/// Apply the rule. Both numbers decide in turn: the first one named by the rule, then the other.
pub fn decide(rule: Winner, players: [Standing; 2]) -> MatchResult {
    let key = |s: &Standing| match rule {
        Winner::Score => (s.score, s.stage),
        Winner::Furthest => (s.stage, s.score),
    };
    let (a, b) = (key(&players[0]), key(&players[1]));
    let winner = match a.cmp(&b) {
        std::cmp::Ordering::Greater => Some(0),
        std::cmp::Ordering::Less => Some(1),
        std::cmp::Ordering::Equal => None,
    };
    let text = match winner {
        Some(w) => {
            let (win, lose) = if w == 0 { (a, b) } else { (b, a) };
            let name = &players[w].name;
            if win.0 != lose.0 {
                match rule {
                    Winner::Score => format!("{name} WINS {} TO {}", win.0, lose.0),
                    Winner::Furthest => format!("{name} WINS STAGE {} TO {}", win.0, lose.0),
                }
            } else {
                // the first number is equal: the second one decided
                match rule {
                    Winner::Score => format!("{name} WINS ON STAGE {} TO {}", win.1, lose.1),
                    Winner::Furthest => format!("{name} WINS ON SCORE {} TO {}", win.1, lose.1),
                }
            }
        }
        None => match rule {
            Winner::Score => format!("A DRAW AT {}", a.0),
            Winner::Furthest => format!("A DRAW AT STAGE {}", a.0),
        },
    };
    MatchResult { rule, players, winner, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, score: u32, stage: u32) -> Standing {
        Standing { name: name.into(), score, stage }
    }

    #[test]
    fn score_rule_takes_the_higher_score_and_the_stage_breaks_a_tie() {
        let r = decide(Winner::Score, [p("GIANA", 1200, 2), p("MARIA", 900, 5)]);
        assert_eq!((r.winner, r.text.as_str()), (Some(0), "GIANA WINS 1200 TO 900"));
        let r = decide(Winner::Score, [p("GIANA", 900, 2), p("MARIA", 900, 5)]);
        assert_eq!((r.winner, r.text.as_str()), (Some(1), "MARIA WINS ON STAGE 5 TO 2"));
        let r = decide(Winner::Score, [p("GIANA", 900, 2), p("MARIA", 900, 2)]);
        assert_eq!((r.winner, r.text.as_str()), (None, "A DRAW AT 900"));
    }

    #[test]
    fn furthest_rule_takes_the_higher_stage_and_the_score_breaks_a_tie() {
        let r = decide(Winner::Furthest, [p("GIANA", 1200, 2), p("MARIA", 900, 5)]);
        assert_eq!((r.winner, r.text.as_str()), (Some(1), "MARIA WINS STAGE 5 TO 2"));
        let r = decide(Winner::Furthest, [p("GIANA", 1200, 5), p("MARIA", 900, 5)]);
        assert_eq!((r.winner, r.text.as_str()), (Some(0), "GIANA WINS ON SCORE 1200 TO 900"));
        let r = decide(Winner::Furthest, [p("GIANA", 900, 5), p("MARIA", 900, 5)]);
        assert_eq!((r.winner, r.text.as_str()), (None, "A DRAW AT STAGE 5"));
    }

    #[test]
    fn the_longest_text_fits_the_screen() {
        let r = decide(Winner::Furthest, [p("GIANA", 999_999, 33), p("MARIA", 999_998, 33)]);
        assert!(r.text.len() <= 36, "{}", r.text);
    }

    #[test]
    fn codes_round_trip() {
        for w in [Winner::Score, Winner::Furthest] {
            assert_eq!(Winner::from_code(w.code()), Some(w));
            assert_ne!(w.other(), w);
        }
        assert_eq!(Winner::from_code(7), None);
    }
}
