pub mod bitset;
pub mod interner;
pub mod span;
pub mod symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Inline {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallingConvention {
    Internal,
    Abi,
}
