#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const fn index(self) -> usize {
        self as usize
    }
}
