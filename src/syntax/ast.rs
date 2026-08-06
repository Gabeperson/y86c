use crate::common::span::Span;
use bumpalo::collections::Vec;

#[derive(Clone, Debug, Copy, Hash, PartialEq, Eq)]
pub struct NodeId(pub u64);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind<'a> {
    Void,
    Int,
    Ptr {
        pointee: Type<'a>,
        noalias: bool,
    },
    Struct {
        name: Ident<'a>,
    },
    Array {
        element_type: Type<'a>,
        size: i64,
    },
    FuncPtr {
        return_type: Option<Type<'a>>,
        param_types: Vec<'a, Type<'a>>,
    },
    Error,
}

impl TypeKind<'_> {
    pub fn is_error(&self) -> bool {
        *self == TypeKind::Error
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Type<'a> {
    pub inner: &'a TypeKind<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Type<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<'a> Type<'a> {
    pub fn new(inner: &'a TypeKind<'a>, span: Span, id: NodeId) -> Self {
        Self { inner, span, id }
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Expr<'a> {
    pub kind: &'a ExprKind<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Expr<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl<'a> Expr<'a> {
    pub fn new(kind: &'a ExprKind<'a>, span: Span, id: NodeId) -> Self {
        Self { kind, span, id }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind<'a> {
    Nullptr(Nullptr),
    Cast(Cast<'a>),
    Ident(Ident<'a>),
    Int(Int),
    BinaryOp(BinaryOp<'a>),
    PrefixOp(PrefixOp<'a>),
    PostfixOp(PostfixOp<'a>),
    Ternary(Ternary<'a>),
    FunctionCall(FunctionCall<'a>),
    ArrayIndex(ArrayIndex<'a>),
    SizeOfType(SizeOfType<'a>),
    StructInit(StructInit<'a>),
    ArrayInit(ArrayInit<'a>),
    MemberAccess(MemberAccess<'a>),
    PointerMemberAccess(PointerMemberAccess<'a>),
    Error,
}

#[derive(Debug, Clone, Eq)]
pub struct Nullptr {
    pub span: Span,
    pub id: NodeId,
}

impl PartialEq for Nullptr {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Cast<'a> {
    pub to_type: Type<'a>,
    pub expr: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Cast<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.to_type == other.to_type && self.expr == other.expr
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Ident<'a> {
    pub ident: &'a str,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Ident<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.ident == other.ident
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Int {
    pub lit: i64,
    pub span: Span,
    pub id: NodeId,
}

impl PartialEq for Int {
    fn eq(&self, other: &Self) -> bool {
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
#[derive(Debug, Clone, Eq)]
pub struct BinaryOp<'a> {
    pub kind: BinaryOpKind,
    pub left: Expr<'a>,
    pub right: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for BinaryOp<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.left == other.left && self.right == other.right
    }
}

#[derive(Debug, Clone, Copy, Eq)]
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

impl PartialEq for PrefixOpKind {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

#[derive(Debug, Clone, Eq)]
pub struct PrefixOp<'a> {
    pub kind: PrefixOpKind,
    pub expr: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for PrefixOp<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.expr == other.expr
    }
}
#[derive(Debug, Clone, Copy, Eq)]
pub enum PostfixOpKind {
    Increment,
    Decrement,
}

impl PartialEq for PostfixOpKind {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

#[derive(Debug, Clone, Eq)]
pub struct PostfixOp<'a> {
    pub kind: PostfixOpKind,
    pub expr: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for PostfixOp<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.expr == other.expr
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Ternary<'a> {
    pub condition: Expr<'a>,
    pub true_branch: Expr<'a>,
    pub false_branch: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Ternary<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.condition == other.condition
            && self.true_branch == other.true_branch
            && self.false_branch == other.false_branch
    }
}

#[derive(Debug, Clone, Eq)]
pub struct FunctionCall<'a> {
    pub func_expr: Expr<'a>,
    pub args: Vec<'a, Expr<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for FunctionCall<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.func_expr == other.func_expr && self.args == other.args
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ArrayIndex<'a> {
    pub array: Expr<'a>,
    pub index: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for ArrayIndex<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.array == other.array && self.index == other.index
    }
}

#[derive(Debug, Clone, Eq)]
pub struct SizeOfType<'a> {
    pub typ: Type<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for SizeOfType<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.typ == other.typ
    }
}

#[derive(Debug, Clone, Eq)]
pub struct StructInit<'a> {
    pub name: Ident<'a>,
    pub field_inits: Vec<'a, (Ident<'a>, Expr<'a>)>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for StructInit<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.field_inits == other.field_inits
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ArrayInit<'a> {
    pub elements: Vec<'a, Expr<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for ArrayInit<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.elements == other.elements
    }
}

#[derive(Debug, Clone, Eq)]
pub struct MemberAccess<'a> {
    pub struct_expr: Expr<'a>,
    pub member_name: Ident<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for MemberAccess<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.struct_expr == other.struct_expr && self.member_name == other.member_name
    }
}

#[derive(Debug, Clone, Eq)]
pub struct PointerMemberAccess<'a> {
    pub struct_ptr_expr: Expr<'a>,
    pub member_name: Ident<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for PointerMemberAccess<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.struct_ptr_expr == other.struct_ptr_expr && self.member_name == other.member_name
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Stmt<'a> {
    pub kind: &'a StmtKind<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Stmt<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StmtKind<'a> {
    Assert(Assert<'a>),
    Break(Break),
    Continue(Continue),
    Block(Block<'a>),
    IfStmt(IfStmt<'a>),
    WhileLoop(WhileLoop<'a>),
    ForLoop(ForLoop<'a>),
    ReturnStmt(ReturnStmt<'a>),
    VariableDeclaration(VariableDeclaration<'a>),
    Expr(Expr<'a>),
}

#[derive(Debug, Clone, Eq)]
pub struct Assert<'a> {
    pub condition: Expr<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Assert<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.condition == other.condition
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Break {
    pub span: Span,
    pub id: NodeId,
}

impl PartialEq for Break {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Continue {
    pub span: Span,
    pub id: NodeId,
}

impl PartialEq for Continue {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Eq)]
pub struct Block<'a> {
    pub body: Vec<'a, Stmt<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for Block<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.body == other.body
    }
}

#[derive(Debug, Clone, Eq)]
pub struct IfStmt<'a> {
    pub condition: Expr<'a>,
    pub then_branch: Stmt<'a>,
    pub else_branch: Option<Stmt<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for IfStmt<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.condition == other.condition
            && self.then_branch == other.then_branch
            && self.else_branch == other.else_branch
    }
}

#[derive(Debug, Clone, Eq)]
pub struct WhileLoop<'a> {
    pub condition: Expr<'a>,
    pub body: Stmt<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for WhileLoop<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.condition == other.condition && self.body == other.body
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ForLoop<'a> {
    pub init: Option<Stmt<'a>>,
    pub condition: Option<Expr<'a>>,
    pub post: Option<Expr<'a>>,
    pub body: Stmt<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for ForLoop<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.init == other.init
            && self.condition == other.condition
            && self.post == other.post
            && self.body == other.body
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ReturnStmt<'a> {
    pub value: Option<Expr<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for ReturnStmt<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[derive(Debug, Clone, Eq)]
pub struct VariableDeclaration<'a> {
    pub var_type: Type<'a>,
    pub name: Ident<'a>,
    pub init_value: Option<Expr<'a>>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for VariableDeclaration<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.var_type == other.var_type
            && self.name == other.name
            && self.init_value == other.init_value
    }
}

#[derive(Debug, Clone, Eq)]
pub struct StructDeclaration<'a> {
    pub name: Ident<'a>,
    pub fields: Vec<'a, (Ident<'a>, Type<'a>)>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for StructDeclaration<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}

#[derive(Debug, Clone, Eq)]
pub struct FunctionDeclaration<'a> {
    pub return_type: Option<Type<'a>>,
    pub name: Ident<'a>,
    pub params: Vec<'a, (Ident<'a>, Type<'a>)>,
    pub body: Block<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for FunctionDeclaration<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.return_type == other.return_type
            && self.name == other.name
            && self.params == other.params
            && self.body == other.body
    }
}

#[derive(Debug, Clone, Eq)]
pub struct GlobalDeclaration<'a> {
    pub kind: GlobalDeclarationKind<'a>,
    pub span: Span,
    pub id: NodeId,
}

impl<'a> PartialEq for GlobalDeclaration<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalDeclarationKind<'a> {
    Variable(VariableDeclaration<'a>),
    Struct(StructDeclaration<'a>),
    Function(FunctionDeclaration<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program<'a> {
    pub decls: Vec<'a, GlobalDeclaration<'a>>,
}

pub mod visitor {
    use super::*;
    pub trait AstVisitor {
        fn visit_expr(&mut self, expr: &Expr<'_>) {
            walk_expr(self, expr)
        }
        fn visit_stmt(&mut self, stmt: &Stmt<'_>) {
            walk_stmt(self, stmt)
        }
        fn visit_global_declaration(&mut self, decl: &GlobalDeclaration<'_>) {
            walk_global_declaration(self, decl)
        }
        fn visit_type(&mut self, typ: &Type<'_>) {
            _ = typ;
        }
        fn visit_nullptr(&mut self, expr: &Nullptr) {
            _ = expr;
        }
        fn visit_cast(&mut self, expr: &Cast<'_>) {
            self.visit_type(&expr.to_type);
            self.visit_expr(&expr.expr);
        }
        fn visit_ident_expr(&mut self, expr: &Ident<'_>) {
            _ = expr;
        }
        fn visit_int(&mut self, expr: &Int) {
            _ = expr;
        }
        fn visit_binary_op(&mut self, expr: &BinaryOp<'_>) {
            self.visit_expr(&expr.left);
            self.visit_expr(&expr.right);
        }
        fn visit_prefix_op(&mut self, expr: &PrefixOp<'_>) {
            self.visit_expr(&expr.expr);
        }
        fn visit_postfix_op(&mut self, expr: &PostfixOp<'_>) {
            self.visit_expr(&expr.expr);
        }
        fn visit_ternary(&mut self, expr: &Ternary<'_>) {
            self.visit_expr(&expr.condition);
            self.visit_expr(&expr.true_branch);
            self.visit_expr(&expr.false_branch);
        }
        fn visit_function_call(&mut self, expr: &FunctionCall<'_>) {
            self.visit_expr(&expr.func_expr);
            for arg in &expr.args {
                self.visit_expr(arg);
            }
        }
        fn visit_array_index(&mut self, expr: &ArrayIndex<'_>) {
            self.visit_expr(&expr.array);
            self.visit_expr(&expr.index);
        }
        fn visit_sizeof_type(&mut self, expr: &SizeOfType<'_>) {
            self.visit_type(&expr.typ);
        }
        fn visit_struct_init(&mut self, expr: &StructInit<'_>) {
            for (_name, expr) in &expr.field_inits {
                self.visit_expr(expr);
            }
        }
        fn visit_array_init(&mut self, expr: &ArrayInit<'_>) {
            for init in &expr.elements {
                self.visit_expr(init);
            }
        }
        fn visit_member_access(&mut self, expr: &MemberAccess<'_>) {
            self.visit_expr(&expr.struct_expr);
        }
        fn visit_pointer_member_access(&mut self, expr: &PointerMemberAccess<'_>) {
            self.visit_expr(&expr.struct_ptr_expr);
        }
        fn visit_assert(&mut self, stmt: &Assert<'_>) {
            self.visit_expr(&stmt.condition);
        }
        fn visit_break(&mut self, stmt: &Break) {
            _ = stmt;
        }
        fn visit_continue(&mut self, stmt: &Continue) {
            _ = stmt;
        }
        fn visit_block(&mut self, stmt: &Block<'_>) {
            for stmt in &stmt.body {
                self.visit_stmt(stmt);
            }
        }
        fn visit_if_stmt(&mut self, stmt: &IfStmt<'_>) {
            self.visit_expr(&stmt.condition);
            self.visit_stmt(&stmt.then_branch);
            if let Some(else_branch) = &stmt.else_branch {
                self.visit_stmt(else_branch);
            }
        }
        fn visit_while_loop(&mut self, stmt: &WhileLoop<'_>) {
            self.visit_expr(&stmt.condition);
            self.visit_stmt(&stmt.body);
        }
        fn visit_for_loop(&mut self, stmt: &ForLoop<'_>) {
            if let Some(init) = &stmt.init {
                self.visit_stmt(init);
            }
            if let Some(condition) = &stmt.condition {
                self.visit_expr(condition);
            }
            if let Some(post) = &stmt.post {
                self.visit_expr(post);
            }
            self.visit_stmt(&stmt.body);
        }
        fn visit_return(&mut self, stmt: &ReturnStmt<'_>) {
            if let Some(expr) = &stmt.value {
                self.visit_expr(expr);
            }
        }
        fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'_>) {
            self.visit_type(&decl.var_type);
            if let Some(init) = &decl.init_value {
                self.visit_expr(init);
            }
        }
        fn visit_struct_declaration(&mut self, decl: &StructDeclaration<'_>) {
            for (_name, typ) in &decl.fields {
                self.visit_type(typ);
            }
        }
        fn visit_function_declaration(&mut self, decl: &FunctionDeclaration<'_>) {
            if let Some(ret_type) = &decl.return_type {
                self.visit_type(ret_type);
            }
            for (_name, typ) in &decl.params {
                self.visit_type(typ);
            }
            self.visit_block(&decl.body);
        }
        fn visit_program(&mut self, program: &Program<'_>) {
            for decl in &program.decls {
                self.visit_global_declaration(decl);
            }
        }
        fn visit_error_expr(&mut self) {}
        fn visit_error_stmt(&mut self) {}
    }

    pub fn walk_expr<V: AstVisitor + ?Sized>(visitor: &mut V, expr: &Expr<'_>) {
        match expr.kind {
            ExprKind::Nullptr(nullptr) => visitor.visit_nullptr(nullptr),
            ExprKind::Cast(cast) => visitor.visit_cast(cast),
            ExprKind::Ident(ident) => visitor.visit_ident_expr(ident),
            ExprKind::Int(int) => visitor.visit_int(int),
            ExprKind::BinaryOp(binary_op) => visitor.visit_binary_op(binary_op),
            ExprKind::PrefixOp(prefix_op) => visitor.visit_prefix_op(prefix_op),
            ExprKind::PostfixOp(postfix_op) => visitor.visit_postfix_op(postfix_op),
            ExprKind::Ternary(ternary) => visitor.visit_ternary(ternary),
            ExprKind::FunctionCall(function_call) => visitor.visit_function_call(function_call),
            ExprKind::ArrayIndex(array_index) => visitor.visit_array_index(array_index),
            ExprKind::SizeOfType(size_of_type) => visitor.visit_sizeof_type(size_of_type),
            ExprKind::StructInit(struct_init) => visitor.visit_struct_init(struct_init),
            ExprKind::ArrayInit(array_init) => visitor.visit_array_init(array_init),
            ExprKind::MemberAccess(member_access) => visitor.visit_member_access(member_access),
            ExprKind::PointerMemberAccess(pointer_member_access) => {
                visitor.visit_pointer_member_access(pointer_member_access)
            }
            ExprKind::Error => visitor.visit_error_expr(),
        }
    }
    pub fn walk_stmt<V: AstVisitor + ?Sized>(visitor: &mut V, stmt: &Stmt<'_>) {
        match stmt.kind {
            StmtKind::Assert(assert) => visitor.visit_assert(assert),
            StmtKind::Break(break_) => visitor.visit_break(break_),
            StmtKind::Continue(continue_) => visitor.visit_continue(continue_),
            StmtKind::Block(block) => visitor.visit_block(block),
            StmtKind::IfStmt(if_stmt) => visitor.visit_if_stmt(if_stmt),
            StmtKind::WhileLoop(while_loop) => visitor.visit_while_loop(while_loop),
            StmtKind::ForLoop(for_loop) => visitor.visit_for_loop(for_loop),
            StmtKind::ReturnStmt(return_stmt) => visitor.visit_return(return_stmt),
            StmtKind::VariableDeclaration(variable_declaration) => {
                visitor.visit_variable_declaration(variable_declaration)
            }
            StmtKind::Expr(expr) => visitor.visit_expr(expr),
        }
    }
    pub fn walk_global_declaration<V: AstVisitor + ?Sized>(
        visitor: &mut V,
        decl: &GlobalDeclaration<'_>,
    ) {
        match &decl.kind {
            GlobalDeclarationKind::Variable(variable_declaration) => {
                visitor.visit_variable_declaration(variable_declaration)
            }
            GlobalDeclarationKind::Struct(struct_declaration) => {
                visitor.visit_struct_declaration(struct_declaration)
            }
            GlobalDeclarationKind::Function(function_declaration) => {
                visitor.visit_function_declaration(function_declaration)
            }
        }
    }
}
