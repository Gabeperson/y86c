use ahash::AHashMap;
use tinyvec::{TinyVec, tiny_vec};

use crate::common::{CallingConvention, Inline, span::Span, symbol::Symbol};

pub mod arenas {
    use super::*;
    use crate::common::interner::define_arena;
    define_arena!(Instruction, InstId, InstArena);
    define_arena!(Block, BlockId, BlockArena);
    define_arena!(Value, ValueId, ValueArena);
    define_arena!(StackSlot, StackSlotId, StackSlotArena);
    define_arena!(Provenance, ProvenanceId, ProvenanceArena; dedup);
    define_arena!(StructInfo, StructId, StructArena; dedup);
    define_arena!(Type, TypeId, TypeArena; dedup);
}
pub use arenas::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum Type {
    I64,
    Ptr,
    FnPtr,
    Void,
    Memory,
    Struct(StructId),
    Array { element: TypeId, len: u64 },
}

impl Type {
    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Type::Array { .. })
    }
    pub fn is_ptr(&self) -> bool {
        matches!(self, Type::Ptr)
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct StructInfo {
    pub size: u64,
    pub align: u64,
    pub offsets: TinyVec<[u64; 4]>,
    pub field_types: TinyVec<[TypeId; 4]>,
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
}

impl Block {
    pub fn new(sym: Symbol) -> Self {
        Self {
            insts: Vec::new(),
            preds: TinyVec::new(),
            succs: TinyVec::new(),
            dbg_name: Some(sym),
        }
    }
    pub fn add_pred(&mut self, pred: BlockId) {
        if !self.preds.contains(&pred) {
            self.preds.push(pred)
        }
    }
    pub fn add_succ(&mut self, succ: BlockId) {
        if !self.succs.contains(&succ) {
            self.succs.push(succ)
        }
    }
}

#[derive(Debug, Clone)]
pub struct StackSlot {
    pub size: u64,
    pub align: u64,
    pub dbg_name: Option<Symbol>,
    pub kind: StackSlotKind,
    pub frame_offset: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackSlotKind {
    AddressTakenLocal,
    Array,
    FnArgument { typ: TypeId },
    Spill,
}

#[derive(Debug, Clone)]
pub struct Value {
    pub undef: bool,
    pub typ: TypeId,
    pub inst: InstId,
    pub index: u32,
    pub dbg_name: Option<Symbol>,
    pub uses: TinyVec<[InstId; 4]>,
}

impl Value {
    pub fn new(typ: TypeId, inst: InstId, index: u32, dbg_name: Option<Symbol>) -> Self {
        Self {
            undef: false,
            typ,
            inst,
            index,
            dbg_name,
            uses: tiny_vec![[InstId; 4] => inst],
        }
    }
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
    ElementType(TypeId),
    ParamIndex {
        index: u32,
    },
    Struct {
        sym: Symbol,
    },
    StructField {
        struct_sym: Symbol,
        field: u32,
    },
    ElementInfo {
        size: u64,
        align: u64,
    },
    Branch {
        then_target: BlockId,
        else_target: BlockId,
    },
    Jump {
        target: BlockId,
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
    pub fn as_jmp_target(&mut self) -> Option<&mut BlockId> {
        if let InstExtraData::Jump { target } = self {
            Some(target)
        } else {
            None
        }
    }
    pub fn as_branch_targets(&mut self) -> Option<(&mut BlockId, &mut BlockId)> {
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

impl Instruction {
    pub fn new_jmp(block: BlockId, span: Span, jmp_target: BlockId) -> Self {
        Instruction {
            op: Opcode::Jmp,
            block,
            span,
            operands: TinyVec::new(),
            results: TinyVec::new(),
            extra: InstExtraData::Jump { target: jmp_target },
        }
    }
    pub fn new_branch(
        block: BlockId,
        span: Span,
        cond: ValueId,
        true_target: BlockId,
        false_target: BlockId,
    ) -> Self {
        Instruction {
            op: Opcode::Jmp,
            block,
            span,
            operands: tiny_vec![{ cond }],
            results: TinyVec::new(),
            extra: InstExtraData::Branch {
                then_target: true_target,
                else_target: false_target,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    GetStackAddr,
    FieldAddr,
    IndexAddr,
    ExtractValue,
    InsertValue,
    MakeStruct,

    Assert,
    Param,
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
    ArrayCopy,
    LoadGlobalLoc,
    Nop,
    BitCast,
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
pub struct FunctionParam {
    pub kind: FunctionParamKind,
    pub typ: TypeId,
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
    pub inline: Inline,
    pub cc: CallingConvention,

    pub blocks: BlockArena,
    pub stack_slots: StackSlotArena,
    pub insts: InstArena,
    pub values: ValueArena,
    pub provenances: ProvenanceArena,
    pub value_provenances: AHashMap<ValueId, ProvenanceId>,
    pub structs: StructArena,
    pub types: TypeArena,
}

impl Function {
    pub fn type_size(&self, type_id: TypeId) -> u64 {
        let typ = self.types.get(type_id);
        match typ {
            Type::I64 => 8,
            Type::Ptr => 8,
            Type::FnPtr => 8,
            Type::Void => 1,
            Type::Memory => unreachable!(),
            Type::Struct(struct_id) => {
                let struct_info = self.structs.get(*struct_id);
                struct_info.size
            }
            Type::Array { element, len } => self.type_size(*element) * *len,
        }
    }
}
