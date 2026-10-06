use dumbfish_core::Square;

#[test]
fn index_file_rank_roundtrip() {
    for (i, sq) in Square::ALL.into_iter().enumerate() {
        assert_eq!(sq.index(), i);
        assert_eq!(Square::from_index(i), sq);
        assert_eq!(Square::new(sq.file(), sq.rank()), sq);
    }
    assert_eq!((Square::E4.file(), Square::E4.rank()), (4, 3));
    assert_eq!(Square::H8.index(), 63);
}

#[test]
fn names() {
    for sq in Square::ALL {
        assert_eq!(sq.to_string().parse::<Square>(), Ok(sq));
    }
    assert_eq!(Square::E4.to_string(), "e4");
    for bad in ["", "e", "e9", "i1", "E4", "e44"] {
        assert!(bad.parse::<Square>().is_err(), "{bad:?}");
    }
}
