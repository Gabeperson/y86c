use ahash::AHashMap;
use ahash::AHashSet;

use crate::common::CallingConvention;
use crate::common::Inline;
use crate::common::symbol::Symbol;
use crate::common::symbol::SymbolArena;
use crate::ir::repr::*;
use std::fmt::Result as FmtResult;
use std::fmt::Write;

struct Formatter<'a> {
    writer: &'a mut dyn Write,
}

impl<'a> std::fmt::Write for Formatter<'a> {
    fn write_str(&mut self, s: &str) -> FmtResult {
        self.writer.write_str(s)
    }
    fn write_char(&mut self, c: char) -> FmtResult {
        self.writer.write_char(c)
    }
    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> FmtResult {
        self.writer.write_fmt(args)
    }
}

pub struct IrPrinter<'a> {
    typectx: &'a TypeContext,
    symbols: &'a SymbolArena,
}

impl<'a> IrPrinter<'a> {
    pub fn new(typectx: &'a TypeContext, symbols: &'a SymbolArena) -> IrPrinter<'a> {
        Self { typectx, symbols }
    }
}

struct Ctx<'a> {
    blocks: &'a BlockArena,
    stack_slots: &'a StackSlotArena,
    insts: &'a InstArena,
    values: &'a ValueArena,
    provenances: &'a ProvenanceArena,
    value_provenances: &'a AHashMap<ValueId, ProvenanceId>,
}

struct Counter {
    map: AHashMap<ValueId, usize>,
    counters_map: AHashMap<Symbol, usize>,
    counter: usize,
}

impl Counter {
    fn new() -> Self {
        Self {
            map: AHashMap::new(),
            counters_map: AHashMap::new(),
            counter: 0,
        }
    }
    fn dbg_counter_get(&mut self, id: ValueId, sym: Symbol) -> usize {
        if let Some(count) = self.map.get(&id) {
            return *count;
        }
        let val = self.counters_map.entry(sym).or_default();
        let val = std::mem::replace(val, *val + 1);
        self.map.insert(id, val);
        val
    }
    fn counter_get(&mut self, id: ValueId) -> usize {
        if let Some(count) = self.map.get(&id) {
            return *count;
        }
        let prev = self.counter;
        self.counter += 1;
        self.map.insert(id, prev);
        prev
    }
}

impl<'a> IrPrinter<'a> {
    pub fn print(&self, func: &Function) -> Result<String, std::fmt::Error> {
        let mut out = String::new();
        let mut formatter = Formatter { writer: &mut out };
        self.fmt_function(func, &mut formatter)?;
        Ok(out)
    }
    fn fmt_sym(&self, sym: Symbol, fmt: &mut Formatter<'_>) -> FmtResult {
        write!(fmt, "{}", self.symbols.get(sym))
    }
    fn fmt_function(&self, func: &Function, f: &mut Formatter<'_>) -> FmtResult {
        self.fmt_cc(func.cc, f)?;
        self.fmt_inline(func.inline, f)?;
        let ctx = &get_func_ctx(func);
        write!(f, "fn ")?;
        self.fmt_sym(func.name, f)?;
        write!(f, "(")?;
        let mut counter = Counter::new();
        let mut write_separator = false;
        for param in &func.sig.params {
            if write_separator {
                write!(f, ", ")?;
            }
            write_separator = true;
            if let FunctionParamKind::HiddenPtr = param.kind {
                write!(f, "sret: ")?;
            }
            self.fmt_type(param.typ, f)?;
        }
        write!(f, ") -> ")?;
        self.fmt_type(func.sig.ret, f)?;
        writeln!(f, " {{")?;
        // TODO stackslots?
        let order = block_order(func.entry, ctx.blocks);
        for block_id in order {
            self.fmt_block(block_id, f, ctx, &mut counter)?;
        }
        write!(f, "}}")?;
        Ok(())
    }
    fn fmt_cc(&self, cc: CallingConvention, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "@[cc: ")?;
        let () = match cc {
            CallingConvention::Internal => write!(f, "internal"),
            CallingConvention::Abi => write!(f, "abi"),
        }?;
        writeln!(f, "]")?;
        Ok(())
    }
    fn fmt_inline(&self, inline: Inline, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "@[inline: ")?;
        let () = match inline {
            Inline::Auto => write!(f, "auto"),
            Inline::Always => write!(f, "always"),
            Inline::Never => write!(f, "never"),
        }?;
        writeln!(f, "]")?;
        Ok(())
    }
    fn fmt_block(
        &self,
        id: BlockId,
        f: &mut Formatter<'_>,
        ctx: &Ctx,
        counter: &mut Counter,
    ) -> FmtResult {
        let block = ctx.blocks.get(id);
        write!(f, "bb_{}", id.get())?;
        if let Some(sym) = block.dbg_name {
            let s = self.symbols.get(sym);
            write!(f, " ({s})")?;
        }
        writeln!(f, ":")?;
        for inst in block.insts.iter().copied() {
            self.fmt_inst(inst, f, ctx, counter)?;
        }
        Ok(())
    }
    fn fmt_inst(
        &self,
        id: InstId,
        f: &mut Formatter<'_>,
        ctx: &Ctx,
        counter: &mut Counter,
    ) -> FmtResult {
        let inst = ctx.insts.get(id);
        write!(f, "    ")?;
        if !inst.results.is_empty() {
            let mut sep = false;
            for res in inst.results.iter().copied() {
                if sep {
                    write!(f, ", ")?;
                }
                sep = true;
                self.fmt_value(res, f, ctx, counter)?;
            }
            write!(f, " (")?;
            sep = false;
            for res in inst.results.iter().copied() {
                if sep {
                    write!(f, ", ")?;
                }
                sep = true;
                let val = ctx.values.get(res);
                self.fmt_type(val.typ, f)?;
            }
            write!(f, ") = ")?;
        }
        self.fmt_opcode(inst.op, f)?;
        let mut sep = false;
        for op in inst.operands.iter().copied() {
            if sep {
                write!(f, ", ")?;
            } else {
                write!(f, " ")?;
            }
            sep = true;
            self.fmt_value(op, f, ctx, counter)?;
        }
        self.fmt_inst_extra(&inst.extra, f, ctx, counter)?;
        for res in &inst.results {
            if let Some(prov) = ctx.value_provenances.get(res) {
                self.fmt_provenance(*prov, f, ctx)?;
            }
        }
        writeln!(f)
    }
    fn fmt_value(
        &self,
        id: ValueId,
        f: &mut Formatter<'_>,
        ctx: &Ctx,
        counter: &mut Counter,
    ) -> FmtResult {
        let val = ctx.values.get(id);
        let is_mem = matches!(self.typectx.get_type(val.typ), Type::Memory);
        if is_mem && val.undef {
            write!(f, "%memstart")?;
        } else if let Some(name) = val.dbg_name {
            let counter = counter.dbg_counter_get(id, name);
            let s = self.symbols.get(name);
            write!(f, "%{s}.{counter}")?;
        } else {
            let counter = counter.counter_get(id);
            write!(f, "%{counter}")?;
        }
        Ok(())
    }
    fn fmt_stackslot(&self, id: StackSlotId, f: &mut Formatter<'_>, ctx: &Ctx) -> FmtResult {
        write!(f, "[stackslot ({}): ", id.get())?;
        let stackslot = ctx.stack_slots.get(id);
        let () = match stackslot.kind {
            StackSlotKind::AddressTakenLocal => write!(f, "addr_taken_local"),
            StackSlotKind::Aggregate => write!(f, "aggregate_local"),
            StackSlotKind::FnArgument { idx, .. } => write!(f, "fn_arg({idx})"),
            StackSlotKind::IntermediateAggregate => write!(f, "intermediate_aggregate"),
        }?;
        write!(f, "]")?;
        Ok(())
    }
    fn fmt_provenance(&self, id: ProvenanceId, f: &mut Formatter<'_>, ctx: &Ctx) -> FmtResult {
        let prov = ctx.provenances.get(id);
        write!(f, "[prov: ")?;
        let () = match prov {
            Provenance::StackSlot(stack_slot_id) => {
                write!(f, "stackslot({})", stack_slot_id.get())
            }
            Provenance::NoaliasArg(_value_id) => {
                write!(f, "noalias_arg")
            }
            Provenance::NewProv(_new_prov_inst) => {
                write!(f, "new_prov")
            }
            Provenance::NoAliasFnReturn(_value_id) => {
                write!(f, "noalias_fn_return")
            }
            Provenance::Global(symbol) => {
                let s = self.symbols.get(*symbol);
                write!(f, "global({s})")
            }
            Provenance::Wildcard => write!(f, "wildcard"),
        }?;
        write!(f, "]")?;
        Ok(())
    }
    fn fmt_inst_extra(
        &self,
        extra: &InstExtraData,
        f: &mut Formatter<'_>,
        ctx: &Ctx,
        counter: &mut Counter,
    ) -> FmtResult {
        write!(f, " ")?;
        let () = match extra {
            InstExtraData::ConstInt(val) => write!(f, "[const: {val}]"),
            InstExtraData::StackSlot(stack_slot_id) => self.fmt_stackslot(*stack_slot_id, f, ctx),
            InstExtraData::Global(symbol) => {
                let s = self.symbols.get(*symbol);
                write!(f, "[global: {s}]")
            }
            InstExtraData::ElementType(type_id) => {
                write!(f, "[elem_type: ")?;
                self.fmt_type(*type_id, f)?;
                write!(f, "]")
            }
            InstExtraData::IndexAddrData { typ, forward } => {
                write!(f, "[elem_type: ")?;
                self.fmt_type(*typ, f)?;
                if !*forward {
                    write!(f, "; neg")?;
                }
                write!(f, "]")
            }
            InstExtraData::ParamIndex { index } => {
                write!(f, "[idx: {index}]")
            }
            InstExtraData::StructField {
                struct_sym,
                field,
                member_sym,
                struct_id,
            } => {
                let struct_ = self.symbols.get(*struct_sym);
                let member = self.symbols.get(*member_sym);
                write!(
                    f,
                    "[field: {struct_}.{member} ({}).({})]",
                    struct_id.get(),
                    field
                )
            }
            InstExtraData::Branch {
                then_target,
                else_target,
            } => {
                write!(
                    f,
                    "[targets: bb_{}, bb_{}]",
                    then_target.get(),
                    else_target.get()
                )
            }
            InstExtraData::Jump { target } => {
                write!(f, "[target: bb_{}]", target.get())
            }
            InstExtraData::Phi { operands } => {
                write!(f, "[")?;
                let mut sep = false;
                for op in operands {
                    if sep {
                        write!(f, ", ")?;
                    }
                    sep = true;
                    write!(f, "(")?;
                    self.fmt_value(op.value, f, ctx, counter)?;
                    write!(f, ", bb_{})", op.block.get())?;
                }
                write!(f, "]")
            }
            InstExtraData::None => Ok(()),
        }?;
        Ok(())
    }
    fn fmt_type(&self, id: TypeId, f: &mut Formatter<'_>) -> FmtResult {
        let typ = self.typectx.get_type(id);
        let () = match typ {
            Type::I64 => write!(f, "i64"),
            Type::Ptr => write!(f, "ptr"),
            Type::FnPtr => write!(f, "fnptr"),
            Type::Void => write!(f, "void"),
            Type::Memory => write!(f, "mem"),
            Type::Struct(struct_id) => write!(f, "struct{}", struct_id.get()),
            Type::Array { element, len } => {
                write!(f, "[")?;
                self.fmt_type(*element, f)?;
                write!(f, "; {len}]")?;
                Ok(())
            }
        }?;
        Ok(())
    }

    fn fmt_opcode(&self, op: Opcode, f: &mut Formatter<'_>) -> FmtResult {
        let () = match op {
            Opcode::GetStackAddr => write!(f, "get_stack_addr"),
            Opcode::FieldAddr => write!(f, "field_addr"),
            Opcode::IndexAddr => write!(f, "index_addr"),
            Opcode::Assert => write!(f, "assert"),
            Opcode::Param => write!(f, "param"),
            Opcode::Phi => write!(f, "phi"),
            Opcode::Jmp => write!(f, "jmp"),
            Opcode::Branch => write!(f, "branch"),
            Opcode::LoadConst => write!(f, "load_const"),
            Opcode::Load => write!(f, "load"),
            Opcode::Store => write!(f, "store"),
            Opcode::Return => write!(f, "ret"),
            Opcode::Call => write!(f, "call"),
            Opcode::IndirectCall => write!(f, "icall"),
            Opcode::CopyProvenance => write!(f, "copy_prov"),
            Opcode::ExposeProvenance => write!(f, "expose_prov"),
            Opcode::UnexposeProvenance => write!(f, "unexpose_prov"),
            Opcode::NewProvenance => write!(f, "new_prov"),
            Opcode::Memcpy => write!(f, "memcpy"),
            Opcode::LoadGlobalLoc => write!(f, "load_global"),
            Opcode::Nop => write!(f, "nop"),
            Opcode::BitCast => write!(f, "bitcast"),
            Opcode::Select => write!(f, "select"),
            Opcode::Add => write!(f, "add"),
            Opcode::Sub => write!(f, "sub"),
            Opcode::Mul => write!(f, "mul"),
            Opcode::Div => write!(f, "div"),
            Opcode::Eq => write!(f, "eq"),
            Opcode::NotEq => write!(f, "noteq"),
            Opcode::Greater => write!(f, "gt"),
            Opcode::Less => write!(f, "lt"),
            Opcode::GreaterOrEqual => write!(f, "ge"),
            Opcode::LessOrEqual => write!(f, "le"),
            Opcode::And => write!(f, "and"),
            Opcode::Or => write!(f, "or"),
            Opcode::BitAnd => write!(f, "bitand"),
            Opcode::BitOr => write!(f, "bitor"),
            Opcode::Xor => write!(f, "xor"),
            Opcode::Mod => write!(f, "mod"),
            Opcode::Neg => write!(f, "neg"),
            Opcode::BitNot => write!(f, "bitnot"),
            Opcode::Not => write!(f, "not"),
            Opcode::Shl => write!(f, "shl"),
            Opcode::Shr => write!(f, "shr"),
        }?;
        Ok(())
    }
}

fn get_func_ctx(f: &Function) -> Ctx<'_> {
    Ctx {
        blocks: &f.blocks,
        stack_slots: &f.stack_slots,
        insts: &f.insts,
        values: &f.values,
        provenances: &f.provenances,
        value_provenances: &f.value_provenances,
    }
}

fn block_order(entry: BlockId, blocks: &BlockArena) -> Vec<BlockId> {
    // Reverse-post order
    let mut visited = AHashSet::new();
    let mut ret = Vec::with_capacity(blocks.len());
    fn dfs(
        block_id: BlockId,
        blocks: &BlockArena,
        visited: &mut AHashSet<BlockId>,
        ret: &mut Vec<BlockId>,
    ) {
        if !visited.insert(block_id) {
            return;
        }
        let block = blocks.get(block_id);
        for &succ in block.succs.iter().rev() {
            dfs(succ, blocks, visited, ret);
        }
        ret.push(block_id)
    }
    dfs(entry, blocks, &mut visited, &mut ret);
    ret.reverse();
    ret
}
