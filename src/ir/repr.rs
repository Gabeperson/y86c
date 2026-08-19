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
    define_arena!(Provenance, ProvenanceId, ProvenanceArena; dedup);
    impl InstId {
        fn invalid() -> Self {
            Self::default()
        }
    }
}
pub use arenas::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
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

#[derive(Clone, Debug)]
pub struct BranchTarget {
    pub target: BlockId,
}

#[derive(Clone, Debug, Default)]
pub struct PhiOperand {
    pub block: BlockId,
    pub value: ValueId,
}

#[derive(Debug, Clone)]
pub struct Block {
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
    pub undef: bool,
    pub typ: Type,
    pub inst: InstId,
    pub index: u32,
    pub dbg_name: Option<Symbol>,
    pub uses: TinyVec<[InstId; 4]>,
}

impl Value {
    pub fn add_use(&mut self, id: InstId) {
        self.uses.push(id);
    }
    pub fn remove_use(&mut self, id: InstId) {
        let pos = self
            .uses
            .iter()
            .rposition(|inst| *inst == id)
            .expect("Remove use called with non-used?");
        self.uses.remove(pos);
    }
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
    Phi {
        operands: TinyVec<[PhiOperand; 4]>,
    },
    None,
}

impl InstExtraData {
    pub fn as_phi_args(&mut self) -> Option<&mut TinyVec<[PhiOperand; 4]>> {
        if let InstExtraData::Phi { operands: args } = self {
            Some(args)
        } else {
            None
        }
    }
    pub fn as_jmp_target(&mut self) -> Option<&mut BranchTarget> {
        if let InstExtraData::Jump { target } = self {
            Some(target)
        } else {
            None
        }
    }
    pub fn as_branch_targets(&mut self) -> Option<(&mut BranchTarget, &mut BranchTarget)> {
        if let InstExtraData::Branch {
            then_target,
            else_target,
        } = self
        {
            Some((then_target, else_target))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub op: Opcode,
    pub block: BlockId,
    pub span: Span,
    pub operands: TinyVec<[ValueId; 2]>,
    pub results: TinyVec<[ValueId; 2]>,
    pub extra: InstExtraData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    GetStackAddr,
    FieldAddr,
    IndexAddr,

    Phi,
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
    Shr,
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
    pub ret: Type,
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
