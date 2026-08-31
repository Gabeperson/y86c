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

#[derive(Clone, Debug, Copy, PartialEq, Eq, Hash)]
pub enum CallKind {
    Internal,
    Abi,
}

#[derive(Clone, Debug)]
pub struct TypeContext {
    types: TypeArena,
    structs: StructArena,
    ptr: TypeId,
    i64: TypeId,
    mem: TypeId,
    fnptr: TypeId,
    void: TypeId,
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeContext {
    pub fn new() -> Self {
        let mut types = TypeArena::new();
        let ptr = types.intern_deduplicated(Type::Ptr);
        let i64 = types.intern_deduplicated(Type::I64);
        let mem = types.intern_deduplicated(Type::Memory);
        let fnptr = types.intern_deduplicated(Type::FnPtr);
        let void = types.intern_deduplicated(Type::Void);
        Self {
            types,
            structs: StructArena::new(),
            ptr,
            i64,
            mem,
            fnptr,
            void,
        }
    }
    pub fn intern_type(&mut self, typ: Type) -> TypeId {
        self.types.intern_deduplicated(typ)
    }
    #[track_caller]
    pub fn get_type(&self, id: TypeId) -> &Type {
        self.types.get(id)
    }
    pub fn intern_struct(&mut self, s: StructInfo) -> StructId {
        self.structs.intern_deduplicated(s)
    }
    pub fn get_struct(&self, id: StructId) -> &StructInfo {
        self.structs.get(id)
    }
    pub fn ptr_typ(&self) -> TypeId {
        self.ptr
    }
    pub fn i64_typ(&self) -> TypeId {
        self.i64
    }
    pub fn mem_typ(&self) -> TypeId {
        self.mem
    }
    pub fn fnptr_typ(&self) -> TypeId {
        self.fnptr
    }
    pub fn void_typ(&self) -> TypeId {
        self.void
    }
    pub fn type_size(&self, type_id: TypeId) -> u64 {
        let typ = self.get_type(type_id);
        match typ {
            Type::I64 => 8,
            Type::Ptr => 8,
            Type::FnPtr => 8,
            Type::Void => 1,
            Type::Memory => unreachable!(),
            Type::Struct(struct_id) => {
                let struct_info = self.get_struct(*struct_id);
                struct_info.size
            }
            Type::Array { element, len } => self.type_size(*element) * *len,
        }
    }
    pub fn type_align(&self, type_id: TypeId) -> u64 {
        let typ = self.get_type(type_id);
        match typ {
            Type::I64 => 8,
            Type::Ptr => 8,
            Type::FnPtr => 8,
            Type::Void => 1,
            Type::Memory => unreachable!(),
            Type::Struct(struct_id) => {
                let struct_info = self.get_struct(*struct_id);
                struct_info.align
            }
            Type::Array { element, .. } => self.type_align(*element),
        }
    }
}

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
    NoaliasArg(ValueId),
    NewProv(NewProvInst),
    NoAliasFnReturn(ValueId),
    Global(Symbol),
    Wildcard,
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
    Aggregate,
    FnArgument { typ: TypeId, idx: u32 },
    IntermediateAggregate,
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
    // LoadConst
    ConstInt(i64),
    // GetStackAddr
    StackSlot(StackSlotId),
    // LoadGlobalLoc
    Global(Symbol),
    // Call
    Function(Symbol),
    // IndirectCall
    ICallKind(CallKind),
    // Memcpy/bitcast
    ElementType(TypeId),
    // IndexAddr
    IndexAddrData {
        typ: TypeId,
        forward: bool,
    },
    // Param
    ParamIndex {
        index: u32,
    },
    // FieldAddr
    StructField {
        struct_sym: Symbol,
        member_sym: Symbol,
        struct_id: StructId,
        field: u32,
    },
    // Branch
    Branch {
        then_target: BlockId,
        else_target: BlockId,
    },
    // Jmp
    Jump {
        target: BlockId,
    },
    // Phi
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    GetStackAddr,
    FieldAddr,
    IndexAddr,

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
    Memcpy,
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
}
