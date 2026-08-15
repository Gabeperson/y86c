use ahash::AHashMap;
use tinyvec::TinyVec;

use crate::common::{span::Span, symbol::Symbol};

pub mod arenas {
    use super::*;
    use crate::common::interner::define_arena;
    define_arena!(Instruction, InstId, InstArena);
    define_arena!(Block, BlockId, BlockArena);
    define_arena!(Value, ValueId, ValueArena);
    define_arena!(StackSlot, StackSlotId, StackSlotArena);
    define_arena!(Type, TypeId, TypeArena; dedup);
    define_arena!(Provenance, ProvenanceId, ProvenanceArena; dedup);
}
pub use arenas::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I64,
    Ptr,
    FnPtr,
    Void,
    Memory,
}

#[derive(Debug, Clone, PartialEq, Copy, Eq, Hash)]
pub struct OpaqueFnReturn(pub InstId);
#[derive(Debug, Clone, PartialEq, Copy, Eq, Hash)]
pub struct TransparentFnReturn(pub InstId);
#[derive(Debug, Clone, PartialEq, Copy, Eq, Hash)]
pub struct NewProvInst(pub InstId);
#[derive(Debug, Clone, PartialEq, Copy, Eq, Hash)]
pub struct FunctionArg(pub ValueId);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Provenance {
    StackSlot(StackSlotId),
    NewProv(NewProvInst),
    TransparentReturn(ValueId),
    OpaqueReturn(OpaqueFnReturn),
    FunctionArg(FunctionArg),
    Global(Symbol),
    Exposed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKind {
    InstResult { inst: InstId, index: u16 },
    BlockArg { block: BlockId, index: u16 },
}

#[derive(Clone, Debug)]
pub struct BranchTarget {
    pub target: BlockId,
    pub args: TinyVec<[ValueId; 5]>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub params: TinyVec<[ValueId; 4]>,
    pub insts: Vec<InstId>,
    pub preds: TinyVec<[BlockId; 4]>,
    pub succs: TinyVec<[BlockId; 4]>,
    pub dbg_name: Option<Symbol>,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub struct StackSlot {
    pub size: u32,
    pub align: u32,
    pub dbg_name: Option<Symbol>,
    pub kind: StackSlotKind,
    pub frame_offset: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackSlotKind {
    AddressTakenLocal,
    // structs or arrs
    Aggregate,
    FnArgument,
    Spill,
}
#[derive(Debug, Clone)]
pub struct Value {
    pub typ: TypeId,
    pub kind: ValueKind,
    pub dbg_name: Option<Symbol>,
    pub uses: TinyVec<[InstId; 4]>,
}
#[derive(Debug, Clone)]
pub enum InstExtraData {
    ConstInt(i64),
    StackSlot(StackSlotId),
    Global(Symbol),
    Function(Symbol),
    Struct {
        sym: Symbol,
    },
    StructField {
        struct_sym: Symbol,
        field: Symbol,
    },
    ElementSize {
        size: u32,
    },
    Branch {
        then_target: BranchTarget,
        else_target: BranchTarget,
    },
    Jump {
        target: BranchTarget,
    },
}
#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: Opcode,
    pub block: BlockId,
    pub span: Span,
    pub operands: TinyVec<[ValueId; 2]>,
    pub results: TinyVec<[ValueId; 2]>,
    pub extra: Option<InstExtraData>,
}

#[derive(Debug, Clone, Copy)]
pub enum Opcode {
    GetStackAddr,
    FieldAddr,
    IndexAddr,

    Jmp,
    Branch,
    LoadConst,
    Load,
    Store,
    Return,
    Call,
    IndirectCall,
    CopyProvenance,
    ExposeProvenance,
    UnexposeProvenance,
    NewProvenance,
    // Technically this is same as arraycopy (which is memcpy/memmove) but
    // can be optimized in later stages from memcpy/memmove to
    // "load each field and write each field" which gets rid of
    // function call/return overhead and copying padding bytes
    StructCopy,
    ArrayCopy,
    LoadGlobalLoc,
    Nop,
    BitCast,
    PtrAdd,
    Select,

    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    And,
    Or,
    BitAnd,
    BitOr,
    Xor,
    Mod,
    Assign,
    Neg,
    BitNot,
    Not,
    Shl,
    Lshr,
    AShr,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum FunctionParamKind {
    HiddenPtr,
    #[default]
    Arg,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum FunctionParamType {
    #[default]
    Int,
    Ptr,
    FnPtr,
    Aggregate {
        size: u32,
        align: u32,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FunctionParam {
    pub kind: FunctionParamKind,
    pub typ: FunctionParamType,
}

#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub params: TinyVec<[FunctionParam; 5]>,
    pub ret: TypeId,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: Symbol,
    pub sig: FunctionSignature,
    pub entry: BlockId,
    pub span: Span,

    pub blocks: BlockArena,
    pub stack_slots: StackSlotArena,
    pub insts: InstArena,
    pub values: ValueArena,
    pub provenances: ProvenanceArena,
    pub value_provenances: AHashMap<ValueId, ProvenanceId>,
}
