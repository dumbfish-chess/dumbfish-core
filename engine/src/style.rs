//! Playing style: percentage multipliers on groups of evaluation features and a contempt for
//! draws. Presets live in `engine/styles.txt`, one line per preset:
//! `name material positional pawns mobility king_attack king_defense contempt`.

use std::sync::LazyLock;

/// Feature groups, in the order of `Style::multipliers`.
pub const GROUPS: [&str; 6] = [
    "material",
    "positional",
    "pawns",
    "mobility",
    "king_attack",
    "king_defense",
];

pub const MAX_MULTIPLIER: i32 = 300;
pub const MAX_CONTEMPT: i32 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Style {
    /// Percent per group of `GROUPS`; 100 keeps the tuned weights.
    pub multipliers: [i32; 6],
    /// Centipawns the engine is ready to give up to avoid a draw; negative values seek draws.
    pub contempt: i32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            multipliers: [100; 6],
            contempt: 0,
        }
    }
}

pub struct Preset {
    pub name: String,
    pub style: Style,
}

/// The shipped presets; the first one is the default.
pub static PRESETS: LazyLock<Vec<Preset>> = LazyLock::new(|| {
    parse_presets(include_str!("../styles.txt")).expect("valid engine/styles.txt")
});

/// Parses preset lines; blank lines and `#` comments are skipped.
pub fn parse_presets(text: &str) -> Result<Vec<Preset>, String> {
    let mut presets: Vec<Preset> = Vec::new();
    for (number, line) in (1..).zip(text.lines()) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let [name, values @ ..] = &fields[..] else {
            unreachable!("a non-empty line has a field")
        };
        if values.len() != GROUPS.len() + 1 {
            return Err(format!(
                "line {number}: expected a name, {} multipliers and a contempt",
                GROUPS.len()
            ));
        }
        let parse = |s: &str, max: i32, min: i32| match s.parse::<i32>() {
            Ok(v) if (min..=max).contains(&v) => Ok(v),
            _ => Err(format!("line {number}: {s:?} is not in {min}..={max}")),
        };
        let mut multipliers = [0; 6];
        for (m, value) in multipliers.iter_mut().zip(values) {
            *m = parse(value, MAX_MULTIPLIER, 0)?;
        }
        let contempt = parse(values[GROUPS.len()], MAX_CONTEMPT, -MAX_CONTEMPT)?;
        if presets.iter().any(|p| p.name == *name) {
            return Err(format!("line {number}: duplicate preset {name:?}"));
        }
        presets.push(Preset {
            name: name.to_string(),
            style: Style {
                multipliers,
                contempt,
            },
        });
    }
    if presets.is_empty() {
        return Err("no presets".into());
    }
    Ok(presets)
}
