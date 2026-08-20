#[derive(Debug, Clone)]
#[repr(u8)]
pub enum BitSet {
    Inline([u8; 23]),
    Heap(Box<[u64]>),
}

const _: () = assert!(std::mem::size_of::<BitSet>() == 24);

impl BitSet {
    pub fn new(bits: u32) -> Self {
        if bits <= 23 * 8 {
            Self::Inline([0u8; 23])
        } else {
            Self::Heap(vec![0; bits.div_ceil(64) as usize].into_boxed_slice())
        }
    }
    pub fn set(&mut self, idx: u32) {
        let idx = idx as usize;
        match self {
            BitSet::Inline(inline) => {
                inline[idx / 8] |= 1 << (idx % 8);
            }
            BitSet::Heap(items) => {
                items[idx / 64] |= 1 << (idx % 64);
            }
        }
    }
    pub fn unset(&mut self, idx: u32) {
        let idx = idx as usize;
        match self {
            BitSet::Inline(inline) => {
                inline[idx / 8] &= !(1 << (idx % 8));
            }
            BitSet::Heap(items) => {
                items[idx / 64] &= !(1 << (idx % 64));
            }
        }
    }
    pub fn get(&self, idx: u32) -> bool {
        let idx = idx as usize;
        match self {
            BitSet::Inline(inline) => (inline[idx / 8] & (1 << (idx % 8))) != 0,
            BitSet::Heap(items) => (items[idx / 64] & (1 << (idx % 64))) != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bitset() {
        let bitset = BitSet::new(0);
        assert!(matches!(bitset, BitSet::Inline(_)));
        let bitset = BitSet::new(23 * 8);
        assert!(matches!(bitset, BitSet::Inline(_)));
        let bitset = BitSet::new(23 * 8 + 1);
        assert!(matches!(bitset, BitSet::Heap(_)));

        let mut bitset = BitSet::new(10);
        for i in 0..10 {
            assert!(!bitset.get(i));
            bitset.set(i);
            assert!(bitset.get(i));
        }
        let mut bitset = BitSet::new(23 * 8);
        for i in 0..(23 * 8) {
            assert!(!bitset.get(i));
            bitset.set(i);
            assert!(bitset.get(i));
        }
        let mut bitset = BitSet::new(23 * 8 + 1 + 64 * 10);
        assert!(matches!(bitset, BitSet::Heap(_)));
        for i in 0..(23 * 8 + 64 * 10) {
            assert!(!bitset.get(i));
            bitset.set(i);
            assert!(bitset.get(i));
        }
    }
}
