use ahash::AHashMap;
use tinyvec::{TinyVec, tiny_vec};

// Scoped hashmap implementation from cranelift
// https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/scoped_hash_map.rs
#[derive(Clone, Debug)]
pub struct ScopedHashMap<K, V> {
    generation_by_depth: TinyVec<[u32; 5]>,
    generation: u32,
    map: AHashMap<K, Val<V>>,
    shadowed: Vec<(K, Val<V>, u32)>,
}

#[derive(Clone, Debug, Copy)]
pub struct Val<V> {
    val: V,
    level: u32,
    generation: u32,
}

impl<K, V> Default for ScopedHashMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> ScopedHashMap<K, V> {
    pub fn new() -> Self {
        let generation_by_depth = tiny_vec![0];
        Self {
            generation_by_depth,
            generation: 0,
            map: AHashMap::new(),
            shadowed: Vec::new(),
        }
    }
}
impl<K: std::hash::Hash + Eq + Copy, V: Copy> ScopedHashMap<K, V> {
    pub fn enter_scope(&mut self) {
        self.generation_by_depth.push(self.generation);
    }
    pub fn exit_scope(&mut self) {
        self.generation += 1;
        let level = self.generation_by_depth.len() as u32;
        self.generation_by_depth.pop();
        while let Some((sym, val, lvl)) = self.shadowed.last()
            && *lvl == level
        {
            self.map.insert(*sym, *val);
            self.shadowed.pop();
        }
    }
    pub fn insert(&mut self, key: K, val: V) {
        let level = (self.generation_by_depth.len() - 1) as u32;
        let val = Val {
            val,
            level,
            generation: self.generation,
        };
        if let Some(prev) = self.map.insert(key, val) {
            self.shadowed
                .push((key, prev, self.generation_by_depth.len() as u32));
        }
    }
    pub fn get(&self, key: K) -> Option<V> {
        let val = self.map.get(&key)?;
        if self.generation_by_depth.get(val.level as usize) == Some(&val.generation) {
            Some(val.val)
        } else {
            None
        }
    }
    pub fn get_current_scope(&self, key: K) -> Option<V> {
        let val = self.map.get(&key)?;
        let lvl = (self.generation_by_depth.len() - 1) as u32;
        if self.generation_by_depth.get(val.level as usize) == Some(&val.generation)
            && val.level == lvl
        {
            Some(val.val)
        } else {
            None
        }
    }
    pub fn gc(&mut self) {
        self.map
            .retain(|_k, v| self.generation_by_depth.get(v.level as usize) == Some(&v.generation))
    }
    pub fn clear(&mut self) {
        self.generation_by_depth.clear();
        self.generation_by_depth.push(0);
        self.generation = 0;
        self.map.clear();
        self.shadowed.clear();
    }
}

#[cfg(test)]
mod test {
    use crate::syntax::{ast::Type, context::Context};

    use super::*;

    #[test]
    pub fn test_scoped_map() {
        let mut ctx = Context::new();
        let s1 = ctx.intern_symbol("1");
        let s2 = ctx.intern_symbol("2");
        let s3 = ctx.intern_symbol("3");
        let s4 = ctx.intern_symbol("4");
        let t1 = ctx.intern_type(Type::Void);
        let t2 = ctx.intern_type(Type::Int);
        let t3 = ctx.intern_type(Type::Ptr {
            pointee: t1,
            noalias: true,
        });
        let t4 = ctx.intern_type(Type::Ptr {
            pointee: t2,
            noalias: false,
        });
        let mut map = ScopedHashMap::new();
        map.insert(s1, t1);
        map.insert(s2, t2);
        map.insert(s3, t3);
        assert_eq!(map.get(s1), Some(t1));
        assert_eq!(map.get(s2), Some(t2));
        assert_eq!(map.get(s3), Some(t3));
        map.enter_scope();
        assert_eq!(map.get(s1), Some(t1));
        assert_eq!(map.get(s2), Some(t2));
        assert_eq!(map.get(s3), Some(t3));
        map.insert(s4, t1);
        assert_eq!(map.get(s4), Some(t1));
        map.insert(s1, t4);
        assert_eq!(map.get(s1), Some(t4));
        map.exit_scope();
        assert_eq!(map.get(s4), None);
        assert_eq!(map.get(s1), Some(t1));
        map.clear();

        map.insert(s1, t1);
        assert_eq!(map.get(s1), Some(t1));

        map.enter_scope();
        assert_eq!(map.get(s1), Some(t1));
        map.insert(s1, t2);
        assert_eq!(map.get(s1), Some(t2));

        map.enter_scope();
        assert_eq!(map.get(s1), Some(t2));
        map.insert(s1, t3);
        assert_eq!(map.get(s1), Some(t3));

        map.enter_scope();
        assert_eq!(map.get(s1), Some(t3));
        map.insert(s1, t4);
        assert_eq!(map.get(s1), Some(t4));

        map.exit_scope();
        assert_eq!(map.get(s1), Some(t3));
        map.exit_scope();
        assert_eq!(map.get(s1), Some(t2));
        map.exit_scope();
        assert_eq!(map.get(s1), Some(t1));

        map.clear();

        assert!(map.get_current_scope(s1).is_none());
        map.insert(s1, t1);
        assert_eq!(map.get_current_scope(s1), Some(t1));
        map.enter_scope();
        assert!(map.get_current_scope(s1).is_none());
        map.insert(s1, t2);
        assert_eq!(map.get_current_scope(s1), Some(t2));
        map.exit_scope();
        assert_eq!(map.get_current_scope(s1), Some(t1));
    }
}
