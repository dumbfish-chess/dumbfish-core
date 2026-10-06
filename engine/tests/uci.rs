//! Drives the `dumbfish` binary over stdin/stdout, the way a GUI does.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use dumbfish_core::Board;

const TIMEOUT: Duration = Duration::from_secs(10);

struct Engine {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Engine {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dumbfish"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    fn send(&mut self, command: &str) {
        writeln!(self.stdin, "{command}").unwrap();
    }

    /// Output lines up to and including the first one that starts with `prefix`.
    fn read_until(&self, prefix: &str) -> Vec<String> {
        let deadline = Instant::now() + TIMEOUT;
        let mut lines = Vec::new();
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(timeout) {
                Ok(line) => {
                    let done = line.starts_with(prefix);
                    lines.push(line);
                    if done {
                        return lines;
                    }
                }
                Err(e) => panic!("no {prefix:?} ({e:?}), got {lines:?}"),
            }
        }
    }

    /// The move from the next `bestmove` line.
    fn best_move(&self) -> String {
        let lines = self.read_until("bestmove");
        lines
            .last()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.child.wait();
    }
}

fn is_legal(fen: &str, moves: &[&str], mv: &str) -> bool {
    let mut board = Board::from_fen(fen).unwrap();
    for played in moves {
        let m = *board
            .legal_moves()
            .iter()
            .find(|m| m.to_string() == *played)
            .unwrap();
        board.make_move(m);
    }
    board.legal_moves().iter().any(|m| m.to_string() == mv)
}

#[test]
fn handshake() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(lines.iter().any(|l| l.starts_with("id name dumbfish")));
    assert!(lines.iter().any(|l| l.starts_with("id author")));
    engine.send("isready");
    engine.read_until("readyok");
}

#[test]
fn go_depth_returns_legal_move() {
    let mut engine = Engine::start();
    engine.send("position startpos moves e2e4 e7e5 g1f3");
    engine.send("go depth 3");
    let lines = engine.read_until("bestmove");
    assert!(lines.iter().any(|l| l.starts_with("info depth 3 ")));
    let best = lines.last().unwrap().split_whitespace().nth(1).unwrap();
    assert!(
        is_legal(
            dumbfish_core::board::STARTPOS,
            &["e2e4", "e7e5", "g1f3"],
            best
        ),
        "{best}"
    );
}

#[test]
fn finds_mate_in_one() {
    let mut engine = Engine::start();
    engine.send("position fen 6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
    engine.send("go depth 3");
    let lines = engine.read_until("bestmove");
    assert!(
        lines.iter().any(|l| l.contains("score mate 1 ")),
        "{lines:?}"
    );
    assert_eq!(lines.last().unwrap(), "bestmove a1a8");
}

#[test]
fn no_legal_moves() {
    let mut engine = Engine::start();
    engine.send("position fen rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    engine.send("go depth 3");
    assert_eq!(engine.best_move(), "0000");
}

#[test]
fn infinite_waits_for_stop() {
    let mut engine = Engine::start();
    engine.send("position startpos");
    engine.send("go infinite");
    // Must keep answering while searching and must not send bestmove on its own.
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert!(!lines.iter().any(|l| l.starts_with("bestmove")));
    match engine.lines.recv_timeout(Duration::from_millis(300)) {
        Ok(line) => assert!(!line.starts_with("bestmove"), "bestmove before stop"),
        Err(e) => assert_eq!(e, RecvTimeoutError::Timeout),
    }
    engine.send("stop");
    engine.best_move();
}

#[test]
fn time_limits_are_respected() {
    let mut engine = Engine::start();
    for go in [
        "go movetime 200",
        "go wtime 1000 btime 1000",
        "go wtime 100 btime 100 winc 0 binc 0 movestogo 1",
    ] {
        engine.send("position startpos");
        let start = Instant::now();
        engine.send(go);
        let best = engine.best_move();
        assert!(
            start.elapsed() < Duration::from_millis(1500),
            "{go}: {:?}",
            start.elapsed()
        );
        assert!(
            is_legal(dumbfish_core::board::STARTPOS, &[], &best),
            "{go}: {best}"
        );
    }
}

#[test]
fn bad_input_is_reported_and_ignored() {
    let mut engine = Engine::start();
    engine.send("position startpos moves e2e4");
    for bad in [
        "position fen not a fen",
        "position startpos moves e2e5",
        "position",
        "frobnicate",
    ] {
        engine.send(bad);
    }
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        4,
        "{lines:?}"
    );
    // The last valid position, after 1.e4, is still in place.
    engine.send("go depth 2");
    let best = engine.best_move();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &["e2e4"], &best),
        "{best}"
    );
}
