use std::hash::Hash;

use smol_str::SmolStr;

use crate::common::symbol::*;
use crate::syntax::ast::{Expr, Stmt, Type};

pub mod arenas {
    use super::*;
    use crate::common::interner::define_arena;
    define_arena!(Type, TypeId, TypeArena; dedup);
    define_arena!(Expr, ExprId, ExprArena);
    define_arena!(Stmt, StmtId, StmtArena);
}
pub use arenas::*;

#[derive(Debug)]
pub struct Context {
    symbol_interner: SymbolArena,
    type_interner: TypeArena,
    expr_interner: ExprArena,
    stmt_interner: StmtArena,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            symbol_interner: SymbolArena::new(),
            type_interner: TypeArena::new(),
            expr_interner: ExprArena::new(),
            stmt_interner: StmtArena::new(),
        }
    }
    pub fn intern_symbol(&mut self, s: &str) -> Symbol {
        self.symbol_interner.intern_deduplicated(SmolStr::new(s))
    }
    pub fn get_symbol(&self, s: Symbol) -> &str {
        self.symbol_interner.get(s)
    }
    pub fn intern_type(&mut self, typ: Type) -> TypeId {
        self.type_interner.intern_deduplicated(typ)
    }
    pub fn get_type(&self, id: TypeId) -> &Type {
        self.type_interner.get(id)
    }
    pub fn intern_expr(&mut self, expr: Expr) -> ExprId {
        self.expr_interner.intern(expr)
    }
    pub fn get_expr(&self, id: ExprId) -> &Expr {
        self.expr_interner.get(id)
    }
    pub fn intern_stmt(&mut self, stmt: Stmt) -> StmtId {
        self.stmt_interner.intern(stmt)
    }
    pub fn get_stmt(&self, id: StmtId) -> &Stmt {
        self.stmt_interner.get(id)
    }
}
