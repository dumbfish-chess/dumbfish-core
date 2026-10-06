use crate::{Piece, Square};

const PIECES: usize = 0;
const CASTLING: usize = PIECES + 12 * 64;
const EN_PASSANT: usize = CASTLING + 16;
const SIDE: usize = EN_PASSANT + 8;

/// splitmix64 stream, computed at compile time.
static KEYS: [u64; SIDE + 1] = {
    let mut keys = [0; SIDE + 1];
    let mut state: u64 = 0x2D35_8DCC_AA6C_78A5;
    let mut i = 0;
    while i < keys.len() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        keys[i] = z ^ (z >> 31);
        i += 1;
    }
    keys
};

pub fn piece(piece: Piece, sq: Square) -> u64 {
    KEYS[PIECES + piece.index() * 64 + sq.index()]
}

/// `rights` is the 4-bit castling mask.
pub fn castling(rights: u8) -> u64 {
    KEYS[CASTLING + rights as usize]
}

pub fn en_passant(sq: Square) -> u64 {
    KEYS[EN_PASSANT + sq.file() as usize]
}

/// XORed in when black is to move.
pub fn side() -> u64 {
    KEYS[SIDE]
}
