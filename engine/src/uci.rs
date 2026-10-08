use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use dumbfish_core::{Board, Color, Move, MoveKind};

use crate::eval;
use crate::search::{self, Iteration, Limits, MATE, MAX_DEPTH, Tables};
use crate::skill::{self, Skill};
use crate::style::{self, GROUPS, PRESETS, Style};
use crate::syzygy::Tablebases;
use crate::time;
use crate::tt::{self, TranspositionTable};
use crate::wdl;

const MAX_THREADS: usize = 256;
/// Commands the engine knows. Unknown tokens before one of them are skipped, as the protocol
/// asks: `joho debug on` turns debug mode on.
const COMMANDS: [&str; 12] = [
    "uci",
    "debug",
    "isready",
    "setoption",
    "register",
    "ucinewgame",
    "position",
    "go",
    "stop",
    "ponderhit",
    "quit",
    "eval",
];
const MAX_MULTIPV: usize = 256;
/// The search reports the root move it is on only after this long, not to flood the GUI.
const CURRMOVE_AFTER: Duration = Duration::from_secs(3);

/// Reads UCI commands from stdin until `quit` or end of input.
pub fn run() {
    let mut engine = Engine {
        board: Board::startpos(),
        tt: Some(TranspositionTable::new(tt::DEFAULT_MEGABYTES)),
        skill_level: skill::MAX_LEVEL,
        style: PRESETS[0].style,
        threads: 1,
        multipv: 1,
        show_wdl: false,
        debug: false,
        chess960: false,
        tablebases: None,
        stop: Arc::new(AtomicBool::new(false)),
        pondering: Arc::new(AtomicBool::new(false)),
        ponder: None,
        tables: Vec::new(),
        worker: None,
    };

    for line in io::stdin().lines() {
        let Ok(line) = line else { break };
        let mut tokens = line.split_whitespace();
        match tokens.by_ref().find(|t| COMMANDS.contains(t)) {
            Some("uci") => {
                println!("id name dumbfish {}", env!("CARGO_PKG_VERSION"));
                println!("id author tolstovrob");
                println!(
                    "option name Hash type spin default {} min 1 max {}",
                    tt::DEFAULT_MEGABYTES,
                    tt::MAX_MEGABYTES
                );
                println!("option name Clear Hash type button");
                println!("option name Threads type spin default 1 min 1 max {MAX_THREADS}");
                println!("option name MultiPV type spin default 1 min 1 max {MAX_MULTIPV}");
                println!("option name Ponder type check default false");
                println!("option name UCI_ShowWDL type check default false");
                println!("option name UCI_Chess960 type check default false");
                println!("option name SyzygyPath type string default <empty>");
                println!(
                    "option name UCI_EngineAbout type string default dumbfish {} by tolstovrob, \
                     a chess engine with a classical evaluation",
                    env!("CARGO_PKG_VERSION")
                );
                println!(
                    "option name Skill Level type spin default {0} min 0 max {0}",
                    skill::MAX_LEVEL
                );
                let presets: Vec<_> = PRESETS.iter().map(|p| format!("var {}", p.name)).collect();
                println!(
                    "option name Style type combo default {} {}",
                    PRESETS[0].name,
                    presets.join(" ")
                );
                for (group, default) in GROUPS.iter().zip(PRESETS[0].style.multipliers) {
                    println!(
                        "option name {} type spin default {default} min 0 max {}",
                        group_option(group),
                        style::MAX_MULTIPLIER
                    );
                }
                println!(
                    "option name Contempt type spin default {} min {} max {}",
                    PRESETS[0].style.contempt,
                    -style::MAX_CONTEMPT,
                    style::MAX_CONTEMPT
                );
                println!("uciok");
            }
            Some("isready") => println!("readyok"),
            Some("debug") => match tokens.next() {
                Some("on") => engine.debug = true,
                Some("off") => engine.debug = false,
                _ => println!("info string expected debug on or debug off"),
            },
            Some("register") => {}
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
                engine.tables.clear();
            }
            Some("position") => {
                engine.wait();
                match parse_position(tokens, engine.chess960) {
                    Ok(b) => engine.board = b,
                    Err(e) => println!("info string {e}"),
                }
                if engine.debug {
                    println!("info string position {}", engine.board.fen());
                }
            }
            Some("go") => {
                engine.wait();
                engine.go(tokens);
            }
            Some("eval") => {
                engine.wait();
                println!("{}", eval::explain(&engine.board, &engine.style));
            }
            Some("ponderhit") => engine.ponderhit(),
            Some("stop") => engine.finish(),
            Some("quit") => break,
            Some(_) => {}
            None => {
                if let Some(first) = line.split_whitespace().next() {
                    println!("info string unknown command: {first}");
                }
            }
        }
    }
    engine.finish();
}

struct Engine {
    board: Board,
    /// Moves into the search thread for the duration of a search.
    tt: Option<TranspositionTable>,
    skill_level: u8,
    /// Set by `Style`; the group options and `Contempt` then override single values.
    style: Style,
    threads: usize,
    multipv: usize,
    show_wdl: bool,
    /// Set by `debug on`: the engine also reports how it understood `position` and `go`.
    debug: bool,
    /// Castling moves are written as the king taking its own rook.
    chess960: bool,
    tablebases: Option<Arc<Tablebases>>,
    stop: Arc<AtomicBool>,
    /// Set while a `go ponder` search waits for `ponderhit`.
    pondering: Arc<AtomicBool>,
    /// Limits and clock of the pondering search, applied at `ponderhit`.
    ponder: Option<(Arc<Limits>, Option<Budget>)>,
    /// What the search threads learned in this game; moves into the search thread with `tt`.
    tables: Vec<Tables>,
    worker: Option<JoinHandle<(TranspositionTable, Vec<Tables>)>>,
}

impl Engine {
    /// Waits until the running search, if any, has printed `bestmove`. Like Stockfish, commands
    /// that change the position wait for the search instead of stopping it.
    fn wait(&mut self) {
        if let Some(handle) = self.worker.take() {
            let (tt, tables) = handle.join().expect("search thread panicked");
            (self.tt, self.tables) = (Some(tt), tables);
        }
        self.ponder = None;
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
        let name = name.join(" ").to_lowercase();
        if let Some(group) = GROUPS
            .iter()
            .position(|g| group_option(g).to_lowercase() == name)
        {
            let percent = parse_in(&value, 0, style::MAX_MULTIPLIER)?;
            self.style.multipliers[group] = percent;
            return Ok(());
        }
        match (name.as_str(), &value[..]) {
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
            ("style", [preset]) => {
                let preset = PRESETS
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(preset))
                    .ok_or_else(|| format!("unknown Style {preset:?}"))?;
                self.style = preset.style;
                Ok(())
            }
            ("ponder", ["true" | "false"]) => Ok(()),
            ("clear hash", []) => {
                self.tt().clear();
                Ok(())
            }
            ("uci_engineabout", _) => Ok(()),
            ("uci_showwdl", [value @ ("true" | "false")]) => {
                self.show_wdl = *value == "true";
                Ok(())
            }
            ("uci_chess960", [value @ ("true" | "false")]) => {
                self.chess960 = *value == "true";
                Ok(())
            }
            ("syzygypath", _) => {
                let paths = value.join(" ");
                let tablebases = match paths.as_str() {
                    "" | "<empty>" => Tablebases::default(),
                    paths => Tablebases::open(paths),
                };
                println!(
                    "info string found {} tablebases, up to {} pieces",
                    tablebases.len(),
                    tablebases.max_pieces()
                );
                self.tablebases = (!tablebases.is_empty()).then(|| Arc::new(tablebases));
                Ok(())
            }
            ("threads", _) => {
                self.threads = parse_in(&value, 1, MAX_THREADS as i32)? as usize;
                Ok(())
            }
            ("multipv", _) => {
                self.multipv = parse_in(&value, 1, MAX_MULTIPV as i32)? as usize;
                Ok(())
            }
            ("contempt", _) => {
                self.style.contempt = parse_in(&value, -style::MAX_CONTEMPT, style::MAX_CONTEMPT)?;
                Ok(())
            }
            (name, _) => Err(format!("unknown option or value: {name:?}")),
        }
    }

    /// Starts a search on a separate thread so that `stop` and `isready` stay responsive.
    fn go<'a>(&mut self, tokens: impl Iterator<Item = &'a str>) {
        let start = Instant::now();
        let go = parse_go(tokens, &self.board, self.chess960);
        if self.debug {
            let budget = go.budget.map(|b| (b.soft, b.hard));
            println!(
                "info string go depth {:?} nodes {:?} mate {:?} time budget (soft, hard) \
                 {budget:?} infinite {} ponder {} searchmoves {}",
                go.limits.depth,
                go.limits.nodes,
                go.limits.mate,
                go.infinite,
                go.ponder,
                go.limits.searchmoves.len()
            );
        }
        let mut limits = go.limits;
        let seed = SystemTime::UNIX_EPOCH
            .elapsed()
            .map_or(0, |t| t.as_nanos() as u64);
        limits.skill = Some(Skill {
            level: self.skill_level,
            seed,
        });
        limits.style = self.style;
        limits.threads = self.threads;
        limits.multipv = self.multipv;
        limits.tablebases = self.tablebases.clone();
        let limits = Arc::new(limits);
        if go.ponder {
            self.ponder = Some((Arc::clone(&limits), go.budget));
        } else if let Some(budget) = go.budget {
            budget.apply(&limits, start);
        }

        self.stop.store(false, Ordering::Relaxed);
        self.pondering.store(go.ponder, Ordering::Relaxed);
        let (stop, pondering) = (Arc::clone(&self.stop), Arc::clone(&self.pondering));
        let infinite = go.infinite;
        let (show_wdl, chess960) = (self.show_wdl, self.chess960);
        let mut board = self.board.clone();
        let root = self.board.clone();
        let tt = self.tt.take().expect("no search is running");
        let mut tables = std::mem::take(&mut self.tables);
        self.worker = Some(thread::spawn(move || {
            let write = |mv: Move| uci_move(&root, mv, chess960);
            let mut line = Vec::new();
            let (best, _) = search::search_reporting(
                &mut board,
                &limits,
                &stop,
                &tt,
                &mut tables,
                |it| {
                    print_info(it, show_wdl, &write);
                    if it.multipv == 1 {
                        line.clone_from(&it.pv);
                    }
                },
                |depth, mv, number| {
                    if start.elapsed() >= CURRMOVE_AFTER {
                        let mv = write(mv);
                        println!("info depth {depth} currmove {mv} currmovenumber {number}");
                    }
                },
            );
            while (infinite || pondering.load(Ordering::Relaxed)) && !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
            }
            let ponder = match (best, &line[..]) {
                (Some(best), [first, reply, ..]) if *first == best => {
                    format!(" ponder {}", write(*reply))
                }
                _ => String::new(),
            };
            println!(
                "bestmove {}{ponder}",
                best.map_or("0000".to_string(), write)
            );
            (tt, tables)
        }));
    }

    /// The opponent played the expected move: the pondering search goes on as a normal one, on
    /// the clock it was given, counted from now.
    fn ponderhit(&mut self) {
        if let Some((limits, budget)) = self.ponder.take() {
            if let Some(budget) = budget {
                budget.apply(&limits, Instant::now());
            }
            self.pondering.store(false, Ordering::Relaxed);
        }
    }
}

/// UCI name of the multiplier option for a style group: `king_attack` -> `Style King Attack`.
fn group_option(group: &str) -> String {
    let words: Vec<_> = group
        .split('_')
        .map(|w| w[..1].to_uppercase() + &w[1..])
        .collect();
    format!("Style {}", words.join(" "))
}

/// The single value of an option, which must be in `min..=max`.
fn parse_in(value: &[&str], min: i32, max: i32) -> Result<i32, String> {
    match value {
        [v] => v
            .parse()
            .ok()
            .filter(|v| (min..=max).contains(v))
            .ok_or_else(|| format!("expected {min}..={max}, got {v:?}")),
        _ => Err(format!("expected one value in {min}..={max}")),
    }
}

/// `mv` in UCI notation; in Chess960 a castling move is the king taking its own rook.
fn uci_move(board: &Board, mv: Move, chess960: bool) -> String {
    match mv.kind() {
        MoveKind::Castle if chess960 => format!("{}{}", mv.from(), board.castling_rook(mv).0),
        _ => mv.to_string(),
    }
}

/// `position (startpos | fen <fen>) [moves <move>...]`. Nothing changes on error.
fn parse_position<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    chess960: bool,
) -> Result<Board, String> {
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
            .find(|&&m| uci_move(&board, m, chess960) == token)
            .copied();
        board.make_move(mv.ok_or_else(|| format!("illegal move {token:?} in {}", board.fen()))?);
    }
    Ok(board)
}

/// Search limits and whether the search is infinite, from the arguments of `go`.
/// What `go` asked for.
struct Go {
    limits: Limits,
    infinite: bool,
    ponder: bool,
    budget: Option<Budget>,
}

/// Time for a search on its own clock, counted from when it starts: at `go`, or at `ponderhit`
/// for a search that began pondering.
#[derive(Clone, Copy)]
struct Budget {
    soft: Option<Duration>,
    hard: Duration,
}

impl Budget {
    fn apply(self, limits: &Limits, from: Instant) {
        limits.soft_deadline.set(self.soft.map(|soft| from + soft));
        limits.clock_start.set(Some(from));
        limits.deadline.set(Some(from + self.hard));
    }
}

fn parse_go<'a>(tokens: impl Iterator<Item = &'a str>, board: &Board, chess960: bool) -> Go {
    let us = board.side_to_move();
    let legal = board.legal_moves();
    let mut limits = Limits::default();
    let (mut time, mut inc, mut moves_to_go, mut movetime) = (None, Duration::ZERO, None, None);
    let (mut infinite, mut ponder) = (false, false);
    let mut tokens = tokens.peekable();
    while let Some(token) = tokens.next() {
        let mut number = || tokens.next().and_then(|v| v.parse::<u64>().ok());
        match token {
            "depth" => limits.depth = number().map(|d| d as u32),
            "nodes" => limits.nodes = number(),
            "mate" => limits.mate = number().map(|n| n as u32),
            "movetime" => movetime = number().map(Duration::from_millis),
            "wtime" | "btime" if (token == "wtime") == (us == Color::White) => {
                time = number().map(Duration::from_millis)
            }
            "winc" | "binc" if (token == "winc") == (us == Color::White) => {
                inc = number().map(Duration::from_millis).unwrap_or_default()
            }
            "movestogo" => moves_to_go = number().filter(|&n| n > 0),
            "infinite" => infinite = true,
            "ponder" => ponder = true,
            "searchmoves" => {
                while let Some(mv) = tokens
                    .peek()
                    .and_then(|t| legal.iter().find(|&&m| uci_move(board, m, chess960) == *t))
                {
                    limits.searchmoves.push(*mv);
                    tokens.next();
                }
            }
            _ => {}
        }
    }
    let budget = match (movetime, time) {
        (Some(hard), _) => Some(Budget { soft: None, hard }),
        (None, Some(time)) => {
            let (soft, hard) = time::allocate(time, inc, moves_to_go.map(|n| n as u32));
            Some(Budget {
                soft: Some(soft),
                hard,
            })
        }
        (None, None) => None,
    };
    Go {
        limits,
        infinite,
        ponder,
        budget,
    }
}

fn print_info(it: &Iteration, show_wdl: bool, write: &dyn Fn(Move) -> String) {
    let nps = (it.nodes as f64 / it.elapsed.as_secs_f64().max(1e-3)) as u64;
    let score = match it.score {
        s if s >= MATE - MAX_DEPTH as i32 => format!("mate {}", (MATE - s + 1) / 2),
        s if s <= MAX_DEPTH as i32 - MATE => format!("mate -{}", (MATE + s) / 2),
        s => format!("cp {s}"),
    };
    let score = if show_wdl {
        let [win, draw, loss] = wdl::MODEL.wdl(it.score);
        format!("{score} wdl {win} {draw} {loss}")
    } else {
        score
    };
    let pv: Vec<_> = it.pv.iter().map(|&m| write(m)).collect();
    println!(
        "info depth {} seldepth {} multipv {} score {score} nodes {} nps {nps} hashfull {} tbhits {} \
         time {} pv {}",
        it.depth,
        it.seldepth,
        it.multipv,
        it.nodes,
        it.hashfull,
        it.tb_hits,
        it.elapsed.as_millis(),
        pv.join(" ")
    );
}
