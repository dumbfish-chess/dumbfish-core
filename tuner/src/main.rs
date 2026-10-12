use std::process::ExitCode;
use std::{env, fs};

use dumbfish::eval::{
    MAX_PHASE, blend, for_each_feature, format_weights, group, parse_weights, phase,
};
use dumbfish::style::GROUPS;
use dumbfish_core::Board;

const USAGE: &str =
    "usage: dumbfish-tuner <dataset.epd> <weights.txt> [epochs] [--freeze=<group>,...]
           [--holdout=<r>/<n>] [--knots=<phase>,...] [--smooth]

Texel tuning: fits the evaluation weights to game results by minimizing the mean squared error
between each result and sigmoid(K * eval), then rewrites <weights.txt> in place. Weights whose
feature is active in less than 0.01% of the positions keep their initial value, as do the
weights of the groups given to --freeze: material, positional, pawns, mobility, king_attack,
king_defense.
With --holdout=<r>/<n> the positions number r, r + n, r + 2n, ... (counting from 0) are held out:
they are not fitted, and their loss is printed next to the loss of the fitted positions.
--knots lists the phases (0 to 24) at which a weight has a value of its own, by default 24,12,0
as in the engine. Between the knots the weight follows straight lines and stays constant beyond
them; with --smooth it follows the polynomial through all the knot values instead. One knot is a
weight that does not depend on the phase, 24,0 a single line from opening to endgame. The file
is rewritten only when the engine can play the result, that is with knots among 24, 12 and 0 and
no more than a straight line under --smooth; otherwise only the losses are printed.
A dataset line starts with a FEN (move counters optional) and contains the result as
\"1-0\", \"0-1\" or \"1/2-1/2\", as in quiet-labeled.epd.";

const DEFAULT_EPOCHS: usize = 2000;

/// A weight is fitted only if its feature, scaled by the share of the weight in the phase,
/// sums to at least this fraction of the dataset size; rarer weights keep their initial value,
/// since with so few positions they only fit noise.
const MIN_ACTIVITY: f64 = 1e-4;

/// The phases of the three weights the engine keeps per feature: opening, middlegame, endgame.
const ENGINE_KNOTS: [i32; 3] = [MAX_PHASE, MAX_PHASE / 2, 0];

fn main() -> ExitCode {
    let result = parse_args(env::args().skip(1)).and_then(|args| run(&args));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    dataset: String,
    weights: String,
    epochs: usize,
    /// Indices into `GROUPS`.
    frozen_groups: Vec<usize>,
    /// `(r, n)`: positions `r`, `r + n`, ... are held out.
    holdout: Option<(usize, usize)>,
    curve: Curve,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut positional = Vec::new();
    let mut frozen_groups = Vec::new();
    let mut holdout = None;
    let mut curve = Curve::engine();
    for arg in args {
        if let Some(value) = arg.strip_prefix("--holdout=") {
            let parsed = value
                .split_once('/')
                .and_then(|(r, n)| Some((r.parse().ok()?, n.parse().ok()?)));
            holdout = Some(
                parsed
                    .filter(|&(r, n): &(usize, usize)| r < n && n >= 2)
                    .ok_or_else(|| USAGE.to_string())?,
            );
            continue;
        }
        if let Some(list) = arg.strip_prefix("--knots=") {
            let knots: Option<Vec<i32>> = list.split(',').map(|knot| knot.parse().ok()).collect();
            curve.knots = knots
                .filter(|knots| knots.iter().all(|knot| (0..=MAX_PHASE).contains(knot)))
                .ok_or_else(|| USAGE.to_string())?;
            curve.knots.sort_unstable_by(|a, b| b.cmp(a));
            curve.knots.dedup();
            continue;
        }
        if arg == "--smooth" {
            curve.smooth = true;
            continue;
        }
        let Some(list) = arg.strip_prefix("--freeze=") else {
            positional.push(arg);
            continue;
        };
        for name in list.split(',') {
            let index = GROUPS.iter().position(|g| *g == name).ok_or_else(|| {
                format!(
                    "unknown group {name:?}, expected one of {}",
                    GROUPS.join(", ")
                )
            })?;
            frozen_groups.push(index);
        }
    }
    let (dataset, weights, epochs) = match &positional[..] {
        [dataset, weights] => (dataset, weights, DEFAULT_EPOCHS),
        [dataset, weights, epochs] => (
            dataset,
            weights,
            epochs.parse().map_err(|_| USAGE.to_string())?,
        ),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Args {
        dataset: dataset.clone(),
        weights: weights.clone(),
        epochs,
        frozen_groups,
        holdout,
        curve,
    })
}

/// How a weight depends on the game phase: it has a value at each knot, and in between the
/// values are joined by straight lines or, when `smooth`, by the polynomial through all of them.
struct Curve {
    /// Phases in descending order, the opening first.
    knots: Vec<i32>,
    smooth: bool,
}

/// Shares of the knot values of a curve in the weight at each phase, `0..=MAX_PHASE`.
type Shares = Vec<Vec<f64>>;

impl Curve {
    fn engine() -> Self {
        Curve {
            knots: ENGINE_KNOTS.to_vec(),
            smooth: false,
        }
    }

    /// Shares of the knot values in the weight at `phase`, summing to 1.
    fn shares_at(&self, phase: i32) -> Vec<f64> {
        let knots = &self.knots;
        let mut shares = vec![0.0; knots.len()];
        if self.smooth {
            for (j, share) in shares.iter_mut().enumerate() {
                *share = (0..knots.len())
                    .filter(|&l| l != j)
                    .map(|l| f64::from(phase - knots[l]) / f64::from(knots[j] - knots[l]))
                    .product();
            }
            return shares;
        }
        match knots.iter().position(|&knot| knot <= phase) {
            Some(0) => shares[0] = 1.0,
            None => shares[knots.len() - 1] = 1.0,
            Some(j) => {
                let share = f64::from(phase - knots[j]) / f64::from(knots[j - 1] - knots[j]);
                shares[j - 1] = share;
                shares[j] = 1.0 - share;
            }
        }
        shares
    }

    fn shares(&self) -> Shares {
        (0..=MAX_PHASE).map(|phase| self.shares_at(phase)).collect()
    }

    /// Whether weights on this curve can be written as the engine's three weights per feature.
    fn playable(&self) -> bool {
        (!self.smooth || self.knots.len() <= 2)
            && self.knots.iter().all(|knot| ENGINE_KNOTS.contains(knot))
    }
}

fn dot(shares: &[f64], weights: &[f64]) -> f64 {
    shares.iter().zip(weights).map(|(s, w)| s * w).sum()
}

fn run(args: &Args) -> Result<(), String> {
    let (dataset_path, weights_path, epochs) = (&args.dataset, &args.weights, args.epochs);
    let read = |path: &str| fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    let engine = Curve::engine().shares();
    for phase in 0..=MAX_PHASE {
        let blended = blend(phase).map(|b| f64::from(b) / f64::from(MAX_PHASE));
        let shares = &engine[phase as usize];
        if shares.len() != 3 || (0..3).any(|i| (shares[i] - blended[i]).abs() > 1e-12) {
            return Err(format!(
                "the engine blends its weights differently at phase {phase}"
            ));
        }
    }
    let curve = &args.curve;
    let (shares, knots) = (curve.shares(), curve.knots.len());

    let file: Vec<[f64; 3]> = parse_weights(&read(weights_path)?)?
        .iter()
        .map(|w| w.map(f64::from))
        .collect();
    let mut weights: Vec<f64> = file
        .iter()
        .flat_map(|w| {
            curve
                .knots
                .iter()
                .map(|&knot| dot(&engine[knot as usize], w))
        })
        .collect();
    let (data, held) = Dataset::load(&read(dataset_path)?, args.holdout)?;
    eprintln!("{} positions", data.len());
    if held.len() > 0 {
        eprintln!("{} positions held out", held.len());
    }

    let k = fit_k(&data, &shares, &weights);
    eprintln!("K = {k:.4}, loss {:.6}", data.loss(&shares, &weights, k));
    let fitted: Vec<bool> = data
        .activity(&shares, file.len())
        .iter()
        .enumerate()
        .map(|(i, &a)| {
            !args.frozen_groups.contains(&group(i / knots)) && a >= MIN_ACTIVITY * data.len() as f64
        })
        .collect();
    let kept = fitted.iter().filter(|&&fit| !fit).count();
    eprintln!(
        "{kept} of {} weights kept: frozen groups or too little data",
        weights.len()
    );
    tune(&data, &held, &shares, &mut weights, &fitted, k, epochs);

    for w in &mut weights {
        *w = w.round();
    }
    if !curve.playable() {
        eprintln!("the engine cannot play this curve: {weights_path} is not rewritten");
        eprintln!(
            "final loss {:.6}{}",
            data.loss(&shares, &weights, k),
            held_out(&held, &shares, &weights, k)
        );
        return Ok(());
    }
    let rows: Vec<[f64; 3]> = (0..file.len())
        .map(|f| {
            let row = f * knots..(f + 1) * knots;
            if !fitted[row.clone()].contains(&true) {
                return file[f];
            }
            ENGINE_KNOTS.map(|knot| dot(&shares[knot as usize], &weights[row.clone()]).round())
        })
        .collect();
    let rounded = rows.iter().map(|row| row.map(|w| w as i32)).collect();
    fs::write(weights_path, format_weights(&rounded))
        .map_err(|e| format!("{weights_path}: {e}"))?;
    let written: Vec<f64> = rows.into_iter().flatten().collect();
    eprintln!(
        "final loss {:.6}{}",
        data.loss(&engine, &written, k),
        held_out(&held, &engine, &written, k)
    );
    Ok(())
}

/// Positions as sparse feature vectors: position `i` owns `features[offsets[i]..offsets[i + 1]]`.
struct Dataset {
    results: Vec<f64>,
    phases: Vec<u8>,
    offsets: Vec<usize>,
    /// `(feature index, value)` pairs; an index may repeat.
    features: Vec<(u16, i8)>,
}

impl Dataset {
    fn new() -> Self {
        Dataset {
            results: Vec::new(),
            phases: Vec::new(),
            offsets: vec![0],
            features: Vec::new(),
        }
    }

    /// The positions to fit and the held-out ones: with `holdout` of `(r, n)` those number `r`,
    /// `r + n`, ... counting from 0.
    fn load(text: &str, holdout: Option<(usize, usize)>) -> Result<(Self, Self), String> {
        let (mut fitted, mut held) = (Dataset::new(), Dataset::new());
        let mut position = 0;
        for (number, line) in (1..).zip(text.lines()) {
            if line.trim().is_empty() {
                continue;
            }
            let data = match holdout {
                Some((r, n)) if position % n == r => &mut held,
                _ => &mut fitted,
            };
            position += 1;
            let fen: Vec<_> = line.split_whitespace().take(4).collect();
            let board =
                Board::from_fen(&fen.join(" ")).map_err(|e| format!("line {number}: {e}"))?;
            let result = [("\"1-0\"", 1.0), ("\"0-1\"", 0.0), ("\"1/2-1/2\"", 0.5)]
                .into_iter()
                .find(|(token, _)| line.contains(token))
                .ok_or_else(|| format!("line {number}: no result"))?
                .1;

            data.results.push(result);
            data.phases.push(phase(&board) as u8);
            for_each_feature(&board, |index, value| {
                data.features.push((index as u16, value as i8))
            });
            data.offsets.push(data.features.len());
        }
        Ok((fitted, held))
    }

    fn len(&self) -> usize {
        self.results.len()
    }

    fn features(&self, i: usize) -> impl Iterator<Item = (usize, f64)> {
        self.features[self.offsets[i]..self.offsets[i + 1]]
            .iter()
            .map(|&(f, value)| (usize::from(f), f64::from(value)))
    }

    /// Evaluation of position `i` from white's point of view, in centipawns; `by_phase` is the
    /// result of `weights_by_phase`.
    fn eval(&self, i: usize, by_phase: &[f64], count: usize) -> f64 {
        let weights = &by_phase[usize::from(self.phases[i]) * count..][..count];
        self.features(i).map(|(f, value)| value * weights[f]).sum()
    }

    /// `weights` holds one value per knot for every feature.
    fn loss(&self, shares: &Shares, weights: &[f64], k: f64) -> f64 {
        let count = weights.len() / shares[0].len();
        let by_phase = weights_by_phase(shares, weights);
        let total: f64 = (0..self.len())
            .map(|i| (self.results[i] - sigmoid(k * self.eval(i, &by_phase, count))).powi(2))
            .sum();
        total / self.len() as f64
    }

    /// Sum of `|value * share|` per weight over all positions.
    fn activity(&self, shares: &Shares, count: usize) -> Vec<f64> {
        let mut by_phase = vec![0.0; shares.len() * count];
        for i in 0..self.len() {
            let sums = &mut by_phase[usize::from(self.phases[i]) * count..][..count];
            for (f, value) in self.features(i) {
                sums[f] += value.abs();
            }
        }
        let absolute: Shares = shares
            .iter()
            .map(|row| row.iter().map(|share| share.abs()).collect())
            .collect();
        sums_by_knot(&absolute, &by_phase, count)
    }

    fn gradient(&self, shares: &Shares, weights: &[f64], k: f64) -> Vec<f64> {
        let count = weights.len() / shares[0].len();
        let weights = weights_by_phase(shares, weights);
        let mut by_phase = vec![0.0; shares.len() * count];
        let scale = 2.0 * k * std::f64::consts::LN_10 / 400.0 / self.len() as f64;
        for i in 0..self.len() {
            let s = sigmoid(k * self.eval(i, &weights, count));
            let d = (s - self.results[i]) * s * (1.0 - s) * scale;
            let sums = &mut by_phase[usize::from(self.phases[i]) * count..][..count];
            for (f, value) in self.features(i) {
                sums[f] += d * value;
            }
        }
        sums_by_knot(shares, &by_phase, count)
    }
}

/// The weight of every feature at every phase, `[phase * features + feature]`, from one value
/// per knot for every feature.
fn weights_by_phase(shares: &Shares, weights: &[f64]) -> Vec<f64> {
    let knots = shares[0].len();
    shares
        .iter()
        .flat_map(|row| weights.chunks(knots).map(|w| dot(row, w)))
        .collect()
}

/// Sums kept per phase and feature, `[phase * count + feature]`, as sums per feature and knot:
/// each goes to the knots by their shares in its phase.
fn sums_by_knot(shares: &Shares, by_phase: &[f64], count: usize) -> Vec<f64> {
    let knots = shares[0].len();
    let mut sums = vec![0.0; count * knots];
    for (row, phase) in shares.iter().zip(by_phase.chunks(count)) {
        for (f, sum) in phase.iter().enumerate() {
            for (knot, share) in row.iter().enumerate() {
                sums[f * knots + knot] += share * sum;
            }
        }
    }
    sums
}

/// The loss of the held-out positions as a suffix for a log line, empty without such positions.
fn held_out(held: &Dataset, shares: &Shares, weights: &[f64], k: f64) -> String {
    if held.len() == 0 {
        return String::new();
    }
    format!(", held out {:.6}", held.loss(shares, weights, k))
}

/// Expected score for a centipawn evaluation.
fn sigmoid(eval: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-eval / 400.0))
}

/// K minimizing the loss of the given weights, by golden-section search on [0, 10].
fn fit_k(data: &Dataset, shares: &Shares, weights: &[f64]) -> f64 {
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut lo, mut hi) = (0.0, 10.0);
    for _ in 0..60 {
        let (a, b) = (hi - ratio * (hi - lo), lo + ratio * (hi - lo));
        if data.loss(shares, weights, a) < data.loss(shares, weights, b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    (lo + hi) / 2.0
}

/// Full-batch Adam with a step of about one centipawn, over the weights marked in `fitted`.
fn tune(
    data: &Dataset,
    held: &Dataset,
    shares: &Shares,
    weights: &mut [f64],
    fitted: &[bool],
    k: f64,
    epochs: usize,
) {
    let (beta1, beta2, rate) = (0.9f64, 0.999f64, 1.0);
    let mut m = vec![0.0; weights.len()];
    let mut v = vec![0.0; weights.len()];
    for epoch in 1..=epochs {
        let gradient = data.gradient(shares, weights, k);
        let (correction1, correction2) = (
            1.0 - beta1.powi(epoch as i32),
            1.0 - beta2.powi(epoch as i32),
        );
        for i in (0..weights.len()).filter(|&i| fitted[i]) {
            let g = gradient[i];
            m[i] = beta1 * m[i] + (1.0 - beta1) * g;
            v[i] = beta2 * v[i] + (1.0 - beta2) * g * g;
            weights[i] -= rate * (m[i] / correction1) / ((v[i] / correction2).sqrt() + 1e-12);
        }
        if epoch % 100 == 0 || epoch == epochs {
            eprintln!(
                "epoch {epoch}: loss {:.6}{}",
                data.loss(shares, weights, k),
                held_out(held, shares, weights, k)
            );
        }
    }
}
