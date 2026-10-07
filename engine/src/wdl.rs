//! Win, draw and loss probabilities for a score, from a model fitted to game results:
//! win = σ((cp - a) / b), loss = σ((-cp - a) / b), draw the rest. The parameters live in
//! `engine/wdl.txt` and come from `dumbfish-tools wdl`.

use std::sync::LazyLock;

use crate::search::{MATE, MAX_DEPTH};

pub struct Model {
    /// The score at which a win becomes as likely as not, in centipawns.
    pub a: f64,
    /// How fast the win probability grows with the score, in centipawns.
    pub b: f64,
}

pub static MODEL: LazyLock<Model> =
    LazyLock::new(|| parse_model(include_str!("../wdl.txt")).expect("valid engine/wdl.txt"));

/// One line `a b` with positive numbers; blank lines and `#` comments are skipped.
pub fn parse_model(text: &str) -> Result<Model, String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let line = lines.next().ok_or("no model")?;
    if lines.next().is_some() {
        return Err("more than one model".into());
    }
    let values: Vec<f64> = line
        .split_whitespace()
        .map(|v| v.parse().map_err(|_| format!("invalid number {v:?}")))
        .collect::<Result<_, _>>()?;
    match values[..] {
        [a, b] if a > 0.0 && b > 0.0 => Ok(Model { a, b }),
        _ => Err(format!("expected two positive numbers, got {line:?}")),
    }
}

impl Model {
    /// `[win, draw, loss]` per mille for the side the score is for; mate scores are certain.
    pub fn wdl(&self, score: i32) -> [u32; 3] {
        if score >= MATE - MAX_DEPTH as i32 {
            return [1000, 0, 0];
        }
        if score <= MAX_DEPTH as i32 - MATE {
            return [0, 0, 1000];
        }
        let sigmoid = |x: f64| 1.0 / (1.0 + (-x).exp());
        let score = f64::from(score);
        let win = (1000.0 * sigmoid((score - self.a) / self.b)).round() as u32;
        let loss = (1000.0 * sigmoid((-score - self.a) / self.b)).round() as u32;
        [win, 1000 - win - loss, loss]
    }
}
