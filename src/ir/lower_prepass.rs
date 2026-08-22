use ahash::AHashMap;

use crate::{
    analysis::scoped_hashmap::ScopedHashMap,
    common::{interner::define_arena, symbol::Symbol},
    syntax::{ast::*, context::*},
};

define_arena!(Var, VarId, VarArena);

#[derive(Debug, Clone)]
pub struct Var {
    pub address_taken: bool,
    pub typ: TypeId,
}

impl Var {
    fn new(address_taken: bool, typ: TypeId) -> Self {
        Self { address_taken, typ }
    }
}

#[derive(Debug, Clone)]
pub struct LoweringPrepassOutput {
    pub id_map: AHashMap<NodeId, VarId>,
    pub vars: VarArena,
}

#[derive(Debug, Clone)]
pub struct LoweringPrepass<'a> {
    scoped: ScopedHashMap<Symbol, VarId>,
    vars: VarArena,
    id_map: AHashMap<NodeId, VarId>,
    ctx: &'a Context,
}

impl LoweringPrepass<'_> {
    pub fn run(program: &Program, ctx: &Context) -> LoweringPrepassOutput {
        let mut prepass = LoweringPrepass {
            scoped: ScopedHashMap::new(),
            vars: VarArena::new(),
            id_map: AHashMap::new(),
            ctx,
        };
        prepass.run_inner(program);
        LoweringPrepassOutput {
            id_map: prepass.id_map,
            vars: prepass.vars,
        }
    }
    fn declare_var(&mut self, sym: Symbol, nodeid: NodeId, addr_taken: bool, typ: TypeId) {
        let var = Var::new(addr_taken, typ);
        let id = self.vars.intern(var);
        self.scoped.insert(sym, id);
        self.id_map.insert(nodeid, id);
    }
    fn var_ref_taken(&mut self, sym: Symbol, nodeid: NodeId) {
        let Some(id) = self.scoped.get(sym) else {
            // If this path is taken it means the symbol above refers to a global variable.
            return;
        };
        let var = self.vars.get_mut(id);
        var.address_taken = true;
        self.id_map.insert(nodeid, id);
    }
    fn run_inner(&mut self, program: &Program) {
        for decl in &program.decls {
            if let GlobalDeclarationKind::Function(func) = &decl.kind {
                self.scoped.enter_scope();
                for (name, typ) in func.params.iter().copied() {
                    self.declare_var(name.sym, name.id, false, typ.inner);
                }
                self.visit_block(&func.body, false);
                self.scoped.exit_scope();
            }
        }
    }
    fn visit_block(&mut self, block: &Block, new_scope: bool) {
        if new_scope {
            self.scoped.enter_scope();
        }
        for stmt in block.body.iter().copied() {
            self.visit_stmt(stmt);
        }
        if new_scope {
            self.scoped.exit_scope();
        }
    }
    fn visit_stmt(&mut self, stmt: StmtId) {
        let stmt = self.ctx.get_stmt(stmt);
        match &stmt.kind {
            StmtKind::Assert(assert) => self.visit_expr(assert.condition, false),
            StmtKind::Block(block) => self.visit_block(&block.clone(), true),
            StmtKind::IfStmt(if_stmt) => {
                let condition = if_stmt.condition;
                let then_branch = if_stmt.then_branch;
                let else_branch = if_stmt.else_branch;
                self.visit_expr(condition, false);
                self.visit_stmt(then_branch);
                if let Some(else_branch) = else_branch {
                    self.visit_stmt(else_branch);
                }
            }
            StmtKind::WhileLoop(while_loop) => {
                let condition = while_loop.condition;
                let body = while_loop.body;
                self.visit_expr(condition, false);
                self.visit_stmt(body);
            }
            StmtKind::ForLoop(for_loop) => {
                self.scoped.enter_scope();
                let init = for_loop.init;
                let condition = for_loop.condition;
                let post = for_loop.post;
                let body = for_loop.body;
                if let Some(init) = init {
                    self.visit_stmt(init);
                }
                if let Some(condition) = condition {
                    self.visit_expr(condition, false);
                }
                if let Some(post) = post {
                    self.visit_expr(post, false);
                }
                if let StmtKind::Block(block) = &self.ctx.get_stmt(body).kind {
                    let block = block.clone();
                    self.visit_block(&block, false);
                } else {
                    self.visit_stmt(body);
                }
            }
            StmtKind::ReturnStmt(return_stmt) => {
                if let Some(expr) = return_stmt.value {
                    self.visit_expr(expr, false);
                }
            }
            StmtKind::VariableDeclaration(variable_declaration) => {
                if let Some(expr) = variable_declaration.init_value {
                    self.visit_expr(expr, false);
                }
                self.declare_var(
                    variable_declaration.name.sym,
                    variable_declaration.name.id,
                    false,
                    variable_declaration.var_type.inner,
                );
            }
            StmtKind::Expr(expr_id) => self.visit_expr(*expr_id, false),
            StmtKind::Break(_) | StmtKind::Continue(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: ExprId, addr_taken: bool) {
        let expr = self.ctx.get_expr(expr);
        match &expr.kind {
            ExprKind::Nullptr(_nullptr) => {}
            ExprKind::SizeOfType(_size_of_type) => {}
            ExprKind::Int(_int) => {}
            ExprKind::Cast(cast) => {
                self.visit_expr(cast.expr, false);
            }
            ExprKind::Ident(ident) => {
                if addr_taken {
                    self.var_ref_taken(ident.sym, ident.id);
                }
            }
            ExprKind::BinaryOp(binary_op) => {
                self.visit_expr(binary_op.left, false);
                self.visit_expr(binary_op.right, false);
            }
            ExprKind::PrefixOp(prefix_op) => {
                self.visit_expr(prefix_op.expr, prefix_op.kind == PrefixOpKind::AddressOf);
            }
            ExprKind::PostfixOp(postfix_op) => {
                self.visit_expr(postfix_op.expr, false);
            }
            ExprKind::Ternary(ternary) => {
                self.visit_expr(ternary.condition, false);
                self.visit_expr(ternary.true_branch, false);
                self.visit_expr(ternary.false_branch, false);
            }
            ExprKind::FunctionCall(function_call) => {
                self.visit_expr(function_call.func_expr, false);
                for arg in function_call.args.iter().copied() {
                    self.visit_expr(arg, false);
                }
            }
            ExprKind::ArrayIndex(array_index) => {
                self.visit_expr(array_index.array, true);
                self.visit_expr(array_index.index, false);
            }
            ExprKind::StructInit(struct_init) => {
                for (_name, expr) in struct_init.field_inits.iter().copied() {
                    self.visit_expr(expr, false);
                }
            }
            ExprKind::ArrayInit(array_init) => {
                for expr in array_init.elements.iter().copied() {
                    self.visit_expr(expr, false);
                }
            }
            ExprKind::MemberAccess(member_access) => {
                self.visit_expr(member_access.struct_expr, true);
            }
            ExprKind::PointerMemberAccess(pointer_member_access) => {
                self.visit_expr(pointer_member_access.struct_ptr_expr, false);
            }
            ExprKind::CopyProvenance(copy_provenance) => {
                self.visit_expr(copy_provenance.prov_ptr, false);
            }
            ExprKind::ExposeProvenance(expose_provenance) => {
                self.visit_expr(expose_provenance.ptr, false);
            }
            ExprKind::UnexposeProvenance(unexpose_provenance) => {
                self.visit_expr(unexpose_provenance.int, false);
            }
            ExprKind::NewProvenance(new_provenance) => {
                self.visit_expr(new_provenance.ptr, false);
            }
            ExprKind::Error => unreachable!(),
        }
    }
}
