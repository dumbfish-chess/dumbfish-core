use std::process::ExitCode;
use std::{env, fs};

use dumbfish::eval::{MAX_PHASE, blend, for_each_feature, format_weights, parse_weights, phase};
use dumbfish_core::Board;

const USAGE: &str = "usage: dumbfish-tuner <dataset.epd> <weights.txt> [epochs]

Texel tuning: fits the evaluation weights to game results by minimizing the mean squared error
between each result and sigmoid(K * eval), then rewrites <weights.txt> in place. Weights whose
feature is active in less than 0.01% of the positions keep their initial value.
A dataset line starts with a FEN (move counters optional) and contains the result as
\"1-0\", \"0-1\" or \"1/2-1/2\", as in quiet-labeled.epd.";

const DEFAULT_EPOCHS: usize = 2000;

/// A weight is fitted only if its feature, scaled by the phase share of the weight, sums to at
/// least this fraction of the dataset size; rarer weights keep their initial value, since with
/// so few positions they only fit noise.
const MIN_ACTIVITY: f64 = 1e-4;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match &args[..] {
        [dataset, weights] => run(dataset, weights, DEFAULT_EPOCHS),
        [dataset, weights, epochs] => match epochs.parse() {
            Ok(epochs) => run(dataset, weights, epochs),
            Err(_) => Err(USAGE.to_string()),
        },
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(dataset_path: &str, weights_path: &str, epochs: usize) -> Result<(), String> {
    let read = |path: &str| fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    let mut weights: Vec<[f64; 3]> = parse_weights(&read(weights_path)?)?
        .iter()
        .map(|w| w.map(f64::from))
        .collect();
    let data = Dataset::load(&read(dataset_path)?)?;
    eprintln!("{} positions", data.len());

    let k = fit_k(&data, &weights);
    eprintln!("K = {k:.4}, loss {:.6}", data.loss(&weights, k));
    let fitted = data
        .activity(weights.len())
        .iter()
        .map(|a| a.map(|x| x >= MIN_ACTIVITY * data.len() as f64))
        .collect::<Vec<_>>();
    let frozen = fitted.iter().flatten().filter(|&&fit| !fit).count();
    eprintln!(
        "{frozen} of {} weights kept: too little data",
        3 * weights.len()
    );
    tune(&data, &mut weights, &fitted, k, epochs);

    let rounded = weights
        .iter()
        .map(|w| w.map(|x| x.round() as i32))
        .collect();
    fs::write(weights_path, format_weights(&rounded))
        .map_err(|e| format!("{weights_path}: {e}"))?;
    let rounded: Vec<_> = rounded.iter().map(|w| w.map(f64::from)).collect();
    eprintln!("final loss {:.6}", data.loss(&rounded, k));
    Ok(())
}

/// Positions as sparse feature vectors: position `i` owns `features[offsets[i]..offsets[i + 1]]`.
struct Dataset {
    results: Vec<f64>,
    /// Shares of the opening, middlegame and endgame weights, summing to 1.
    blends: Vec<[f64; 3]>,
    offsets: Vec<usize>,
    /// `(feature index, value)` pairs; an index may repeat.
    features: Vec<(u16, i8)>,
}

impl Dataset {
    fn load(text: &str) -> Result<Self, String> {
        let mut data = Dataset {
            results: Vec::new(),
            blends: Vec::new(),
            offsets: vec![0],
            features: Vec::new(),
        };
        for (number, line) in (1..).zip(text.lines()) {
            if line.trim().is_empty() {
                continue;
            }
            let fen: Vec<_> = line.split_whitespace().take(4).collect();
            let board =
                Board::from_fen(&fen.join(" ")).map_err(|e| format!("line {number}: {e}"))?;
            let result = [("\"1-0\"", 1.0), ("\"0-1\"", 0.0), ("\"1/2-1/2\"", 0.5)]
                .into_iter()
                .find(|(token, _)| line.contains(token))
                .ok_or_else(|| format!("line {number}: no result"))?
                .1;

            data.results.push(result);
            data.blends
                .push(blend(phase(&board)).map(|b| f64::from(b) / f64::from(MAX_PHASE)));
            for_each_feature(&board, |index, value| {
                data.features.push((index as u16, value as i8))
            });
            data.offsets.push(data.features.len());
        }
        Ok(data)
    }

    fn len(&self) -> usize {
        self.results.len()
    }

    fn features(&self, i: usize) -> impl Iterator<Item = (usize, f64)> {
        self.features[self.offsets[i]..self.offsets[i + 1]]
            .iter()
            .map(|&(f, value)| (usize::from(f), f64::from(value)))
    }

    /// Evaluation of position `i` from white's point of view, in centipawns.
    fn eval(&self, i: usize, weights: &[[f64; 3]]) -> f64 {
        let b = self.blends[i];
        self.features(i)
            .map(|(f, value)| {
                value * (b[0] * weights[f][0] + b[1] * weights[f][1] + b[2] * weights[f][2])
            })
            .sum()
    }

    fn loss(&self, weights: &[[f64; 3]], k: f64) -> f64 {
        let total: f64 = (0..self.len())
            .map(|i| (self.results[i] - sigmoid(k * self.eval(i, weights))).powi(2))
            .sum();
        total / self.len() as f64
    }

    /// Sum of `|value| * phase share` per weight over all positions.
    fn activity(&self, count: usize) -> Vec<[f64; 3]> {
        let mut activity = vec![[0.0; 3]; count];
        for i in 0..self.len() {
            let b = self.blends[i];
            for (f, value) in self.features(i) {
                for phase in 0..3 {
                    activity[f][phase] += value.abs() * b[phase];
                }
            }
        }
        activity
    }

    fn gradient(&self, weights: &[[f64; 3]], k: f64) -> Vec<[f64; 3]> {
        let mut gradient = vec![[0.0; 3]; weights.len()];
        let scale = 2.0 * k * std::f64::consts::LN_10 / 400.0 / self.len() as f64;
        for i in 0..self.len() {
            let s = sigmoid(k * self.eval(i, weights));
            let d = (s - self.results[i]) * s * (1.0 - s) * scale;
            let b = self.blends[i];
            for (f, value) in self.features(i) {
                for phase in 0..3 {
                    gradient[f][phase] += d * value * b[phase];
                }
            }
        }
        gradient
    }
}

/// Expected score for a centipawn evaluation.
fn sigmoid(eval: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-eval / 400.0))
}

/// K minimizing the loss of the given weights, by golden-section search on [0, 10].
fn fit_k(data: &Dataset, weights: &[[f64; 3]]) -> f64 {
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (0.0, 10.0);
    for _ in 0..60 {
        let (a, b) = (hi - ratio * (hi - lo), lo + ratio * (hi - lo));
        if data.loss(weights, a) < data.loss(weights, b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    (lo + hi) / 2.0
}

/// Full-batch Adam with a step of about one centipawn, over the weights marked in `fitted`.
fn tune(data: &Dataset, weights: &mut [[f64; 3]], fitted: &[[bool; 3]], k: f64, epochs: usize) {
    let (beta1, beta2, rate) = (0.9f64, 0.999f64, 1.0);
    let mut m = vec![[0.0; 3]; weights.len()];
    let mut v = vec![[0.0; 3]; weights.len()];
    for epoch in 1..=epochs {
        let gradient = data.gradient(weights, k);
        let (correction1, correction2) = (
            1.0 - beta1.powi(epoch as i32),
            1.0 - beta2.powi(epoch as i32),
        );
        for f in 0..weights.len() {
            for phase in (0..3).filter(|&p| fitted[f][p]) {
                let g = gradient[f][phase];
                m[f][phase] = beta1 * m[f][phase] + (1.0 - beta1) * g;
                v[f][phase] = beta2 * v[f][phase] + (1.0 - beta2) * g * g;
                let step =
                    (m[f][phase] / correction1) / ((v[f][phase] / correction2).sqrt() + 1e-12);
                weights[f][phase] -= rate * step;
            }
        }
        if epoch % 100 == 0 || epoch == epochs {
            eprintln!("epoch {epoch}: loss {:.6}", data.loss(weights, k));
        }
    }
}
