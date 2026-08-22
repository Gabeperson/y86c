use std::ops::ControlFlow;

use ahash::AHashMap;
use tinyvec::{TinyVec, tiny_vec};

use crate::analysis::symbol_table::SymbolTable;
use crate::analysis::type_checker::ExprTypeInfo;
use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::ir::lower_prepass::{self as prepass, LoweringPrepassOutput};
use crate::ir::repr::*;
use crate::syntax::ast::{self, NodeId, Program};

use crate::syntax::context::{Context, ExprId, StmtId};

const MEM_SYM: &str = "_MEM";
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

#[derive(Debug, Clone, Copy)]
pub struct LoopBlocks {
    cont: BlockId,
    brk: BlockId,
}

#[derive(Debug, Clone, Copy)]
pub struct ExprResult {
    ret: ValueId,
    block: BlockId,
    place: Option<ValueId>,
}

#[derive(Debug)]
pub struct FunctionLowerer<'a> {
    prepass: &'a LoweringPrepassOutput,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    function: &'a mut Function,
    sret: ValueId,

    current_def: &'a mut Vec<AHashMap<VarId, ValueId>>,
    incomplete_phis: &'a mut Vec<AHashMap<VarId, ValueId>>,
    sealed_blocks: &'a mut Vec<bool>,
    aliases: &'a mut AHashMap<ValueId, ValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarId {
    Mem,
    Id(prepass::VarId),
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
        let mut sret_add = 0;
        if let Some(return_type) = decl.return_type
            && let typ = self.ctx.get_type(return_type.inner)
            && (typ.is_struct() || typ.is_array())
        {
            let typ_id = self.ast_to_ir_type(return_type.inner);
            let sret = self.ctx.intern_symbol("sret");
            let (val_id, _, val, inst) =
                self.new_inst1(Opcode::Param, block, return_type.span, &[], typ_id);
            inst.extra = InstExtraData::ParamIndex { index: 0 };
            val.dbg_name = Some(sret);
            self.sret = val_id;
            sret_add = 1;
        }
        for (i, (name, typ)) in decl.params.iter().enumerate() {
            let (size, align) = layout_of_ast_type(typ.inner, self.symbol_table, self.ctx);
            let typ_id = self.ast_to_ir_type(typ.inner);
            let typ = self.function.types.get(typ_id);
            let var_id = *self
                .prepass
                .id_map
                .get(&name.id)
                .expect("Function param should be inserted");

            // TODO: provenances
            // TODO: value uses
            // TODO: seal block
            match typ {
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
                    let (val_id, _, val, inst) =
                        self.new_inst1(Opcode::Param, block, name.span, &[], typ_id);
                    inst.extra = InstExtraData::ParamIndex {
                        index: (i + sret_add) as u32,
                    };
                    val.dbg_name = Some(name.sym);
                    self.write_variable(VarId::Id(var_id), block, val_id);
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

                    let ptr_typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let (val_id, _, val, inst) =
                        self.new_inst1(Opcode::GetStackAddr, block, name.span, &[], ptr_typ);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    val.dbg_name = Some(name.sym);

                    self.write_variable(VarId::Id(var_id), block, val_id);
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
                        let mem = self.read_variable(VarId::Mem, block);
                        let (val_id, _, val, inst) =
                            self.new_inst1(Opcode::Load, block, name.span, &[val_id, mem], typ_id);
                        inst.extra = InstExtraData::ElementType(typ_id);
                        val.dbg_name = Some(name.sym);
                        self.write_variable(VarId::Id(var_id), block, val_id);
                    }
                }
            }
        }
        let cf_res = self.lower_block(&decl.body, block, None);
        std::assert_matches!(cf_res, ControlFlow::Continue(_));
    }
    fn lower_block(
        &mut self,
        block: &ast::Block,
        mut block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) -> ControlFlow<(), BlockId> {
        for stmt in &block.body {
            let next = self.lower_stmt(*stmt, block_id, loop_blocks)?;
            block_id = next;
        }
        ControlFlow::Continue(block_id)
    }
    fn lower_stmt(
        &mut self,
        stmt: StmtId,
        block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) -> ControlFlow<(), BlockId> {
        let stmt = self.ctx.get_stmt(stmt);
        match &stmt.kind {
            ast::StmtKind::Assert(assert) => self.lower_assert(*assert, block_id),
            ast::StmtKind::Break(brk) => {
                self.lower_break(*brk, block_id, loop_blocks);
                ControlFlow::Break(())
            }
            ast::StmtKind::Continue(cont) => {
                self.lower_continue(*cont, block_id, loop_blocks);
                ControlFlow::Break(())
            }
            ast::StmtKind::Block(block) => self.lower_block(&block.clone(), block_id, loop_blocks),
            ast::StmtKind::IfStmt(if_stmt) => self.lower_ifstmt(*if_stmt, block_id, loop_blocks),
            ast::StmtKind::WhileLoop(while_loop) => self.lower_while(*while_loop, block_id),
            ast::StmtKind::ForLoop(for_loop) => self.lower_for(*for_loop, block_id),
            ast::StmtKind::ReturnStmt(return_stmt) => {
                self.lower_return(*return_stmt, block_id);
                ControlFlow::Break(())
            }
            ast::StmtKind::VariableDeclaration(var_decl) => {
                self.lower_var_decl(*var_decl, block_id)
            }
            ast::StmtKind::Expr(expr_id) => {
                let (_val, next) = self.lower_expr(*expr_id, block_id, false);
                ControlFlow::Continue(next)
            }
        }
    }
    fn lower_assert(&mut self, assert: ast::Assert, block_id: BlockId) -> ControlFlow<(), BlockId> {
        let (res, block) = self.lower_expr(assert.condition, block_id, false);
        self.new_inst0(Opcode::Assert, block, assert.span, &[res]);
        ControlFlow::Continue(block)
    }
    fn lower_break(&mut self, brk: ast::Break, block_id: BlockId, loop_blocks: Option<LoopBlocks>) {
        let loop_blocks = loop_blocks.expect("Should be checked in AST Validation");
        let inst = Instruction::new_jmp(block_id, brk.span, loop_blocks.brk);
        let inst_id = self.function.insts.intern(inst);
        self.function.blocks.get_mut(block_id).insts.push(inst_id);
        self.connect(block_id, loop_blocks.brk);
    }
    fn lower_continue(
        &mut self,
        cont: ast::Continue,
        block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) {
        let loop_blocks = loop_blocks.expect("Should be checked in AST Validation");
        let inst = Instruction::new_jmp(block_id, cont.span, loop_blocks.cont);
        let inst_id = self.function.insts.intern(inst);
        self.function.blocks.get_mut(block_id).insts.push(inst_id);
        self.connect(block_id, loop_blocks.cont)
    }
    fn lower_ifstmt(
        &mut self,
        ifstmt: ast::IfStmt,
        block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) -> ControlFlow<(), BlockId> {
        let (cond, cond_block) = self.lower_expr(ifstmt.condition, block_id, false);
        let if_true = self.ctx.intern_symbol("if_true");
        let if_false = self.ctx.intern_symbol("if_false");
        let if_end = self.ctx.intern_symbol("if_end");
        let true_block_id = self.new_block(if_true);
        let false_blockid_stmtid = ifstmt
            .else_branch
            .map(|else_branch| (self.new_block(if_false), else_branch));
        let end_block_id = self.new_block(if_end);
        let false_target_id = if let Some((false_block, _stmt_id)) = false_blockid_stmtid {
            false_block
        } else {
            end_block_id
        };
        self.connect(cond_block, true_block_id);
        self.connect(cond_block, false_target_id);
        let inst = Instruction::new_branch(
            cond_block,
            ifstmt.span,
            cond,
            true_block_id,
            false_target_id,
        );
        let inst_id = self.function.insts.intern(inst);
        self.function.blocks.get_mut(cond_block).insts.push(inst_id);
        let true_cf = self.lower_stmt(ifstmt.then_branch, true_block_id, loop_blocks);
        let false_cf = if let Some((block_id, stmt_id)) = false_blockid_stmtid {
            self.lower_stmt(stmt_id, block_id, loop_blocks)
        } else {
            ControlFlow::Continue(end_block_id)
        };
        for cf in [true_cf, false_cf] {
            if let ControlFlow::Continue(block) = cf {
                if block == end_block_id {
                    continue;
                }
                let inst = Instruction::new_jmp(block, ifstmt.span, end_block_id);
                let inst_id = self.function.insts.intern(inst);
                self.function
                    .blocks
                    .get_mut(end_block_id)
                    .insts
                    .push(inst_id);
                self.connect(block, end_block_id);
            }
        }
        if let (ControlFlow::Break(()), ControlFlow::Break(())) = (true_cf, false_cf) {
            // If both branches diverge then we dont even need to codegen stuff after it
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(end_block_id)
        }
    }
    fn lower_while(
        &mut self,
        while_loop: ast::WhileLoop,
        block_id: BlockId,
    ) -> ControlFlow<(), BlockId> {
        let while_start = self.ctx.intern_symbol("while_start");
        let while_body = self.ctx.intern_symbol("while_body");
        let while_end = self.ctx.intern_symbol("while_end");
        let start_block = self.new_block(while_start);
        let body_block = self.new_block(while_body);
        let end_block = self.new_block(while_end);
        // origin block always jumps to start block for cond eval
        self.connect(block_id, start_block);
        let inst = Instruction::new_jmp(block_id, while_loop.span, start_block);
        let inst_id = self.function.insts.intern(inst);
        self.function.blocks.get_mut(block_id).insts.push(inst_id);
        // start block checks cond, then branches to either loop body or the loop end
        let (val, val_block) = self.lower_expr(while_loop.condition, start_block, false);
        let inst = Instruction::new_branch(val_block, while_loop.span, val, body_block, end_block);
        let inst_id = self.function.insts.intern(inst);
        self.function.blocks.get_mut(val_block).insts.push(inst_id);
        self.connect(start_block, body_block);
        self.connect(start_block, end_block);

        let loop_blocks = LoopBlocks {
            cont: start_block,
            brk: end_block,
        };
        match self.lower_stmt(while_loop.body, body_block, Some(loop_blocks)) {
            ControlFlow::Continue(block) => {
                let inst = Instruction::new_jmp(block, while_loop.span, start_block);
                let inst_id = self.function.insts.intern(inst);
                self.function.blocks.get_mut(block).insts.push(inst_id);
                self.connect(block, start_block);
            }
            ControlFlow::Break(()) => {}
        }
        ControlFlow::Continue(end_block)
    }
    fn lower_for(&mut self, for_loop: ast::ForLoop, block_id: BlockId) -> ControlFlow<(), BlockId> {
        let for_start = self.ctx.intern_symbol("for_start");
        let for_body = self.ctx.intern_symbol("for_body");
        let for_post = self.ctx.intern_symbol("for_post");
        let for_end = self.ctx.intern_symbol("for_end");

        let start_block = self.new_block(for_start);
        let body_block = self.new_block(for_body);
        let post_block = self.new_block(for_post);
        let end_block = self.new_block(for_end);

        let origin_block = if let Some(init) = for_loop.init {
            let ControlFlow::Continue(cont_block) = self.lower_stmt(init, block_id, None) else {
                unreachable!()
            };
            cont_block
        } else {
            block_id
        };

        // origin block always jmps to start block
        let inst = Instruction::new_jmp(origin_block, for_loop.span, start_block);
        let inst_id = self.function.insts.intern(inst);
        self.function
            .blocks
            .get_mut(origin_block)
            .insts
            .push(inst_id);
        self.connect(origin_block, start_block);

        // start block checks cond then branches to either loop body or end
        if let Some(cond) = for_loop.condition {
            let (cond, next) = self.lower_expr(cond, start_block, false);
            let inst = Instruction::new_branch(next, for_loop.span, cond, body_block, end_block);
            let inst_id = self.function.insts.intern(inst);
            self.function.blocks.get_mut(next).insts.push(inst_id);
            self.connect(next, body_block);
            self.connect(next, end_block);
        } else {
            let inst = Instruction::new_jmp(start_block, for_loop.span, body_block);
            let inst_id = self.function.insts.intern(inst);
            self.function
                .blocks
                .get_mut(start_block)
                .insts
                .push(inst_id);
            self.connect(start_block, body_block);
        }
        let loop_blocks = LoopBlocks {
            cont: if for_loop.post.is_some() {
                post_block
            } else {
                start_block
            },
            brk: end_block,
        };
        match self.lower_stmt(for_loop.body, body_block, Some(loop_blocks)) {
            ControlFlow::Continue(cont) => {
                let inst = Instruction::new_jmp(cont, for_loop.span, post_block);
                let inst_id = self.function.insts.intern(inst);
                self.function.blocks.get_mut(cont).insts.push(inst_id);
                self.connect(cont, post_block);
            }
            ControlFlow::Break(()) => {}
        }
        if let Some(post) = for_loop.post {
            let (_val, post_end_block) = self.lower_expr(post, post_block, false);
            let uses_post = !self.function.blocks.get(post_block).preds.is_empty();
            if uses_post {
                let inst = Instruction::new_jmp(post_end_block, for_loop.span, start_block);
                let inst_id = self.function.insts.intern(inst);
                let block = self.function.blocks.get_mut(post_end_block);
                block.insts.push(inst_id);
                self.connect(post_end_block, start_block);
            }
        }
        ControlFlow::Continue(end_block)
    }
    fn lower_return(&mut self, return_stmt: ast::ReturnStmt, block_id: BlockId) {
        let (block, val) = if let Some(expr) = return_stmt.value {
            let (val, block) = self.lower_expr(expr, block_id, false);
            (block, Some(val))
        } else {
            (block_id, None)
        };
        let ret_type_id = self.function.sig.ret;
        let ret_type = self.function.types.get(ret_type_id);
        if ret_type.is_struct() || ret_type.is_array() {
            let mem_typ = self.function.types.intern_deduplicated(Type::Memory);
            let mem_sym = self.ctx.intern_symbol(MEM_SYM);

            let (val_id, _, val, _) = self.new_inst1(
                Opcode::Store,
                block,
                return_stmt.span,
                &[val.expect("Should be caught in type checking"), self.sret],
                mem_typ,
            );
            val.dbg_name = Some(mem_sym);

            self.write_variable(VarId::Mem, block, val_id);

            let mem_val = self.read_variable(VarId::Mem, block);
            self.new_inst0(Opcode::Return, block, return_stmt.span, &[mem_val]);
        } else {
            let ops = if let Some(val) = val {
                &[val]
            } else {
                &[] as &[ValueId]
            };
            self.new_inst0(Opcode::Return, block, return_stmt.span, ops);
        }
    }
    fn lower_var_decl(
        &mut self,
        var_decl: ast::VariableDeclaration,
        block_id: BlockId,
    ) -> ControlFlow<(), BlockId> {
        // TODO ref-taken
        // TODO array & struct inits
        let (next, expr) = if let Some(expr) = var_decl.init_value {
            let (expr, next) = self.lower_expr(expr, block_id, false);
            (next, Some(expr))
        } else {
            (block_id, None)
        };
        if let Some(expr) = expr {
            let id = self
                .prepass
                .id_map
                .get(&var_decl.name.id)
                .expect("Should be added");
            self.write_variable(VarId::Id(*id), next, expr);
        }
        ControlFlow::Continue(next)
    }

    fn lower_expr(
        &mut self,
        expr: ExprId,
        block_id: BlockId,
        addr_taken: bool,
    ) -> (ValueId, BlockId) {
        let expr = self.ctx.get_expr(expr);
        match expr.kind.clone() {
            ast::ExprKind::Nullptr(nullptr) => self.lower_nullptr(nullptr, block_id),
            ast::ExprKind::Cast(cast) => self.lower_cast(cast, block_id),
            ast::ExprKind::Ident(ident) => self.lower_ident(ident, block_id, addr_taken),
            ast::ExprKind::Int(int) => self.lower_int(int, block_id),
            ast::ExprKind::BinaryOp(binary_op) => self.lower_binary_op(binary_op, block_id),
            ast::ExprKind::PrefixOp(prefix_op) => self.lower_prefix_op(prefix_op, block_id),
            ast::ExprKind::PostfixOp(postfix_op) => self.lower_postfix_op(postfix_op, block_id),
            ast::ExprKind::Ternary(ternary) => self.lower_ternary(ternary, block_id),
            ast::ExprKind::FunctionCall(function_call) => {
                self.lower_function_call(function_call, block_id)
            }
            ast::ExprKind::ArrayIndex(array_index) => {
                self.lower_array_index(array_index, block_id, addr_taken)
            }
            ast::ExprKind::SizeOfType(size_of_type) => {
                self.lower_size_of_type(size_of_type, block_id)
            }
            ast::ExprKind::StructInit(struct_init) => self.lower_struct_init(struct_init, block_id),
            ast::ExprKind::ArrayInit(array_init) => self.lower_array_init(array_init, block_id),
            ast::ExprKind::MemberAccess(member_access) => {
                self.lower_member_access(member_access, block_id, addr_taken)
            }
            ast::ExprKind::PointerMemberAccess(pointer_member_access) => {
                self.lower_pointer_member_access(pointer_member_access, block_id, addr_taken)
            }
            ast::ExprKind::CopyProvenance(copy_prov) => self.lower_copy_prov(copy_prov, block_id),
            ast::ExprKind::ExposeProvenance(expose_prov) => {
                self.lower_expose_prov(expose_prov, block_id)
            }
            ast::ExprKind::UnexposeProvenance(unexpose_prov) => {
                self.lower_unexpose_prov(unexpose_prov, block_id)
            }
            ast::ExprKind::NewProvenance(new_prov) => self.lower_new_prov(new_prov, block_id),
            ast::ExprKind::Error => unreachable!(),
        }
    }
    fn lower_nullptr(&mut self, nullptr: ast::Nullptr, block_id: BlockId) -> (ValueId, BlockId) {
        let val_id = self.load_const(0, block_id, nullptr.span);

        let typ = self.function.types.intern_deduplicated(Type::Ptr);
        let (val_id, _, _, inst) =
            self.new_inst1(Opcode::BitCast, block_id, nullptr.span, &[val_id], typ);
        inst.extra = InstExtraData::ElementType(typ);
        (val_id, block_id)
    }
    fn lower_cast(&mut self, cast: ast::Cast, block_id: BlockId) -> (ValueId, BlockId) {
        let (val, block) = self.lower_expr(cast.expr, block_id, false);
        let orig_typ_id = self.function.values.get_mut(val).typ;
        let casted_to_id = self.ast_to_ir_type(cast.to_type.inner);
        if orig_typ_id == casted_to_id {
            return (val, block);
        }
        let (val_id, _, _, inst) =
            self.new_inst1(Opcode::BitCast, block, cast.span, &[val], casted_to_id);
        inst.extra = InstExtraData::ElementType(casted_to_id);
        (val_id, block)
    }
    fn lower_ident(
        &mut self,
        ident: ast::Ident,
        block_id: BlockId,
        addr_taken: bool,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_int(&mut self, int: ast::Int, block_id: BlockId) -> (ValueId, BlockId) {
        let val_id = self.load_const(int.lit, block_id, int.span);
        (val_id, block_id)
    }
    fn lower_binary_op(&mut self, binop: ast::BinaryOp, block_id: BlockId) -> (ValueId, BlockId) {
        let (lhs, block) = self.lower_expr(binop.left, block_id, false);
        let (rhs, block) = self.lower_expr(binop.right, block, false);
        let lhs_typ_id = self.function.values.get(lhs).typ;
        let rhs_typ_id = self.function.values.get(rhs).typ;
        let lhs_typ = *self.function.types.get(lhs_typ_id);
        let rhs_typ = *self.function.types.get(rhs_typ_id);
        // TODO: load/store reqs while lowering
        match binop.kind {
            ast::BinaryOpKind::Add => match (lhs_typ, rhs_typ) {
                (Type::I64, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Add, block, binop.span, &[lhs, rhs], typ);
                    (val_id, block)
                }
                (Type::Ptr, Type::I64) | (Type::I64, Type::Ptr) => {
                    let (side_expr_id, operands) = if lhs_typ == Type::Ptr {
                        (binop.left, &[lhs, rhs])
                    } else {
                        (binop.right, &[rhs, lhs])
                    };
                    let typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let base = {
                        let ptr_expr_nodeid = self.ctx.get_expr(side_expr_id).id;
                        let ast_typ = self
                            .type_table
                            .get(&ptr_expr_nodeid)
                            .expect("Should be added by type checker");
                        let ast::Type::Ptr { pointee: base, .. } = self.ctx.get_type(ast_typ.id)
                        else {
                            unreachable!()
                        };
                        self.ast_to_ir_type(*base)
                    };
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, operands, typ);
                    inst.extra = InstExtraData::ElementType(base);
                    (val_id, block)
                }
                _ => unreachable!(),
            },
            ast::BinaryOpKind::Sub => match (lhs_typ, rhs_typ) {
                (Type::I64, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);
                    (val_id, block)
                }
                (Type::Ptr, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Neg, block, binop.span, &[rhs], typ);
                    let typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let base = {
                        let ptr_expr_nodeid = self.ctx.get_expr(binop.right).id;
                        let ast_typ = self
                            .type_table
                            .get(&ptr_expr_nodeid)
                            .expect("Should be added by type checker");
                        let ast::Type::Ptr { pointee: base, .. } = self.ctx.get_type(ast_typ.id)
                        else {
                            unreachable!()
                        };
                        self.ast_to_ir_type(*base)
                    };
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, &[lhs, val_id], typ);
                    inst.extra = InstExtraData::ElementType(base);
                    (val_id, block)
                }
                (Type::Ptr, Type::Ptr) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (lhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[lhs], typ);
                    let (rhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[rhs], typ);
                    let (raw_sub, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);
                    let base_size = {
                        let ptr_expr_nodeid = self.ctx.get_expr(binop.right).id;
                        let ast_typ = self
                            .type_table
                            .get(&ptr_expr_nodeid)
                            .expect("Should be added by type checker");
                        let ast::Type::Ptr { pointee: base, .. } = self.ctx.get_type(ast_typ.id)
                        else {
                            unreachable!()
                        };
                        let base = self.ast_to_ir_type(*base);
                        self.function.type_size(base)
                    };
                    let base_size = i64::try_from(base_size).expect("Internal compiler error");
                    let base_size = self.load_const(base_size, block, binop.span);
                    let (res, _, _, _) =
                        self.new_inst1(Opcode::Div, block, binop.span, &[raw_sub, base_size], typ);
                    (res, block)
                }
                _ => unreachable!(),
            },

            ast::BinaryOpKind::Mul
            | ast::BinaryOpKind::Div
            | ast::BinaryOpKind::Mod
            | ast::BinaryOpKind::BitAnd
            | ast::BinaryOpKind::BitOr
            | ast::BinaryOpKind::Xor => todo!(),

            ast::BinaryOpKind::MulAssign
            | ast::BinaryOpKind::DivAssign
            | ast::BinaryOpKind::BitAndAssign
            | ast::BinaryOpKind::BitOrAssign
            | ast::BinaryOpKind::XorAssign
            | ast::BinaryOpKind::ModAssign => todo!(),

            ast::BinaryOpKind::Greater
            | ast::BinaryOpKind::Less
            | ast::BinaryOpKind::GreaterOrEqual
            | ast::BinaryOpKind::LessOrEqual => todo!(),

            ast::BinaryOpKind::Eq => todo!(),
            ast::BinaryOpKind::NotEq => todo!(),

            ast::BinaryOpKind::And => todo!(),
            ast::BinaryOpKind::Or => todo!(),

            ast::BinaryOpKind::AddAssign => todo!(),
            ast::BinaryOpKind::SubAssign => todo!(),

            ast::BinaryOpKind::Assign => todo!(),

            ast::BinaryOpKind::Shl => todo!(),
            ast::BinaryOpKind::Shr => todo!(),

            ast::BinaryOpKind::ShlAssign => todo!(),
            ast::BinaryOpKind::ShrAssign => todo!(),
        }
    }
    fn lower_prefix_op(
        &mut self,
        prefixop: ast::PrefixOp,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_postfix_op(
        &mut self,
        postfixop: ast::PostfixOp,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_ternary(&mut self, ternary: ast::Ternary, block_id: BlockId) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_function_call(
        &mut self,
        funccall: ast::FunctionCall,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_array_index(
        &mut self,
        arrayindex: ast::ArrayIndex,
        block_id: BlockId,
        addr_taken: bool,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_size_of_type(
        &mut self,
        size_of_type: ast::SizeOfType,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_struct_init(
        &mut self,
        struct_init: ast::StructInit,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_array_init(
        &mut self,
        array_init: ast::ArrayInit,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_member_access(
        &mut self,
        member_access: ast::MemberAccess,
        block_id: BlockId,
        addr_taken: bool,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_pointer_member_access(
        &mut self,
        pointer_member_access: ast::PointerMemberAccess,
        block_id: BlockId,
        addr_taken: bool,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_copy_prov(
        &mut self,
        copy_prov: ast::CopyProvenance,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_expose_prov(
        &mut self,
        expose_prov: ast::ExposeProvenance,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_unexpose_prov(
        &mut self,
        unexpose_prov: ast::UnexposeProvenance,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
    fn lower_new_prov(
        &mut self,
        new_prov: ast::NewProvenance,
        block_id: BlockId,
    ) -> (ValueId, BlockId) {
        todo!()
    }
}

impl<'a> FunctionLowerer<'a> {
    fn new_inst1(
        &mut self,
        op: Opcode,
        block: BlockId,
        span: Span,
        operands: &[ValueId],
        val_typ: TypeId,
    ) -> (ValueId, InstId, &mut Value, &mut Instruction) {
        let inst = Instruction {
            op,
            block,
            span,
            operands: operands.iter().copied().collect(),
            results: TinyVec::new(),
            extra: InstExtraData::None,
        };
        let (inst_id, inst) = self.function.insts.intern_mut(inst);
        let val = Value::new(val_typ, inst_id, 0, None);
        let (val_id, val) = self.function.values.intern_mut(val);
        inst.results.push(val_id);
        self.function.blocks.get_mut(block).insts.push(inst_id);
        (val_id, inst_id, val, inst)
    }
    fn new_inst0(
        &mut self,
        op: Opcode,
        block: BlockId,
        span: Span,
        operands: &[ValueId],
    ) -> (InstId, &mut Instruction) {
        let inst = Instruction {
            op,
            block,
            span,
            operands: operands.iter().copied().collect(),
            results: TinyVec::new(),
            extra: InstExtraData::None,
        };
        let (inst_id, inst) = self.function.insts.intern_mut(inst);
        self.function.blocks.get_mut(block).insts.push(inst_id);
        (inst_id, inst)
    }
    fn load_const(&mut self, int: i64, block: BlockId, span: Span) -> ValueId {
        let typ = self.function.types.intern_deduplicated(Type::I64);
        let (val_id, _, _, inst) = self.new_inst1(Opcode::LoadConst, block, span, &[], typ);
        inst.extra = InstExtraData::ConstInt(int);
        val_id
    }
    fn connect(&mut self, from_id: BlockId, to_id: BlockId) {
        let from = self.function.blocks.get_mut(from_id);
        from.add_succ(to_id);
        let to = self.function.blocks.get_mut(to_id);
        to.add_pred(from_id);
    }
    fn new_block(&mut self, dbg_name: Symbol) -> BlockId {
        let block = Block::new(dbg_name);
        self.function.blocks.intern(block)
    }
    fn ast_to_ir_type(&mut self, typ: crate::syntax::context::TypeId) -> TypeId {
        ast_to_ir_type(
            typ,
            self.symbol_table,
            self.ctx,
            &mut self.function.types,
            &mut self.function.structs,
        )
    }
    fn get_var_type(&mut self, var: VarId) -> TypeId {
        match var {
            VarId::Mem => self.function.types.intern_deduplicated(Type::Memory),
            VarId::Id(var_id) => {
                let var = self.prepass.vars.get(var_id);
                self.ast_to_ir_type(var.typ)
            }
        }
    }
    fn block_sealed(&mut self, block: BlockId) -> bool {
        let block = block.get() as usize;
        if self.sealed_blocks.len() <= block {
            self.sealed_blocks.resize(block + 1, false);
        }
        self.sealed_blocks[block]
    }
    fn write_variable(&mut self, var: VarId, block: BlockId, value: ValueId) {
        self.current_def[block.get() as usize].insert(var, value);
    }
    fn read_variable(&mut self, var: VarId, block: BlockId) -> ValueId {
        if let Some(val) = self.current_def[block.get() as usize].get(&var) {
            return self.resolve_alias(*val);
        }
        self.read_variable_recursive(var, block)
    }
    fn read_variable_recursive(&mut self, var: VarId, block_id: BlockId) -> ValueId {
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
            self.write_variable(var, block_id, val);
            self.add_phi_operands(var, block_id, val)
        };
        self.write_variable(var, block_id, val);
        val
    }
    fn add_phi_operands(&mut self, var: VarId, block_id: BlockId, phi_id: ValueId) -> ValueId {
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
        self.blocks.get_mut(block_id).insts.push(inst_id);
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

fn binop_kind_to_opcode(kind: ast::BinaryOpKind) -> Opcode {
    match kind {
        ast::BinaryOpKind::Mul => Opcode::Mul,
        ast::BinaryOpKind::Div => Opcode::Div,
        ast::BinaryOpKind::Mod => Opcode::Mod,
        ast::BinaryOpKind::BitAnd => Opcode::BitAnd,
        ast::BinaryOpKind::BitOr => Opcode::BitOr,
        ast::BinaryOpKind::Xor => Opcode::Xor,

        ast::BinaryOpKind::MulAssign => Opcode::Mul,
        ast::BinaryOpKind::DivAssign => Opcode::Div,
        ast::BinaryOpKind::BitAndAssign => Opcode::BitAnd,
        ast::BinaryOpKind::BitOrAssign => Opcode::BitOr,
        ast::BinaryOpKind::XorAssign => Opcode::Xor,
        ast::BinaryOpKind::ModAssign => Opcode::Mod,

        ast::BinaryOpKind::Greater => Opcode::Greater,
        ast::BinaryOpKind::Less => Opcode::Less,
        ast::BinaryOpKind::GreaterOrEqual => Opcode::GreaterOrEqual,
        ast::BinaryOpKind::LessOrEqual => Opcode::LessOrEqual,

        ast::BinaryOpKind::NotEq => Opcode::NotEq,
        ast::BinaryOpKind::Eq => Opcode::Eq,
        _ => unreachable!(),
    }
}
