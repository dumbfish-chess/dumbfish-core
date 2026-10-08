//! One Syzygy file: its header, the decompression of a stored value (canonical Huffman codes of
//! symbols that expand by recursive pairing) and the index of a position in the file.
//!
//! The format is the one of Ronald de Man's generator; the layout follows the reading code of
//! Stockfish (`src/syzygy/tbprobe.cpp`).

use std::sync::LazyLock;

use dumbfish_core::{Board, Color, PieceKind, Square, attacks};

/// What a file stores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Win, draw or loss with the 50-move rule (`.rtbw`), for both sides to move.
    Wdl,
    /// Distance to the next capture or pawn move (`.rtbz`), for one side to move.
    Dtz,
}

impl Kind {
    fn magic(self) -> [u8; 4] {
        match self {
            Kind::Wdl => [0x71, 0xE8, 0x23, 0x5D],
            Kind::Dtz => [0xD7, 0x66, 0x0C, 0xA5],
        }
    }
}

/// The DTZ values are stored for this side to move (white 0, black 1).
const STM: u8 = 1;
/// DTZ values go through a map, `WIDE` with 16-bit entries.
const MAPPED: u8 = 2;
/// Wins, and losses, are counted in plies rather than moves.
const WIN_PLIES: u8 = 4;
const LOSS_PLIES: u8 = 8;
const WIDE: u8 = 16;
/// Every position has the same value.
const SINGLE_VALUE: u8 = 128;

/// Most pieces a table can have.
pub const MAX_PIECES: usize = 7;

/// The material of a position or a table: 4 bits per piece kind and color.
pub fn material_key(counts: impl IntoIterator<Item = (Color, PieceKind)>) -> u64 {
    counts
        .into_iter()
        .map(|(color, kind)| 1 << (4 * (6 * color.index() + kind.index())))
        .sum()
}

/// `material_key` of the pieces on `board`.
pub fn board_key(board: &Board) -> u64 {
    let mut key = 0;
    for color in [Color::White, Color::Black] {
        for kind in PieceKind::ALL {
            let count = u64::from(board.pieces(color, kind).count());
            key += count << (4 * (6 * color.index() + kind.index()));
        }
    }
    key
}

/// What a table holds, from its name (`KRPvKR`): the first side is white in the table.
#[derive(Clone, Debug)]
pub struct Material {
    /// Of white as the stronger side, and with the colors swapped.
    pub keys: [u64; 2],
    pub pieces: usize,
    pub has_pawns: bool,
    /// Some piece other than a king is alone of its kind and color.
    pub has_unique_pieces: bool,
    /// Pawns of the leading color, the one whose pawns are encoded first, and of the other.
    pub pawns: [usize; 2],
}

impl Material {
    /// `None` unless `name` is like `KQPvKR`, each side a king and up to `MAX_PIECES` in all.
    pub fn parse(name: &str) -> Option<Self> {
        let (white, black) = name.split_once('v')?;
        let side = |text: &str, color| -> Option<Vec<(Color, PieceKind)>> {
            text.starts_with('K').then_some(())?;
            text.chars()
                .map(|c| {
                    let kind = PieceKind::ALL
                        .into_iter()
                        .find(|k| k.char() == c.to_ascii_lowercase() && c.is_ascii_uppercase())?;
                    Some((color, kind))
                })
                .collect()
        };
        let (white, black) = (side(white, Color::White)?, side(black, Color::Black)?);
        let all: Vec<_> = white.iter().chain(&black).copied().collect();
        let kings = all.iter().filter(|p| p.1 == PieceKind::King).count();
        if kings != 2 || all.len() > MAX_PIECES {
            return None;
        }
        let count = |color, kind| all.iter().filter(|&&p| p == (color, kind)).count();
        let swapped = all.iter().map(|&(color, kind)| (!color, kind));
        let (white_pawns, black_pawns) = (
            count(Color::White, PieceKind::Pawn),
            count(Color::Black, PieceKind::Pawn),
        );
        let white_leads = black_pawns == 0 || (white_pawns > 0 && black_pawns >= white_pawns);
        Some(Self {
            keys: [material_key(all.iter().copied()), material_key(swapped)],
            pieces: all.len(),
            has_pawns: white_pawns + black_pawns > 0,
            has_unique_pieces: [Color::White, Color::Black].into_iter().any(|color| {
                PieceKind::ALL[..5]
                    .iter()
                    .any(|&kind| count(color, kind) == 1)
            }),
            pawns: if white_leads {
                [white_pawns, black_pawns]
            } else {
                [black_pawns, white_pawns]
            },
        })
    }

    fn symmetric(&self) -> bool {
        self.keys[0] == self.keys[1]
    }
}

/// Square encodings and binomial coefficients shared by all tables.
struct Maps {
    /// Squares a2 ..= h7 by how far the leading pawn is from the edge files and the first rank;
    /// the leading pawn has the largest value.
    pawns: [u64; 64],
    /// Squares below the a1-h8 diagonal, 0 ..= 27.
    b1h1h7: [u64; 64],
    /// The triangle a1-d1-d4: 0 ..= 5 below the diagonal, 6 ..= 9 on it.
    a1d1d4: [u64; 64],
    /// The 462 placements of two kings with the first in the triangle a1-d1-d4.
    kk: [[u64; 64]; 10],
    /// `binomial[k][n]`: ways to choose `k` of `n`.
    binomial: [[u64; 64]; 6],
    lead_pawn_index: [[u64; 64]; 6],
    lead_pawns_size: [[u64; 4]; 6],
}

/// Rank minus file: 0 on the a1-h8 diagonal, negative below it.
fn off_diagonal(sq: usize) -> i32 {
    (sq / 8) as i32 - (sq % 8) as i32
}

static MAPS: LazyLock<Maps> = LazyLock::new(|| {
    let mut maps = Maps {
        pawns: [0; 64],
        b1h1h7: [0; 64],
        a1d1d4: [u64::MAX; 64],
        kk: [[0; 64]; 10],
        binomial: [[0; 64]; 6],
        lead_pawn_index: [[0; 64]; 6],
        lead_pawns_size: [[0; 4]; 6],
    };
    for (code, sq) in (0..64).filter(|&sq| off_diagonal(sq) < 0).enumerate() {
        maps.b1h1h7[sq] = code as u64;
    }
    let triangle = (0..28).filter(|&sq| sq % 8 <= 3);
    let below = triangle.clone().filter(|&sq| off_diagonal(sq) < 0);
    let on = triangle.filter(|&sq| off_diagonal(sq) == 0);
    for (code, sq) in below.chain(on).enumerate() {
        maps.a1d1d4[sq] = code as u64;
    }

    let mut code = 0;
    let mut both_on_diagonal = Vec::new();
    for index in 0..10 {
        let first = (0..64).find(|&sq| maps.a1d1d4[sq] == index).unwrap();
        let near = attacks::king(Square::from_index(first)) | Square::from_index(first).bb();
        for second in 0..64 {
            if near.contains(Square::from_index(second)) {
                continue;
            }
            match (off_diagonal(first), off_diagonal(second)) {
                (0, o) if o > 0 => {}
                (0, 0) => both_on_diagonal.push((index, second)),
                _ => {
                    maps.kk[index as usize][second] = code;
                    code += 1;
                }
            }
        }
    }
    for (index, second) in both_on_diagonal {
        maps.kk[index as usize][second] = code;
        code += 1;
    }

    maps.binomial[0][0] = 1;
    for n in 1..64 {
        for k in 0..6.min(n + 1) {
            maps.binomial[k][n] = if k > 0 {
                maps.binomial[k - 1][n - 1]
            } else {
                0
            } + if k < n { maps.binomial[k][n - 1] } else { 0 };
        }
    }

    for lead in 1..=5 {
        for file in 0..4 {
            let mut index = 0;
            for rank in 1..=6 {
                let sq = rank * 8 + file;
                if lead == 1 {
                    let available = 47 - 2 * (6 * file + rank - 1) as u64;
                    maps.pawns[sq] = available;
                    maps.pawns[sq ^ 7] = available - 1;
                }
                maps.lead_pawn_index[lead][sq] = index;
                index += maps.binomial[lead - 1][maps.pawns[sq] as usize];
            }
            maps.lead_pawns_size[lead][file] = index;
        }
    }
    maps
});

/// Decoding data of one side to move and, with pawns, one file of the leading pawn.
#[derive(Clone, Default)]
struct Pairs {
    flags: u8,
    /// Piece codes in encoding order: kind 1 ..= 6 from pawn to king, plus 8 for black.
    pieces: [u8; MAX_PIECES],
    /// Sizes of the groups of pieces encoded together, ending with 0.
    group_len: [usize; MAX_PIECES + 1],
    /// Multiplier of each group's index; the entry after the last group is the table size.
    group_index: [u64; MAX_PIECES + 1],
    min_sym_len: u8,
    block_size: usize,
    span: u64,
    blocks: usize,
    block_length_size: usize,
    sparse_index_size: usize,
    /// Offsets in the file.
    lowest_sym: usize,
    sparse_index: usize,
    block_length: usize,
    data: usize,
    data_end: usize,
    /// `base64[l]`: the lowest code of length `min_sym_len + l`, left-aligned in 64 bits.
    base64: Vec<u64>,
    /// How many values a symbol stands for, minus one.
    symlen: Vec<u8>,
    /// The two symbols a symbol expands to; a leaf has `0xFFF` on the right and its value on
    /// the left.
    btree: Vec<(u16, u16)>,
    /// Offsets of the DTZ maps for win, loss, cursed win and blessed loss.
    map_index: [usize; 4],
}

/// A loaded file.
pub struct Table {
    bytes: Vec<u8>,
    kind: Kind,
    material: Material,
    /// `[side to move][file of the leading pawn]`, one side for DTZ and symmetric tables.
    pairs: Vec<[Pairs; 4]>,
    map: usize,
}

/// Bounds-checked little- and big-endian reads.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&self, at: usize) -> Option<u8> {
        self.0.get(at).copied()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.0.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.0.get(at..at + 4)?.try_into().ok()?))
    }
}

impl Table {
    /// Parses the file `bytes` of the table with `material`; `None` if it is malformed.
    pub fn new(bytes: Vec<u8>, kind: Kind, material: Material) -> Option<Self> {
        if bytes.len() % 64 != 16 || bytes.get(..4)? != kind.magic() {
            return None;
        }
        let mut table = Self {
            bytes,
            kind,
            pairs: Vec::new(),
            map: 0,
            material,
        };
        table.parse()?;
        Some(table)
    }

    fn parse(&mut self) -> Option<()> {
        let read = Reader(&self.bytes);
        let m = &self.material;
        let flags = read.u8(4)?;
        if (flags & 2 != 0) != m.has_pawns || (flags & 1 != 0) != !m.symmetric() {
            return None;
        }
        let sides = if self.kind == Kind::Wdl && !m.symmetric() {
            2
        } else {
            1
        };
        let files = if m.has_pawns { 4 } else { 1 };
        let both_pawns = m.has_pawns && m.pawns[1] > 0;
        let mut pairs = vec![<[Pairs; 4]>::default(); sides];
        let mut at = 5;
        for file in 0..files {
            let order_byte = |i: usize| read.u8(at + i);
            let first = order_byte(0)?;
            let second = if both_pawns { order_byte(1)? } else { 0xFF };
            let orders = [[first & 0xF, second & 0xF], [first >> 4, second >> 4]];
            at += 1 + usize::from(both_pawns);
            for k in 0..m.pieces {
                let byte = read.u8(at)?;
                for (side, p) in pairs.iter_mut().enumerate() {
                    p[file].pieces[k] = if side == 0 { byte & 0xF } else { byte >> 4 };
                }
                at += 1;
            }
            for (side, p) in pairs.iter_mut().enumerate() {
                set_groups(&mut p[file], m, orders[side], file)?;
            }
        }
        at += at & 1;

        for file in 0..files {
            for p in pairs.iter_mut() {
                at = set_sizes(&mut p[file], &read, at)?;
            }
        }
        if self.kind == Kind::Dtz {
            self.map = at;
            let p = &mut pairs[0];
            for pairs in p.iter_mut().take(files) {
                if pairs.flags & MAPPED == 0 {
                    continue;
                }
                if pairs.flags & WIDE != 0 {
                    at += at & 1;
                    for i in 0..4 {
                        pairs.map_index[i] = (at - self.map) / 2 + 1;
                        at += 2 * usize::from(read.u16(at)?) + 2;
                    }
                } else {
                    for i in 0..4 {
                        pairs.map_index[i] = at - self.map + 1;
                        at += usize::from(read.u8(at)?) + 1;
                    }
                }
            }
            at += at & 1;
        }
        for file in 0..files {
            for p in pairs.iter_mut() {
                p[file].sparse_index = at;
                at += 6 * p[file].sparse_index_size;
            }
        }
        for file in 0..files {
            for p in pairs.iter_mut() {
                p[file].block_length = at;
                at += 2 * p[file].block_length_size;
            }
        }
        for file in 0..files {
            for p in pairs.iter_mut() {
                at = (at + 63) & !63;
                p[file].data = at;
                at += p[file].blocks * p[file].block_size;
                p[file].data_end = at;
            }
        }
        (at <= self.bytes.len()).then_some(())?;
        self.pairs = pairs;
        Some(())
    }

    /// The stored value of `board`, whose material is this table's. For WDL tables -2 ..= 2
    /// from the side to move. For DTZ tables the distance in plies, its sign left to the caller
    /// who passes the position's `wdl`; `None` if the table stores the other side to move.
    pub fn probe(&self, board: &Board, wdl: i32) -> Option<i32> {
        let maps = &*MAPS;
        let m = &self.material;
        let us = board.side_to_move();
        let flip = (m.symmetric() && us == Color::Black) || board_key(board) != m.keys[0];
        let (flip_color, flip_squares) = if flip { (8, 56) } else { (0, 0) };
        let stm = usize::from(flip) ^ us.index();

        let mut squares = [0usize; MAX_PIECES];
        let mut pieces = [0u8; MAX_PIECES];
        let mut size = 0;
        let mut lead_pawns = dumbfish_core::Bitboard::EMPTY;
        let mut file = 0;
        if m.has_pawns {
            let lead = self.pairs[0][0].pieces[0] ^ flip_color;
            let color = if lead & 8 == 0 {
                Color::White
            } else {
                Color::Black
            };
            lead_pawns = board.pieces(color, PieceKind::Pawn);
            for sq in lead_pawns {
                squares[size] = sq.index() ^ flip_squares;
                size += 1;
            }
            let lead = (0..size).max_by_key(|&i| maps.pawns[squares[i]]).unwrap();
            squares.swap(0, lead);
            file = (squares[0] % 8).min(7 - squares[0] % 8);
        }
        let lead_count = size;

        if self.kind == Kind::Dtz {
            let flags = self.pairs[0][file].flags;
            if usize::from(flags & STM) != stm && !(m.symmetric() && !m.has_pawns) {
                return None;
            }
        }

        for sq in board.occupied() & !lead_pawns {
            let piece = board.piece_on(sq).unwrap();
            squares[size] = sq.index() ^ flip_squares;
            pieces[size] =
                (piece.kind.index() as u8 + 1 + 8 * piece.color.index() as u8) ^ flip_color;
            size += 1;
        }
        let d = &self.pairs[stm % self.pairs.len()][file];
        for i in lead_count..size.saturating_sub(1) {
            if let Some(j) = (i + 1..size).find(|&j| pieces[j] == d.pieces[i]) {
                pieces.swap(i, j);
                squares.swap(i, j);
            }
        }

        if squares[0] % 8 > 3 {
            for sq in &mut squares[..size] {
                *sq ^= 7;
            }
        }
        let mut index;
        if m.has_pawns {
            index = maps.lead_pawn_index[lead_count][squares[0]];
            squares[1..lead_count].sort_by_key(|&sq| maps.pawns[sq]);
            for (i, &sq) in squares[..lead_count].iter().enumerate().skip(1) {
                index += maps.binomial[i][maps.pawns[sq] as usize];
            }
        } else {
            if squares[0] / 8 > 3 {
                for sq in &mut squares[..size] {
                    *sq ^= 56;
                }
            }
            for i in 0..d.group_len[0] {
                match off_diagonal(squares[i]) {
                    0 => continue,
                    o if o > 0 => {
                        for sq in &mut squares[i..size] {
                            *sq = ((*sq >> 3) | (*sq << 3)) & 63;
                        }
                    }
                    _ => {}
                }
                break;
            }
            let [s0, s1, s2] = [squares[0], squares[1], squares[2]].map(|s| s as u64);
            let rank = |s: u64| s / 8;
            index = if m.has_unique_pieces {
                let adjust1 = u64::from(s1 > s0);
                let adjust2 = u64::from(s2 > s0) + u64::from(s2 > s1);
                if off_diagonal(s0 as usize) != 0 {
                    (maps.a1d1d4[s0 as usize] * 63 + (s1 - adjust1)) * 62 + s2 - adjust2
                } else if off_diagonal(s1 as usize) != 0 {
                    (6 * 63 + rank(s0) * 28 + maps.b1h1h7[s1 as usize]) * 62 + s2 - adjust2
                } else if off_diagonal(s2 as usize) != 0 {
                    6 * 63 * 62
                        + 4 * 28 * 62
                        + rank(s0) * 7 * 28
                        + (rank(s1) - adjust1) * 28
                        + maps.b1h1h7[s2 as usize]
                } else {
                    6 * 63 * 62
                        + 4 * 28 * 62
                        + 4 * 7 * 28
                        + rank(s0) * 7 * 6
                        + (rank(s1) - adjust1) * 6
                        + (rank(s2) - adjust2)
                }
            } else {
                maps.kk[maps.a1d1d4[s0 as usize] as usize][s1 as usize]
            };
        }

        index *= d.group_index[0];
        let mut start = d.group_len[0];
        let mut remaining_pawns = m.has_pawns && m.pawns[1] > 0;
        let mut next = 1;
        while d.group_len[next] != 0 {
            let len = d.group_len[next];
            squares[start..start + len].sort_unstable();
            let mut n = 0;
            for i in 0..len {
                let sq = squares[start + i];
                let adjust = squares[..start].iter().filter(|&&s| sq > s).count();
                n += maps.binomial[i + 1][sq - adjust - 8 * usize::from(remaining_pawns)];
            }
            remaining_pawns = false;
            index += n * d.group_index[next];
            start += len;
            next += 1;
        }

        let value = i32::from(self.decompress(d, index)?);
        Some(match self.kind {
            Kind::Wdl => value - 2,
            Kind::Dtz => self.map_dtz(file, value, wdl)?,
        })
    }

    /// The DTZ in plies, unsigned, of a stored `value` in a position with result `wdl`.
    fn map_dtz(&self, file: usize, mut value: i32, wdl: i32) -> Option<i32> {
        let read = Reader(&self.bytes);
        let p = &self.pairs[0][file];
        if p.flags & MAPPED != 0 {
            let at = p.map_index[[1, 3, 0, 2, 0][(wdl + 2) as usize]] + value as usize;
            value = i32::from(if p.flags & WIDE != 0 {
                read.u16(self.map + 2 * at)?
            } else {
                u16::from(read.u8(self.map + at)?)
            });
        }
        let in_moves = match wdl {
            2 => p.flags & WIN_PLIES == 0,
            -2 => p.flags & LOSS_PLIES == 0,
            _ => true,
        };
        Some(if in_moves { 2 * value } else { value } + 1)
    }

    /// The value at `index`: find the block holding it through the sparse index, walk the
    /// Huffman codes of the block to the symbol covering it, and expand that symbol's pairs.
    fn decompress(&self, d: &Pairs, index: u64) -> Option<u16> {
        if d.flags & SINGLE_VALUE != 0 {
            return Some(u16::from(d.min_sym_len));
        }
        let read = Reader(&self.bytes);
        let k = (index / d.span) as usize;
        let mut block = read.u32(d.sparse_index + 6 * k)? as usize;
        let mut offset = i64::from(read.u16(d.sparse_index + 6 * k + 4)?);
        offset += (index % d.span) as i64 - (d.span / 2) as i64;
        block = block.min(d.block_length_size.checked_sub(1)?);
        let length = |block: usize| read.u16(d.block_length + 2 * block).map(i64::from);
        while offset < 0 && block > 0 {
            block -= 1;
            offset += length(block)? + 1;
        }
        while offset > length(block)? && block + 1 < d.block_length_size {
            offset -= length(block)? + 1;
            block += 1;
        }

        let word = |at: usize, bytes: usize| -> u64 {
            if at + bytes <= d.data_end {
                self.bytes[at..at + bytes]
                    .iter()
                    .fold(0, |acc, &b| acc << 8 | u64::from(b))
            } else {
                0
            }
        };
        let mut at = d.data + block * d.block_size;
        let mut buffer = word(at, 8);
        at += 8;
        let mut buffer_bits = 64;
        let min = u32::from(d.min_sym_len);
        let mut sym;
        loop {
            let mut len = 0;
            while buffer < d.base64[len] {
                len += 1;
                (len < d.base64.len()).then_some(())?;
            }
            let shift = 64 - len as u32 - min;
            sym = (((buffer - d.base64[len]) >> shift) as u16)
                .wrapping_add(read.u16(d.lowest_sym + 2 * len)?)
                & 0xFFF;
            let covered = i64::from(d.symlen[sym as usize]) + 1;
            if offset < covered {
                break;
            }
            offset -= covered;
            let bits = len as u32 + min;
            buffer <<= bits;
            buffer_bits -= bits;
            if buffer_bits <= 32 {
                buffer_bits += 32;
                buffer |= word(at, 4) << (64 - buffer_bits);
                at += 4;
            }
        }
        while d.symlen[sym as usize] != 0 {
            let (left, right) = d.btree[sym as usize];
            let covered = i64::from(d.symlen[left as usize]) + 1;
            if offset < covered {
                sym = left;
            } else {
                offset -= covered;
                sym = right;
            }
        }
        Some(d.btree[sym as usize].0)
    }
}

/// Splits the pieces into groups encoded together and sets each group's index multiplier;
/// `order` gives the position of the leading group and of the other side's pawns among them.
fn set_groups(d: &mut Pairs, m: &Material, order: [u8; 2], file: usize) -> Option<()> {
    let maps = &*MAPS;
    let mut first_len: i32 = if m.has_pawns {
        0
    } else if m.has_unique_pieces {
        3
    } else {
        2
    };
    let mut n = 0;
    d.group_len[0] = 1;
    for i in 1..m.pieces {
        first_len -= 1;
        if first_len > 0 || d.pieces[i] == d.pieces[i - 1] {
            d.group_len[n] += 1;
        } else {
            n += 1;
            d.group_len[n] = 1;
        }
    }
    n += 1;
    d.group_len[n] = 0;

    let both_pawns = m.has_pawns && m.pawns[1] > 0;
    let mut next = if both_pawns { 2 } else { 1 };
    let mut free = 64 - d.group_len[0] - if both_pawns { d.group_len[1] } else { 0 };
    let mut index: u64 = 1;
    let (order0, order1) = (usize::from(order[0]), usize::from(order[1]));
    let mut k = 0;
    while next < n || k == order0 || k == order1 {
        if k == order0 {
            d.group_index[0] = index;
            index *= if m.has_pawns {
                maps.lead_pawns_size[d.group_len[0]][file]
            } else if m.has_unique_pieces {
                31_332
            } else {
                462
            };
        } else if k == order1 {
            d.group_index[1] = index;
            index *= maps.binomial[d.group_len[1]][48 - d.group_len[0]];
        } else {
            d.group_index[next] = index;
            index *= maps.binomial[*d.group_len.get(next)?][free];
            free -= d.group_len[next];
            next += 1;
        }
        k += 1;
        (k <= 16).then_some(())?;
    }
    d.group_index[n] = index;
    Some(())
}

/// Reads the block and Huffman parameters of `d` at `at`; returns the offset after them.
fn set_sizes(d: &mut Pairs, read: &Reader, mut at: usize) -> Option<usize> {
    d.flags = read.u8(at)?;
    at += 1;
    if d.flags & SINGLE_VALUE != 0 {
        d.min_sym_len = read.u8(at)?;
        return Some(at + 1);
    }
    let size = d.group_index[d.group_len.iter().position(|&len| len == 0)?];
    let (block_bits, span_bits) = (read.u8(at)?, read.u8(at + 1)?);
    if block_bits >= 64 || span_bits >= 64 {
        return None;
    }
    d.block_size = 1 << block_bits;
    d.span = 1 << span_bits;
    d.sparse_index_size = size.div_ceil(d.span) as usize;
    let padding = usize::from(read.u8(at + 2)?);
    d.blocks = read.u32(at + 3)? as usize;
    d.block_length_size = d.blocks + padding;
    let (max_sym_len, min_sym_len) = (read.u8(at + 7)?, read.u8(at + 8)?);
    if min_sym_len == 0 || min_sym_len > max_sym_len || max_sym_len >= 64 {
        return None;
    }
    d.min_sym_len = min_sym_len;
    at += 9;
    d.lowest_sym = at;
    let lengths = usize::from(max_sym_len - min_sym_len) + 1;
    let lowest = |i: usize| read.u16(at + 2 * i).map(u64::from);
    d.base64 = vec![0; lengths];
    for i in (0..lengths - 1).rev() {
        d.base64[i] = d.base64[i + 1]
            .wrapping_add(lowest(i)?)
            .wrapping_sub(lowest(i + 1)?)
            / 2;
        if d.base64[i] * 2 < d.base64[i + 1] {
            return None;
        }
    }
    for (i, base) in d.base64.iter_mut().enumerate() {
        *base <<= 64 - i as u32 - u32::from(min_sym_len);
    }
    at += 2 * lengths;

    let symbols = usize::from(read.u16(at)?);
    at += 2;
    d.btree = vec![(0, 0xFFF); 4096];
    for (sym, entry) in d.btree.iter_mut().enumerate().take(symbols) {
        let b = read.0.get(at + 3 * sym..at + 3 * sym + 3)?;
        *entry = (
            u16::from(b[1] & 0xF) << 8 | u16::from(b[0]),
            u16::from(b[2]) << 4 | u16::from(b[1] >> 4),
        );
    }
    d.symlen = vec![0; 4096];
    let mut state = vec![0u8; 4096];
    for sym in 0..symbols {
        if state[sym] == 0 {
            set_symlen(d, sym, &mut state)?;
        }
    }
    Some(at + 3 * symbols + (symbols & 1))
}

/// Sets how many values `sym` expands to, minus one, after its children; `state` marks symbols
/// in progress (1) and done (2), and a cycle makes the file invalid.
fn set_symlen(d: &mut Pairs, sym: usize, state: &mut [u8]) -> Option<()> {
    state[sym] = 1;
    let (left, right) = d.btree[sym];
    if right != 0xFFF {
        let (left, right) = (usize::from(left), usize::from(right));
        for child in [left, right] {
            match state[child] {
                1 => return None,
                0 => set_symlen(d, child, state)?,
                _ => {}
            }
        }
        d.symlen[sym] = d.symlen[left]
            .checked_add(d.symlen[right])?
            .checked_add(1)?;
    }
    state[sym] = 2;
    Some(())
}
