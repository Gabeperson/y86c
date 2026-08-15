use ahash::{AHashMap, AHashSet};

use crate::{
    analysis::{scoped_hashmap::ScopedHashMap, symbol_table::SymbolTable},
    common::span::Span,
    common::symbol::Symbol,
    syntax::{
        ast::*,
        context::{Context, ExprId, StmtId, TypeId},
    },
};

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum ConditionType {
    For,
    While,
    If,
    Ternary,
    Assert,
}

#[derive(Clone, Debug, Copy)]
pub enum MemberAccessType {
    Pointer,
    Struct,
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
    InvalidCast {
        from: TypeId,
        to: TypeId,
        span: Span,
    },
    NoVariableInScope {
        sym: Symbol,
        span: Span,
    },
    TernaryBranchesDifferentTypes {
        true_branch_typ: TypeId,
        true_branch_span: Span,
        false_branch_typ: TypeId,
        false_branch_span: Span,
    },
    IndexedNonIndexable {
        typ: TypeId,
        span: Span,
    },
    ArrayIndexWithNonInt {
        typ: TypeId,
        span: Span,
    },
    StructDoesntExist {
        sym: Symbol,
        span: Span,
    },
    StructFieldDoesntExist {
        struct_sym: Symbol,
        sym: Symbol,
        span: Span,
    },
    InvalidStructInitType {
        sym: Symbol,
        expected_typ: TypeId,
        found_type: TypeId,
        span: Span,
    },
    DifferingArrayInitTypes {
        first_elem: TypeId,
        first_elem_span: Span,
        differing_elem: TypeId,
        differing_elem_span: Span,
    },
    EmptyArrayInit {
        span: Span,
    },
    UnknownMemberAccess {
        struct_typ: TypeId,
        member_sym: Symbol,
        span: Span,
        access_type: MemberAccessType,
    },
    // This is technically the same as InvalidMemberAccess, but
    // happens if you used "wrong type" of member access, ex
    // .member on a ptr or ->member on a struct, and it's for
    // better error messages
    WrongMemberAccessType {
        typ: TypeId,
        member_sym: Symbol,
        used_access_type: MemberAccessType,
        span: Span,
    },
    InvalidMemberAccess {
        typ: TypeId,
        member_sym: Symbol,
        used_access_type: MemberAccessType,
        span: Span,
    },
    FunctionCallOnNonFnPtr {
        typ: TypeId,
        span: Span,
    },
    FunctionCallArgumentLengths {
        ptr_len: usize,
        call_len: usize,
        span: Span,
    },
    FunctionCallArgumentTypeWrong {
        index: usize,
        param_type: TypeId,
        arg_type: TypeId,
        arg_span: Span,
    },
    InvalidBinopTypes {
        lhs_id: TypeId,
        rhs_id: TypeId,
        span: Span,
        op: BinaryOpKind,
    },
    LhsNotAssignable {
        span: Span,
        op: BinaryOpKind,
    },
    PtrSubDifferentTypes {
        lhs_id: TypeId,
        rhs_id: TypeId,
        span: Span,
    },
    TypesNotAssignable {
        lhs_id: TypeId,
        rhs_id: TypeId,
        span: Span,
    },
    InvalidPrefixOpType {
        expr_id: TypeId,
        span: Span,
        op: PrefixOpKind,
    },
    PrefixExprNotAssignable {
        span: Span,
        op: PrefixOpKind,
    },
    InvalidPostfixOpType {
        expr_id: TypeId,
        span: Span,
        op: PostfixOpKind,
    },
    PostfixExprNotAssignable {
        span: Span,
        op: PostfixOpKind,
    },
    InvalidAddressOf {
        expr_type_id: TypeId,
        span: Span,
    },
    InvalidDereference {
        expr_type_id: TypeId,
        span: Span,
    },
    InvalidCopyProvPtr {
        typ: TypeId,
        span: Span,
    },
    InvalidCopyProvAddr {
        typ: TypeId,
        span: Span,
    },
    InvalidNewProvPtr {
        typ: TypeId,
        span: Span,
    },
    InvalidExposeProvPtr {
        typ: TypeId,
        span: Span,
    },
    InvalidUnexposeProvAddr {
        typ: TypeId,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy)]
struct VarInfo {
    id: TypeId,
    span: Span,
    assignable: bool,
}

impl VarInfo {
    fn new(id: TypeId, span: Span, assignable: bool) -> Self {
        Self {
            id,
            span,
            assignable,
        }
    }
}

pub struct TypeChecker<'a> {
    map: ScopedHashMap<Symbol, VarInfo>,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: AHashMap<NodeId, ExprTypeInfo>,
    errors: Vec<TypeCheckError>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExprTypeInfo {
    pub id: TypeId,
    pub assignable: bool,
}

impl ExprTypeInfo {
    fn new(id: TypeId, assignable: bool) -> Self {
        Self { id, assignable }
    }
}

pub struct TypeCheckOutput {
    pub errors: Vec<TypeCheckError>,
    pub type_table: AHashMap<NodeId, ExprTypeInfo>,
}

impl<'a> TypeChecker<'a> {
    pub fn check(
        ctx: &'a mut Context,
        symbol_table: &'a SymbolTable,
        program: &Program,
    ) -> TypeCheckOutput {
        let mut map = ScopedHashMap::new();
        for (sym, entry) in symbol_table.vars.iter() {
            map.insert(
                *sym,
                VarInfo {
                    id: entry.typ,
                    span: entry.span,
                    assignable: !entry.is_function,
                },
            )
        }
        let mut checker = TypeChecker {
            map,
            symbol_table,
            ctx,
            type_table: AHashMap::new(),
            errors: Vec::new(),
        };
        checker.check_inner(program);
        TypeCheckOutput {
            errors: checker.errors,
            type_table: checker.type_table,
        }
    }
    fn check_inner(&mut self, program: &Program) {
        for decl in &program.decls {
            if let GlobalDeclarationKind::Function(func) = &decl.kind {
                let ret_type = match &func.return_type {
                    Some(typ) => typ.inner,
                    None => self.ctx.intern_type(Type::Void),
                };
                self.map.enter_scope();
                for (name, typ) in func.params.iter().copied() {
                    self.map
                        .insert(name.sym, VarInfo::new(typ.inner, typ.span, true));
                }
                self.check_block(&func.body, ret_type, false);
                self.map.exit_scope();
            };
        }
    }
    fn check_block(&mut self, block: &Block, func_ret: TypeId, new_scope: bool) {
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
    fn check_stmt(&mut self, stmt: StmtId, func_ret: TypeId) {
        let stmt = self.ctx.get_stmt(stmt);
        match &stmt.kind {
            StmtKind::Assert(assert) => {
                let condition = assert.condition;
                if let Some(info) = self.check_expr(condition) {
                    let typ = self.ctx.get_type(info.id);
                    if !typ.is_intlike() {
                        let expr = self.ctx.get_expr(condition);
                        self.errors.push(TypeCheckError::ConditionNotIntLike {
                            typ: info.id,
                            span: expr.span,
                            cond_type: ConditionType::Assert,
                        });
                    }
                }
            }
            StmtKind::Continue(_) | StmtKind::Break(_) => {
                // No checks needec
            }
            StmtKind::Block(block) => self.check_block(&block.clone(), func_ret, true),
            StmtKind::IfStmt(if_stmt) => {
                let condition = if_stmt.condition;
                let then_branch = if_stmt.then_branch;
                let else_branch = if_stmt.else_branch;
                if let Some(info) = self.check_expr(condition)
                    && let typ = self.ctx.get_type(info.id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: info.id,
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

                if let Some(info) = self.check_expr(condition)
                    && let typ = self.ctx.get_type(info.id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: info.id,
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
                    && let Some(info) = self.check_expr(condition)
                    && let typ = self.ctx.get_type(info.id)
                    && !typ.is_intlike()
                {
                    let expr = self.ctx.get_expr(condition);
                    self.errors.push(TypeCheckError::ConditionNotIntLike {
                        typ: info.id,
                        span: expr.span,
                        cond_type: ConditionType::For,
                    })
                }
                if let Some(post) = post {
                    self.check_expr(post);
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
                let ret_type_info = if let Some(expr) = value {
                    if let Some(typ) = self.check_expr(expr) {
                        typ
                    } else {
                        return;
                    }
                } else {
                    ExprTypeInfo::new(self.ctx.intern_type(Type::Void), false)
                };
                let ret_type = self.ctx.get_type(ret_type_info.id);
                let func_ret_type = self.ctx.get_type(func_ret);
                if !ret_type.is_assignable_to(func_ret_type, self.ctx) {
                    self.errors.push(TypeCheckError::InvalidReturnType {
                        expected: func_ret,
                        found: ret_type_info.id,
                        span,
                    })
                }
            }
            StmtKind::VariableDeclaration(variable_declaration) => {
                let name = variable_declaration.name;
                let var_type = variable_declaration.var_type;
                let init_value = variable_declaration.init_value;
                let span = variable_declaration.span;
                if self.check_type(var_type.inner, var_type.span).is_none() {
                    let void = self.ctx.intern_type(Type::Void);
                    self.map.insert(name.sym, VarInfo::new(void, span, true));
                    return;
                }
                if let Some(var_info) = self.map.get_current_scope(name.sym) {
                    self.errors.push(TypeCheckError::RedeclaredVariable {
                        def: name.span,
                        prev_def: var_info.span,
                        symbol: name.sym,
                    })
                }
                if let Some(expr_id) = init_value {
                    let Some(typ_info) = self.check_expr(expr_id) else {
                        return;
                    };
                    let init_type = self.ctx.get_type(typ_info.id);
                    let var_declared_type = self.ctx.get_type(var_type.inner);
                    if !init_type.is_assignable_to(var_declared_type, self.ctx) {
                        let expr = self.ctx.get_expr(expr_id);
                        self.errors.push(TypeCheckError::InvalidAssignmentTypes {
                            expected: var_type.inner,
                            found: typ_info.id,
                            span: expr.span,
                        })
                    }
                }
                self.map
                    .insert(name.sym, VarInfo::new(var_type.inner, span, true));
            }
            StmtKind::Expr(expr) => {
                self.check_expr(*expr);
            }
        }
    }
    fn check_cast(&mut self, cast: Cast) -> Option<ExprTypeInfo> {
        let info = self.check_expr(cast.expr)?;
        self.check_type(cast.to_type.inner, cast.to_type.span)?;
        let expr_type = self.ctx.get_type(info.id);
        let to_type = self.ctx.get_type(cast.to_type.inner);
        if !expr_type.is_castable_to(to_type) {
            self.errors.push(TypeCheckError::InvalidCast {
                from: info.id,
                to: cast.to_type.inner,
                span: cast.span,
            });
            return None;
        }
        let expr_type_info = ExprTypeInfo::new(cast.to_type.inner, false);
        self.type_table.insert(cast.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_ident(&mut self, ident: Ident) -> Option<ExprTypeInfo> {
        if let Some(var_info) = self.map.get(ident.sym) {
            if *self.ctx.get_type(var_info.id) == Type::Void {
                return None;
            }
            let expr_type_info = ExprTypeInfo::new(var_info.id, var_info.assignable);
            self.type_table.insert(ident.id, expr_type_info);
            return Some(expr_type_info);
        }
        self.errors.push(TypeCheckError::NoVariableInScope {
            sym: ident.sym,
            span: ident.span,
        });
        None
    }
    fn check_ternary(&mut self, ternary: Ternary) -> Option<ExprTypeInfo> {
        if let Some(info) = self.check_expr(ternary.condition) {
            let typ = self.ctx.get_type(info.id);
            if !typ.is_intlike() {
                let expr = self.ctx.get_expr(ternary.condition);
                self.errors.push(TypeCheckError::ConditionNotIntLike {
                    typ: info.id,
                    span: expr.span,
                    cond_type: ConditionType::Ternary,
                });
            }
        }
        let true_branch = self.check_expr(ternary.true_branch);
        let false_branch = self.check_expr(ternary.false_branch);
        let (Some(true_branch), Some(false_branch)) = (true_branch, false_branch) else {
            return None;
        };
        if true_branch.id != false_branch.id {
            let span1 = self.ctx.get_expr(ternary.true_branch).span;
            let span2 = self.ctx.get_expr(ternary.false_branch).span;
            self.errors
                .push(TypeCheckError::TernaryBranchesDifferentTypes {
                    true_branch_typ: true_branch.id,
                    true_branch_span: span1,
                    false_branch_typ: false_branch.id,
                    false_branch_span: span2,
                });
            None
        } else {
            let expr_type_info = ExprTypeInfo::new(true_branch.id, false);
            self.type_table.insert(ternary.id, expr_type_info);
            Some(expr_type_info)
        }
    }
    fn check_function_call(&mut self, function_call: FunctionCall) -> Option<ExprTypeInfo> {
        let fnptr_info = self.check_expr(function_call.func_expr)?;
        let typ = self.ctx.get_type(fnptr_info.id);
        let Type::FuncPtr {
            return_type,
            param_types,
        } = typ
        else {
            self.errors.push(TypeCheckError::FunctionCallOnNonFnPtr {
                typ: fnptr_info.id,
                span: function_call.span,
            });
            return None;
        };
        let return_type = *return_type;
        if function_call.args.len() != param_types.len() {
            self.errors
                .push(TypeCheckError::FunctionCallArgumentLengths {
                    ptr_len: param_types.len(),
                    call_len: function_call.args.len(),
                    span: function_call.span,
                })
        }
        for (index, (param_id, arg)) in param_types
            .clone()
            .into_iter()
            .zip(function_call.args.iter())
            .enumerate()
        {
            let Some(id) = self.check_expr(*arg) else {
                continue;
            };
            let arg_typ = self.ctx.get_type(id.id);
            let param_typ = self.ctx.get_type(param_id);
            if !arg_typ.is_assignable_to(param_typ, self.ctx) {
                let expr = self.ctx.get_expr(*arg);
                self.errors
                    .push(TypeCheckError::FunctionCallArgumentTypeWrong {
                        index,
                        param_type: param_id,
                        arg_type: id.id,
                        arg_span: expr.span,
                    });
            }
        }

        let expr_type_info = ExprTypeInfo::new(return_type, false);
        self.type_table.insert(function_call.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_array_index(&mut self, array_index: ArrayIndex) -> Option<ExprTypeInfo> {
        let arr = self.check_expr(array_index.array);
        let index = self.check_expr(array_index.index);
        let elem_type = if let Some(arr) = &arr {
            let typ = self.ctx.get_type(arr.id);
            if let Some(indexed) = typ.indexed_type()
                && let typ = self.ctx.get_type(indexed)
                && !typ.is_void()
            {
                Some(indexed)
            } else {
                let span = self.ctx.get_expr(array_index.array).span;
                self.errors
                    .push(TypeCheckError::IndexedNonIndexable { typ: arr.id, span });
                None
            }
        } else {
            None
        };
        if let Some(index) = index {
            let typ = self.ctx.get_type(index.id);
            if !typ.is_int() {
                let span = self.ctx.get_expr(array_index.index).span;
                self.errors.push(TypeCheckError::ArrayIndexWithNonInt {
                    typ: index.id,
                    span,
                });
            }
        }
        let elem_type = elem_type?;
        let expr_type_info = ExprTypeInfo::new(elem_type, true);
        self.type_table.insert(array_index.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_struct_init(&mut self, struct_init: StructInit) -> Option<ExprTypeInfo> {
        let sym = struct_init.name.sym;
        let Some(info) = self.symbol_table.structs.get(&sym) else {
            self.errors.push(TypeCheckError::StructDoesntExist {
                sym,
                span: struct_init.name.span,
            });
            return None;
        };
        let mut inits = AHashSet::new();
        for (name, expr_id) in struct_init.field_inits {
            if !inits.insert(name.sym) {
                // Duplicate struct field init, taken care of in ast validator
                continue;
            }
            let Some(field_info) = info.fields.get(&name.sym) else {
                self.errors.push(TypeCheckError::StructFieldDoesntExist {
                    struct_sym: sym,
                    sym: name.sym,
                    span: name.span,
                });
                continue;
            };
            let Some(expr_info) = self.check_expr(expr_id) else {
                continue;
            };
            let expr_typ = self.ctx.get_type(expr_info.id);
            let decl_typ = self.ctx.get_type(field_info.typ);
            if !expr_typ.is_assignable_to(decl_typ, self.ctx) {
                self.errors.push(TypeCheckError::InvalidStructInitType {
                    sym: name.sym,
                    expected_typ: field_info.typ,
                    found_type: expr_info.id,
                    span: name.span,
                });
            }
        }
        let id = self.ctx.intern_type(Type::Struct { name: sym });
        let expr_type_info = ExprTypeInfo::new(id, false);
        self.type_table.insert(struct_init.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_array_init(&mut self, array_init: ArrayInit) -> Option<ExprTypeInfo> {
        let Some(first) = array_init.elements.first() else {
            self.errors.push(TypeCheckError::EmptyArrayInit {
                span: array_init.span,
            });
            return None;
        };
        let first_typ = self.check_expr(*first)?;
        for init in array_init.elements.get(1..).unwrap_or(&[]) {
            let typ = self.check_expr(*init)?;
            if first_typ.id != typ.id {
                let first_span = self.ctx.get_expr(*first).span;
                let diff_span = self.ctx.get_expr(*init).span;
                self.errors.push(TypeCheckError::DifferingArrayInitTypes {
                    first_elem: first_typ.id,
                    first_elem_span: first_span,
                    differing_elem: typ.id,
                    differing_elem_span: diff_span,
                });
                return None;
            }
        }
        let typ = self.ctx.intern_type(Type::Array {
            element_type: first_typ.id,
            len: array_init.elements.len() as i64,
        });
        let expr_type_info = ExprTypeInfo::new(typ, false);
        self.type_table.insert(array_init.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_member_access(&mut self, member_access: MemberAccess) -> Option<ExprTypeInfo> {
        let struct_typ_info = self.check_expr(member_access.struct_expr)?;
        let typ = self.ctx.get_type(struct_typ_info.id);
        let struct_name = match typ {
            Type::Struct { name } => *name,
            Type::Ptr { pointee, .. }
                if let typ = self.ctx.get_type(*pointee)
                    && typ.is_struct() =>
            {
                self.errors.push(TypeCheckError::WrongMemberAccessType {
                    typ: struct_typ_info.id,
                    member_sym: member_access.member_name.sym,
                    used_access_type: MemberAccessType::Struct,
                    span: member_access.span,
                });
                return None;
            }
            _ => {
                self.errors.push(TypeCheckError::InvalidMemberAccess {
                    typ: struct_typ_info.id,
                    member_sym: member_access.member_name.sym,
                    used_access_type: MemberAccessType::Struct,
                    span: member_access.span,
                });
                return None;
            }
        };
        let info = self.symbol_table.structs.get(&struct_name)?;
        let Some(field_info) = info.fields.get(&member_access.member_name.sym) else {
            self.errors.push(TypeCheckError::UnknownMemberAccess {
                struct_typ: struct_typ_info.id,
                member_sym: member_access.member_name.sym,
                span: member_access.member_name.span,
                access_type: MemberAccessType::Struct,
            });
            return None;
        };

        let expr_type_info = ExprTypeInfo::new(field_info.typ, struct_typ_info.assignable);
        self.type_table.insert(member_access.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_pointer_member_access(
        &mut self,
        pointer_member_access: PointerMemberAccess,
    ) -> Option<ExprTypeInfo> {
        let struct_ptr_typ_info = self.check_expr(pointer_member_access.struct_ptr_expr)?;
        let typ = self.ctx.get_type(struct_ptr_typ_info.id);
        let struct_name = match typ {
            Type::Struct { .. } => {
                self.errors.push(TypeCheckError::WrongMemberAccessType {
                    typ: struct_ptr_typ_info.id,
                    member_sym: pointer_member_access.member_name.sym,
                    used_access_type: MemberAccessType::Pointer,
                    span: pointer_member_access.span,
                });
                return None;
            }
            Type::Ptr { pointee, .. }
                if let typ = self.ctx.get_type(*pointee)
                    && let Type::Struct { name } = typ =>
            {
                *name
            }
            _ => {
                self.errors.push(TypeCheckError::InvalidMemberAccess {
                    typ: struct_ptr_typ_info.id,
                    member_sym: pointer_member_access.member_name.sym,
                    used_access_type: MemberAccessType::Pointer,
                    span: pointer_member_access.span,
                });
                return None;
            }
        };
        let info = self.symbol_table.structs.get(&struct_name)?;
        let Some(field_info) = info.fields.get(&pointer_member_access.member_name.sym) else {
            self.errors.push(TypeCheckError::UnknownMemberAccess {
                struct_typ: struct_ptr_typ_info.id,
                member_sym: pointer_member_access.member_name.sym,
                span: pointer_member_access.member_name.span,
                access_type: MemberAccessType::Pointer,
            });
            return None;
        };

        let expr_type_info = ExprTypeInfo::new(
            field_info.typ,
            // The existence of a pointer SHOULD mean that there is a backing
            // memory location that is actually writable.
            true,
        );
        self.type_table
            .insert(pointer_member_access.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_expr(&mut self, expr: ExprId) -> Option<ExprTypeInfo> {
        let expr = self.ctx.get_expr(expr);
        match &expr.kind {
            ExprKind::Nullptr(nullptr) => {
                let id = nullptr.id;
                let void = self.ctx.intern_type(Type::Void);
                let expr_type_info = ExprTypeInfo::new(
                    self.ctx.intern_type(Type::Ptr {
                        pointee: void,
                        noalias: false,
                    }),
                    false,
                );
                self.type_table.insert(id, expr_type_info);
                Some(expr_type_info)
            }
            ExprKind::Int(int) => {
                let id = int.id;
                let expr_type_info = ExprTypeInfo::new(self.ctx.intern_type(Type::Int), false);
                self.type_table.insert(id, expr_type_info);
                Some(expr_type_info)
            }
            ExprKind::Cast(cast) => self.check_cast(*cast),
            ExprKind::Ident(ident) => self.check_ident(*ident),
            ExprKind::BinaryOp(binary_op) => self.check_binary_op(*binary_op),
            ExprKind::PrefixOp(prefix_op) => self.check_prefix_op(*prefix_op),
            ExprKind::PostfixOp(postfix_op) => self.check_postfix_op(*postfix_op),
            ExprKind::Ternary(ternary) => self.check_ternary(*ternary),
            ExprKind::ArrayIndex(array_index) => self.check_array_index(*array_index),
            ExprKind::SizeOfType(size_of_type) => {
                let id = size_of_type.id;
                self.check_type(size_of_type.typ.inner, size_of_type.typ.span);
                let expr_type_info = ExprTypeInfo::new(self.ctx.intern_type(Type::Int), false);
                self.type_table.insert(id, expr_type_info);
                Some(expr_type_info)
            }
            // Ideally we don't need to clone these, but alas, we must, due to lifetimes
            ExprKind::FunctionCall(function_call) => {
                self.check_function_call(function_call.clone())
            }
            ExprKind::StructInit(struct_init) => self.check_struct_init(struct_init.clone()),
            ExprKind::ArrayInit(array_init) => self.check_array_init(array_init.clone()),
            ExprKind::MemberAccess(member_access) => self.check_member_access(*member_access),
            ExprKind::PointerMemberAccess(pointer_member_access) => {
                self.check_pointer_member_access(*pointer_member_access)
            }
            ExprKind::Error => None,
            ExprKind::CopyProvenance(copy_prov) => self.check_copy_prov(*copy_prov),
            ExprKind::ExposeProvenance(expose_prov) => self.check_expose_prov(*expose_prov),
            ExprKind::UnexposeProvenance(unexpose_prov) => self.check_unexpose_prov(*unexpose_prov),
            ExprKind::NewProvenance(new_prov) => self.check_new_prov(*new_prov),
        }
    }

    fn check_copy_prov(&mut self, copy_prov: CopyProvenance) -> Option<ExprTypeInfo> {
        let ptr = self.check_expr(copy_prov.prov_ptr);
        let addr = self.check_expr(copy_prov.addr);
        let (Some(ptr), Some(addr)) = (ptr, addr) else {
            return None;
        };
        let ptr_type = self.ctx.get_type(ptr.id);
        let addr_type = self.ctx.get_type(addr.id);
        if !ptr_type.is_ptr() {
            let expr = self.ctx.get_expr(copy_prov.prov_ptr);
            self.errors.push(TypeCheckError::InvalidCopyProvPtr {
                typ: ptr.id,
                span: expr.span,
            });
        }
        if !addr_type.is_int() {
            let expr = self.ctx.get_expr(copy_prov.addr);
            self.errors.push(TypeCheckError::InvalidCopyProvAddr {
                typ: addr.id,
                span: expr.span,
            });
        }
        let void = self.ctx.intern_type(Type::Void);
        let typ = self.ctx.intern_type(Type::Ptr {
            pointee: void,
            noalias: false,
        });
        Some(ExprTypeInfo::new(typ, false))
    }
    fn check_expose_prov(&mut self, expose_prov: ExposeProvenance) -> Option<ExprTypeInfo> {
        let ptr = self.check_expr(expose_prov.ptr)?;
        let ptr_type = self.ctx.get_type(ptr.id);
        if !ptr_type.is_ptr() {
            let expr = self.ctx.get_expr(expose_prov.ptr);
            self.errors.push(TypeCheckError::InvalidExposeProvPtr {
                typ: ptr.id,
                span: expr.span,
            });
        }
        let typ = self.ctx.intern_type(Type::Int);
        Some(ExprTypeInfo::new(typ, false))
    }
    fn check_unexpose_prov(&mut self, unexpose_prov: UnexposeProvenance) -> Option<ExprTypeInfo> {
        let int = self.check_expr(unexpose_prov.int)?;
        let int_type = self.ctx.get_type(int.id);
        if !int_type.is_int() {
            let expr = self.ctx.get_expr(unexpose_prov.int);
            self.errors.push(TypeCheckError::InvalidUnexposeProvAddr {
                typ: int.id,
                span: expr.span,
            });
        }
        let void = self.ctx.intern_type(Type::Void);
        let typ = self.ctx.intern_type(Type::Ptr {
            pointee: void,
            noalias: false,
        });
        Some(ExprTypeInfo::new(typ, false))
    }
    fn check_new_prov(&mut self, new_prov: NewProvenance) -> Option<ExprTypeInfo> {
        let ptr = self.check_expr(new_prov.ptr)?;
        let ptr_type = self.ctx.get_type(ptr.id);
        if !ptr_type.is_ptr() {
            let expr = self.ctx.get_expr(new_prov.ptr);
            self.errors.push(TypeCheckError::InvalidNewProvPtr {
                typ: ptr.id,
                span: expr.span,
            });
        }
        let void = self.ctx.intern_type(Type::Void);
        let typ = self.ctx.intern_type(Type::Ptr {
            pointee: void,
            noalias: true,
        });
        Some(ExprTypeInfo::new(typ, false))
    }

    fn check_type(&mut self, typ: TypeId, span: Span) -> Option<()> {
        let typ = self.ctx.get_type(typ);
        match typ {
            Type::Void | Type::Int => Some(()),
            Type::Ptr { pointee, .. } => self.check_type(*pointee, span),
            Type::Struct { name } => {
                if self.symbol_table.structs.contains_key(name) {
                    Some(())
                } else {
                    self.errors
                        .push(TypeCheckError::StructDoesntExist { sym: *name, span });
                    None
                }
            }
            Type::Array { element_type, .. } => self.check_type(*element_type, span),
            Type::FuncPtr {
                return_type,
                param_types,
            } => {
                let return_type = *return_type;
                let param_types = param_types.clone();
                let mut ok = self.check_type(return_type, span).is_some();
                for param in param_types {
                    if self.check_type(param, span).is_none() {
                        ok = false;
                    }
                }
                if ok { Some(()) } else { None }
            }
        }
    }
    fn check_binary_op(&mut self, binop: BinaryOp) -> Option<ExprTypeInfo> {
        let lhs = self.check_expr(binop.left);
        let rhs = self.check_expr(binop.right);
        let (Some(lhs_info), Some(rhs_info)) = (lhs, rhs) else {
            return None;
        };
        let lhs_id = lhs_info.id;
        let rhs_id = rhs_info.id;
        let lhs = self.ctx.get_type(lhs_id);
        let rhs = self.ctx.get_type(rhs_id);
        let span = binop.span;
        let op = binop.kind;
        let id = match binop.kind {
            BinaryOpKind::Add => match (lhs, rhs) {
                (Type::Int, Type::Int) => self.ctx.intern_type(Type::Int),
                (Type::Ptr { .. }, Type::Int) => lhs_id,
                (Type::Int, Type::Ptr { .. }) => rhs_id,
                _ => {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            },
            BinaryOpKind::Sub => match (lhs, rhs) {
                (Type::Int, Type::Int) => self.ctx.intern_type(Type::Int),
                (Type::Ptr { .. }, Type::Int) => lhs_id,
                (Type::Ptr { pointee: p1, .. }, Type::Ptr { pointee: p2, .. }) => {
                    if p1 != p2 {
                        self.errors.push(TypeCheckError::PtrSubDifferentTypes {
                            lhs_id,
                            rhs_id,
                            span,
                        });
                    }
                    self.ctx.intern_type(Type::Int)
                }
                _ => {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            },
            BinaryOpKind::Mul
            | BinaryOpKind::Div
            | BinaryOpKind::Mod
            | BinaryOpKind::BitAnd
            | BinaryOpKind::BitOr
            | BinaryOpKind::Xor
            | BinaryOpKind::MulAssign
            | BinaryOpKind::DivAssign
            | BinaryOpKind::BitAndAssign
            | BinaryOpKind::BitOrAssign
            | BinaryOpKind::XorAssign
            | BinaryOpKind::ModAssign => {
                if binop.kind.needs_assignable() && !lhs_info.assignable {
                    self.errors
                        .push(TypeCheckError::LhsNotAssignable { span, op });
                }
                if lhs.is_int() && rhs.is_int() {
                    self.ctx.intern_type(Type::Int)
                } else {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            }
            BinaryOpKind::Greater
            | BinaryOpKind::Less
            | BinaryOpKind::GreaterOrEqual
            | BinaryOpKind::LessOrEqual => match (lhs, rhs) {
                (Type::Int, Type::Int) | (Type::Ptr { .. }, Type::Ptr { .. }) => {
                    self.ctx.intern_type(Type::Int)
                }
                _ => {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            },
            BinaryOpKind::Eq | BinaryOpKind::NotEq => match (lhs, rhs) {
                (Type::FuncPtr { .. }, Type::Ptr { pointee, .. })
                    if let Type::Void = self.ctx.get_type(*pointee) =>
                {
                    self.ctx.intern_type(Type::Int)
                }
                (Type::Ptr { pointee, .. }, Type::FuncPtr { .. })
                    if let Type::Void = self.ctx.get_type(*pointee) =>
                {
                    self.ctx.intern_type(Type::Int)
                }
                (Type::FuncPtr { .. }, Type::FuncPtr { .. })
                | (Type::Int, Type::Int)
                | (Type::Ptr { .. }, Type::Ptr { .. }) => self.ctx.intern_type(Type::Int),
                _ => {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            },
            BinaryOpKind::And | BinaryOpKind::Or => {
                if lhs.is_intlike() && rhs.is_intlike() {
                    self.ctx.intern_type(Type::Int)
                } else {
                    self.errors.push(TypeCheckError::InvalidBinopTypes {
                        lhs_id,
                        rhs_id,
                        span,
                        op,
                    });
                    return None;
                }
            }
            BinaryOpKind::AddAssign | BinaryOpKind::SubAssign => {
                if !lhs_info.assignable {
                    self.errors
                        .push(TypeCheckError::LhsNotAssignable { span, op });
                }
                match (lhs, rhs) {
                    (Type::Int, Type::Int) => self.ctx.intern_type(Type::Int),
                    (Type::Ptr { .. }, Type::Int) => lhs_id,
                    _ => {
                        self.errors.push(TypeCheckError::InvalidBinopTypes {
                            lhs_id,
                            rhs_id,
                            span,
                            op,
                        });
                        return None;
                    }
                }
            }
            BinaryOpKind::Assign => {
                if !lhs_info.assignable {
                    self.errors
                        .push(TypeCheckError::LhsNotAssignable { span, op });
                }
                if !rhs.is_assignable_to(lhs, self.ctx) {
                    self.errors.push(TypeCheckError::TypesNotAssignable {
                        lhs_id,
                        rhs_id,
                        span,
                    });
                    return None;
                }
                lhs_id
            }
        };
        let expr_type_info = ExprTypeInfo::new(id, false);
        self.type_table.insert(binop.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_prefix_op(&mut self, op: PrefixOp) -> Option<ExprTypeInfo> {
        let expr_info = self.check_expr(op.expr)?;
        let expr_id = expr_info.id;
        let expr_typ = self.ctx.get_type(expr_id);
        let id = match op.kind {
            PrefixOpKind::Increment | PrefixOpKind::Decrement => {
                if !expr_info.assignable {
                    self.errors.push(TypeCheckError::PrefixExprNotAssignable {
                        span: op.span,
                        op: op.kind,
                    });
                    return None;
                }
                match expr_typ {
                    Type::Int => self.ctx.intern_type(Type::Int),
                    Type::Ptr { .. } => expr_id,
                    _ => {
                        self.errors.push(TypeCheckError::InvalidPrefixOpType {
                            expr_id,
                            span: op.span,
                            op: op.kind,
                        });
                        return None;
                    }
                }
            }
            PrefixOpKind::UnaryPlus | PrefixOpKind::UnaryMinus => {
                if let Type::Int = expr_typ {
                    self.ctx.intern_type(Type::Int)
                } else {
                    self.errors.push(TypeCheckError::InvalidPrefixOpType {
                        expr_id,
                        span: op.span,
                        op: op.kind,
                    });
                    return None;
                }
            }
            PrefixOpKind::AddressOf => {
                if !expr_info.assignable {
                    self.errors.push(TypeCheckError::InvalidAddressOf {
                        expr_type_id: expr_id,
                        span: op.span,
                    });
                    return None;
                }
                self.ctx.intern_type(Type::Ptr {
                    pointee: expr_id,
                    noalias: false,
                })
            }
            PrefixOpKind::Dereference => {
                if let Type::Ptr { pointee, .. } = expr_typ
                    && let typ = self.ctx.get_type(*pointee)
                    && !matches!(typ, Type::Void)
                {
                    let expr_type_info = ExprTypeInfo::new(*pointee, false);
                    self.type_table.insert(op.id, expr_type_info);
                    return Some(expr_type_info);
                } else {
                    self.errors.push(TypeCheckError::InvalidDereference {
                        expr_type_id: expr_id,
                        span: op.span,
                    });
                    return None;
                }
            }
            PrefixOpKind::Not => {
                if expr_typ.is_intlike() {
                    self.ctx.intern_type(Type::Int)
                } else {
                    self.errors.push(TypeCheckError::InvalidPrefixOpType {
                        expr_id,
                        span: op.span,
                        op: op.kind,
                    });
                    return None;
                }
            }
            PrefixOpKind::BitNot => {
                if matches!(expr_typ, Type::Int) {
                    self.ctx.intern_type(Type::Int)
                } else {
                    self.errors.push(TypeCheckError::InvalidPrefixOpType {
                        expr_id,
                        span: op.span,
                        op: op.kind,
                    });
                    return None;
                }
            }
        };
        let expr_type_info = ExprTypeInfo::new(id, false);
        self.type_table.insert(op.id, expr_type_info);
        Some(expr_type_info)
    }
    fn check_postfix_op(&mut self, op: PostfixOp) -> Option<ExprTypeInfo> {
        let expr_info = self.check_expr(op.expr)?;
        let expr_id = expr_info.id;
        let expr = self.ctx.get_type(expr_id);
        if !expr_info.assignable {
            self.errors.push(TypeCheckError::PostfixExprNotAssignable {
                span: op.span,
                op: op.kind,
            });
            return None;
        }
        let id = match expr {
            Type::Int => self.ctx.intern_type(Type::Int),
            Type::Ptr { .. } => expr_id,
            _ => {
                self.errors.push(TypeCheckError::InvalidPostfixOpType {
                    expr_id,
                    span: op.span,
                    op: op.kind,
                });
                return None;
            }
        };
        let expr_type_info = ExprTypeInfo::new(id, false);
        self.type_table.insert(op.id, expr_type_info);
        Some(expr_type_info)
    }
}

impl Type {
    pub fn is_assignable_to(&self, other: &Self, ctx: &Context) -> bool {
        self.is_assignable_to_inner(other, ctx, false)
    }
    pub fn is_assignable_to_inner(&self, other: &Self, ctx: &Context, no_void_conv: bool) -> bool {
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
                let void_conversion = !no_void_conv && (t1 == &Type::Void || t2 == &Type::Void);
                alias_ok && (void_conversion || t1 == t2)
            }
            (Type::Ptr { pointee, .. }, Type::FuncPtr { .. })
                if let Type::Void = ctx.get_type(*pointee) =>
            {
                true
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
                let ret_type_matches = r1.is_assignable_to_inner(r2, ctx, true);
                if !ret_type_matches {
                    return false;
                }
                p1.iter().zip(p2.iter()).all(|(id1, id2)| {
                    let t1 = ctx.get_type(*id1);
                    let t2 = ctx.get_type(*id2);
                    // Parameter types are contravariant, not covariant
                    t2.is_assignable_to_inner(t1, ctx, true)
                })
            }
            _ => false,
        }
    }
    fn is_castable_to(&self, other: &Self) -> bool {
        self.is_intlike() && other.is_intlike()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analysis::symbol_table::SymbolTableBuilder,
        syntax::{
            ast::{GlobalDeclarationKind, Program},
            context::Context,
            lexer::Lexer,
            parser::Parser,
        },
    };

    #[test]
    fn test_type_check() {
        let page = std::fs::read_to_string("testfiles/type_check.y86").unwrap();
        let mut ctx = Context::new();
        let lexed = Lexer::lex(&page, &mut ctx);
        assert!(!lexed.has_errors());
        let parsed = Parser::parse_test(&lexed.tokens, &mut ctx);
        assert!(!parsed.has_errors());
        let program = parsed.program;
        let mut globals = Vec::new();
        let mut funcs = Vec::new();
        for decl in program.decls {
            match decl.kind {
                GlobalDeclarationKind::Variable(_) => globals.push(decl),
                GlobalDeclarationKind::Struct(_) => globals.push(decl),
                GlobalDeclarationKind::Function(ref f) => {
                    let s = ctx.get_symbol(f.name.sym);
                    if s.starts_with('u') {
                        globals.push(decl);
                    } else {
                        funcs.push(decl);
                    }
                }
            }
        }
        for func in funcs {
            let mut program = Program { decls: Vec::new() };
            let GlobalDeclarationKind::Function(f) = &func.kind else {
                unreachable!();
            };
            for decl in globals.clone() {
                program.decls.push(decl);
            }
            let s = ctx.get_symbol(f.name.sym);
            let is_valid = if s.starts_with('v') {
                true
            } else if s.starts_with('i') {
                false
            } else {
                panic!("Function {s} doesn't start with v or i");
            };
            println!("Testing function {s}");
            program.decls.push(func);
            let symbol_table_output = SymbolTableBuilder::build(&program, &mut ctx);
            assert!(symbol_table_output.errors.is_empty());
            let symbol_table = symbol_table_output.symbol_table;
            let type_check_output = TypeChecker::check(&mut ctx, &symbol_table, &program);
            if is_valid {
                assert!(
                    type_check_output.errors.is_empty(),
                    "{:?}",
                    type_check_output.errors
                );
            } else {
                assert!(!type_check_output.errors.is_empty());
            }
        }
    }
}
