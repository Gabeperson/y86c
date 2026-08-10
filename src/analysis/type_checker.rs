use crate::{
    analysis::{scoped_hashmap::ScopedHashMap, symbol_table::SymbolTable},
    common::span::Span,
    syntax::{
        ast::*,
        context::{Context, ExprId, StmtId, Symbol, TypeId},
    },
};

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum ConditionType {
    For,
    While,
    If,
}

#[derive(Clone, Debug)]
pub enum TypeCheckError {
    ConditionNotIntLike {
        typ: TypeId,
        span: Span,
        cond_type: ConditionType,
    },
    InvalidReturnType {
        expected: TypeId,
        found: TypeId,
        span: Span,
    },
    RedeclaredVariable {
        def: Span,
        prev_def: Span,
        symbol: Symbol,
    },
    InvalidAssignmentTypes {
        expected: TypeId,
        found: TypeId,
        span: Span,
    },
}

pub struct TypeChecker<'a> {
    map: ScopedHashMap<Symbol, (TypeId, Span)>,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    errors: Vec<TypeCheckError>,
}

impl<'a> TypeChecker<'a> {
    pub fn check(&mut self, program: &Program) {
        for decl in &program.decls {
            if let GlobalDeclarationKind::Function(func) = &decl.kind {
                let ret_type = match &func.return_type {
                    Some(typ) => typ.inner,
                    None => self.ctx.intern_type(Type::Void),
                };
                self.check_block(&func.body, ret_type, true)
            };
        }
    }
    pub fn check_block(&mut self, block: &Block, func_ret: TypeId, new_scope: bool) {
        if new_scope {
            self.map.enter_scope();
        }
        for stmt in block.body.iter().copied() {
            self.check_stmt(stmt, func_ret)
        }
        if new_scope {
            self.map.exit_scope();
        }
    }
    pub fn check_stmt(&mut self, stmt: StmtId, func_ret: TypeId) {
        let stmt = self.ctx.get_stmt(stmt);
        match &stmt.kind {
            StmtKind::Assert(assert) => todo!(),
            StmtKind::Continue(_) | StmtKind::Break(_) => {
                // No checks needec
            }
            StmtKind::Block(block) => {
                let stmts: Vec<_> = block.body.iter().copied().collect();
                for stmt in stmts {
                    self.check_stmt(stmt, func_ret);
                }
            }
            StmtKind::IfStmt(if_stmt) => {
                let condition = if_stmt.condition;
                let then_branch = if_stmt.then_branch;
                let else_branch = if_stmt.else_branch;
                if let Some(id) = self.check_expr_id(condition)
                    && let typ = self.ctx.get_type(id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: id,
                        span: expr.span,
                        cond_type: ConditionType::If,
                    })
                }
                self.check_stmt(then_branch, func_ret);
                if let Some(id) = else_branch {
                    self.check_stmt(id, func_ret)
                }
            }
            StmtKind::WhileLoop(while_loop) => {
                let condition = while_loop.condition;
                let body = while_loop.body;

                if let Some(id) = self.check_expr_id(condition)
                    && let typ = self.ctx.get_type(id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: id,
                        span: expr.span,
                        cond_type: ConditionType::While,
                    })
                }
                self.check_stmt(body, func_ret);
            }
            StmtKind::ForLoop(for_loop) => {
                // For loop needs to be specialized because of the init
                self.map.enter_scope();
                let init = for_loop.init;
                let condition = for_loop.condition;
                let post = for_loop.post;
                let body = for_loop.body;
                if let Some(init) = init {
                    self.check_stmt(init, func_ret);
                }
                if let Some(condition) = condition
                    && let Some(id) = self.check_expr_id(condition)
                    && let typ = self.ctx.get_type(id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: id,
                        span: expr.span,
                        cond_type: ConditionType::For,
                    })
                }
                if let Some(post) = post {
                    self.check_expr_id(post);
                }
                if let StmtKind::Block(block) = &self.ctx.get_stmt(body).kind {
                    let block = block.clone();
                    self.check_block(&block, func_ret, false);
                }
                self.map.exit_scope();
            }
            StmtKind::ReturnStmt(return_stmt) => {
                let value = return_stmt.value;
                let span = return_stmt.span;
                let ret_type = if let Some(expr) = value {
                    if let Some(typ) = self.check_expr_id(expr) {
                        Some(typ)
                    } else {
                        return;
                    }
                } else {
                    None
                };
                if let Some(ret_type) = ret_type
                    && ret_type != func_ret
                {
                    self.errors.push(TypeCheckError::InvalidReturnType {
                        expected: func_ret,
                        found: ret_type,
                        span,
                    })
                }
            }
            StmtKind::VariableDeclaration(variable_declaration) => {
                let name = variable_declaration.name;
                let var_type = variable_declaration.var_type;
                let init_value = variable_declaration.init_value;
                let span = variable_declaration.span;
                if let Some((_typ, span)) = self.map.get_current_scope(name.sym) {
                    self.errors.push(TypeCheckError::RedeclaredVariable {
                        def: name.span,
                        prev_def: span,
                        symbol: name.sym,
                    })
                }
                if let Some(expr_id) = init_value {
                    let Some(typ_id) = self.check_expr_id(expr_id) else {
                        return;
                    };
                    let init_type = self.ctx.get_type(typ_id);
                    let var_declared_type = self.ctx.get_type(var_type.inner);
                    if !init_type.is_assignable_to(var_declared_type, self.ctx) {
                        let expr = self.ctx.get_expr(expr_id);
                        self.errors.push(TypeCheckError::InvalidAssignmentTypes {
                            expected: var_type.inner,
                            found: typ_id,
                            span: expr.span,
                        })
                    }
                }
                self.map.insert(name.sym, (var_type.inner, span));
            }
            StmtKind::Expr(expr) => todo!(),
        }
    }
    pub fn check_expr_id(&mut self, expr: ExprId) -> Option<TypeId> {
        todo!()
    }
}

impl Type {
    pub fn is_assignable_to(&self, other: &Self, ctx: &Context) -> bool {
        if self == other {
            return true;
        }
        match (self, other) {
            (
                Type::Ptr {
                    pointee: p1,
                    noalias: n1,
                },
                Type::Ptr {
                    pointee: p2,
                    noalias: n2,
                },
            ) => {
                // A 'noalias' ptr is a ptr with an extra assumption so it's valid to cast it to a regular pointer.
                // If the second ptr is a 'regular' ptr, then it is trivially a supertype,
                // and if it isn't, then first type must also be a 'noalias' ptr to be a subtype of 2nd
                let alias_ok = !n2 || *n1;
                let t1 = ctx.get_type(*p1);
                let t2 = ctx.get_type(*p2);
                let void_conversion = t1 == &Type::Void || t2 == &Type::Void;
                alias_ok && (void_conversion || t1 == t2)
            }

            (
                Type::FuncPtr {
                    return_type: r1,
                    param_types: p1,
                },
                Type::FuncPtr {
                    return_type: r2,
                    param_types: p2,
                },
            ) => {
                if p1.len() != p2.len() {
                    return false;
                }
                let r1 = ctx.get_type(*r1);
                let r2 = ctx.get_type(*r2);
                let ret_type_matches = r1.is_assignable_to(r2, ctx);
                if !ret_type_matches {
                    return false;
                }
                p1.iter().zip(p2.iter()).all(|(id1, id2)| {
                    let t1 = ctx.get_type(*id1);
                    let t2 = ctx.get_type(*id2);
                    // Parameter types are contravariant, not covariant
                    t2.is_assignable_to(t1, ctx)
                })
            }
            _ => false,
        }
    }
}
