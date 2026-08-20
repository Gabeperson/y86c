use ahash::AHashMap;
use tinyvec::{TinyVec, tiny_vec};

use crate::analysis::symbol_table::SymbolTable;
use crate::analysis::type_checker::ExprTypeInfo;
use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::ir::lower_prepass::LoweringPrepassOutput;
use crate::ir::repr::*;
use crate::syntax::ast::{
    self, CopyProvenance, FunctionDeclaration, Ident, NodeId, Program, SizeOfType,
};
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
    fn lower_function(&mut self, decl: &ast::FunctionDeclaration) {}
}

#[derive(Debug)]
pub struct FunctionLowerer<'a> {
    prepass: &'a LoweringPrepassOutput,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    function: &'a mut Function,
    mem_sym: Symbol,

    current_def: &'a mut Vec<AHashMap<Symbol, ValueId>>,
    incomplete_phis: &'a mut Vec<AHashMap<Ident, ValueId>>,
    sealed_blocks: &'a mut Vec<bool>,
    aliases: &'a mut AHashMap<ValueId, ValueId>,
}

impl<'a> FunctionLowerer<'a> {
    fn lower(&mut self, decl: &ast::FunctionDeclaration) {
        let entry_block = Block {
            insts: Vec::new(),
            preds: TinyVec::new(),
            succs: TinyVec::new(),
            dbg_name: Some(self.ctx.intern_symbol("Entry")),
        };
        let block = self.function.blocks.intern(entry_block);
        for (name, typ) in &decl.params {
            let (size, align) = layout_of_ast_type(typ.inner, self.symbol_table, self.ctx);
            let typ_id = self.ast_to_ir_type(typ.inner);
            let typ = self.function.types.get(typ_id);
            let val_id = match typ {
                Type::Memory | Type::Void => unreachable!(),
                Type::I64 | Type::Ptr | Type::FnPtr
                    if let id = self
                        .prepass
                        .id_map
                        .get(&name.id)
                        .expect("Should be inserted")
                        && let var = self.prepass.vars.get(*id)
                        && !var.address_taken =>
                {
                    todo!()
                }
                _ => {
                    let stackslot = StackSlot {
                        size,
                        align,
                        dbg_name: Some(name.sym),
                        kind: StackSlotKind::FnArgument { typ: typ_id },
                        frame_offset: None,
                    };
                    let slot_id = self.function.stack_slots.intern(stackslot);
                    let inst = Instruction {
                        op: Opcode::GetStackAddr,
                        block,
                        span: name.span,
                        operands: TinyVec::new(),
                        results: TinyVec::new(),
                        extra: InstExtraData::StackSlot(slot_id),
                    };
                    let (inst_id, inst) = self.function.insts.intern_mut(inst);
                    let value = Value::new(
                        self.function.types.intern_deduplicated(Type::Ptr),
                        inst_id,
                        0,
                        Some(name.sym),
                    );
                    let val_id = self.function.values.intern(value);
                    inst.results.push(val_id);
                    self.write_variable(name.sym, block, val_id);
                    val_id
                }
            };
            let typ = self.function.types.get(typ_id);
            if let Type::Struct(_struct_id) = typ
                && let id = self
                    .prepass
                    .id_map
                    .get(&name.id)
                    .expect("Should be inserted")
                && let var = self.prepass.vars.get(*id)
                && !var.address_taken
            {
                let mem = self.read_variable(self.mem_sym;
                let inst = Instruction {
                    op: Opcode::Load,
                    block,
                    span: name.span,
                    operands: tiny_vec![[ValueId; 2] => val_id],
                    results: TinyVec::new(),
                    extra: InstExtraData::ElementType(typ_id),
                };
                let (inst_id, inst) = self.function.insts.intern_mut(inst);
                let value = Value::new(typ_id, inst_id, 0, Some(name.sym));
                let val_id = self.function.values.intern(value);
                inst.results.push()
            }
        }
        self.lower_block(&decl.body, block);
    }
    fn lower_block(&mut self, body: &ast::Block, block_id: BlockId) {}
    fn lower_stmt(&mut self, stmt: &ast::Stmt, block_id: BlockId) {}
    fn lower_assert(&mut self, assert: &ast::Assert, block_id: BlockId) {}
    fn lower_break(&mut self, brk: &ast::Break, block_id: BlockId) {}
    fn lower_continue(&mut self, cont: &ast::Continue, block_id: BlockId) {}
    fn lower_ifstmt(&mut self, ifstmt: &ast::IfStmt, block_id: BlockId) {}
    fn lower_while(&mut self, ifstmt: &ast::IfStmt, block_id: BlockId) {}
    fn lower_for(&mut self, ifstmt: &ast::IfStmt, block_id: BlockId) {}
    fn lower_return(&mut self, ifstmt: &ast::IfStmt, block_id: BlockId) {}
    fn lower_var_decl(&mut self, ifstmt: &ast::IfStmt, block_id: BlockId) {}
    fn lower_expr(&mut self, expr: &ast::Expr, block_id: BlockId) {}
    fn lower_nullptr(&mut self, nullptr: &ast::Expr, block_id: BlockId) {}
    fn lower_cast(&mut self, cast: &ast::Cast, block_id: BlockId) {}
    fn lower_ident(&mut self, ident: &ast::Cast, block_id: BlockId) {}
    fn lower_int(&mut self, int: &ast::Int, block_id: BlockId) {}
    fn lower_binary_op(&mut self, binop: &ast::BinaryOp, block_id: BlockId) {}
    fn lower_prefix_op(&mut self, prefixop: &ast::PrefixOp, block_id: BlockId) {}
    fn lower_postfix_op(&mut self, postfixop: &ast::PostfixOp, block_id: BlockId) {}
    fn lower_ternary(&mut self, ternary: &ast::Ternary, block_id: BlockId) {}
    fn lower_function_call(&mut self, funccall: &ast::FunctionCall, block_id: BlockId) {}
    fn lower_array_index(&mut self, arrayindex: &ast::ArrayIndex, block_id: BlockId) {}
    fn lower_size_of_type(&mut self, size_of_type: &ast::SizeOfType, block_id: BlockId) {}
    fn lower_struct_init(&mut self, struct_init: &ast::StructInit, block_id: BlockId) {}
    fn lower_array_init(&mut self, array_init: &ast::ArrayInit, block_id: BlockId) {}
    fn lower_member_access(&mut self, member_access: &ast::MemberAccess, block_id: BlockId) {}
    fn lower_pointer_member_access(
        &mut self,
        pointer_member_access: &ast::PointerMemberAccess,
        block_id: BlockId,
    ) {
    }
    fn lower_copy_prov(&mut self, copy_prov: &CopyProvenance, block_id: BlockId) {}
    fn lower_expose_prov(&mut self, expose_prov: &ast::ExposeProvenance, block_id: BlockId) {}
    fn lower_unexpose_prov(&mut self, unexpose_prov: &ast::UnexposeProvenance, block_id: BlockId) {}
    fn lower_new_prov(&mut self, new_prov: &ast::NewProvenance, block_id: BlockId) {}
}

impl<'a> FunctionLowerer<'a> {
    fn ast_to_ir_type(&mut self, typ: crate::syntax::context::TypeId) -> TypeId {
        ast_to_ir_type(
            typ,
            self.symbol_table,
            self.ctx,
            &mut self.function.types,
            &mut self.function.structs,
        )
    }
    fn get_var_type(&mut self, var: Ident) -> TypeId {
        let info = self.type_table[&var.id];
        ast_to_ir_type(
            info.id,
            self.symbol_table,
            self.ctx,
            &mut self.function.types,
            &mut self.function.structs,
        )
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
    fn new_undef(&mut self, typ: TypeId, ctx: &mut Context) -> ValueId {
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
    fn new_phi(&mut self, block_id: BlockId, typ: TypeId) -> ValueId {
        let inst = Instruction {
            op: Opcode::Phi,
            block: block_id,
            span: Span::empty(),
            operands: TinyVec::new(),
            results: TinyVec::new(),
            extra: InstExtraData::Phi {
                operands: TinyVec::new(),
            },
        };
        let (inst_id, inst) = self.insts.intern_mut(inst);
        let val = Value::new(typ, inst_id, 0, None);
        let val_id = self.values.intern(val);
        inst.results.push(val_id);
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
fn ast_to_ir_type(
    typ: crate::syntax::context::TypeId,
    symbol_table: &SymbolTable,
    ctx: &Context,
    type_arena: &mut TypeArena,
    struct_arena: &mut StructArena,
) -> TypeId {
    let typ = ctx.get_type(typ);
    let typ = match typ {
        ast::Type::Void => Type::Void,
        ast::Type::Int => Type::I64,
        ast::Type::Ptr { .. } => Type::Ptr,
        ast::Type::Struct { name } => {
            let s = symbol_table.structs.get(name).expect("Should be interned");
            let offsets = s.fields.values().map(|field| field.offset as u64).collect();
            let field_types = s
                .fields
                .values()
                .map(|field| ast_to_ir_type(field.typ, symbol_table, ctx, type_arena, struct_arena))
                .collect();
            let s = StructInfo {
                size: s.layout.size as u64,
                align: s.layout.align as u64,
                offsets,
                field_types,
            };
            let sid = struct_arena.intern_deduplicated(s);
            Type::Struct(sid)
        }
        ast::Type::Array { element_type, len } => Type::Array {
            element: ast_to_ir_type(*element_type, symbol_table, ctx, type_arena, struct_arena),
            len: *len as u64,
        },
        ast::Type::FuncPtr { .. } => Type::FnPtr,
    };
    type_arena.intern_deduplicated(typ)
}

fn layout_of_ast_type(
    typ: crate::syntax::context::TypeId,
    table: &SymbolTable,
    ctx: &Context,
) -> (u64, u64) {
    let typ = ctx.get_type(typ);
    match typ {
        ast::Type::Void => unreachable!(),
        ast::Type::Int => (8, 8),
        ast::Type::Ptr { .. } => (8, 8),
        ast::Type::Struct { name } => {
            let layout = table.structs.get(name).expect("Should be interned").layout;
            (layout.size as u64, layout.align as u64)
        }
        ast::Type::Array { element_type, len } => {
            let (e_size, e_align) = layout_of_ast_type(*element_type, table, ctx);
            (e_size * *len as u64, e_align)
        }
        ast::Type::FuncPtr { .. } => (8, 8),
    }
}
