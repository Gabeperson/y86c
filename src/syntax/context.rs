use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use ahash::AHashMap;
use smol_str::SmolStr;

use crate::syntax::ast::{Expr, Stmt, Type};

#[derive(Debug)]
pub struct Context {
    symbol_interner: Interner<SmolStr>,
    type_interner: Interner<Type>,
    expr_interner: Interner<Expr>,
    stmt_interner: Interner<Stmt>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            symbol_interner: Interner::new(),
            type_interner: Interner::new(),
            expr_interner: Interner::new(),
            stmt_interner: Interner::new(),
        }
    }
    pub fn intern_symbol(&mut self, s: &str) -> Symbol {
        let id = self.symbol_interner.intern_deduplicated(SmolStr::new(s));
        Symbol(id)
    }
    pub fn get_symbol(&self, s: Symbol) -> &str {
        self.symbol_interner.get(s.0)
    }
    pub fn intern_type(&mut self, typ: Type) -> TypeId {
        let id = self.type_interner.intern_deduplicated(typ);
        TypeId(id)
    }
    pub fn get_type(&self, id: TypeId) -> &Type {
        self.type_interner.get(id.0)
    }
    pub fn intern_expr(&mut self, expr: Expr) -> ExprId {
        let id = self.expr_interner.intern(expr);
        ExprId(id)
    }
    pub fn get_expr(&self, id: ExprId) -> &Expr {
        self.expr_interner.get(id.0)
    }
    pub fn intern_stmt(&mut self, stmt: Stmt) -> StmtId {
        let id = self.stmt_interner.intern(stmt);
        StmtId(id)
    }
    pub fn get_stmt(&self, id: StmtId) -> &Stmt {
        self.stmt_interner.get(id.0)
    }
}

#[derive(Debug, Eq)]
pub struct Id<T> {
    index: NonZeroU32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Hash for Id<T> {
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
impl<T: Clone + Eq + Hash> Interner<T> {
    pub fn intern_deduplicated(&mut self, item: T) -> Id<T> {
        if let Some(id) = self.map.get(&item) {
            return *id;
        }
        let id = self.intern(item.clone());
        self.map.insert(item, id);
        id
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
        Id {
            index: nonzero,
            _marker: PhantomData,
        }
    }
    pub fn get(&self, id: Id<T>) -> &T {
        if id.index.get() == u32::MAX {
            panic!("Internal compiler error");
        }
        let index = (id.index.get() - 1) as usize;
        self.arr.get(index).expect("Internal Compiler Error")
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
pub struct Symbol(Id<SmolStr>);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
pub struct TypeId(Id<Type>);
#[derive(Debug, Clone, Copy, Default)]
pub struct ExprId(Id<Expr>);

impl Hash for ExprId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl Eq for ExprId {}
impl PartialEq for ExprId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct StmtId(Id<Stmt>);

impl Hash for StmtId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl PartialEq for StmtId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for StmtId {}
