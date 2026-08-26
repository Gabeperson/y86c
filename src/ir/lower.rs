use std::ops::{AddAssign, ControlFlow};

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
pub enum Place {
    // "temporary" ssa var (not identifiers mainly/only?)
    SsaVar {
        val: ValueId,
    },
    // non-address-taken ssa var
    SsaAssignableVar {
        val: ValueId,
        prepass_varid: prepass::VarId,
    },
    // address-taken var or struct, array, etc
    Ptr {
        val: ValueId,
        span: Span,
        base_typ: TypeId,
    },
}

impl Place {
    fn read(self, lowerer: &mut FunctionLowerer, block: BlockId) -> ValueId {
        match self {
            Place::SsaVar { val } => val,
            Place::SsaAssignableVar { val, .. } => val,
            Place::Ptr {
                val,
                span,
                base_typ,
            } => {
                let mem = lowerer.read_variable(VarId::Mem, block);
                let (val_id, _, _, _) =
                    lowerer.new_inst1(Opcode::Load, block, span, &[val, mem], base_typ);
                val_id
            }
        }
    }
    #[track_caller]
    fn write(self, val: ValueId, lowerer: &mut FunctionLowerer, block: BlockId) {
        match self {
            Place::SsaVar { .. } => {
                unreachable!()
            }
            Place::SsaAssignableVar { prepass_varid, .. } => {
                lowerer.write_variable(VarId::Id(prepass_varid), block, val);
            }
            Place::Ptr { val: ptr, span, .. } => {
                let mem = lowerer.function.types.intern_deduplicated(Type::Memory);
                let (val_id, _, _, _) =
                    lowerer.new_inst1(Opcode::Store, block, span, &[val, ptr], mem);
                lowerer.write_variable(VarId::Mem, block, val_id);
            }
        }
    }
    fn get_ptr(self) -> ValueId {
        match self {
            Place::Ptr { val, .. } => val,
            _ => unreachable!(),
        }
    }
    fn ssa(val: ValueId) -> Self {
        Self::SsaVar { val }
    }
    fn ssa_ident(val: ValueId, prepass_varid: prepass::VarId) -> Self {
        Self::SsaAssignableVar { val, prepass_varid }
    }
    fn ptr(val: ValueId, span: Span, base_typ: TypeId) -> Self {
        Self::Ptr {
            val,
            span,
            base_typ,
        }
    }
}

#[derive(Debug)]
pub struct FunctionLowerer<'a> {
    prepass: &'a mut LoweringPrepassOutput,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    function: &'a mut Function,
    sret: ValueId,
    struct_mapping: &'a mut AHashMap<Symbol, StructId>,

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
        let wildcard_prov = self
            .function
            .provenances
            .intern_deduplicated(Provenance::Wildcard);
        for (i, (name, typnode)) in decl.params.iter().enumerate() {
            let (size, align) = layout_of_ast_type(typnode.inner, self.symbol_table, self.ctx);
            let typ_id = self.ast_to_ir_type(typnode.inner);
            let typ = self.function.types.get(typ_id);
            let var_id = *self
                .prepass
                .id_map
                .get(&name.id)
                .expect("Function param should be inserted");

            // TODO: provenances
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

                    let ast_typ = self.ctx.get_type(typnode.inner);
                    if let ast::Type::Ptr { noalias, .. } = ast_typ {
                        let prov = if *noalias {
                            self.function
                                .provenances
                                .intern_deduplicated(Provenance::NoaliasPtr(val_id))
                        } else {
                            wildcard_prov
                        };
                        self.function.value_provenances.insert(val_id, prov);
                    }
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

                    let prov = self
                        .function
                        .provenances
                        .intern_deduplicated(Provenance::StackSlot(slot_id));
                    self.function.value_provenances.insert(val_id, prov);

                    self.write_variable(VarId::Id(var_id), block, val_id);
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
                let (_val, next) = self.lower_expr(*expr_id, block_id, None);
                ControlFlow::Continue(next)
            }
        }
    }
    fn lower_assert(&mut self, assert: ast::Assert, block_id: BlockId) -> ControlFlow<(), BlockId> {
        let (res, block) = self.lower_expr(assert.condition, block_id, None);
        let res = res.read(self, block);
        self.new_inst0(Opcode::Assert, block, assert.span, &[res]);
        ControlFlow::Continue(block)
    }
    fn lower_break(&mut self, brk: ast::Break, block_id: BlockId, loop_blocks: Option<LoopBlocks>) {
        let loop_blocks = loop_blocks.expect("Should be checked in AST Validation");
        self.new_jmp(block_id, brk.span, loop_blocks.brk);
    }
    fn lower_continue(
        &mut self,
        cont: ast::Continue,
        block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) {
        let loop_blocks = loop_blocks.expect("Should be checked in AST Validation");
        self.new_jmp(block_id, cont.span, loop_blocks.cont);
    }
    fn lower_ifstmt(
        &mut self,
        ifstmt: ast::IfStmt,
        block_id: BlockId,
        loop_blocks: Option<LoopBlocks>,
    ) -> ControlFlow<(), BlockId> {
        let (cond, cond_block) = self.lower_expr(ifstmt.condition, block_id, None);
        let cond = cond.read(self, cond_block);
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
        self.new_branch(
            cond_block,
            ifstmt.span,
            cond,
            true_block_id,
            false_target_id,
        );
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
                self.new_jmp(block, ifstmt.span, end_block_id);
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
        self.new_jmp(block_id, while_loop.span, start_block);
        // start block checks cond, then branches to either loop body or the loop end
        let (val, val_block) = self.lower_expr(while_loop.condition, start_block, None);
        let val = val.read(self, val_block);
        self.new_branch(val_block, while_loop.span, val, body_block, end_block);

        let loop_blocks = LoopBlocks {
            cont: start_block,
            brk: end_block,
        };
        match self.lower_stmt(while_loop.body, body_block, Some(loop_blocks)) {
            ControlFlow::Continue(block) => {
                self.new_jmp(block, while_loop.span, start_block);
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
        self.new_jmp(origin_block, for_loop.span, start_block);

        // start block checks cond then branches to either loop body or end
        if let Some(cond) = for_loop.condition {
            let (cond, next) = self.lower_expr(cond, start_block, None);
            let cond = cond.read(self, next);
            self.new_branch(next, for_loop.span, cond, body_block, end_block);
        } else {
            self.new_jmp(start_block, for_loop.span, body_block);
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
                self.new_jmp(cont, for_loop.span, post_block);
            }
            ControlFlow::Break(()) => {}
        }
        if let Some(post) = for_loop.post {
            let (_val, post_end_block) = self.lower_expr(post, post_block, None);
            let uses_post = !self.function.blocks.get(post_block).preds.is_empty();
            if uses_post {
                self.new_jmp(post_end_block, for_loop.span, start_block);
            }
        }
        ControlFlow::Continue(end_block)
    }
    fn lower_return(&mut self, return_stmt: ast::ReturnStmt, block_id: BlockId) {
        let (block, val) = if let Some(expr) = return_stmt.value {
            let (val, block) = self.lower_expr(expr, block_id, None);
            let val = val.read(self, block);
            (block, Some(val))
        } else {
            (block_id, None)
        };
        let ret_type_id = self.function.sig.ret;
        let ret_type = self.function.types.get(ret_type_id);
        if ret_type.is_struct() || ret_type.is_array() {
            let mem_typ = self.function.types.intern_deduplicated(Type::Memory);
            let operand1 = val.expect("Should be caught in type checking");
            let (val_id, _, _, _) = self.new_inst1(
                Opcode::Store,
                block,
                return_stmt.span,
                &[operand1, self.sret],
                mem_typ,
            );

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
        let Some(init) = var_decl.init_value else {
            // In SSA a non-initializer variable declaration is actually just a no-op
            return ControlFlow::Continue(block_id);
        };

        let typ_id = self.get_expr_type(init);
        let typ = self.function.types.get(typ_id);

        let varid = *self
            .prepass
            .id_map
            .get(&var_decl.name.id)
            .expect("Should be added");
        let var = self.prepass.vars.get(varid);
        let size = self.function.type_size(typ_id);
        let align = self.function.type_align(typ_id);

        if !(typ.is_struct() || typ.is_array()) {
            if var.address_taken {
                let stack_slot = StackSlot {
                    size,
                    align,
                    dbg_name: Some(var_decl.name.sym),
                    kind: StackSlotKind::AddressTakenLocal,
                    frame_offset: None,
                };
                let slot_id = self.function.stack_slots.intern(stack_slot);
                let (var_ptr, _, _, inst) =
                    self.new_inst1(Opcode::GetStackAddr, block_id, var_decl.span, &[], typ_id);
                inst.extra = InstExtraData::StackSlot(slot_id);

                let (expr, next) = self.lower_expr(init, block_id, None);
                let val_id = expr.read(self, next);

                Place::ptr(var_ptr, var_decl.span, typ_id).write(val_id, self, block_id);
                self.write_variable(VarId::Id(varid), next, var_ptr);
                return ControlFlow::Continue(next);
            } else {
                let (expr, next) = self.lower_expr(init, block_id, None);
                let val_id = expr.read(self, next);
                self.write_variable(VarId::Id(varid), next, val_id);
                return ControlFlow::Continue(next);
            }
        }

        let stack_slot = self.new_stackslot(
            size,
            align,
            Some(var_decl.name.sym),
            StackSlotKind::Aggregate,
        );
        let (var_ptr, _, _, inst) =
            self.new_inst1(Opcode::GetStackAddr, block_id, var_decl.span, &[], typ_id);
        inst.extra = InstExtraData::StackSlot(stack_slot);
        let (_val, next) = self.lower_expr(init, block_id, Some(var_ptr));

        ControlFlow::Continue(next)
    }

    fn lower_expr(
        &mut self,
        expr_id: ExprId,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let exprspan = {
            let expr = self.ctx.get_expr(expr_id);
            expr.span
        };
        let sptr = {
            let typ_id = self.get_expr_type(expr_id);
            let typ = self.function.types.get(typ_id);
            if typ.is_array() || typ.is_struct() {
                sptr.or_else(|| {
                    let size = self.function.type_size(typ_id);
                    let align = self.function.type_align(typ_id);
                    let stackslot = StackSlot {
                        size,
                        align,
                        dbg_name: None,
                        kind: StackSlotKind::IntermediateAggregate,
                        frame_offset: None,
                    };
                    let slot_id = self.function.stack_slots.intern(stackslot);
                    let (val, _, _, inst) =
                        self.new_inst1(Opcode::GetStackAddr, block_id, exprspan, &[], typ_id);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    Some(val)
                })
            } else {
                None
            }
        };
        let expr = self.ctx.get_expr(expr_id);
        match expr.kind.clone() {
            ast::ExprKind::Nullptr(nullptr) => self.lower_nullptr(nullptr, block_id),
            ast::ExprKind::Cast(cast) => self.lower_cast(cast, block_id),
            ast::ExprKind::Ident(ident) => self.lower_ident(ident, block_id, sptr),
            ast::ExprKind::Int(int) => self.lower_int(int, block_id),
            ast::ExprKind::BinaryOp(binary_op) => self.lower_binary_op(binary_op, block_id),
            ast::ExprKind::PrefixOp(prefix_op) => self.lower_prefix_op(prefix_op, block_id),
            ast::ExprKind::PostfixOp(postfix_op) => self.lower_postfix_op(postfix_op, block_id),
            ast::ExprKind::Ternary(ternary) => self.lower_ternary(ternary, block_id, sptr),
            ast::ExprKind::FunctionCall(function_call) => {
                self.lower_function_call(function_call, block_id, sptr)
            }
            ast::ExprKind::ArrayIndex(array_index) => {
                self.lower_array_index(array_index, block_id, sptr)
            }
            ast::ExprKind::SizeOfType(size_of_type) => {
                self.lower_size_of_type(size_of_type, block_id)
            }
            ast::ExprKind::StructInit(struct_init) => self.lower_struct_init(
                struct_init,
                block_id,
                sptr.expect("Should have been assigned above"),
            ),
            ast::ExprKind::ArrayInit(array_init) => self.lower_array_init(
                array_init,
                block_id,
                sptr.expect("Should have been assigned above"),
            ),
            ast::ExprKind::MemberAccess(member_access) => {
                self.lower_member_access(member_access, block_id, sptr)
            }
            ast::ExprKind::PointerMemberAccess(pointer_member_access) => {
                self.lower_pointer_member_access(pointer_member_access, block_id, sptr)
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
    fn lower_nullptr(&mut self, nullptr: ast::Nullptr, block_id: BlockId) -> (Place, BlockId) {
        let val_id = self.load_const(0, block_id, nullptr.span);

        let typ = self.function.types.intern_deduplicated(Type::Ptr);
        let (val_id, _, _, inst) =
            self.new_inst1(Opcode::BitCast, block_id, nullptr.span, &[val_id], typ);
        inst.extra = InstExtraData::ElementType(typ);
        (Place::ssa(val_id), block_id)
    }
    fn lower_cast(&mut self, cast: ast::Cast, block_id: BlockId) -> (Place, BlockId) {
        let (val, block) = self.lower_expr(cast.expr, block_id, None);
        let res = val.read(self, block);
        let orig_typ_id = self.function.values.get_mut(res).typ;
        let casted_to_id = self.ast_to_ir_type(cast.to_type.inner);
        if orig_typ_id == casted_to_id {
            return (val, block);
        }
        let (val_id, _, _, inst) =
            self.new_inst1(Opcode::BitCast, block, cast.span, &[res], casted_to_id);
        inst.extra = InstExtraData::ElementType(casted_to_id);
        (Place::ssa(val_id), block)
    }
    fn lower_ident(
        &mut self,
        ident: ast::Ident,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        todo!()
    }
    fn lower_int(&mut self, int: ast::Int, block_id: BlockId) -> (Place, BlockId) {
        let val_id = self.load_const(int.lit, block_id, int.span);
        (Place::ssa(val_id), block_id)
    }
    fn lower_binary_op(&mut self, binop: ast::BinaryOp, block_id: BlockId) -> (Place, BlockId) {
        match binop.kind {
            ast::BinaryOpKind::Assign => {
                if self.needs_sptr(binop.right) {
                    let (lhs_place, block) = self.lower_expr(binop.left, block_id, None);
                    let lhs_ptr = lhs_place.get_ptr();
                    // Passing the lhs_ptr as the sptr will make the children of this node copy into it
                    // effectively performing the assignment. So we don't need to do anything else but just exit.
                    let (_rhs_place, block) = self.lower_expr(binop.right, block, Some(lhs_ptr));
                    return (Place::ssa(lhs_ptr), block);
                } else {
                    let (lhs_place, block) = self.lower_expr(binop.left, block_id, None);
                    let (rhs_place, block) = self.lower_expr(binop.right, block, None);
                    let rhs = rhs_place.read(self, block);
                    lhs_place.write(rhs, self, block);
                    return (Place::ssa(rhs), block);
                }
            }
            ast::BinaryOpKind::And => {
                let and_end = self.ctx.intern_symbol("and_end");
                let and_true = self.ctx.intern_symbol("and_true");
                let end_block = self.new_block(and_end);
                let true_block = self.new_block(and_true);

                let int = self.ctx.intern_type(ast::Type::Int);
                let output_var = prepass::Var {
                    address_taken: false,
                    typ: int,
                };
                let var_id = self.prepass.vars.intern(output_var);

                let (lhs_place, next) = self.lower_expr(binop.left, block_id, None);
                let lhs = lhs_place.read(self, next);

                let f = self.load_const(0, next, binop.span);
                self.write_variable(VarId::Id(var_id), next, f);

                self.new_branch(next, binop.span, lhs, true_block, end_block);

                let (rhs_place, next) = self.lower_expr(binop.right, true_block, None);
                let rhs = rhs_place.read(self, next);
                self.write_variable(VarId::Id(var_id), next, rhs);
                self.new_jmp(next, binop.span, end_block);

                let res = self.read_variable(VarId::Id(var_id), end_block);
                return (Place::ssa(res), end_block);
            }
            ast::BinaryOpKind::Or => {
                todo!()
            }
            _ => {}
        }
        let (lhs_place, block) = self.lower_expr(binop.left, block_id, None);
        let lhs = lhs_place.read(self, block);
        let (rhs_place, block) = self.lower_expr(binop.right, block, None);
        let rhs = rhs_place.read(self, block);
        let lhs_typ_id = self.function.values.get(lhs).typ;
        let rhs_typ_id = self.function.values.get(rhs).typ;
        let lhs_typ = *self.function.types.get(lhs_typ_id);
        let rhs_typ = *self.function.types.get(rhs_typ_id);
        match binop.kind {
            ast::BinaryOpKind::Assign | ast::BinaryOpKind::And | ast::BinaryOpKind::Or => {
                unreachable!()
            }
            ast::BinaryOpKind::Add => match (lhs_typ, rhs_typ) {
                (Type::I64, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Add, block, binop.span, &[lhs, rhs], typ);
                    (Place::ssa(val_id), block)
                }
                (Type::Ptr, Type::I64) | (Type::I64, Type::Ptr) => {
                    let (side_expr_id, operands) = if lhs_typ == Type::Ptr {
                        (binop.left, &[lhs, rhs])
                    } else {
                        (binop.right, &[rhs, lhs])
                    };
                    let typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let pointee_type = self.get_expr_pointee_type(side_expr_id);
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, operands, typ);
                    inst.extra = InstExtraData::IndexAddrData {
                        typ: pointee_type,
                        forward: true,
                    };
                    (Place::ssa(val_id), block)
                }
                _ => unreachable!(),
            },
            ast::BinaryOpKind::Sub => match (lhs_typ, rhs_typ) {
                (Type::I64, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);
                    (Place::ssa(val_id), block)
                }
                (Type::Ptr, Type::I64) => {
                    let typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let pointee_type = self.get_expr_pointee_type(binop.left);
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, &[lhs, rhs], typ);
                    inst.extra = InstExtraData::IndexAddrData {
                        typ: pointee_type,
                        forward: false,
                    };
                    (Place::ssa(val_id), block)
                }
                (Type::Ptr, Type::Ptr) => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (lhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[lhs], typ);
                    let (rhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[rhs], typ);
                    let (raw_sub, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);

                    let pointee_type = self.get_expr_pointee_type(binop.left);
                    let base_size = self.function.type_size(pointee_type);
                    let base_size = i64::try_from(base_size).expect("Internal compiler error");
                    let base_size = self.load_const(base_size, block, binop.span);
                    let (res, _, _, _) =
                        self.new_inst1(Opcode::Div, block, binop.span, &[raw_sub, base_size], typ);
                    (Place::ssa(res), block)
                }
                _ => unreachable!(),
            },

            ast::BinaryOpKind::Mul
            | ast::BinaryOpKind::Div
            | ast::BinaryOpKind::Mod
            | ast::BinaryOpKind::BitAnd
            | ast::BinaryOpKind::BitOr
            | ast::BinaryOpKind::Xor
            | ast::BinaryOpKind::Shl
            | ast::BinaryOpKind::Shr
            | ast::BinaryOpKind::MulAssign
            | ast::BinaryOpKind::DivAssign
            | ast::BinaryOpKind::BitAndAssign
            | ast::BinaryOpKind::BitOrAssign
            | ast::BinaryOpKind::XorAssign
            | ast::BinaryOpKind::ModAssign
            | ast::BinaryOpKind::ShlAssign
            | ast::BinaryOpKind::ShrAssign
            | ast::BinaryOpKind::Greater
            | ast::BinaryOpKind::Less
            | ast::BinaryOpKind::GreaterOrEqual
            | ast::BinaryOpKind::LessOrEqual
            | ast::BinaryOpKind::NotEq
            | ast::BinaryOpKind::Eq => {
                let typ = self.function.types.intern_deduplicated(Type::I64);
                let (val_id, _, _, _) = self.new_inst1(
                    binop_kind_to_opcode(binop.kind),
                    block,
                    binop.span,
                    &[lhs, rhs],
                    typ,
                );
                if binop.kind.is_assign() {
                    lhs_place.write(val_id, self, block);
                }
                (Place::ssa(val_id), block)
            }

            ast::BinaryOpKind::AddAssign | ast::BinaryOpKind::SubAssign => match lhs_typ {
                Type::I64 => {
                    let typ = self.function.types.intern_deduplicated(Type::I64);
                    let (val_id, _, _, _) = self.new_inst1(
                        if binop.kind == ast::BinaryOpKind::Add {
                            Opcode::Add
                        } else {
                            Opcode::Sub
                        },
                        block,
                        binop.span,
                        &[lhs, rhs],
                        typ,
                    );
                    lhs_place.write(val_id, self, block);
                    (Place::ssa(val_id), block)
                }
                Type::Ptr => {
                    let typ = self.function.types.intern_deduplicated(Type::Ptr);
                    let pointee_type = self.get_expr_pointee_type(binop.left);
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, &[lhs, rhs], typ);
                    inst.extra = InstExtraData::IndexAddrData {
                        typ: pointee_type,
                        forward: binop.kind == ast::BinaryOpKind::Add,
                    };
                    (Place::ssa(val_id), block)
                }
                _ => unreachable!(),
            },
        }
    }
    fn lower_prefix_op(&mut self, prefixop: ast::PrefixOp, block_id: BlockId) -> (Place, BlockId) {
        match prefixop.kind {
            ast::PrefixOpKind::Increment | ast::PrefixOpKind::Decrement => {
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let expr_type = self.get_expr_type(prefixop.expr);
                let one = self.load_const(1, block, prefixop.span);
                let typ = self.function.types.get(expr_type);
                match typ {
                    Type::I64 => {
                        let typ = self.function.types.intern_deduplicated(Type::I64);
                        let (val_id, _, _, _) = self.new_inst1(
                            if prefixop.kind == ast::PrefixOpKind::Increment {
                                Opcode::Add
                            } else {
                                Opcode::Sub
                            },
                            block,
                            prefixop.span,
                            &[val, one],
                            typ,
                        );
                        place.write(val_id, self, block);
                        (Place::ssa(val_id), block)
                    }
                    Type::Ptr => {
                        let typ = self.function.types.intern_deduplicated(Type::Ptr);
                        let pointee_type = self.get_expr_pointee_type(prefixop.expr);
                        let (val_id, _, _, inst) = self.new_inst1(
                            Opcode::IndexAddr,
                            block,
                            prefixop.span,
                            &[val, one],
                            typ,
                        );
                        inst.extra = InstExtraData::IndexAddrData {
                            typ: pointee_type,
                            forward: prefixop.kind == ast::PrefixOpKind::Increment,
                        };
                        place.write(val_id, self, block);
                        (Place::ssa(val_id), block)
                    }
                    _ => unreachable!(),
                }
            }
            ast::PrefixOpKind::UnaryPlus => self.lower_expr(prefixop.expr, block_id, None),
            ast::PrefixOpKind::UnaryMinus => {
                let typ = self.function.types.intern_deduplicated(Type::I64);
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let (val_id, _, _, _) =
                    self.new_inst1(Opcode::Neg, block, prefixop.span, &[val], typ);
                (Place::ssa(val_id), block)
            }
            ast::PrefixOpKind::AddressOf => {
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.get_ptr();
                (Place::ssa(val), block)
            }
            ast::PrefixOpKind::Dereference => {
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let ptr = place.read(self, block);
                let base_typ = self.get_expr_pointee_type(prefixop.expr);
                let val = Place::ptr(ptr, prefixop.span, base_typ).read(self, block);
                (Place::ssa(val), block)
            }
            ast::PrefixOpKind::Not => {
                let typ = self.function.types.intern_deduplicated(Type::I64);
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let (val_id, _, _, _) =
                    self.new_inst1(Opcode::Not, block, prefixop.span, &[val], typ);
                (Place::ssa(val_id), block)
            }
            ast::PrefixOpKind::BitNot => {
                let typ = self.function.types.intern_deduplicated(Type::I64);
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let (val_id, _, _, _) =
                    self.new_inst1(Opcode::BitNot, block, prefixop.span, &[val], typ);
                (Place::ssa(val_id), block)
            }
        }
    }
    fn lower_postfix_op(
        &mut self,
        postfixop: ast::PostfixOp,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        let (place, block) = self.lower_expr(postfixop.expr, block_id, None);
        let original_val = place.read(self, block);
        let expr_type = self.get_expr_type(postfixop.expr);
        let one = self.load_const(1, block, postfixop.span);
        let typ = self.function.types.get(expr_type);
        match typ {
            Type::I64 => {
                let typ = self.function.types.intern_deduplicated(Type::I64);
                let (val_id, _, _, _) = self.new_inst1(
                    if postfixop.kind == ast::PostfixOpKind::Increment {
                        Opcode::Add
                    } else {
                        Opcode::Sub
                    },
                    block,
                    postfixop.span,
                    &[original_val, one],
                    typ,
                );
                place.write(val_id, self, block);
                (Place::ssa(original_val), block)
            }
            Type::Ptr => {
                let typ = self.function.types.intern_deduplicated(Type::Ptr);
                let pointee_type = self.get_expr_pointee_type(postfixop.expr);
                let (val_id, _, _, inst) = self.new_inst1(
                    Opcode::IndexAddr,
                    block,
                    postfixop.span,
                    &[original_val, one],
                    typ,
                );
                inst.extra = InstExtraData::IndexAddrData {
                    typ: pointee_type,
                    forward: postfixop.kind == ast::PostfixOpKind::Increment,
                };
                place.write(val_id, self, block);
                (Place::ssa(original_val), block)
            }
            _ => unreachable!(),
        }
    }
    fn lower_ternary(
        &mut self,
        ternary: ast::Ternary,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let typ = self
            .type_table
            .get(&ternary.id)
            .expect("Should be added in type checker");
        let output_var = prepass::Var {
            address_taken: false,
            typ: typ.id,
        };
        let var_id = self.prepass.vars.intern(output_var);
        let ternary_true = self.ctx.intern_symbol("ternary_true");
        let ternary_false = self.ctx.intern_symbol("ternary_false");
        let ternary_end = self.ctx.intern_symbol("ternary_end");

        let true_block = self.new_block(ternary_true);
        let false_block = self.new_block(ternary_false);
        let end_block = self.new_block(ternary_end);

        let (cond_val, block) = self.lower_expr(ternary.condition, block_id, None);
        let val_id = cond_val.read(self, block);
        // struct handling
        self.new_branch(block, ternary.span, val_id, true_block, false_block);
        let (true_val, true_next) = self.lower_expr(ternary.true_branch, true_block, sptr);
        let (false_val, false_next) = self.lower_expr(ternary.false_branch, false_block, sptr);

        let val = true_val.read(self, true_next);
        self.write_variable(VarId::Id(var_id), true_next, val);
        self.new_jmp(true_next, ternary.span, end_block);

        let val = false_val.read(self, false_next);
        self.write_variable(VarId::Id(var_id), false_next, val);
        self.new_jmp(false_next, ternary.span, end_block);
        let val = self.read_variable(VarId::Id(var_id), end_block);
        (Place::ssa(val), end_block)
    }
    fn lower_function_call(
        &mut self,
        funccall: ast::FunctionCall,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        todo!()
    }
    fn lower_array_index(
        &mut self,
        arrayindex: ast::ArrayIndex,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let (ptr_place, block) = self.lower_expr(arrayindex.array, block_id, None);
        let ptr = ptr_place.read(self, block);
        let (index_place, block) = self.lower_expr(arrayindex.index, block, None);
        let index = index_place.read(self, block);
        let pointee_typ = self.get_expr_pointee_type(arrayindex.array);
        let (val_id, _, _, inst) = self.new_inst1(
            Opcode::IndexAddr,
            block,
            arrayindex.span,
            &[ptr, index],
            pointee_typ,
        );
        inst.extra = InstExtraData::IndexAddrData {
            typ: pointee_typ,
            forward: true,
        };
        if let Some(ptr) = sptr {
            // If sptr was given it means this array index is being copied somewhere,
            // whether this be an imtermediate aggregate access, or an assignment to a variable, etc.
            // It also means it's an aggregate, since it would've just been loaded from otherwise.
            // So we just instantly copy the aggregate into the sptr and return it.
            let (_, inst) = self.new_inst0(Opcode::Memcpy, block, arrayindex.span, &[val_id, ptr]);
            inst.extra = InstExtraData::ElementType(pointee_typ);
            (Place::ssa(ptr), block)
        } else {
            (Place::ptr(val_id, arrayindex.span, pointee_typ), block)
        }
    }
    fn lower_size_of_type(
        &mut self,
        size_of_type: ast::SizeOfType,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        let typ = self.ast_to_ir_type(size_of_type.typ.inner);
        let size = self.function.type_size(typ);
        let size: i64 = size.try_into().expect("Bigger than i64 size... why???");
        let constant = self.load_const(size, block_id, size_of_type.span);
        (Place::ssa(constant), block_id)
    }
    fn lower_struct_init(
        &mut self,
        struct_init: ast::StructInit,
        block_id: BlockId,
        sptr: ValueId,
    ) -> (Place, BlockId) {
        let s_info = self
            .symbol_table
            .structs
            .get(&struct_init.name.sym)
            .expect("Should have been added");
        let inits: Vec<_> = struct_init
            .field_inits
            .iter()
            .copied()
            .map(|(name, expr)| (expr, s_info.order[&name.sym]))
            .collect();
        let ptr_typ = self.function.types.intern_deduplicated(Type::Ptr);
        let mut block = block_id;
        for (expr, idx) in inits {
            if self.needs_sptr(expr) {
                let (ptr, _, _, inst) =
                    self.new_inst1(Opcode::FieldAddr, block, struct_init.span, &[sptr], ptr_typ);
                inst.extra = InstExtraData::StructField {
                    struct_sym: struct_init.name.sym,
                    field: idx as u32,
                };
                let (_val, next) = self.lower_expr(expr, block, Some(ptr));
                block = next;
            } else {
                let (val, next) = self.lower_expr(expr, block, None);
                let val = val.read(self, next);
                let (ptr, _, _, inst) =
                    self.new_inst1(Opcode::FieldAddr, next, struct_init.span, &[sptr], ptr_typ);
                inst.extra = InstExtraData::StructField {
                    struct_sym: struct_init.name.sym,
                    field: idx as u32,
                };
                let mem = self.function.types.intern_deduplicated(Type::Memory);
                let (mem_id, _, _, _) =
                    self.new_inst1(Opcode::Store, next, struct_init.span, &[val, ptr], mem);
                self.write_variable(VarId::Mem, next, mem_id);
                block = next;
            }
        }
        (Place::ssa(sptr), block)
    }
    fn lower_array_init(
        &mut self,
        array_init: ast::ArrayInit,
        block_id: BlockId,
        sptr: ValueId,
    ) -> (Place, BlockId) {
        let elem_type = self.get_expr_type(array_init.elements[0]);
        let mut block = block_id;
        let ptr_typ = self.function.types.intern_deduplicated(Type::Ptr);
        for (index, expr) in array_init.elements.into_iter().enumerate() {
            if self.needs_sptr(expr) {
                let num = self.load_const(index as i64, block, array_init.span);
                let (ptr, _, _, inst) = self.new_inst1(
                    Opcode::IndexAddr,
                    block,
                    array_init.span,
                    &[sptr, num],
                    ptr_typ,
                );
                inst.extra = InstExtraData::IndexAddrData {
                    typ: elem_type,
                    forward: true,
                };
                let (_val, next) = self.lower_expr(expr, block, Some(ptr));
                block = next;
            } else {
                let (val, next) = self.lower_expr(expr, block, None);
                let val = val.read(self, next);
                let num = self.load_const(index as i64, block, array_init.span);
                let (ptr, _, _, inst) = self.new_inst1(
                    Opcode::IndexAddr,
                    next,
                    array_init.span,
                    &[sptr, num],
                    ptr_typ,
                );
                inst.extra = InstExtraData::IndexAddrData {
                    typ: elem_type,
                    forward: true,
                };

                let mem = self.function.types.intern_deduplicated(Type::Memory);
                let (mem_id, _, _, _) =
                    self.new_inst1(Opcode::Store, next, array_init.span, &[val, ptr], mem);
                self.write_variable(VarId::Mem, next, mem_id);
                block = next;
            }
        }
        (Place::ssa(sptr), block)
    }
    fn lower_member_access(
        &mut self,
        member_access: ast::MemberAccess,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let struct_id = self.ctx.get_expr(member_access.struct_expr).id;
        let s_type = self.type_table[&struct_id];
        let typ = self.ctx.get_type(s_type.id);
        let ast::Type::Struct { name } = typ else {
            unreachable!()
        };
        let name = *name;
        let s_info = self
            .symbol_table
            .structs
            .get(&name)
            .expect("Should have been added");
        let index = s_info.order[&member_access.member_name.sym] as u32;
        let field_typ = self.ast_to_ir_type(s_type.id);

        let (place, block) = self.lower_expr(member_access.struct_expr, block_id, None);
        let ptr = place.read(self, block);
        let ptr_typ = self.function.types.intern_deduplicated(Type::Ptr);

        let (field_ptr, _, _, inst) = self.new_inst1(
            Opcode::FieldAddr,
            block,
            member_access.span,
            &[ptr],
            ptr_typ,
        );
        inst.extra = InstExtraData::StructField {
            struct_sym: name,
            field: index,
        };
        if let Some(sptr) = sptr {
            // If sptr was given it means this struct field is an aggregate and is being copied somewhere,
            // So we just instantly copy the aggregate into the sptr and return it.
            let (_, inst) = self.new_inst0(
                Opcode::Memcpy,
                block,
                member_access.span,
                &[field_ptr, sptr],
            );
            inst.extra = InstExtraData::ElementType(field_typ);
            (Place::ssa(ptr), block)
        } else {
            (Place::ptr(field_ptr, member_access.span, field_typ), block)
        }
    }
    fn lower_pointer_member_access(
        &mut self,
        pointer_member_access: ast::PointerMemberAccess,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let struct_id = self.ctx.get_expr(pointer_member_access.struct_ptr_expr).id;
        let s_type = self.type_table[&struct_id];
        let typ = self.ctx.get_type(s_type.id);
        let ast::Type::Ptr { pointee, .. } = typ else {
            unreachable!()
        };
        let typ = self.ctx.get_type(*pointee);
        let ast::Type::Struct { name } = typ else {
            unreachable!()
        };
        let name = *name;
        let s_info = self
            .symbol_table
            .structs
            .get(&name)
            .expect("Should have been added");
        let index = s_info.order[&pointer_member_access.member_name.sym] as u32;
        let field_typ = self.ast_to_ir_type(s_type.id);

        let (place, block) = self.lower_expr(pointer_member_access.struct_ptr_expr, block_id, None);
        let ptr = place.read(self, block);
        let ptr_typ = self.function.types.intern_deduplicated(Type::Ptr);

        let (field_ptr, _, _, inst) = self.new_inst1(
            Opcode::FieldAddr,
            block,
            pointer_member_access.span,
            &[ptr],
            ptr_typ,
        );
        inst.extra = InstExtraData::StructField {
            struct_sym: name,
            field: index,
        };
        if let Some(sptr) = sptr {
            // If sptr was given it means this struct field is an aggregate and is being copied somewhere,
            // So we just instantly copy the aggregate into the sptr and return it.
            let (_, inst) = self.new_inst0(
                Opcode::Memcpy,
                block,
                pointer_member_access.span,
                &[field_ptr, sptr],
            );
            inst.extra = InstExtraData::ElementType(field_typ);
            (Place::ssa(ptr), block)
        } else {
            (
                Place::ptr(field_ptr, pointer_member_access.span, field_typ),
                block,
            )
        }
    }
    fn lower_copy_prov(
        &mut self,
        copy_prov: ast::CopyProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        todo!()
    }
    fn lower_expose_prov(
        &mut self,
        expose_prov: ast::ExposeProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        todo!()
    }
    fn lower_unexpose_prov(
        &mut self,
        unexpose_prov: ast::UnexposeProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        todo!()
    }
    fn lower_new_prov(
        &mut self,
        new_prov: ast::NewProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        todo!()
    }
}

impl<'a> FunctionLowerer<'a> {
    fn new_stackslot(
        &mut self,
        size: u64,
        align: u64,
        dbg_name: Option<Symbol>,
        kind: StackSlotKind,
    ) -> StackSlotId {
        let slot = StackSlot {
            size,
            align,
            dbg_name,
            kind,
            frame_offset: None,
        };
        self.function.stack_slots.intern(slot)
    }
    pub fn new_jmp(
        &mut self,
        block: BlockId,
        span: Span,
        jmp_target: BlockId,
    ) -> (InstId, &mut Instruction) {
        self.connect1(block, jmp_target);
        let (instid, inst) = self.new_inst0(Opcode::Jmp, block, span, &[]);
        inst.extra = InstExtraData::Jump { target: jmp_target };
        (instid, inst)
    }
    pub fn new_branch(
        &mut self,
        block: BlockId,
        span: Span,
        cond: ValueId,
        true_target: BlockId,
        false_target: BlockId,
    ) -> (InstId, &mut Instruction) {
        self.connect1(block, true_target);
        self.connect1(block, false_target);
        let (instid, inst) = self.new_inst0(Opcode::Branch, block, span, &[cond]);
        inst.extra = InstExtraData::Branch {
            then_target: true_target,
            else_target: false_target,
        };
        (instid, inst)
    }

    fn needs_sptr(&mut self, expr: ExprId) -> bool {
        let typ_id = self.get_expr_type(expr);
        let typ = self.function.types.get(typ_id);
        typ.is_array() || typ.is_struct()
    }
    #[track_caller]
    fn get_expr_type(&mut self, expr: ExprId) -> TypeId {
        let expr_nodeid = self.ctx.get_expr(expr).id;
        let ast_typ = self
            .type_table
            .get(&expr_nodeid)
            .expect("Should be added by type checker");
        self.ast_to_ir_type(ast_typ.id)
    }
    #[track_caller]
    fn get_expr_pointee_type(&mut self, expr: ExprId) -> TypeId {
        let expr_nodeid = self.ctx.get_expr(expr).id;
        let ast_typ = self
            .type_table
            .get(&expr_nodeid)
            .expect("Should be added by type checker");
        let (ast::Type::Ptr { pointee, .. }
        | ast::Type::Array {
            element_type: pointee,
            ..
        }) = self.ctx.get_type(ast_typ.id)
        else {
            unreachable!()
        };
        self.ast_to_ir_type(*pointee)
    }
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
        for operand in operands {
            self.function.values.get_mut(*operand).add_use(inst_id)
        }
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
        for operand in operands {
            self.function.values.get_mut(*operand).add_use(inst_id)
        }
        self.function.blocks.get_mut(block).insts.push(inst_id);
        (inst_id, inst)
    }
    fn load_const(&mut self, int: i64, block: BlockId, span: Span) -> ValueId {
        let typ = self.function.types.intern_deduplicated(Type::I64);
        let (val_id, _, _, inst) = self.new_inst1(Opcode::LoadConst, block, span, &[], typ);
        inst.extra = InstExtraData::ConstInt(int);
        val_id
    }
    fn connect1(&mut self, from_id: BlockId, to_id: BlockId) {
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
        ast_to_ir_type(typ, self)
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
        {
            let block = self.function.blocks.get(block_id);
            if block.preds.is_empty() {
                let typ = self.function.types.intern_deduplicated(Type::Void);
                return self.function.new_undef(typ, self.ctx);
            }
        }
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
fn ast_to_ir_type(typ: crate::syntax::context::TypeId, lowerer: &mut FunctionLowerer) -> TypeId {
    let typ = lowerer.ctx.get_type(typ);
    let typ = match typ {
        ast::Type::Void => Type::Void,
        ast::Type::Int => Type::I64,
        ast::Type::Ptr { .. } => Type::Ptr,
        ast::Type::Struct { name } if let Some(id) = lowerer.struct_mapping.get(name) => {
            Type::Struct(*id)
        }

        ast::Type::Struct { name } => {
            let s = lowerer
                .symbol_table
                .structs
                .get(name)
                .expect("Should be interned");
            let offsets = s.fields.values().map(|field| field.offset as u64).collect();
            let field_types = s
                .fields
                .values()
                .map(|field| ast_to_ir_type(field.typ, lowerer))
                .collect();
            let s = StructInfo {
                size: s.layout.size as u64,
                align: s.layout.align as u64,
                offsets,
                field_types,
            };
            let sid = lowerer.function.structs.intern_deduplicated(s);
            Type::Struct(sid)
        }
        ast::Type::Array { element_type, len } => {
            let len = *len as u64;
            Type::Array {
                element: ast_to_ir_type(*element_type, lowerer),
                len,
            }
        }
        ast::Type::FuncPtr { .. } => Type::FnPtr,
    };
    lowerer.function.types.intern_deduplicated(typ)
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
        ast::BinaryOpKind::Shl => Opcode::Shl,
        ast::BinaryOpKind::Shr => Opcode::Shr,

        ast::BinaryOpKind::MulAssign => Opcode::Mul,
        ast::BinaryOpKind::DivAssign => Opcode::Div,
        ast::BinaryOpKind::BitAndAssign => Opcode::BitAnd,
        ast::BinaryOpKind::BitOrAssign => Opcode::BitOr,
        ast::BinaryOpKind::XorAssign => Opcode::Xor,
        ast::BinaryOpKind::ModAssign => Opcode::Mod,
        ast::BinaryOpKind::ShlAssign => Opcode::Shl,
        ast::BinaryOpKind::ShrAssign => Opcode::Shr,

        ast::BinaryOpKind::Greater => Opcode::Greater,
        ast::BinaryOpKind::Less => Opcode::Less,
        ast::BinaryOpKind::GreaterOrEqual => Opcode::GreaterOrEqual,
        ast::BinaryOpKind::LessOrEqual => Opcode::LessOrEqual,

        ast::BinaryOpKind::NotEq => Opcode::NotEq,
        ast::BinaryOpKind::Eq => Opcode::Eq,

        ast::BinaryOpKind::And => Opcode::And,
        ast::BinaryOpKind::Or => Opcode::Or,
        _ => unreachable!(),
    }
}
