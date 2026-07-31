use crate::{arena::*, span::Span};

#[derive(Clone, Debug)]
pub struct AstArenas {
    pub type_arena: Arena<TypeKind>,
    pub expr_arena: Arena<Expr>,
    pub stmt_arena: Arena<Stmt>,
}

impl AstArenas {
    pub fn new() -> Self {
        AstArenas {
            type_arena: Arena::new(),
            expr_arena: Arena::new(),
            stmt_arena: Arena::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
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
#[derive(Debug, Clone)]
pub enum TypeKind {
    Void,
    Int(IntType),
    Ptr {
        pointee: Id<Type>,
    },
    Struct {
        name: InternedString,
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
    pub typ: Id<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub struct Nullptr;

#[derive(Debug, Clone)]
pub struct Cast {
    pub to_type: Id<Type>,
    pub expr: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub ident: InternedString,
}

#[derive(Clone, Debug, Copy)]
pub enum IntLiteralKind {
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
}

#[derive(Clone, Debug, Copy)]
pub struct IntLiteral {
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Int {
    pub lit: IntLiteral,
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
    pub kind: BinaryOpKind,
    pub left: Id<Expr>,
    pub right: Id<Expr>,
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
    pub kind: PrefixOpKind,
    pub expr: Id<Expr>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostfixOpKind {
    PostfixIncrement,
    PostfixDecrement,
}

#[derive(Debug, Clone)]
pub struct PostfixOp {
    pub kind: PrefixOpKind,
    pub expr: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Ternary {
    pub condition: Id<Expr>,
    pub true_branch: Id<Expr>,
    pub false_branch: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub func_expr: Id<Expr>,
    pub args: Vec<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct ArrayIndex {
    pub array: Id<Expr>,
    pub index: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct SizeOfType {
    pub typ: Id<Type>,
}

#[derive(Debug, Clone)]
pub struct StructInit {
    pub name: InternedString,
    pub field_inits: Vec<(InternedString, Id<Expr>)>,
}

#[derive(Debug, Clone)]
pub struct ArrayInit {
    pub elements: Vec<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct MemberAccess {
    pub struct_expr: Id<Expr>,
    pub member_name: InternedString,
}

#[derive(Debug, Clone)]
pub struct PointerMemberAccess {
    pub struct_ptr_expr: Id<Expr>,
    pub member_name: InternedString,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
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
}

#[derive(Debug, Clone)]
pub struct Assert {
    pub condition: Id<Expr>,
}

#[derive(Debug, Clone)]
pub struct Break;

#[derive(Debug, Clone)]
pub struct Continue;

#[derive(Debug, Clone)]
pub struct Block {
    pub body: Vec<Id<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: Id<Expr>,
    pub then_branch: Id<Stmt>,
    pub else_branch: Option<Id<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct WhileLoop {
    pub condition: Id<Expr>,
    pub body: Id<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ForLoop {
    pub init: Option<Id<Stmt>>,
    pub condition: Option<Id<Expr>>,
    pub post: Option<Id<Stmt>>,
    pub body: Id<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub value: Option<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    pub var_type: Id<Type>,
    pub name: InternedString,
    pub init_value: Option<Id<Expr>>,
}

#[derive(Debug, Clone)]
pub struct StructDeclaration {
    pub name: InternedString,
    pub fields: Vec<(InternedString, Id<Type>)>,
}

#[derive(Debug, Clone)]
pub struct FunctionDeclaration {
    pub return_type: Option<Id<Type>>,
    pub name: InternedString,
    pub params: Vec<(InternedString, Id<Type>)>,
    pub body: (Block, Span),
}

#[derive(Debug, Clone)]
pub struct GlobalDeclaration {
    pub kind: GlobalDeclarationKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum GlobalDeclarationKind {
    Variable(VariableDeclaration),
    Struct(StructDeclaration),
    Function(FunctionDeclaration),
}

#[derive(Debug, Clone)]
pub struct Program {
    pub decls: Vec<GlobalDeclaration>,
    pub arenas: AstArenas,
    pub span: Span,
}
