use indexmap::set::Intersection;
use tinyvec::TinyVec;

use crate::{arena::*, span::Span};

#[derive(Clone, Debug)]
pub struct AstCtx {
    type_arena: Arena<TypeKind>,
}

#[derive(Debug, Clone, Copy)]
pub enum IntWidth {
    I8,
    I16,
    I32,
    I64,
}
#[derive(Debug, Clone)]
pub enum TypeKind {
    Void,
    Int(IntWidth),
    Ptr {
        pointee: Id<Type>,
    },
    Struct {
        name: InternedSymbol,
    },
    Array {
        element_type: Id<Type>,
        size: u64,
    },
    FuncPtr {
        return_type: Option<Id<Type>>,
        param_types: Vec<Id<Type>>,
    },
}

#[derive(Debug, Clone)]
pub struct Type {
    pub inner: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub typ: Id<Type>,
}

#[derive(Debug, Clone)]
pub enum ExprKind {}

#[derive(Debug, Clone)]
pub struct Nullptr;

#[derive(Debug, Clone)]
pub struct Cast {
    pub to_type: Id<Type>,
    pub expr: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub ident: InternedSymbol,
}

#[derive(Clone, Debug, Copy)]
pub enum NumberLiteralKind {
    // I8(i8),
    // U8(u8),
    // I16(i16),
    // U16(u16),
    // I32(i32),
    // U32(u32),
    I64(i64),
    // U64(u64),
    // F64(f64),
}

#[derive(Clone, Debug, Copy)]
pub struct NumberLiteral {
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Number {
    pub lit: NumberLiteral,
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
    AddEq,
    SubEq,
    MulEq,
    DivEq,
    BitAndEqual,
    BitOrEqual,
    XorEqual,
    ModEqual,
    Assign,
}
#[derive(Debug, Clone)]
pub struct BinaryOp {
    left: Id<Expr>,
    right: Id<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefixOpKind {
    PrefixIncrement,
    PrefixDecrement,
    UnaryPlus,
    UnaryMinus,
    AddressOf,
    Dereference,
    Not,
}

#[derive(Debug, Clone)]
pub struct PrefixOp {
    kind: PrefixOpKind,
    expr: Id<Expr>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostfixOpKind {
    PostfixIncrement,
    PostfixDecrement,
}

#[derive(Debug, Clone)]
pub struct PostfixOp {
    kind: PrefixOpKind,
    expr: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Ternary {
    condition: Id<Expr>,
    true_branch: Id<Expr>,
    false_branch: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    func_expr: Id<Expr>,
    args: Vec<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct ArrayIndex {
    array: Id<Expr>,
    index: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct SizeOfType {
    typ: Id<Type>,
}

#[derive(Debug, Clone)]
pub struct StructInit {
    name: InternedSymbol,
    field_inits: Vec<(InternedSymbol, Id<Expr>)>,
}

#[derive(Debug, Clone)]
pub struct ArrayInit {
    elements: Vec<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct MemberAccess {
    struct_expr: Id<Expr>,
    member_name: InternedSymbol,
}

#[derive(Debug, Clone)]
pub struct PointerMemberAccess {
    struct_ptr_expr: Id<Expr>,
    member_name: InternedSymbol,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {}

#[derive(Debug, Clone)]
pub struct Assert {
    condition: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Break;

#[derive(Debug, Clone)]
pub struct Continue;

#[derive(Debug, Clone)]
pub struct Block {
    body: Vec<Id<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    condition: Id<Expr>,
    then_branch: Id<Stmt>,
    else_branch: Option<Id<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct WhileLoop {
    condition: Id<Expr>,
    body: Id<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ForLoop {
    init: Option<Id<Stmt>>,
    condition: Option<Id<Expr>>,
    post: Option<Id<Stmt>>,
    body: Id<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    value: Option<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    var_type: Id<Type>,
    name: InternedSymbol,
    init_value: Option<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct StructDeclaration {
    name: InternedSymbol,
    fields: Vec<(InternedSymbol, Id<Type>)>,
}

#[derive(Debug, Clone)]
pub struct FunctionDeclaration {
    return_type: Option<Id<Type>>,
    name: InternedSymbol,
    params: Vec<(InternedSymbol, Id<Type>)>,
    body: (Block, Span),
}
