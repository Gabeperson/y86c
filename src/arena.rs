use std::hash::Hash;
use std::{marker::PhantomData, num::NonZeroU32};

use ahash::AHashMap;
use smol_str::SmolStr;

use crate::span::Span;

#[derive(Debug, PartialEq, Eq)]
pub struct Id<T> {
    index: NonZeroU32,
    _marker: PhantomData<fn() -> T>,
}
impl<T> Copy for Id<T> {}
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Id<T> {
    fn new(index: NonZeroU32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Arena<T> {
    map: AHashMap<T, Id<T>>,
    vec: Vec<T>,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self {
            map: AHashMap::new(),
            vec: Vec::new(),
        }
    }
    pub fn insert(&mut self, item: T) -> Id<T> {
        self.vec.push(item);
        // The vec push BEFORE getting the ID via the len is intentional.
        // The first item inserted into the vec will then get the ID of 1
        // which means we can immediately convert this to a NonZeroU32.
        let id = self.vec.len() as u32;
        let Some(id_nonzero) = NonZeroU32::new(id) else {
            // If we get here, it means the u32 in the previous statement
            // wrapped, as we hit the max u32 capacity. Realistically,
            // the compiler should not need to deal with 4 billion unique
            // items, but we panic to avoid possible miscompilation on systems
            // that might have a LOT of ram.
            panic!("Tried to intern more than u32::MAX - 1 items");
        };
        Id::new(id_nonzero)
    }
    pub fn get(&self, id: Id<T>) -> &T {
        &self.vec[id.index.get() as usize - 1]
    }
    pub fn get_mut(&mut self, id: Id<T>) -> &mut T {
        &mut self.vec[id.index.get() as usize - 1]
    }
}

impl<T: Hash + Clone + Eq> Arena<T> {
    fn intern(&mut self, item: T) -> Id<T> {
        if let Some(id) = self.map.get(&item) {
            return *id;
        }
        let id = self.insert(item.clone());
        self.map.insert(item, id);
        id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(pub SmolStr);

impl Symbol {
    pub fn new(s: &str) -> Self {
        Self(SmolStr::new(s))
    }
}

#[derive(Clone, Debug)]
pub struct StringInterner {
    interner: Arena<Symbol>,
}

#[derive(Clone, Debug, Copy)]
pub struct InternedString {
    id: Id<Symbol>,
    pub span: Span,
}
impl PartialEq for InternedString {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl StringInterner {
    pub fn new() -> Self {
        Self {
            interner: Arena::new(),
        }
    }
    pub fn intern(&mut self, item: Symbol, span: Span) -> InternedString {
        InternedString {
            id: self.interner.intern(item),
            span,
        }
    }
    pub fn get(&self, interned_symbol: InternedString) -> &Symbol {
        self.interner.get(interned_symbol.id)
    }
}

#[test]
fn test_arena() {
    let mut arena = Arena::<u64>::new();
    let h1 = arena.insert(0);
    let h2 = arena.insert(1);
    let h3 = arena.insert(0);
    assert_eq!(*arena.get(h1), 0);
    assert_eq!(*arena.get(h2), 1);
    assert_eq!(*arena.get(h3), 0);
}

#[test]
fn test_intern() {
    let mut interner = StringInterner::new();
    let h1 = interner.intern(Symbol::new("hello"), Span::new(1, 100));
    let h2 = interner.intern(Symbol::new("hello"), Span::empty());
    let h3 = interner.intern(
        Symbol::new("abcdefghijklmnopqrstuvwxyz01234567890"),
        Span::new(0, 1_000_000),
    );
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
    assert_ne!(h2, h3);
    assert_eq!(h1.span, Span::new(1, 100));
    assert_eq!(h2.span, Span::empty());
    assert_eq!(h3.span, Span::new(0, 1_000_000));
    assert_eq!(interner.get(h1), interner.get(h2));
    assert_ne!(interner.get(h1), interner.get(h3));
    assert_ne!(interner.get(h2), interner.get(h3));
    assert_eq!(interner.get(h1), &Symbol::new("hello"));
    assert_eq!(interner.get(h2), &Symbol::new("hello"));
    assert_eq!(
        interner.get(h3),
        &Symbol::new("abcdefghijklmnopqrstuvwxyz01234567890")
    );
}
