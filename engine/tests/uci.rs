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
    assert_eq!(
        lines.last().unwrap().split_whitespace().nth(1),
        Some("a1a8")
    );
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
            start.elapsed() < Duration::from_secs(5),
            "{go}: {:?}, an engine ignoring the clock searches for minutes",
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
    engine.send("go depth 2");
    let best = engine.best_move();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &["e2e4"], &best),
        "{best}"
    );
}

#[test]
fn takes_hanging_queen() {
    let mut engine = Engine::start();
    engine.send("position fen 4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1");
    engine.send("go depth 3");
    let lines = engine.read_until("bestmove");
    assert_eq!(
        lines.last().unwrap().split_whitespace().nth(1),
        Some("e4d5"),
        "{lines:?}"
    );
}

#[test]
fn eval_command() {
    let mut engine = Engine::start();
    engine.send("position startpos moves e2e4 d7d5 e4d5");
    engine.send("eval");
    let lines = engine.read_until("total:");
    assert!(lines[0].starts_with("feature"), "{lines:?}");
    let board =
        Board::from_fen("rnbqkbnr/ppp1pppp/8/3P4/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2").unwrap();
    let stm = dumbfish::eval::evaluate(&board);
    assert_eq!(
        lines.last().unwrap(),
        &format!(
            "total: {} cp for white, {stm} cp for the side to move",
            -stm
        )
    );
}

#[test]
fn avoids_repetition_when_ahead() {
    let mut engine = Engine::start();
    engine.send("position fen 4k3/8/8/8/8/8/8/R3K3 w - - 0 1 moves e1d1 e8d8 d1e1 d8e8");
    engine.send("go depth 3");
    assert_ne!(engine.best_move(), "e1d1", "e1d1 repeats the position");
}

#[test]
fn fifty_move_rule_is_a_draw() {
    let mut engine = Engine::start();
    engine.send("position fen 4k3/8/8/8/8/8/8/R3K3 w - - 99 80");
    engine.send("go depth 2");
    let lines = engine.read_until("bestmove");
    assert!(
        lines.iter().any(|l| l.contains(" score cp 0 ")),
        "{lines:?}"
    );
}

#[test]
fn sees_recapture_beyond_horizon() {
    let mut engine = Engine::start();
    engine.send("position fen 4k3/8/4p3/3p4/8/8/8/3QK3 w - - 0 1");
    engine.send("go depth 1");
    assert_ne!(engine.best_move(), "d1d5", "e6xd5 wins the queen");
}

#[test]
fn hash_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("option name Hash type spin")),
        "{lines:?}"
    );
    engine.send("setoption name Hash value 0");
    engine.send("setoption name Hash value lots");
    engine.send("setoption name UCI_Chess960 value true");
    engine.send("setoption name Hash value 1");
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        3,
        "{lines:?}"
    );
    engine.send("position startpos");
    engine.send("go depth 3");
    engine.best_move();
}

#[test]
fn finds_mate_in_two_at_every_depth() {
    let mut engine = Engine::start();
    engine.send("position fen 7k/8/5K2/8/8/8/8/R7 w - - 0 1");
    engine.send("go depth 6");
    let lines = engine.read_until("bestmove");
    for line in lines.iter().filter(|l| l.starts_with("info depth")) {
        let depth: u32 = line.split_whitespace().nth(2).unwrap().parse().unwrap();
        if depth >= 3 {
            assert!(line.contains(" score mate 2 "), "{line}");
        }
    }
}

#[test]
fn skill_level_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name Skill Level type spin default 20 min 0 max 20"),
        "{lines:?}"
    );
    engine.send("setoption name Skill Level value 21");
    engine.send("setoption name Skill Level value 0");
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        1,
        "{lines:?}"
    );
    engine.send("position startpos");
    engine.send("go wtime 1000 btime 1000");
    let best = engine.best_move();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &[], &best),
        "{best}"
    );
}

#[test]
fn style_options() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    for expected in [
        "option name Style type combo default balanced var balanced var aggressive",
        "option name Style King Attack type spin default 100 min 0 max 300",
        "option name Contempt type spin default 0 min -200 max 200",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(expected)),
            "{expected:?} in {lines:?}"
        );
    }
    for command in [
        "setoption name Style value aggressive",
        "setoption name Style King Attack value 200",
        "setoption name Style value reckless",
        "setoption name Style Mobility value -1",
        "setoption name Contempt value 500",
    ] {
        engine.send(command);
    }
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        3,
        "{lines:?}"
    );

    engine.send("eval");
    let lines = engine.read_until("total:");
    let percent = |group: &str| {
        let line = lines.iter().find(|l| l.starts_with(group)).unwrap();
        line.split_whitespace().nth(2).unwrap().to_string()
    };
    assert_eq!(
        percent("king_attack"),
        "200%",
        "manual value overrides the preset"
    );
    assert_eq!(percent("mobility"), "120%", "from the aggressive preset");
}

#[test]
fn contempt_decides_about_repetition() {
    let position = "position startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1";
    for (contempt, repeats) in [(-200, true), (200, false)] {
        let mut engine = Engine::start();
        engine.send(&format!("setoption name Contempt value {contempt}"));
        engine.send(position);
        engine.send("go depth 3");
        let best = engine.best_move();
        assert_eq!(best == "f6g8", repeats, "contempt {contempt}: {best}");
    }
}

#[test]
fn threads_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name Threads type spin default 1 min 1 max 256"),
        "{lines:?}"
    );
    engine.send("setoption name Threads value 0");
    engine.send("setoption name Threads value 3");
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        1,
        "{lines:?}"
    );
    engine.send("position startpos");
    engine.send("go infinite");
    std::thread::sleep(Duration::from_millis(200));
    engine.send("stop");
    let best = engine.best_move();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &[], &best),
        "{best}"
    );
}

#[test]
fn multipv_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name MultiPV type spin default 1 min 1 max 256"),
        "{lines:?}"
    );
    engine.send("setoption name MultiPV value 0");
    engine.send("setoption name MultiPV value 2");
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("info string"))
            .count(),
        1,
        "{lines:?}"
    );
    engine.send("position startpos");
    engine.send("go depth 4");
    let lines = engine.read_until("bestmove");
    for rank in ["multipv 1 ", "multipv 2 "] {
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("info depth 4 ") && l.contains(rank)),
            "{rank} in {lines:?}"
        );
    }
}

#[test]
fn ponder_waits_for_ponderhit() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name Ponder type check default false"),
        "{lines:?}"
    );
    engine.send("position startpos moves e2e4");
    engine.send("go ponder wtime 600 btime 600");
    match engine.lines.recv_timeout(Duration::from_millis(1500)) {
        Ok(line) => assert!(
            !line.starts_with("bestmove"),
            "bestmove while pondering: {line}"
        ),
        Err(e) => assert_eq!(e, RecvTimeoutError::Timeout),
    }
    let start = Instant::now();
    engine.send("ponderhit");
    let lines = engine.read_until("bestmove");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    let fields: Vec<_> = lines.last().unwrap().split_whitespace().collect();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &["e2e4"], fields[1]),
        "{fields:?}"
    );
    if let [_, best, "ponder", reply] = fields[..] {
        assert!(
            is_legal(dumbfish_core::board::STARTPOS, &["e2e4", best], reply),
            "{fields:?}"
        );
    }
}

#[test]
fn stop_ends_pondering() {
    let mut engine = Engine::start();
    engine.send("position startpos");
    engine.send("go ponder wtime 600 btime 600");
    std::thread::sleep(Duration::from_millis(200));
    engine.send("stop");
    let best = engine.best_move();
    assert!(
        is_legal(dumbfish_core::board::STARTPOS, &[], &best),
        "{best}"
    );
}

#[test]
fn searchmoves_restricts_the_root() {
    let mut engine = Engine::start();
    engine.send("position startpos");
    engine.send("go depth 4 searchmoves a2a3 h2h3 e2e5");
    let best = engine.best_move();
    assert!(best == "a2a3" || best == "h2h3", "{best}");
}

#[test]
fn show_wdl_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name UCI_ShowWDL type check default false"),
        "{lines:?}"
    );
    engine.send("position startpos");
    engine.send("go depth 2");
    let lines = engine.read_until("bestmove");
    assert!(!lines.iter().any(|l| l.contains(" wdl ")), "{lines:?}");
    engine.send("setoption name UCI_ShowWDL value true");
    engine.send("go depth 2");
    let lines = engine.read_until("bestmove");
    let info = lines
        .iter()
        .find(|l| l.starts_with("info depth 2 "))
        .unwrap();
    let fields: Vec<_> = info.split_whitespace().collect();
    let at = fields.iter().position(|&f| f == "wdl").unwrap();
    let sum: u32 = fields[at + 1..at + 4]
        .iter()
        .map(|v| v.parse::<u32>().unwrap())
        .sum();
    assert_eq!(sum, 1000, "{info}");
}

#[test]
fn syzygy_path_option() {
    let mut engine = Engine::start();
    engine.send("uci");
    let lines = engine.read_until("uciok");
    assert!(
        lines
            .iter()
            .any(|l| l == "option name SyzygyPath type string default <empty>"),
        "{lines:?}"
    );
    engine.send("setoption name SyzygyPath value /nonexistent");
    assert_eq!(
        engine.read_until("info string").last().unwrap(),
        "info string found 0 tablebases, up to 0 pieces",
        "a path without tables"
    );
}

#[test]
#[ignore = "needs Syzygy tables"]
fn plays_tablebase_wins() {
    let dir = std::env::var("SYZYGY_PATH")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../target/syzygy").into());
    let tablebases = dumbfish::syzygy::Tablebases::open(&dir);
    let mut engine = Engine::start();
    engine.send(&format!("setoption name SyzygyPath value {dir}"));
    let found = engine.read_until("info string");
    assert!(!found.last().unwrap().contains("found 0 "), "{found:?}");
    let fen = "8/8/8/3k4/8/8/8/KBN5 w - - 0 1";
    engine.send(&format!("position fen {fen}"));
    engine.send("go depth 3");
    let lines = engine.read_until("bestmove");
    let score: i32 = lines
        .iter()
        .rev()
        .find_map(|l| {
            l.split(" score cp ")
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap();
    assert!(score > 29_000, "a tablebase win: {lines:?}");
    let best = lines.last().unwrap().split_whitespace().nth(1).unwrap();
    let mut board = Board::from_fen(fen).unwrap();
    let mv = *board
        .legal_moves()
        .iter()
        .find(|m| m.to_string() == best)
        .unwrap();
    board.make_move(mv);
    assert_eq!(
        tablebases.probe_wdl(&mut board),
        Some(-2),
        "{best} keeps the win"
    );
}

#[test]
fn unknown_tokens_before_a_command_are_skipped() {
    let mut engine = Engine::start();
    engine.send("joho isready");
    engine.read_until("readyok");
    engine.send("joho debug on");
    engine.send("position startpos moves e2e4");
    assert_eq!(
        engine.read_until("info string").last().unwrap(),
        "info string position rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        "debug mode reports the position"
    );
    engine.send("debug off");
    engine.send("register later");
    engine.send("position startpos");
    engine.send("isready");
    let lines = engine.read_until("readyok");
    assert_eq!(lines, ["readyok"], "nothing reported without debug mode");
}

#[test]
fn go_mate_stops_at_the_mate() {
    let mut engine = Engine::start();
    engine.send("position fen 7k/8/5K2/8/8/8/8/R7 w - - 0 1");
    engine.send("go mate 2");
    let lines = engine.read_until("bestmove");
    let last_info = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("info depth"))
        .unwrap();
    assert!(last_info.contains(" score mate 2 "), "{lines:?}");
    let depth: u32 = last_info
        .split_whitespace()
        .nth(2)
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        depth < 10,
        "no deeper search once the mate is found: {lines:?}"
    );
}
