use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use dumbfish_core::{Board, Color};

use crate::eval;
use crate::search::{self, Iteration, Limits, MATE, MAX_DEPTH};
use crate::skill::{self, Skill};
use crate::tt::{self, TranspositionTable};

const MOVE_OVERHEAD: Duration = Duration::from_millis(30);

/// Reads UCI commands from stdin until `quit` or end of input.
pub fn run() {
    let mut engine = Engine {
        board: Board::startpos(),
        tt: Some(TranspositionTable::new(tt::DEFAULT_MEGABYTES)),
        skill_level: skill::MAX_LEVEL,
        stop: Arc::new(AtomicBool::new(false)),
        worker: None,
    };

    for line in io::stdin().lines() {
        let Ok(line) = line else { break };
        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some("uci") => {
                println!("id name dumbfish {}", env!("CARGO_PKG_VERSION"));
                println!("id author tolstovrob");
                println!(
                    "option name Hash type spin default {} min 1 max {}",
                    tt::DEFAULT_MEGABYTES,
                    tt::MAX_MEGABYTES
                );
                println!(
                    "option name Skill Level type spin default {0} min 0 max {0}",
                    skill::MAX_LEVEL
                );
                println!("uciok");
            }
            Some("isready") => println!("readyok"),
            Some("setoption") => {
                engine.wait();
                if let Err(e) = engine.set_option(tokens) {
                    println!("info string {e}");
                }
            }
            Some("ucinewgame") => {
                engine.wait();
                engine.board = Board::startpos();
                engine.tt().clear();
            }
            Some("position") => {
                engine.wait();
                match parse_position(tokens) {
                    Ok(b) => engine.board = b,
                    Err(e) => println!("info string {e}"),
                }
            }
            Some("go") => {
                engine.wait();
                engine.go(tokens);
            }
            Some("eval") => {
                engine.wait();
                println!("{}", eval::explain(&engine.board));
            }
            Some("stop") => engine.finish(),
            Some("quit") => break,
            Some(other) => println!("info string unknown command: {other}"),
            None => {}
        }
    }
    engine.finish();
}

struct Engine {
    board: Board,
    /// Moves into the search thread for the duration of a search.
    tt: Option<TranspositionTable>,
    skill_level: u8,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<TranspositionTable>>,
}

impl Engine {
    /// Waits until the running search, if any, has printed `bestmove`. Like Stockfish, commands
    /// that change the position wait for the search instead of stopping it.
    fn wait(&mut self) {
        if let Some(handle) = self.worker.take() {
            self.tt = Some(handle.join().expect("search thread panicked"));
        }
    }

    /// Stops the running search, if any, and waits for its `bestmove`.
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.wait();
    }

    /// The table while no search is running.
    fn tt(&mut self) -> &mut TranspositionTable {
        self.tt.as_mut().expect("no search is running")
    }

    /// `setoption name <name> [value <value>]`.
    fn set_option<'a>(&mut self, mut tokens: impl Iterator<Item = &'a str>) -> Result<(), String> {
        if tokens.next() != Some("name") {
            return Err("expected setoption name <name> [value <value>]".into());
        }
        let name: Vec<_> = tokens.by_ref().take_while(|&t| t != "value").collect();
        let value: Vec<_> = tokens.collect();
        match (name.join(" ").to_lowercase().as_str(), &value[..]) {
            ("hash", [megabytes]) => match megabytes.parse::<usize>() {
                Ok(mb) if (1..=tt::MAX_MEGABYTES).contains(&mb) => {
                    self.tt = Some(TranspositionTable::new(mb));
                    Ok(())
                }
                _ => Err(format!(
                    "Hash must be 1..={}, got {megabytes:?}",
                    tt::MAX_MEGABYTES
                )),
            },
            ("skill level", [level]) => match level.parse::<u8>() {
                Ok(level) if level <= skill::MAX_LEVEL => {
                    self.skill_level = level;
                    Ok(())
                }
                _ => Err(format!(
                    "Skill Level must be 0..={}, got {level:?}",
                    skill::MAX_LEVEL
                )),
            },
            (name, _) => Err(format!("unknown option or value: {name:?}")),
        }
    }

    /// Starts a search on a separate thread so that `stop` and `isready` stay responsive.
    fn go<'a>(&mut self, tokens: impl Iterator<Item = &'a str>) {
        let (mut limits, infinite) = parse_go(tokens, self.board.side_to_move());
        let seed = SystemTime::UNIX_EPOCH
            .elapsed()
            .map_or(0, |t| t.as_nanos() as u64);
        limits.skill = Some(Skill {
            level: self.skill_level,
            seed,
        });
        self.stop.store(false, Ordering::Relaxed);
        let stop = Arc::clone(&self.stop);
        let mut board = self.board.clone();
        let mut tt = self.tt.take().expect("no search is running");
        self.worker = Some(thread::spawn(move || {
            let (best, _) = search::search(&mut board, &limits, &stop, &mut tt, print_info);
            while infinite && !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
            }
            println!(
                "bestmove {}",
                best.map_or("0000".to_string(), |m| m.to_string())
            );
            tt
        }));
    }
}

/// `position (startpos | fen <fen>) [moves <move>...]`. Nothing changes on error.
fn parse_position<'a>(mut tokens: impl Iterator<Item = &'a str>) -> Result<Board, String> {
    let mut board = match tokens.next() {
        Some("startpos") => {
            match tokens.next() {
                None | Some("moves") => {}
                Some(t) => return Err(format!("expected moves, got {t:?}")),
            }
            Board::startpos()
        }
        Some("fen") => {
            let fen: Vec<_> = tokens.by_ref().take_while(|&t| t != "moves").collect();
            Board::from_fen(&fen.join(" "))?
        }
        _ => return Err("expected startpos or fen".into()),
    };
    for token in tokens {
        let mv = board
            .legal_moves()
            .iter()
            .find(|m| m.to_string() == token)
            .copied();
        board.make_move(mv.ok_or_else(|| format!("illegal move {token:?} in {}", board.fen()))?);
    }
    Ok(board)
}

/// Search limits and whether the search is infinite, from the arguments of `go`.
fn parse_go<'a>(mut tokens: impl Iterator<Item = &'a str>, us: Color) -> (Limits, bool) {
    let start = Instant::now();
    let mut limits = Limits::default();
    let (mut time, mut inc, mut moves_to_go, mut infinite) = (None, Duration::ZERO, None, false);
    while let Some(token) = tokens.next() {
        let mut number = || tokens.next().and_then(|v| v.parse::<u64>().ok());
        match token {
            "depth" => limits.depth = number().map(|d| d as u32),
            "nodes" => limits.nodes = number(),
            "movetime" => limits.deadline = number().map(|ms| start + Duration::from_millis(ms)),
            "wtime" | "btime" if (token == "wtime") == (us == Color::White) => {
                time = number().map(Duration::from_millis)
            }
            "winc" | "binc" if (token == "winc") == (us == Color::White) => {
                inc = number().map(Duration::from_millis).unwrap_or_default()
            }
            "movestogo" => moves_to_go = number().filter(|&n| n > 0),
            "infinite" => infinite = true,
            _ => {}
        }
    }
    if let Some(time) = time
        && limits.deadline.is_none()
    {
        let budget = time / moves_to_go.unwrap_or(30) as u32 + inc / 2;
        limits.deadline = Some(start + budget.min(time.saturating_sub(MOVE_OVERHEAD)));
    }
    (limits, infinite)
}

fn print_info(it: &Iteration) {
    let nps = (it.nodes as f64 / it.elapsed.as_secs_f64().max(1e-3)) as u64;
    let score = match it.score {
        s if s >= MATE - MAX_DEPTH as i32 => format!("mate {}", (MATE - s + 1) / 2),
        s if s <= MAX_DEPTH as i32 - MATE => format!("mate -{}", (MATE + s) / 2),
        s => format!("cp {s}"),
    };
    println!(
        "info depth {} score {score} nodes {} nps {nps} time {} pv {}",
        it.depth,
        it.nodes,
        it.elapsed.as_millis(),
        it.best
    );
}
