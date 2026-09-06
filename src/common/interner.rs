use std::{marker::PhantomData, num::NonZeroU32};

use ahash::AHashMap;

#[derive(Eq)]
pub struct Id<T> {
    index: NonZeroU32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    fn new(index: NonZeroU32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Id").field(&self.index).finish()
    }
}

impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self._marker.hash(state);
    }
}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self._marker == other._marker
    }
}

impl<T> Default for Id<T> {
    fn default() -> Self {
        Self {
            index: const { NonZeroU32::new(u32::MAX).unwrap() },
            _marker: PhantomData,
        }
    }
}

impl<T> Copy for Id<T> {}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Id<T> {
    pub fn get(self) -> u32 {
        // So ids are contiguous from 0
        self.index.get() - 1
    }
    pub fn is_invalid(self) -> bool {
        self.index.get() == u32::MAX
    }
}

#[derive(Clone, Debug)]
pub struct Interner<T> {
    map: AHashMap<T, Id<T>>,
    arr: Vec<T>,
}
impl<T> Default for Interner<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Interner<T> {
    pub fn new() -> Self {
        Self {
            map: AHashMap::new(),
            arr: Vec::new(),
        }
    }
}
impl<T: Clone + Eq + std::hash::Hash> Interner<T> {
    pub fn intern_deduplicated(&mut self, item: T) -> Id<T> {
        if let Some(id) = self.map.get(&item) {
            return *id;
        }
        let id = self.intern(item.clone());
        self.map.insert(item, id);
        id
    }
    #[track_caller]
    pub fn get_id_for(&self, item: T) -> Option<Id<T>> {
        self.map.get(&item).copied()
    }
}

impl<T> Interner<T> {
    pub fn intern(&mut self, item: T) -> Id<T> {
        self.arr.push(item);
        let idx = self.arr.len() as u32;
        if idx == u32::MAX {
            panic!("More than 4 billion entries...?");
        }
        let nonzero = NonZeroU32::new(idx).unwrap();
        Id::new(nonzero)
    }
    pub fn intern_mut(&mut self, item: T) -> (Id<T>, &mut T) {
        let idx = self.arr.len() as u32 + 1;
        let item = self.arr.push_mut(item);
        if idx == u32::MAX {
            panic!("More than 4 billion entries...?");
        }
        let nonzero = NonZeroU32::new(idx).unwrap();
        (Id::new(nonzero), item)
    }
    pub fn len(&self) -> usize {
        self.arr.len()
    }
    pub fn is_empty(&self) -> bool {
        self.arr.is_empty()
    }
    #[track_caller]
    pub fn get(&self, id: Id<T>) -> &T {
        if id.index.get() == u32::MAX {
            panic!("Internal compiler error");
        }
        let index = (id.index.get() - 1) as usize;
        self.arr.get(index).expect("Internal Compiler Error")
    }
    pub fn get_mut(&mut self, id: Id<T>) -> &mut T {
        if id.index.get() == u32::MAX {
            panic!("Internal compiler error");
        }
        let index = (id.index.get() - 1) as usize;
        self.arr.get_mut(index).expect("Internal Compiler Error")
    }
    pub fn maybe_get(&mut self, id: Id<T>) -> Option<&T> {
        if id.index.get() == u32::MAX {
            return None;
        }
        let index = (id.index.get() - 1) as usize;
        self.arr.get(index)
    }
    pub fn maybe_get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        if id.index.get() == u32::MAX {
            return None;
        }
        let index = (id.index.get() - 1) as usize;
        self.arr.get_mut(index)
    }
    pub fn fold<I, F: FnMut(I, Id<T>, &T) -> I>(&self, init: I, mut f: F) -> I {
        self.arr.iter().enumerate().fold(init, |acc, (i, item)| {
            f(acc, Id::new(NonZeroU32::new(i as u32 + 1).unwrap()), item)
        })
    }
}

macro_rules! define_id {
    ($typ:ident, $id:ident, $arena:ident; dedup) => {
        $crate::common::interner::define_arena!($typ, $id, $arena);
        impl $arena {
            pub fn intern_deduplicated(&mut self, item: $typ) -> $id {
                $id(self.0.intern_deduplicated(item))
            }
            #[track_caller]
            pub fn get_id_for(&self, item: $typ) -> Option<$id> {
                self.0.get_id_for(item).map($id)
            }
        }
    };
    ($typ:ident, $id:ident, $arena:ident) => {
        #[derive(Clone, Copy, Default)]
        pub struct $id($crate::common::interner::Id<$typ>);

        impl std::fmt::Debug for $id {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($id), self.get())
            }
        }

        impl std::hash::Hash for $id {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }
        impl Eq for $id {}
        impl PartialEq for $id {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }
        impl $id {
            pub fn get(self) -> u32 {
                self.0.get()
            }
            pub fn is_invalid(self) -> bool {
                self.0.is_invalid()
            }
        }
        #[derive(Debug, Clone)]
        pub struct $arena($crate::common::interner::Interner<$typ>);

        impl Default for $arena {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $arena {
            pub fn new() -> Self {
                Self($crate::common::interner::Interner::new())
            }
            #[track_caller]
            pub fn get(&self, id: $id) -> &$typ {
                self.0.get(id.0)
            }
            #[track_caller]
            pub fn get_mut(&mut self, id: $id) -> &mut $typ {
                self.0.get_mut(id.0)
            }
            pub fn len(&self) -> usize {
                self.0.len()
            }
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
            pub fn intern(&mut self, item: $typ) -> $id {
                $id(self.0.intern(item))
            }
            pub fn maybe_get(&mut self, id: $id) -> Option<&$typ> {
                self.0.maybe_get(id.0)
            }
            pub fn maybe_get_mut(&mut self, id: $id) -> Option<&mut $typ> {
                self.0.maybe_get_mut(id.0)
            }
            pub fn intern_mut(&mut self, item: $typ) -> ($id, &mut $typ) {
                let (id, item) = self.0.intern_mut(item);
                ($id(id), item)
            }
            pub fn fold<I, F: FnMut(I, $id, &$typ) -> I>(&self, init: I, mut f: F) -> I {
                self.0.fold(init, |acc, id, typ| f(acc, $id(id), typ))
            }
        }
    };
}

pub(crate) use define_id as define_arena;
