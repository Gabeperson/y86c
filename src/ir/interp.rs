use ahash::{AHashMap, HashMap, HashMapExt};
use smol_str::SmolStr;

use crate::common::symbol::Symbol;
use crate::ir::print::IrInstPrinter;
use crate::ir::repr::*;
use crate::syntax::context::Context;

#[derive(Debug, Clone)]
pub struct IrInterpreter<'a> {
    pub program: &'a IrProgram,
    pub buf: Vec<u8>,
    pub program_text: &'a str,
    pub global_locs: AHashMap<Symbol, usize>,
    pub instruction_count: usize,
    pub ctx: &'a Context,
}

impl<'a> IrInterpreter<'a> {
    pub fn new(program: &'a IrProgram, program_text: &'a str, ctx: &'a Context) -> Self {
        Self {
            program,
            buf: Vec::new(),
            program_text,
            global_locs: AHashMap::new(),
            instruction_count: 0,
            ctx,
        }
    }
    pub fn run(&mut self) {
        self.buf.clear();
        self.instruction_count = 0;
        self.global_locs.clear();
        self.prep();
        let main = self
            .ctx
            .symbol_interner
            .get_id_for(SmolStr::new("main"))
            .expect("main should exist for interp'd code");
        let res = self.run_func(main, &[]);
        assert!(res.is_none());
    }
    pub fn prep(&mut self) {
        // Find errors with nullptr + X pointer stores/loads
        self.buf.resize(4096, 0);
        let mut pos = 4096usize;
        for (name, global) in self.program.globals.map.iter() {
            let layout = global.val.layout();
            pos = pos.next_multiple_of(layout.align);
            assert!(self.global_locs.insert(*name, pos).is_none());
            pos += layout.size;
        }
        self.buf.resize(pos, 0);
        for (i, name) in self.program.functions.keys().enumerate() {
            assert!(self.global_locs.insert(*name, i).is_none());
        }
        for (name, global) in self.program.globals.map.iter() {
            let pos = self.global_locs[name];
            let size = global.val.layout().size;
            let buf = &mut self.buf[pos..][..size];
            global.val.write(buf, |sym| self.global_locs[&sym] as i64);
        }
    }
    pub fn run_func(&mut self, func: Symbol, args: &[i64]) -> Option<i64> {
        let Some(func) = self.program.functions.get(&func) else {
            let func = self.ctx.get_symbol(func);
            panic!("function '{func}' not found");
        };
        let typectx = &self.program.typectx;
        let mut stackslot_locs = Vec::with_capacity(func.stack_slots.len());
        let stackslots_size = func.stack_slots.fold(0usize, |pos, id, stackslot| {
            let pos: usize = pos.next_multiple_of(stackslot.align as usize);
            assert_eq!(id.get() as usize, stackslot_locs.len());
            stackslot_locs.push(pos);
            pos + stackslot.size as usize
        });
        let stack_start = self.buf.len();
        self.buf.resize(stack_start + stackslots_size, 0);
        let mut prev_block_id = BlockId::default();
        let mut block_id = func.entry;
        let mut inst_idx = 0;
        let mut values: HashMap<ValueId, i64> = HashMap::new();
        let mut printer =
            IrInstPrinter::new(func, &self.program.typectx, &self.ctx.symbol_interner);

        let ret = 'mainloop: loop {
            let block = func.blocks.get(block_id);
            let inst_id = if let Some(id) = block.insts.get(inst_idx) {
                *id
            } else {
                break 'mainloop None;
            };
            let inst = func.insts.get(inst_id);
            {
                // println!("{}", printer.fmt_inst(inst_id));
            }
            for val in &inst.operands {
                assert!(!val.is_invalid());
            }
            for val in &inst.results {
                assert!(!val.is_invalid());
            }
            if let InstExtraData::Phi { operands } = &inst.extra {
                for operand in operands {
                    assert!(!operand.value.is_invalid());
                }
            }
            self.instruction_count += 1;
            match inst.op {
                Opcode::GetStackAddr => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 0);
                    let InstExtraData::StackSlot(slot_id) = inst.extra else {
                        unreachable!();
                    };
                    let offset = stack_start + stackslot_locs[slot_id.get() as usize];
                    values.insert(inst.results[0], offset as i64);
                }
                Opcode::FieldAddr => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let InstExtraData::StructField {
                        struct_id, field, ..
                    } = inst.extra
                    else {
                        unreachable!()
                    };
                    let s = typectx.get_struct(struct_id);
                    let offset = s.offsets[field as usize] as i64;
                    let ptr = values[&inst.operands[0]];
                    values.insert(inst.results[0], ptr + offset);
                }
                Opcode::IndexAddr => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 2);
                    let InstExtraData::IndexAddrData { typ, forward } = inst.extra else {
                        unreachable!()
                    };
                    let mut size = typectx.type_size(typ) as i64;
                    if !forward {
                        size = -size;
                    }
                    let ptr = values[&inst.operands[0]];
                    let index = values[&inst.operands[1]];
                    values.insert(inst.results[0], ptr + index * size);
                }
                Opcode::Assert => {
                    assert_eq!(inst.results.len(), 0);
                    assert_eq!(inst.operands.len(), 1);
                    let cond = values[&inst.operands[0]];
                    if cond == 0 {
                        panic!(
                            "Assertion failed at:\n{}",
                            &self.program_text[inst.span.start as usize..inst.span.end as usize]
                        );
                    }
                }
                Opcode::Param => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 0);
                    let InstExtraData::ParamIndex { index } = inst.extra else {
                        unreachable!()
                    };
                    let arg = args[index as usize];
                    values.insert(inst.results[0], arg);
                }
                Opcode::Phi => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 0);
                    let val = func.values.get(inst.results[0]);
                    // If memphi, we ignore
                    if val.typ != typectx.mem_typ() {
                        let InstExtraData::Phi { operands } = &inst.extra else {
                            unreachable!();
                        };
                        let val_id = operands
                            .iter()
                            .find(|op| op.block == prev_block_id)
                            .map(|op| op.value)
                            .unwrap();
                        let val = values[&val_id];
                        dbg!(val);
                        values.insert(inst.results[0], val);
                    }
                }
                Opcode::Jmp => {
                    assert_eq!(inst.results.len(), 0);
                    assert_eq!(inst.operands.len(), 0);
                    let InstExtraData::Jump { target } = inst.extra else {
                        unreachable!()
                    };
                    prev_block_id = block_id;
                    block_id = target;
                    inst_idx = 0;
                    continue;
                }
                Opcode::Branch => {
                    assert_eq!(inst.results.len(), 0);
                    assert_eq!(inst.operands.len(), 1);
                    let InstExtraData::Branch {
                        then_target,
                        else_target,
                    } = inst.extra
                    else {
                        unreachable!()
                    };
                    let cond = values[&inst.operands[0]];
                    prev_block_id = block_id;
                    if cond != 0 {
                        block_id = then_target;
                    } else {
                        block_id = else_target;
                    }
                    inst_idx = 0;
                    continue;
                }
                Opcode::LoadConst => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 0);
                    let InstExtraData::ConstInt(int) = inst.extra else {
                        unreachable!()
                    };
                    values.insert(inst.results[0], int);
                }
                Opcode::Load => {
                    // operands[1] is mem
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 2);
                    let ptr = values[&inst.operands[0]] as usize;
                    assert!(ptr >= 4096, "invalid pointer dereference");
                    assert!(ptr.is_multiple_of(8), "Load from unaligned pointer");
                    let arr: [u8; 8] = self.buf[ptr..][..8].try_into().unwrap();
                    let val = i64::from_le_bytes(arr);
                    values.insert(inst.results[0], val);
                }
                Opcode::Store => {
                    // results[0] is mem
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 2);
                    let val = values[&inst.operands[0]];
                    let ptr = values[&inst.operands[1]] as usize;
                    assert!(ptr >= 4096, "invalid pointer dereference");
                    assert!(ptr.is_multiple_of(8), "Store into unaligned pointer");
                    let arr = val.to_le_bytes();
                    self.buf[ptr..][..8].copy_from_slice(&arr);
                }
                Opcode::Return => {
                    assert!(inst.results.is_empty());
                    std::assert_matches!(inst.operands.len(), 0 | 1);
                    let val = inst.operands.first().map(|v| values[v]);
                    break 'mainloop val;
                }
                Opcode::Call => {
                    std::assert_matches!(inst.results.len(), 0 | 1);
                    let InstExtraData::Function(name) = inst.extra else {
                        unreachable!()
                    };
                    let v: Vec<_> = inst.operands.iter().map(|v| values[v]).collect();
                    let res = self.run_func(name, &v);
                    if let Some(res) = res {
                        assert_eq!(inst.results.len(), 1);
                        values.insert(inst.results[0], res);
                    } else {
                        assert_eq!(inst.results.len(), 0);
                    }
                }
                Opcode::IndirectCall => {
                    std::assert_matches!(inst.results.len(), 0 | 1);
                    std::assert_matches!(inst.operands.len(), 1..);
                    let index = values[&inst.operands[0]];
                    let name = self.program.functions[index as usize].name;
                    let args = &inst.operands[1..];
                    let v: Vec<_> = args.iter().map(|v| values[v]).collect();
                    let res = self.run_func(name, &v);
                    if let Some(res) = res {
                        assert_eq!(inst.results.len(), 1);
                        values.insert(inst.results[0], res);
                    } else {
                        assert_eq!(inst.results.len(), 0);
                    }
                }
                Opcode::CopyProvenance => {
                    // operands[0] is ptr to copy provenance from
                    // we dont handle provenance atm during interp so we ignore.
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 2);
                    let val = values[&inst.operands[1]];
                    values.insert(inst.results[0], val);
                }
                Opcode::ExposeProvenance => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let val = values[&inst.operands[0]];
                    values.insert(inst.results[0], val);
                }
                Opcode::UnexposeProvenance => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let val = values[&inst.operands[0]];
                    values.insert(inst.results[0], val);
                }
                Opcode::NewProvenance => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let val = values[&inst.operands[0]];
                    values.insert(inst.results[0], val);
                }
                Opcode::Memcpy => {
                    // inst.results[0] and inst.operands[2] are Mem values
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 3);
                    let InstExtraData::ElementType(id) = inst.extra else {
                        unreachable!()
                    };
                    let size = typectx.type_size(id) as usize;
                    let dst = values[&inst.operands[0]] as usize;
                    let src = values[&inst.operands[1]] as usize;
                    self.buf.copy_within(src..src + size, dst);
                }
                Opcode::LoadGlobalLoc => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 0);
                    let InstExtraData::Global(sym) = inst.extra else {
                        unreachable!()
                    };
                    let val = self.global_locs[&sym] as i64;
                    values.insert(inst.results[0], val);
                }
                Opcode::Nop => {
                    assert_eq!(inst.results.len(), 0);
                    assert_eq!(inst.operands.len(), 0);
                }
                Opcode::BitCast => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let val = values[&inst.operands[0]];
                    values.insert(inst.results[0], val);
                }
                Opcode::Select => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 3);
                    let cond = values[&inst.operands[0]];
                    let res = if cond != 0 {
                        values[&inst.operands[1]]
                    } else {
                        values[&inst.operands[2]]
                    };
                    values.insert(inst.results[0], res);
                }
                Opcode::Add
                | Opcode::Sub
                | Opcode::Mul
                | Opcode::Div
                | Opcode::Eq
                | Opcode::NotEq
                | Opcode::Greater
                | Opcode::Less
                | Opcode::GreaterOrEqual
                | Opcode::LessOrEqual
                | Opcode::And
                | Opcode::Or
                | Opcode::BitAnd
                | Opcode::BitOr
                | Opcode::Xor
                | Opcode::Mod
                | Opcode::Shl
                | Opcode::Shr => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 2);
                    let lhs = values[&inst.operands[0]];
                    let rhs = values[&inst.operands[1]];
                    let res = binop(lhs, rhs, inst.op);
                    values.insert(inst.results[0], res);
                }
                Opcode::Neg | Opcode::BitNot | Opcode::Not => {
                    assert_eq!(inst.results.len(), 1);
                    assert_eq!(inst.operands.len(), 1);
                    let val = values[&inst.operands[0]];
                    let res = prefixop(val, inst.op);
                    values.insert(inst.results[0], res);
                }
            }
            inst_idx += 1;
        };
        // essentially decrement stack pointer
        self.buf.truncate(stack_start);
        ret
    }
}

fn binop(lhs: i64, rhs: i64, op: Opcode) -> i64 {
    match op {
        Opcode::Add => lhs.wrapping_add(rhs),
        Opcode::Sub => lhs.wrapping_sub(rhs),
        Opcode::Mul => lhs.wrapping_mul(rhs),
        Opcode::Div => lhs.wrapping_div(rhs),
        Opcode::Eq => (lhs == rhs) as i64,
        Opcode::NotEq => (lhs != rhs) as i64,
        Opcode::Greater => (lhs > rhs) as i64,
        Opcode::Less => (lhs < rhs) as i64,
        Opcode::GreaterOrEqual => (lhs >= rhs) as i64,
        Opcode::LessOrEqual => (lhs <= rhs) as i64,
        Opcode::And => (lhs != 0 && rhs != 0) as i64,
        Opcode::Or => (lhs != 0 || rhs != 0) as i64,
        Opcode::BitAnd => lhs & rhs,
        Opcode::BitOr => lhs | rhs,
        Opcode::Xor => lhs ^ rhs,
        Opcode::Mod => lhs.wrapping_rem(rhs),
        Opcode::Shl => lhs.wrapping_shl(rhs as u32),
        Opcode::Shr => lhs.wrapping_shr(rhs as u32),
        _ => unreachable!(),
    }
}

fn prefixop(val: i64, op: Opcode) -> i64 {
    match op {
        Opcode::Neg => val.wrapping_neg(),
        Opcode::BitNot => !val,
        Opcode::Not => (val == 0) as i64,
        _ => unreachable!(),
    }
}
