use crate::analysis::types::*;
use ahash::AHashMap;
use smol_str::SmolStr;
use tinyvec::TinyVec;

#[derive(Clone, Debug)]
pub struct ScopedHashMap {
    stack: TinyVec<[u32; 5]>,
    generation: u32,
    map: AHashMap<SmolStr, TypeId>,
}
