use dumbfish::see::see;
use dumbfish_core::Board;

/// SEE of the legal move written in UCI notation.
fn see_of(fen: &str, uci: &str) -> i32 {
    let board = Board::from_fen(fen).unwrap();
    let mv = *board
        .legal_moves()
        .iter()
        .find(|m| m.to_string() == uci)
        .unwrap();
    see(&board, mv)
}

#[test]
fn exchanges() {
    let cases = [
        (
            "1k1r4/1pp4p/p7/4p3/8/P5P1/1PP4P/2K1R3 w - - 0 1",
            "e1e5",
            100,
            "undefended pawn",
        ),
        (
            "1k1r3q/1ppn3p/p4b2/4p3/8/P2N2P1/1PP1R1BP/2K1Q3 w - - 0 1",
            "d3e5",
            -200,
            "CPW example",
        ),
        (
            "4k3/8/3p4/4p3/8/8/8/4QK2 w - - 0 1",
            "e1e5",
            -800,
            "queen takes a defended pawn",
        ),
        (
            "4k3/4r3/8/4p3/8/8/4R3/4R1K1 w - - 0 1",
            "e2e5",
            100,
            "the doubled rook recaptures through the first",
        ),
        ("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", "a7a8q", 800, "promotion"),
        (
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "e5d6",
            100,
            "en passant",
        ),
        (
            "4k3/8/2p5/3p4/8/8/8/3RK3 w - - 0 1",
            "d1d5",
            -400,
            "defended once, recaptured",
        ),
    ];
    for (fen, mv, expected, what) in cases {
        assert_eq!(see_of(fen, mv), expected, "{what}: {mv} in {fen}");
    }
}
