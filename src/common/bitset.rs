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
    pub fn union(&self, other: &Self) -> BitSet {
        match (self, other) {
            (BitSet::Inline(arr1), BitSet::Inline(arr2)) => {
                let mut arr = [0u8; 23];
                for (out, (&in1, &in2)) in arr.iter_mut().zip(arr1.iter().zip(arr2)) {
                    *out = in1 | in2;
                }
                BitSet::Inline(arr)
            }
            (BitSet::Heap(arr1), BitSet::Heap(arr2)) => {
                assert!(arr1.len() == arr2.len());
                let mut arr = vec![0u64; arr1.len()].into_boxed_slice();
                for (out, (&in1, &in2)) in arr.iter_mut().zip(arr1.iter().zip(arr2)) {
                    *out = in1 | in2;
                }
                BitSet::Heap(arr)
            }
            _ => unreachable!(),
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

        let mut bitset1 = BitSet::new(10);
        let mut bitset2 = BitSet::new(10);
        bitset1.set(0);
        bitset1.set(1);
        bitset1.set(2);
        bitset1.set(3);
        bitset2.set(4);
        bitset2.set(5);
        bitset2.set(6);
        bitset2.set(7);
        bitset1.set(8);
        bitset2.set(9);
        let bitset3 = bitset1.union(&bitset2);
        for i in 0..10 {
            assert!(bitset3.get(i));
        }
        let mut bitset1 = BitSet::new(10000);
        let mut bitset2 = BitSet::new(10000);
        for i in 0..500 {
            bitset1.set(i);
        }
        for i in 500..1000 {
            bitset2.set(i);
        }
        for i in 0..500 {
            assert!(!bitset2.get(i));
        }
        for i in 500..1000 {
            assert!(!bitset1.get(i));
        }
        for i in 1000..10000 {
            assert!(!bitset1.get(i));
            assert!(!bitset2.get(i));
        }
        let bitset3 = bitset1.union(&bitset2);
        for i in 0..1000 {
            assert!(bitset3.get(i));
        }
        for i in 1000..10000 {
            assert!(!bitset3.get(i));
        }
    }
}
