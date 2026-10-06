use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use dumbfish_core::{Board, Color};

use crate::search::{self, Iteration, Limits, MATE, MAX_DEPTH};

const MOVE_OVERHEAD: Duration = Duration::from_millis(30);

/// Reads UCI commands from stdin until `quit` or end of input.
pub fn run() {
    let mut board = Board::startpos();
    let stop = Arc::new(AtomicBool::new(false));
    let mut worker = None;

    for line in io::stdin().lines() {
        let Ok(line) = line else { break };
        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some("uci") => {
                println!("id name dumbfish {}", env!("CARGO_PKG_VERSION"));
                println!("id author tolstovrob");
                println!("uciok");
            }
            Some("isready") => println!("readyok"),
            Some("ucinewgame") => {
                wait(&mut worker);
                board = Board::startpos();
            }
            Some("position") => {
                wait(&mut worker);
                match parse_position(tokens) {
                    Ok(b) => board = b,
                    Err(e) => println!("info string {e}"),
                }
            }
            Some("go") => {
                wait(&mut worker);
                worker = Some(go(tokens, &board, &stop));
            }
            Some("stop") => finish(&mut worker, &stop),
            Some("quit") => break,
            Some(other) => println!("info string unknown command: {other}"),
            None => {}
        }
    }
    finish(&mut worker, &stop);
}

/// Waits until the running search, if any, has printed `bestmove`. Like Stockfish, commands
/// that change the position wait for the search instead of stopping it.
fn wait(worker: &mut Option<JoinHandle<()>>) {
    if let Some(handle) = worker.take() {
        handle.join().expect("search thread panicked");
    }
}

/// Stops the running search, if any, and waits for its `bestmove`.
fn finish(worker: &mut Option<JoinHandle<()>>, stop: &AtomicBool) {
    stop.store(true, Ordering::Relaxed);
    wait(worker);
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

/// Starts a search on a separate thread so that `stop` and `isready` stay responsive.
fn go<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    board: &Board,
    stop: &Arc<AtomicBool>,
) -> JoinHandle<()> {
    let start = Instant::now();
    let mut limits = Limits::default();
    let (mut time, mut inc, mut moves_to_go, mut infinite) = (None, Duration::ZERO, None, false);
    let us = board.side_to_move();
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
    // ponytail: fixed share of the remaining time; real time management comes with the search work.
    if let Some(time) = time
        && limits.deadline.is_none()
    {
        let budget = time / moves_to_go.unwrap_or(30) as u32 + inc / 2;
        limits.deadline = Some(start + budget.min(time.saturating_sub(MOVE_OVERHEAD)));
    }

    stop.store(false, Ordering::Relaxed);
    let stop = Arc::clone(stop);
    let mut board = board.clone();
    thread::spawn(move || {
        let (best, _) = search::search(&mut board, &limits, &stop, print_info);
        // In infinite mode `bestmove` may only follow `stop`.
        while infinite && !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(5));
        }
        println!(
            "bestmove {}",
            best.map_or("0000".to_string(), |m| m.to_string())
        );
    })
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
