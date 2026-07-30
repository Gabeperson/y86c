#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
    pub fn empty() -> Self {
        Self { start: 0, end: 0 }
    }
}

// impl PartialEq for Span {
//     fn eq(&self, other: &Self) -> bool {
//         // This is implemented in this way so that == comparisons between
//         // two nodes that contain differens `Span`s compare equal.
//         true
//     }
// }
