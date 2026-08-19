use ahash::AHashMap;
use tinyvec::{TinyVec, tiny_vec};

use crate::analysis::symbol_table::SymbolTable;
use crate::analysis::type_checker::ExprTypeInfo;
use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::ir::lower_prepass::LoweringPrepassOutput;
use crate::ir::repr::*;
use crate::syntax::ast::{self, Ident, NodeId, Program};
use crate::syntax::context::Context;
#[derive(Debug)]
pub struct Lowerer<'a> {
    prepass: &'a LoweringPrepassOutput,
    functions: Vec<Function>,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,

    curr_fn: usize,

    current_def: Vec<AHashMap<Symbol, ValueId>>,
    incomplete_args: Vec<AHashMap<Symbol, ValueId>>,
    sealed_blocks: Vec<bool>,
    aliases: AHashMap<ValueId, ValueId>,
}

impl<'a> Lowerer<'a> {
    pub fn lower(&mut self, program: &Program) {}
}

#[derive(Debug)]
pub struct FunctionLowerer<'a> {
    prepass: &'a LoweringPrepassOutput,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    function: &'a mut Function,

    current_def: &'a mut Vec<AHashMap<Symbol, ValueId>>,
    incomplete_phis: &'a mut Vec<AHashMap<Ident, ValueId>>,
    sealed_blocks: &'a mut Vec<bool>,
    aliases: &'a mut AHashMap<ValueId, ValueId>,
}

impl<'a> FunctionLowerer<'a> {
    fn get_var_type(&self, var: Ident) -> Type {
        let info = self.type_table[&var.id];
        let typ = self.ctx.get_type(info.id);
        ast_to_ir_type(typ)
    }
    fn block_sealed(&mut self, block: BlockId) -> bool {
        let block = block.get() as usize;
        if self.sealed_blocks.len() <= block {
            self.sealed_blocks.resize(block + 1, false);
        }
        self.sealed_blocks[block]
    }
    fn write_variable(&mut self, var: Symbol, block: BlockId, value: ValueId) {
        self.current_def[block.get() as usize].insert(var, value);
    }
    fn read_variable(&mut self, var: Ident, block: BlockId) -> ValueId {
        if let Some(val) = self.current_def[block.get() as usize].get(&var.sym) {
            return self.resolve_alias(*val);
        }
        self.read_variable_recursive(var, block)
    }
    fn read_variable_recursive(&mut self, var: Ident, block_id: BlockId) -> ValueId {
        let val = if !self.block_sealed(block_id) {
            let typ = self.get_var_type(var);
            let val = self.function.new_phi(block_id, typ);
            self.incomplete_phis[block_id.get() as usize].insert(var, val);
            val
        } else if let block = self.function.blocks.get(block_id)
            && block.preds.len() == 1
        {
            self.read_variable(var, block.preds[0])
        } else {
            let typ = self.get_var_type(var);
            let val = self.function.new_phi(block_id, typ);
            self.write_variable(var.sym, block_id, val);
            self.add_phi_operands(var, block_id, val)
        };
        self.write_variable(var.sym, block_id, val);
        val
    }
    fn add_phi_operands(&mut self, var: Ident, block_id: BlockId, phi_id: ValueId) -> ValueId {
        let block = self.function.blocks.get(block_id);
        let preds = block.preds.clone();
        for pred in preds {
            let val_id = self.read_variable(var, pred);
            let phi = self.function.values.get_mut(phi_id);
            let phi_inst_id = phi.inst;
            let phi_inst = self.function.insts.get_mut(phi_inst_id);
            let phi_operands = phi_inst
                .extra
                .as_phi_args()
                .expect("Should only be called with phi instructions");
            let phi_op = PhiOperand {
                block: pred,
                value: val_id,
            };
            phi_operands.push(phi_op);
            self.function.values.get_mut(val_id).add_use(phi_inst_id);
        }
        self.try_remove_trivial_phi(phi_id)
    }
    fn try_remove_trivial_phi(&mut self, phi_id: ValueId) -> ValueId {
        let mut same = None;
        let phi_val = self.function.values.get(phi_id);
        let typ = phi_val.typ;
        let phi_inst = self.function.insts.get_mut(phi_val.inst);
        let operands = phi_inst
            .extra
            .as_phi_args()
            .expect("Should only be called with phi instructions");
        for op in operands {
            if Some(op.value) == same || op.value == phi_id {
                continue;
            }
            if same.is_some() {
                return phi_id;
            }
            same = Some(op.value);
        }
        let users: TinyVec<[InstId; 4]> = {
            phi_val
                .uses
                .iter()
                .copied()
                .filter(|i| *i != phi_val.inst)
                .collect()
        };
        let same = same.unwrap_or_else(|| self.function.new_undef(typ, self.ctx));
        self.function.replace_value(phi_id, same);
        self.aliases.insert(phi_id, same);
        for user in users {
            let inst = self.function.insts.get(user);
            if inst.op == Opcode::Phi {
                self.try_remove_trivial_phi(inst.results[0]);
            }
        }
        same
    }
    fn seal_block(&mut self, block_id: BlockId) {
        for (var, arg) in self.incomplete_phis[block_id.get() as usize].clone() {
            self.add_phi_operands(var, block_id, arg);
        }
        let block = block_id.get() as usize;
        if self.sealed_blocks.len() <= block {
            self.sealed_blocks.resize(block + 1, false);
        }
        self.sealed_blocks[block] = true;
    }
    fn resolve_alias(&self, mut val: ValueId) -> ValueId {
        while let Some(alias) = self.aliases.get(&val) {
            val = *alias;
        }
        val
    }
}

impl Function {
    fn new_undef(&mut self, typ: Type, ctx: &mut Context) -> ValueId {
        let value = Value {
            undef: true,
            typ,
            inst: InstId::default(),
            index: 0,
            dbg_name: Some(ctx.intern_symbol("undef")),
            uses: TinyVec::new(),
        };
        self.values.intern(value)
    }
    fn new_phi(&mut self, block_id: BlockId, typ: Type) -> ValueId {
        let val = Value {
            undef: false,
            typ,
            inst: InstId::default(),
            index: 0,
            dbg_name: None,
            uses: TinyVec::new(),
        };
        let (val_id, val) = self.values.intern_mut(val);
        let inst = Instruction {
            op: Opcode::Phi,
            block: block_id,
            span: Span::empty(),
            operands: TinyVec::new(),
            results: tiny_vec![[ValueId; 2] => val_id],
            extra: InstExtraData::Phi {
                operands: TinyVec::new(),
            },
        };
        let inst_id = self.insts.intern(inst);
        val.inst = inst_id;
        val_id
    }
    fn replace_value(&mut self, from: ValueId, to: ValueId) {
        let val = self.values.get(from);
        let uses = val.uses.clone();
        for user in uses {
            self.replace_inst_value(user, from, to);
        }
    }
    fn replace_inst_value(&mut self, inst_id: InstId, from: ValueId, to: ValueId) {
        let inst = self.insts.get_mut(inst_id);
        for operand in inst.operands.iter_mut() {
            if *operand == from {
                *operand = to;
                let to_val = self.values.get_mut(to);
                to_val.add_use(inst_id);
                let from_val = self.values.get_mut(from);
                from_val.remove_use(inst_id);
            }
        }
        if let Some(operands) = inst.extra.as_phi_args() {
            for operand in operands.iter_mut() {
                if operand.value == from {
                    operand.value = to;
                    let to_val = self.values.get_mut(to);
                    to_val.add_use(inst_id);
                    let from_val = self.values.get_mut(from);
                    from_val.remove_use(inst_id)
                }
            }
        }
    }
}
fn ast_to_ir_type(typ: &ast::Type) -> Type {
    match typ {
        ast::Type::Void => Type::Void,
        ast::Type::Int => Type::I64,
        ast::Type::Ptr { .. } | ast::Type::Struct { .. } | ast::Type::Array { .. } => Type::Ptr,
        ast::Type::FuncPtr { .. } => Type::FnPtr,
    }
}
