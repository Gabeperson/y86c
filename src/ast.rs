use crate::span::Span;
use bumpalo::collections::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind<'a> {
    Void,
    Int(IntType),
    Ptr {
        pointee: Type<'a>,
    },
    Struct {
        name: Ident<'a>,
    },
    Array {
        element_type: Type<'a>,
        size: u64,
    },
    FuncPtr {
        return_type: Option<Type<'a>>,
        param_types: Vec<'a, Type<'a>>,
    },
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type<'a> {
    pub inner: &'a TypeKind<'a>,
    pub span: Span,
}

impl<'a> Type<'a> {
    pub fn new(inner: &'a TypeKind<'a>, span: Span) -> Self {
        Self { inner, span }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr<'a> {
    pub kind: &'a ExprKind<'a>,
    pub span: Span,
}

impl<'a> Expr<'a> {
    pub fn new(kind: &'a ExprKind<'a>, span: Span) -> Self {
        Self { kind, span }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nullptr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cast<'a> {
    pub to_type: Type<'a>,
    pub expr: Expr<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident<'a> {
    pub ident: &'a str,
    pub span: Span,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum IntLiteralKind {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Error,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct IntLiteral {
    pub kind: IntLiteralKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Int {
    pub lit: IntLiteral,
    pub span: Span,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryOp<'a> {
    pub kind: BinaryOpKind,
    pub left: Expr<'a>,
    pub right: Expr<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixOp<'a> {
    pub kind: PrefixOpKind,
    pub expr: Expr<'a>,
    pub span: Span,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostfixOpKind {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixOp<'a> {
    pub kind: PostfixOpKind,
    pub expr: Expr<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ternary<'a> {
    pub condition: Expr<'a>,
    pub true_branch: Expr<'a>,
    pub false_branch: Expr<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCall<'a> {
    pub func_expr: Expr<'a>,
    pub args: Vec<'a, Expr<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayIndex<'a> {
    pub array: Expr<'a>,
    pub index: Expr<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeOfType<'a> {
    pub typ: Type<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructInit<'a> {
    pub name: Ident<'a>,
    pub field_inits: Vec<'a, (Ident<'a>, Expr<'a>)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayInit<'a> {
    pub elements: Vec<'a, Expr<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberAccess<'a> {
    pub struct_expr: Expr<'a>,
    pub member_name: Ident<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerMemberAccess<'a> {
    pub struct_ptr_expr: Expr<'a>,
    pub member_name: Ident<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stmt<'a> {
    pub kind: &'a StmtKind<'a>,
    pub span: Span,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assert<'a> {
    pub condition: Expr<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Break {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continue {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<'a> {
    pub body: Vec<'a, Stmt<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt<'a> {
    pub condition: Expr<'a>,
    pub then_branch: Stmt<'a>,
    pub else_branch: Option<Stmt<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileLoop<'a> {
    pub condition: Expr<'a>,
    pub body: Stmt<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForLoop<'a> {
    pub init: Option<Stmt<'a>>,
    pub condition: Option<Expr<'a>>,
    pub post: Option<Expr<'a>>,
    pub body: Stmt<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt<'a> {
    pub value: Option<Expr<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDeclaration<'a> {
    pub var_type: Type<'a>,
    pub name: Ident<'a>,
    pub init_value: Option<Expr<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDeclaration<'a> {
    pub name: Ident<'a>,
    pub fields: Vec<'a, (Ident<'a>, Type<'a>)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration<'a> {
    pub return_type: Option<Type<'a>>,
    pub name: Ident<'a>,
    pub params: Vec<'a, (Ident<'a>, Type<'a>)>,
    pub body: Block<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDeclaration<'a> {
    pub kind: GlobalDeclarationKind<'a>,
    pub span: Span,
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
