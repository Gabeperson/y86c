use crate::common::symbol::Symbol;
use crate::{
    common::span::Span,
    syntax::context::{Context, ExprId, StmtId, TypeId},
};
use tinyvec::TinyVec;

pub trait CtxEq {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool;
}

impl<T: CtxEq> CtxEq for [T] {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(a, b)| a.ctx_eq(b, ctx))
    }
}
impl<T: CtxEq> CtxEq for Option<T> {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        match (self, other) {
            (None, None) => true,
            (Some(lhs), Some(rhs)) => lhs.ctx_eq(rhs, ctx),
            _ => false,
        }
    }
}

impl CtxEq for TypeId {
    fn ctx_eq(&self, other: &Self, _ctx: &Context) -> bool {
        self == other
    }
}
impl CtxEq for Symbol {
    fn ctx_eq(&self, other: &Self, _ctx: &Context) -> bool {
        self == other
    }
}
impl CtxEq for ExprId {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        ctx.get_expr(*self).ctx_eq(ctx.get_expr(*other), ctx)
    }
}
impl CtxEq for StmtId {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        ctx.get_stmt(*self).ctx_eq(ctx.get_stmt(*other), ctx)
    }
}
impl<A: CtxEq, B: CtxEq> CtxEq for (A, B) {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.0.ctx_eq(&other.0, ctx) && self.1.ctx_eq(&other.1, ctx)
    }
}

#[derive(Clone, Debug, Copy, Hash, PartialEq, Eq)]
pub struct NodeId(pub u64);

impl Default for NodeId {
    fn default() -> Self {
        Self(u64::MAX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Int,
    Ptr {
        pointee: TypeId,
        noalias: bool,
    },
    Struct {
        name: Symbol,
    },
    Array {
        element_type: TypeId,
        len: i64,
    },
    FuncPtr {
        return_type: TypeId,
        param_types: TinyVec<[TypeId; 5]>,
    },
}

impl Type {
    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void)
    }
    pub fn is_int(&self) -> bool {
        matches!(self, Type::Int)
    }
    pub fn is_ptr(&self) -> bool {
        matches!(self, Type::Ptr { .. })
    }
    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct { .. })
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Type::Array { .. })
    }
    pub fn is_fnptr(&self) -> bool {
        matches!(self, Type::FuncPtr { .. })
    }
    pub fn indexed_type(&self) -> Option<TypeId> {
        match self {
            Type::Ptr { pointee, .. } => Some(*pointee),
            Type::Array { element_type, .. } => Some(*element_type),
            _ => None,
        }
    }
    pub fn is_intlike(&self) -> bool {
        match self {
            Type::Void => false,
            Type::Int => true,
            Type::Ptr { .. } => true,
            Type::Struct { .. } => false,
            Type::Array { .. } => false,
            Type::FuncPtr { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Default, Copy)]
pub struct TypeNode {
    pub inner: TypeId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for TypeNode {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.inner.ctx_eq(&other.inner, ctx)
    }
}

impl TypeNode {
    pub fn new(inner: TypeId, span: Span, id: NodeId) -> Self {
        Self { inner, span, id }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Expr {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind.ctx_eq(&other.kind, ctx)
    }
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span, id: NodeId) -> Self {
        Self { kind, span, id }
    }
}

#[derive(Debug, Clone, Default)]
pub enum ExprKind {
    Nullptr(Nullptr),
    Cast(Cast),
    Ident(Ident),
    Int(Int),
    BinaryOp(BinaryOp),
    PrefixOp(PrefixOp),
    PostfixOp(PostfixOp),
    Ternary(Ternary),
    FunctionCall(FunctionCall),
    ArrayIndex(ArrayIndex),
    SizeOfType(SizeOfType),
    StructInit(StructInit),
    ArrayInit(ArrayInit),
    MemberAccess(MemberAccess),
    PointerMemberAccess(PointerMemberAccess),
    #[default]
    Error,
}

impl CtxEq for ExprKind {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        match (self, other) {
            (Self::Nullptr(lhs), Self::Nullptr(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Cast(lhs), Self::Cast(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Ident(lhs), Self::Ident(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Int(lhs), Self::Int(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::BinaryOp(lhs), Self::BinaryOp(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::PrefixOp(lhs), Self::PrefixOp(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::PostfixOp(lhs), Self::PostfixOp(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Ternary(lhs), Self::Ternary(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::FunctionCall(lhs), Self::FunctionCall(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::ArrayIndex(lhs), Self::ArrayIndex(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::SizeOfType(lhs), Self::SizeOfType(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::StructInit(lhs), Self::StructInit(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::ArrayInit(lhs), Self::ArrayInit(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::MemberAccess(lhs), Self::MemberAccess(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::PointerMemberAccess(lhs), Self::PointerMemberAccess(rhs)) => {
                lhs.ctx_eq(rhs, ctx)
            }
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Nullptr {
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Nullptr {
    fn ctx_eq(&self, _other: &Self, _ctx: &Context) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cast {
    pub to_type: TypeNode,
    pub expr: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Cast {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.to_type.ctx_eq(&other.to_type, ctx) && self.expr.ctx_eq(&other.expr, ctx)
    }
}

#[derive(Debug, Clone, Default, Copy)]
pub struct Ident {
    pub sym: Symbol,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Ident {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.sym.ctx_eq(&other.sym, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct Int {
    pub lit: i64,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Int {
    fn ctx_eq(&self, other: &Self, _ctx: &Context) -> bool {
        self.lit == other.lit
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    NotEq,
    And,
    Or,
    BitAnd,
    BitOr,
    Xor,
    Mod,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    BitAndAssign,
    BitOrAssign,
    XorAssign,
    ModAssign,
    Assign,
}

impl BinaryOpKind {
    pub fn needs_assignable(&self) -> bool {
        matches!(
            self,
            BinaryOpKind::AddAssign
                | BinaryOpKind::SubAssign
                | BinaryOpKind::MulAssign
                | BinaryOpKind::DivAssign
                | BinaryOpKind::BitAndAssign
                | BinaryOpKind::BitOrAssign
                | BinaryOpKind::XorAssign
                | BinaryOpKind::ModAssign
                | BinaryOpKind::Assign
        )
    }
}

#[derive(Copy, Debug, Clone)]
pub struct BinaryOp {
    pub kind: BinaryOpKind,
    pub left: ExprId,
    pub right: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for BinaryOp {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind == other.kind
            && self.left.ctx_eq(&other.left, ctx)
            && self.right.ctx_eq(&other.right, ctx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOpKind {
    Increment,
    Decrement,
    UnaryPlus,
    UnaryMinus,
    AddressOf,
    Dereference,
    Not,
    BitNot,
}

#[derive(Copy, Debug, Clone)]
pub struct PrefixOp {
    pub kind: PrefixOpKind,
    pub expr: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for PrefixOp {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind == other.kind && self.expr.ctx_eq(&other.expr, ctx)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostfixOpKind {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, Copy)]
pub struct PostfixOp {
    pub kind: PostfixOpKind,
    pub expr: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for PostfixOp {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind == other.kind && self.expr.ctx_eq(&other.expr, ctx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ternary {
    pub condition: ExprId,
    pub true_branch: ExprId,
    pub false_branch: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Ternary {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.condition.ctx_eq(&other.condition, ctx)
            && self.true_branch.ctx_eq(&other.true_branch, ctx)
            && self.false_branch.ctx_eq(&other.false_branch, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub func_expr: ExprId,
    pub args: TinyVec<[ExprId; 5]>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for FunctionCall {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.func_expr.ctx_eq(&other.func_expr, ctx) && self.args.ctx_eq(&other.args, ctx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArrayIndex {
    pub array: ExprId,
    pub index: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for ArrayIndex {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.array.ctx_eq(&other.array, ctx) && self.index.ctx_eq(&other.index, ctx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SizeOfType {
    pub typ: TypeNode,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for SizeOfType {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.typ.ctx_eq(&other.typ, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct StructInit {
    pub name: Ident,
    pub field_inits: TinyVec<[(Ident, ExprId); 5]>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for StructInit {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.name.ctx_eq(&other.name, ctx) && self.field_inits.ctx_eq(&other.field_inits, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct ArrayInit {
    pub elements: TinyVec<[ExprId; 5]>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for ArrayInit {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.elements.ctx_eq(&other.elements, ctx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemberAccess {
    pub struct_expr: ExprId,
    pub member_name: Ident,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for MemberAccess {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.struct_expr.ctx_eq(&other.struct_expr, ctx)
            && self.member_name.ctx_eq(&other.member_name, ctx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PointerMemberAccess {
    pub struct_ptr_expr: ExprId,
    pub member_name: Ident,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for PointerMemberAccess {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.struct_ptr_expr.ctx_eq(&other.struct_ptr_expr, ctx)
            && self.member_name.ctx_eq(&other.member_name, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Stmt {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind.ctx_eq(&other.kind, ctx)
    }
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Assert(Assert),
    Break(Break),
    Continue(Continue),
    Block(Block),
    IfStmt(IfStmt),
    WhileLoop(WhileLoop),
    ForLoop(ForLoop),
    ReturnStmt(ReturnStmt),
    VariableDeclaration(VariableDeclaration),
    Expr(ExprId),
}

impl CtxEq for StmtKind {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        match (self, other) {
            (Self::Assert(lhs), Self::Assert(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Break(lhs), Self::Break(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Continue(lhs), Self::Continue(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Block(lhs), Self::Block(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::IfStmt(lhs), Self::IfStmt(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::WhileLoop(lhs), Self::WhileLoop(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::ForLoop(lhs), Self::ForLoop(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::ReturnStmt(lhs), Self::ReturnStmt(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::VariableDeclaration(lhs), Self::VariableDeclaration(rhs)) => {
                lhs.ctx_eq(rhs, ctx)
            }
            (Self::Expr(lhs), Self::Expr(rhs)) => lhs.ctx_eq(rhs, ctx),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Assert {
    pub condition: ExprId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Assert {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.condition.ctx_eq(&other.condition, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct Break {
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Break {
    fn ctx_eq(&self, _other: &Self, _ctx: &Context) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct Continue {
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Continue {
    fn ctx_eq(&self, _other: &Self, _ctx: &Context) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub body: TinyVec<[StmtId; 5]>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for Block {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.body.ctx_eq(&other.body, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: ExprId,
    pub then_branch: StmtId,
    pub else_branch: Option<StmtId>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for IfStmt {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.condition.ctx_eq(&other.condition, ctx)
            && self.then_branch.ctx_eq(&other.then_branch, ctx)
            && self.else_branch.ctx_eq(&other.else_branch, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct WhileLoop {
    pub condition: ExprId,
    pub body: StmtId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for WhileLoop {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.condition.ctx_eq(&other.condition, ctx) && self.body.ctx_eq(&other.body, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct ForLoop {
    pub init: Option<StmtId>,
    pub condition: Option<ExprId>,
    pub post: Option<ExprId>,
    pub body: StmtId,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for ForLoop {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.init.ctx_eq(&other.init, ctx)
            && self.condition.ctx_eq(&other.condition, ctx)
            && self.post.ctx_eq(&other.post, ctx)
            && self.body.ctx_eq(&other.body, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<ExprId>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for ReturnStmt {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.value.ctx_eq(&other.value, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    pub var_type: TypeNode,
    pub name: Ident,
    pub init_value: Option<ExprId>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for VariableDeclaration {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.var_type.ctx_eq(&other.var_type, ctx)
            && self.name.ctx_eq(&other.name, ctx)
            && self.init_value.ctx_eq(&other.init_value, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct StructDeclaration {
    pub name: Ident,
    pub fields: TinyVec<[(Ident, TypeNode); 5]>,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for StructDeclaration {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.name.ctx_eq(&other.name, ctx) && self.fields.ctx_eq(&other.fields, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct FunctionDeclaration {
    pub return_type: Option<TypeNode>,
    pub name: Ident,
    pub params: TinyVec<[(Ident, TypeNode); 5]>,
    pub body: Block,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for FunctionDeclaration {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.return_type.ctx_eq(&other.return_type, ctx)
            && self.name.ctx_eq(&other.name, ctx)
            && self.params.ctx_eq(&other.params, ctx)
            && self.body.ctx_eq(&other.body, ctx)
    }
}

#[derive(Debug, Clone)]
pub struct GlobalDeclaration {
    pub kind: GlobalDeclarationKind,
    pub span: Span,
    pub id: NodeId,
}

impl CtxEq for GlobalDeclaration {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.kind.ctx_eq(&other.kind, ctx)
    }
}

#[derive(Debug, Clone)]
pub enum GlobalDeclarationKind {
    Variable(VariableDeclaration),
    Struct(StructDeclaration),
    Function(FunctionDeclaration),
}

impl CtxEq for GlobalDeclarationKind {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        match (self, other) {
            (Self::Variable(lhs), Self::Variable(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Struct(lhs), Self::Struct(rhs)) => lhs.ctx_eq(rhs, ctx),
            (Self::Function(lhs), Self::Function(rhs)) => lhs.ctx_eq(rhs, ctx),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Program {
    pub decls: Vec<GlobalDeclaration>,
}

impl CtxEq for Program {
    fn ctx_eq(&self, other: &Self, ctx: &Context) -> bool {
        self.decls.ctx_eq(&other.decls, ctx)
    }
}

pub mod visitor {
    use crate::syntax::context::Context;

    use super::*;
    pub trait AstVisitor {
        fn visit_expr(&mut self, expr: &Expr, ctx: &Context) {
            walk_expr(self, expr, ctx)
        }
        fn visit_stmt(&mut self, stmt: &Stmt, ctx: &Context) {
            walk_stmt(self, stmt, ctx)
        }
        fn visit_global_declaration(&mut self, decl: &GlobalDeclaration, ctx: &Context) {
            walk_global_declaration(self, decl, ctx)
        }
        fn visit_typenode(&mut self, typ: &TypeNode, ctx: &Context) {
            self.visit_type(ctx.get_type(typ.inner), ctx)
        }
        fn visit_type(&mut self, typ: &Type, ctx: &Context) {
            _ = typ;
            _ = ctx;
        }
        fn visit_nullptr(&mut self, expr: &Nullptr, ctx: &Context) {
            _ = expr;
            _ = ctx;
        }
        fn visit_cast(&mut self, expr: &Cast, ctx: &Context) {
            self.visit_typenode(&expr.to_type, ctx);
            self.visit_expr(ctx.get_expr(expr.expr), ctx);
        }
        fn visit_ident_expr(&mut self, expr: &Ident, ctx: &Context) {
            _ = expr;
            _ = ctx;
        }
        fn visit_int(&mut self, expr: &Int, ctx: &Context) {
            _ = expr;
            _ = ctx;
        }
        fn visit_binary_op(&mut self, expr: &BinaryOp, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.left), ctx);
            self.visit_expr(ctx.get_expr(expr.right), ctx);
        }
        fn visit_prefix_op(&mut self, expr: &PrefixOp, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.expr), ctx);
        }
        fn visit_postfix_op(&mut self, expr: &PostfixOp, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.expr), ctx);
        }
        fn visit_ternary(&mut self, expr: &Ternary, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.condition), ctx);
            self.visit_expr(ctx.get_expr(expr.true_branch), ctx);
            self.visit_expr(ctx.get_expr(expr.false_branch), ctx);
        }
        fn visit_function_call(&mut self, expr: &FunctionCall, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.func_expr), ctx);
            for arg in &expr.args {
                self.visit_expr(ctx.get_expr(*arg), ctx);
            }
        }
        fn visit_array_index(&mut self, expr: &ArrayIndex, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.array), ctx);
            self.visit_expr(ctx.get_expr(expr.index), ctx);
        }
        fn visit_sizeof_type(&mut self, expr: &SizeOfType, ctx: &Context) {
            self.visit_typenode(&expr.typ, ctx);
        }
        fn visit_struct_init(&mut self, expr: &StructInit, ctx: &Context) {
            for (_name, expr) in &expr.field_inits {
                self.visit_expr(ctx.get_expr(*expr), ctx);
            }
        }
        fn visit_array_init(&mut self, expr: &ArrayInit, ctx: &Context) {
            for init in &expr.elements {
                self.visit_expr(ctx.get_expr(*init), ctx);
            }
        }
        fn visit_member_access(&mut self, expr: &MemberAccess, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.struct_expr), ctx);
        }
        fn visit_pointer_member_access(&mut self, expr: &PointerMemberAccess, ctx: &Context) {
            self.visit_expr(ctx.get_expr(expr.struct_ptr_expr), ctx);
        }
        fn visit_assert(&mut self, stmt: &Assert, ctx: &Context) {
            self.visit_expr(ctx.get_expr(stmt.condition), ctx);
        }
        fn visit_break(&mut self, stmt: &Break, ctx: &Context) {
            _ = stmt;
            _ = ctx;
        }
        fn visit_continue(&mut self, stmt: &Continue, ctx: &Context) {
            _ = stmt;
            _ = ctx;
        }
        fn visit_block(&mut self, stmt: &Block, ctx: &Context) {
            for stmt in &stmt.body {
                self.visit_stmt(ctx.get_stmt(*stmt), ctx);
            }
        }
        fn visit_if_stmt(&mut self, stmt: &IfStmt, ctx: &Context) {
            self.visit_expr(ctx.get_expr(stmt.condition), ctx);
            self.visit_stmt(ctx.get_stmt(stmt.then_branch), ctx);
            if let Some(else_branch) = stmt.else_branch {
                self.visit_stmt(ctx.get_stmt(else_branch), ctx);
            }
        }
        fn visit_while_loop(&mut self, stmt: &WhileLoop, ctx: &Context) {
            self.visit_expr(ctx.get_expr(stmt.condition), ctx);
            self.visit_stmt(ctx.get_stmt(stmt.body), ctx);
        }
        fn visit_for_loop(&mut self, stmt: &ForLoop, ctx: &Context) {
            if let Some(init) = stmt.init {
                self.visit_stmt(ctx.get_stmt(init), ctx);
            }
            if let Some(condition) = stmt.condition {
                self.visit_expr(ctx.get_expr(condition), ctx);
            }
            if let Some(post) = stmt.post {
                self.visit_expr(ctx.get_expr(post), ctx);
            }
            self.visit_stmt(ctx.get_stmt(stmt.body), ctx);
        }
        fn visit_return(&mut self, stmt: &ReturnStmt, ctx: &Context) {
            if let Some(expr) = stmt.value {
                self.visit_expr(ctx.get_expr(expr), ctx);
            }
        }
        fn visit_variable_declaration(&mut self, decl: &VariableDeclaration, ctx: &Context) {
            self.visit_typenode(&decl.var_type, ctx);
            if let Some(init) = decl.init_value {
                self.visit_expr(ctx.get_expr(init), ctx);
            }
        }
        fn visit_struct_declaration(&mut self, decl: &StructDeclaration, ctx: &Context) {
            for (_name, typ) in &decl.fields {
                self.visit_typenode(typ, ctx);
            }
        }
        fn visit_function_declaration(&mut self, decl: &FunctionDeclaration, ctx: &Context) {
            if let Some(ret_type) = &decl.return_type {
                self.visit_typenode(ret_type, ctx);
            }
            for (_name, typ) in &decl.params {
                self.visit_typenode(typ, ctx);
            }
            self.visit_block(&decl.body, ctx);
        }
        fn visit_program(&mut self, program: &Program, ctx: &Context) {
            for decl in &program.decls {
                self.visit_global_declaration(decl, ctx);
            }
        }
        fn visit_error_expr(&mut self, ctx: &Context) {
            _ = ctx;
        }
        fn visit_error_stmt(&mut self, ctx: &Context) {
            _ = ctx;
        }
    }

    pub fn walk_expr<V: AstVisitor + ?Sized>(visitor: &mut V, expr: &Expr, ctx: &Context) {
        match &expr.kind {
            ExprKind::Nullptr(nullptr) => visitor.visit_nullptr(nullptr, ctx),
            ExprKind::Cast(cast) => visitor.visit_cast(cast, ctx),
            ExprKind::Ident(ident) => visitor.visit_ident_expr(ident, ctx),
            ExprKind::Int(int) => visitor.visit_int(int, ctx),
            ExprKind::BinaryOp(binary_op) => visitor.visit_binary_op(binary_op, ctx),
            ExprKind::PrefixOp(prefix_op) => visitor.visit_prefix_op(prefix_op, ctx),
            ExprKind::PostfixOp(postfix_op) => visitor.visit_postfix_op(postfix_op, ctx),
            ExprKind::Ternary(ternary) => visitor.visit_ternary(ternary, ctx),
            ExprKind::FunctionCall(function_call) => {
                visitor.visit_function_call(function_call, ctx)
            }
            ExprKind::ArrayIndex(array_index) => visitor.visit_array_index(array_index, ctx),
            ExprKind::SizeOfType(size_of_type) => visitor.visit_sizeof_type(size_of_type, ctx),
            ExprKind::StructInit(struct_init) => visitor.visit_struct_init(struct_init, ctx),
            ExprKind::ArrayInit(array_init) => visitor.visit_array_init(array_init, ctx),
            ExprKind::MemberAccess(member_access) => {
                visitor.visit_member_access(member_access, ctx)
            }
            ExprKind::PointerMemberAccess(pointer_member_access) => {
                visitor.visit_pointer_member_access(pointer_member_access, ctx)
            }
            ExprKind::Error => visitor.visit_error_expr(ctx),
        }
    }
    pub fn walk_stmt<V: AstVisitor + ?Sized>(visitor: &mut V, stmt: &Stmt, ctx: &Context) {
        match &stmt.kind {
            StmtKind::Assert(assert) => visitor.visit_assert(assert, ctx),
            StmtKind::Break(break_) => visitor.visit_break(break_, ctx),
            StmtKind::Continue(continue_) => visitor.visit_continue(continue_, ctx),
            StmtKind::Block(block) => visitor.visit_block(block, ctx),
            StmtKind::IfStmt(if_stmt) => visitor.visit_if_stmt(if_stmt, ctx),
            StmtKind::WhileLoop(while_loop) => visitor.visit_while_loop(while_loop, ctx),
            StmtKind::ForLoop(for_loop) => visitor.visit_for_loop(for_loop, ctx),
            StmtKind::ReturnStmt(return_stmt) => visitor.visit_return(return_stmt, ctx),
            StmtKind::VariableDeclaration(variable_declaration) => {
                visitor.visit_variable_declaration(variable_declaration, ctx)
            }
            StmtKind::Expr(expr) => visitor.visit_expr(ctx.get_expr(*expr), ctx),
        }
    }
    pub fn walk_global_declaration<V: AstVisitor + ?Sized>(
        visitor: &mut V,
        decl: &GlobalDeclaration,
        ctx: &Context,
    ) {
        match &decl.kind {
            GlobalDeclarationKind::Variable(variable_declaration) => {
                visitor.visit_variable_declaration(variable_declaration, ctx)
            }
            GlobalDeclarationKind::Struct(struct_declaration) => {
                visitor.visit_struct_declaration(struct_declaration, ctx)
            }
            GlobalDeclarationKind::Function(function_declaration) => {
                visitor.visit_function_declaration(function_declaration, ctx)
            }
        }
    }
}
