#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.start..self.end).fmt(f)
    }
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
    pub fn empty() -> Self {
        Self { start: 0, end: 0 }
    }
}
