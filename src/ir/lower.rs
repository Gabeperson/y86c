use std::ops::ControlFlow;

use ahash::AHashMap;
use tinyvec::TinyVec;

use crate::analysis::symbol_table::SymbolTable;
use crate::analysis::type_checker::ExprTypeInfo;
use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::ir::lower_prepass::{self as prepass, LoweringPrepassOutput};
use crate::ir::repr::*;
use crate::syntax::ast::{self, NodeId};

use crate::syntax::context::{Context, ExprId, StmtId};

#[derive(Debug)]
pub struct Lowerer<'a> {
    prepass: &'a mut LoweringPrepassOutput,
    functions: Vec<Function>,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    typectx: TypeContext,
    struct_mapping: AHashMap<Symbol, StructId>,

    current_def: Vec<AHashMap<VarId, ValueId>>,
    incomplete_phis: Vec<AHashMap<VarId, ValueId>>,
    sealed_blocks: Vec<bool>,
    aliases: AHashMap<ValueId, ValueId>,
}

impl<'a> Lowerer<'a> {
    pub fn new(
        prepass: &'a mut LoweringPrepassOutput,
        symbol_table: &'a SymbolTable,
        ctx: &'a mut Context,
        type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    ) -> Self {
        Self {
            prepass,
            functions: Vec::new(),
            symbol_table,
            ctx,
            type_table,
            struct_mapping: AHashMap::new(),
            typectx: TypeContext::new(),
            current_def: Vec::new(),
            incomplete_phis: Vec::new(),
            sealed_blocks: Vec::new(),
            aliases: AHashMap::new(),
        }
    }
    pub fn lower(mut self, program: &ast::Program) -> (TypeContext, Vec<Function>) {
        for decl in &program.decls {
            if let ast::GlobalDeclarationKind::Function(func) = &decl.kind {
                self.lower_function(func);
            }
        }
        (self.typectx, self.functions)
    }
    fn lower_function(&mut self, decl: &ast::FunctionDeclaration) {
        let mut function = Function {
            name: decl.name.sym,
            sig: FunctionSignature {
                params: TinyVec::new(),
                ret: TypeId::default(),
            },
            // Will be replaced with a valid one
            entry: BlockId::default(),
            span: decl.span,
            inline: decl.inline,
            cc: decl.calling_convention,
            blocks: BlockArena::new(),
            stack_slots: StackSlotArena::new(),
            insts: InstArena::new(),
            values: ValueArena::new(),
            provenances: ProvenanceArena::new(),
            value_provenances: AHashMap::new(),
        };
        self.current_def.clear();
        self.incomplete_phis.clear();
        self.sealed_blocks.clear();
        self.aliases.clear();
        let mut lowerer = FunctionLowerer {
            prepass: self.prepass,
            symbol_table: self.symbol_table,
            ctx: self.ctx,
            type_table: self.type_table,
            function: &mut function,
            sret: ValueId::default(),
            return_typ: TypeId::default(),
            struct_mapping: &mut self.struct_mapping,
            typectx: &mut self.typectx,
            current_def: &mut self.current_def,
            incomplete_phis: &mut self.incomplete_phis,
            sealed_blocks: &mut self.sealed_blocks,
            aliases: &mut self.aliases,
        };
        lowerer.lower(decl);
        self.functions.push(function);
    }
}

#[derive(Debug, Clone, Copy)]
struct LoopBlocks {
    cont: BlockId,
    brk: BlockId,
}

#[derive(Debug, Clone, Copy)]
enum Place {
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
    // void-returning exprs
    // currently only function calls that call -> void functions.
    // These are not readable NOR writable to, just there for
    // "error checking" in traversal basically
    None,
}

impl Place {
    #[track_caller]
    fn read(self, lowerer: &mut FunctionLowerer, block: BlockId) -> ValueId {
        match self {
            Place::SsaVar { val } => val,
            Place::SsaAssignableVar { val, .. } => val,
            Place::Ptr {
                val,
                span,
                base_typ,
            } => lowerer.new_load(block, span, val, base_typ),
            Place::None => unreachable!(),
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
                lowerer.new_store(block, span, val, ptr);
            }
            Place::None => unreachable!(),
        }
    }
    #[track_caller]
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
struct FunctionLowerer<'a> {
    prepass: &'a mut LoweringPrepassOutput,
    symbol_table: &'a SymbolTable,
    ctx: &'a mut Context,
    type_table: &'a AHashMap<NodeId, ExprTypeInfo>,
    function: &'a mut Function,
    sret: ValueId,
    struct_mapping: &'a mut AHashMap<Symbol, StructId>,
    typectx: &'a mut TypeContext,
    return_typ: TypeId,

    current_def: &'a mut Vec<AHashMap<VarId, ValueId>>,
    incomplete_phis: &'a mut Vec<AHashMap<VarId, ValueId>>,
    sealed_blocks: &'a mut Vec<bool>,
    aliases: &'a mut AHashMap<ValueId, ValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VarId {
    Mem,
    Id(prepass::VarId),
}

impl<'a> FunctionLowerer<'a> {
    fn lower(&mut self, decl: &ast::FunctionDeclaration) {
        let entry_sym = self.ctx.intern_symbol("entry");
        let entry_block = self.new_block(entry_sym);
        self.seal_block(entry_block);
        self.function.entry = entry_block;
        let mut sret_add = 0;

        let ret_id = if let Some(return_type) = decl.return_type {
            let typ_id = self.ast_to_ir_type(return_type.inner);
            let ast_typ = self.ctx.get_type(return_type.inner);
            if ast_typ.is_struct() || ast_typ.is_array() {
                let sret = self.ctx.intern_symbol("sret");
                let ptr_typ = self.typectx.ptr_typ();
                let (val_id, _, val, inst) =
                    self.new_inst1(Opcode::Param, entry_block, return_type.span, &[], ptr_typ);
                inst.extra = InstExtraData::ParamIndex { index: 0 };
                val.dbg_name = Some(sret);
                self.sret = val_id;
                self.return_typ = typ_id;
                sret_add = 1;
                self.function.sig.params.push(FunctionParam {
                    kind: FunctionParamKind::HiddenPtr,
                    typ: ptr_typ,
                });
                self.typectx.void_typ()
            } else {
                self.return_typ = typ_id;
                typ_id
            }
        } else {
            let void = self.typectx.void_typ();
            self.return_typ = void;
            void
        };
        self.function.sig.ret = ret_id;

        let wildcard_prov = self
            .function
            .provenances
            .intern_deduplicated(Provenance::Wildcard);
        for (i, (name, typnode)) in decl.params.iter().enumerate() {
            let typ_id = self.ast_to_ir_type(typnode.inner);
            let typ = self.typectx.get_type(typ_id);
            let var_id = self.prepass.get_node_varid(name.id);

            // TODO: provenances
            match typ {
                Type::Memory | Type::Void => unreachable!(),
                Type::I64 | Type::Ptr | Type::FnPtr
                    if let var = self.prepass.get_var(var_id)
                        && !var.address_taken =>
                {
                    let (val_id, _, val, inst) =
                        self.new_inst1(Opcode::Param, entry_block, name.span, &[], typ_id);
                    inst.extra = InstExtraData::ParamIndex {
                        index: (i + sret_add) as u32,
                    };
                    val.dbg_name = Some(val.dbg_name.unwrap_or(name.sym));
                    self.function.sig.params.push(FunctionParam {
                        kind: FunctionParamKind::Arg,
                        typ: typ_id,
                    });

                    let ast_typ = self.ctx.get_type(typnode.inner);
                    if let ast::Type::Ptr { noalias, .. } = ast_typ {
                        let prov = if *noalias {
                            self.function
                                .provenances
                                .intern_deduplicated(Provenance::NoaliasArg(val_id))
                        } else {
                            wildcard_prov
                        };
                        self.function.value_provenances.insert(val_id, prov);
                    }
                    self.write_variable(VarId::Id(var_id), entry_block, val_id);
                }
                Type::I64 | Type::Ptr | Type::FnPtr => {
                    let (val_id, _, val, inst) =
                        self.new_inst1(Opcode::Param, entry_block, name.span, &[], typ_id);
                    inst.extra = InstExtraData::ParamIndex {
                        index: (i + sret_add) as u32,
                    };
                    val.dbg_name = Some(val.dbg_name.unwrap_or(name.sym));
                    self.function.sig.params.push(FunctionParam {
                        kind: FunctionParamKind::Arg,
                        typ: typ_id,
                    });

                    let slot_id =
                        self.new_stackslot(8, 8, Some(name.sym), StackSlotKind::AddressTakenLocal);
                    let ptr_typ = self.typectx.ptr_typ();
                    let (ptr, _, val, inst) =
                        self.new_inst1(Opcode::GetStackAddr, entry_block, name.span, &[], ptr_typ);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    val.dbg_name = Some(val.dbg_name.unwrap_or(name.sym));

                    self.new_store(entry_block, name.span, val_id, ptr);

                    let prov = self
                        .function
                        .provenances
                        .intern_deduplicated(Provenance::StackSlot(slot_id));
                    self.function.value_provenances.insert(val_id, prov);

                    self.write_variable(VarId::Id(var_id), entry_block, ptr);
                }
                _ => {
                    let size = self.typectx.type_size(typ_id);
                    let align = self.typectx.type_align(typ_id);
                    let slot_id = self.new_stackslot(
                        size,
                        align,
                        Some(name.sym),
                        StackSlotKind::FnArgument {
                            typ: typ_id,
                            idx: (i + sret_add) as u32,
                        },
                    );

                    let ptr_typ = self.typectx.ptr_typ();
                    let (val_id, _, val, inst) =
                        self.new_inst1(Opcode::GetStackAddr, entry_block, name.span, &[], ptr_typ);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    val.dbg_name = Some(val.dbg_name.unwrap_or(name.sym));

                    self.function.sig.params.push(FunctionParam {
                        kind: FunctionParamKind::Arg,
                        typ: ptr_typ,
                    });

                    let prov = self
                        .function
                        .provenances
                        .intern_deduplicated(Provenance::StackSlot(slot_id));
                    self.function.value_provenances.insert(val_id, prov);

                    self.write_variable(VarId::Id(var_id), entry_block, val_id);
                }
            }
        }
        let mem_typ = self.typectx.mem_typ();
        let start_mem_val = self.function.new_undef(mem_typ, self.ctx);
        self.write_variable(VarId::Mem, entry_block, start_mem_val);
        _ = self.lower_block(&decl.body, entry_block, None);
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
                let (_place, next) = self._lower_expr(*expr_id, block_id, None, false);
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
        let end_block_id = self.new_block(if_end);
        let (false_target_id, false_stmtid) = match ifstmt.else_branch {
            Some(else_branch) => (self.new_block(if_false), Some(else_branch)),
            None => (end_block_id, None),
        };

        self.new_branch(
            cond_block,
            ifstmt.span,
            cond,
            true_block_id,
            false_target_id,
        );
        self.seal_block(true_block_id);
        if false_target_id != end_block_id {
            self.seal_block(false_target_id);
        }
        let true_cf = self.lower_stmt(ifstmt.then_branch, true_block_id, loop_blocks);
        let false_cf = if let Some(stmtid) = false_stmtid {
            self.lower_stmt(stmtid, false_target_id, loop_blocks)
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
        self.seal_block(end_block_id);
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
        self.seal_block(body_block);

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
        self.seal_block(start_block);
        self.seal_block(end_block);
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
        self.seal_block(body_block);

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
                self.new_jmp(cont, for_loop.span, loop_blocks.cont);
            }
            ControlFlow::Break(()) => {}
        }
        let uses_post = !self.function.blocks.get(post_block).preds.is_empty();
        if let Some(post) = for_loop.post
            && uses_post
        {
            self.seal_block(post_block);
            let (_val, post_end_block) = self.lower_expr(post, post_block, None);
            self.new_jmp(post_end_block, for_loop.span, start_block);
        }
        self.seal_block(start_block);
        let uses_end = !self.function.blocks.get(end_block).preds.is_empty();
        if uses_end {
            self.seal_block(end_block);
            ControlFlow::Continue(end_block)
        } else {
            ControlFlow::Break(())
        }
    }
    fn lower_return(&mut self, return_stmt: ast::ReturnStmt, block_id: BlockId) {
        let ret_type_id = self.return_typ;
        let ret_type = self.typectx.get_type(ret_type_id);
        if ret_type.is_struct() || ret_type.is_array() {
            let expr = return_stmt.value.expect("Checked in type checker");
            let sret = self.sret;
            let (_val, next) = self.lower_expr(expr, block_id, Some(sret));
            self.new_inst0(Opcode::Return, next, return_stmt.span, &[]);
            return;
        }

        let (block, val) = if let Some(expr) = return_stmt.value {
            let (val, block) = self.lower_expr(expr, block_id, None);
            let val = val.read(self, block);
            (block, Some(val))
        } else {
            (block_id, None)
        };

        let ops = if let Some(val) = val {
            &[val]
        } else {
            &[] as &[ValueId]
        };
        self.new_inst0(Opcode::Return, block, return_stmt.span, ops);
    }
    fn lower_var_decl(
        &mut self,
        var_decl: ast::VariableDeclaration,
        block_id: BlockId,
    ) -> ControlFlow<(), BlockId> {
        let ast_id = var_decl.var_type.inner;
        let typ_id = self.ast_to_ir_type(ast_id);
        let asttyp = self.ctx.get_type(ast_id);

        let varid = self.prepass.get_node_varid(var_decl.name.id);
        let var = self.prepass.get_var(varid);
        let size = self.typectx.type_size(typ_id);
        let align = self.typectx.type_align(typ_id);

        let is_aggregate = asttyp.is_array() || asttyp.is_struct();

        match (is_aggregate, var_decl.init_value) {
            (false, None) => {
                // This is technically not needed, as accessing an uninitialized
                // variable is UB, but we'll be nice and allow it and just load the zero-value.
                // It also makes us not need to worry about dealing with "undefined places".
                // Plus, if they actually properly initialize the variable, this will get
                // optimized out by the DCE pass, so no hits there.
                let typ = self.typectx.get_type(typ_id);
                let val = match typ {
                    Type::I64 => {
                        let (val_id, val) = self.load_const(0, block_id, var_decl.span);
                        val.dbg_name = Some(var_decl.name.sym);
                        val_id
                    }
                    Type::Ptr | Type::FnPtr => {
                        let (val_id, _) = self.load_const(0, block_id, var_decl.span);
                        let (val_id, _, val, inst) = self.new_inst1(
                            Opcode::BitCast,
                            block_id,
                            var_decl.span,
                            &[val_id],
                            typ_id,
                        );
                        inst.extra = InstExtraData::ElementType(typ_id);
                        val.dbg_name = Some(var_decl.name.sym);
                        val_id
                    }
                    _ => unreachable!(),
                };
                self.write_variable(VarId::Id(varid), block_id, val);
                ControlFlow::Continue(block_id)
            }
            (false, Some(init)) => {
                if var.address_taken {
                    let slot_id = self.new_stackslot(
                        size,
                        align,
                        Some(var_decl.name.sym),
                        StackSlotKind::AddressTakenLocal,
                    );
                    let ptr_typ = self.typectx.ptr_typ();
                    let (var_ptr, _, val, inst) =
                        self.new_inst1(Opcode::GetStackAddr, block_id, var_decl.span, &[], ptr_typ);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    val.dbg_name = Some(val.dbg_name.unwrap_or(var_decl.name.sym));

                    let (expr, next) = self.lower_expr(init, block_id, None);
                    let val_id = expr.read(self, next);

                    Place::ptr(var_ptr, var_decl.span, typ_id).write(val_id, self, next);
                    self.write_variable(VarId::Id(varid), next, var_ptr);
                    ControlFlow::Continue(next)
                } else {
                    let (expr, next) = self.lower_expr(init, block_id, None);
                    let val_id = expr.read(self, next);
                    let val = self.function.values.get_mut(val_id);
                    val.dbg_name = Some(val.dbg_name.unwrap_or(var_decl.name.sym));

                    self.write_variable(VarId::Id(varid), next, val_id);
                    ControlFlow::Continue(next)
                }
            }
            (true, init) => {
                let stack_slot = self.new_stackslot(
                    size,
                    align,
                    Some(var_decl.name.sym),
                    StackSlotKind::Aggregate,
                );
                let ptr_typ = self.typectx.ptr_typ();
                let (var_ptr, _, val, inst) =
                    self.new_inst1(Opcode::GetStackAddr, block_id, var_decl.span, &[], ptr_typ);
                inst.extra = InstExtraData::StackSlot(stack_slot);
                val.dbg_name = Some(val.dbg_name.unwrap_or(var_decl.name.sym));

                let next = if let Some(init) = init {
                    let (_place, next) = self.lower_expr(init, block_id, Some(var_ptr));
                    next
                } else {
                    block_id
                };

                self.write_variable(VarId::Id(varid), next, var_ptr);

                ControlFlow::Continue(next)
            }
        }
    }

    fn lower_expr(
        &mut self,
        expr_id: ExprId,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        self._lower_expr(expr_id, block_id, sptr, true)
    }

    fn _lower_expr(
        &mut self,
        expr_id: ExprId,
        block_id: BlockId,
        sptr: Option<ValueId>,
        // sometimes we want to call this with no sptr with an expression that has a
        // "return type" of a struct. If we do this regularly, the lower_expr will think
        // this is an "intermediate aggregate access" and will generate an sptr.
        // But sometimes we don't want this, like when we want the location of a struct we wish to write to.
        // when the ExprStmt is lowered, it would generate a stackslot which is unneeded and unused
        // and also when trying to assign to an ident that is undefined that is also a struct/array.
        // This arg is here to prevent that in these cases.
        gen_sptr: bool,
    ) -> (Place, BlockId) {
        let exprspan = {
            let expr = self.ctx.get_expr(expr_id);
            expr.span
        };
        let sptr = {
            let typ_id = self.get_expr_type(expr_id);
            if self.needs_sptr(expr_id) && gen_sptr {
                sptr.or_else(|| {
                    let size = self.typectx.type_size(typ_id);
                    let align = self.typectx.type_align(typ_id);
                    let slot_id =
                        self.new_stackslot(size, align, None, StackSlotKind::IntermediateAggregate);
                    let ptr_typ = self.typectx.ptr_typ();
                    let (val, _, _, inst) =
                        self.new_inst1(Opcode::GetStackAddr, block_id, exprspan, &[], ptr_typ);
                    inst.extra = InstExtraData::StackSlot(slot_id);
                    Some(val)
                })
            } else {
                assert!(sptr.is_none());
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
            ast::ExprKind::PrefixOp(prefix_op) => self.lower_prefix_op(prefix_op, block_id, sptr),
            ast::ExprKind::PostfixOp(postfix_op) => self.lower_postfix_op(postfix_op, block_id),
            ast::ExprKind::Ternary(ternary) => self.lower_ternary(ternary, block_id, sptr),
            ast::ExprKind::FunctionCall(function_call) => {
                self.lower_function_call(function_call, block_id, sptr, expr_id)
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
        let (val_id, _) = self.load_const(0, block_id, nullptr.span);

        let typ = self.typectx.ptr_typ();
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
        let var_id = self.prepass.get_node_varid(ident.id);
        let var = self.prepass.get_var(var_id);
        if var.is_global {
            let typ = self.ast_to_ir_type(var.typ);
            let (val_id, _, _, inst) =
                self.new_inst1(Opcode::LoadGlobalLoc, block_id, ident.span, &[], typ);
            inst.extra = InstExtraData::Global(ident.sym);
            if var.is_function {
                return (Place::ssa(val_id), block_id);
            } else {
                return (Place::ptr(val_id, ident.span, typ), block_id);
            }
        }
        let val = self.read_variable(VarId::Id(var_id), block_id);
        if let Some(sptr) = sptr {
            let typ = self.ast_to_ir_type(var.typ);
            self.new_memcpy(block_id, ident.span, sptr, val, typ);
            return (Place::ptr(sptr, ident.span, typ), block_id);
        }
        if var.address_taken {
            let typ = self.ast_to_ir_type(var.typ);
            (Place::ptr(val, ident.span, typ), block_id)
        } else {
            (Place::ssa_ident(val, var_id), block_id)
        }
    }
    fn lower_int(&mut self, int: ast::Int, block_id: BlockId) -> (Place, BlockId) {
        let (val_id, _) = self.load_const(int.lit, block_id, int.span);
        (Place::ssa(val_id), block_id)
    }
    fn lower_binary_op(&mut self, binop: ast::BinaryOp, block_id: BlockId) -> (Place, BlockId) {
        match binop.kind {
            ast::BinaryOpKind::Assign => {
                if self.needs_sptr(binop.right) {
                    let (lhs_place, block) = self._lower_expr(binop.left, block_id, None, false);
                    let lhs_ptr = lhs_place.get_ptr();
                    // Passing the lhs_ptr as the sptr will make the children of this node copy into it
                    // effectively performing the assignment. So we don't need to do anything else but just exit.
                    let (_place, block) = self.lower_expr(binop.right, block, Some(lhs_ptr));
                    return (Place::ssa(lhs_ptr), block);
                } else {
                    let (lhs_place, block) = self.lower_expr(binop.left, block_id, None);
                    let (rhs_place, block) = self.lower_expr(binop.right, block, None);
                    let rhs = rhs_place.read(self, block);
                    if let ast::ExprKind::Ident(ident) = &self.ctx.get_expr(binop.left).kind {
                        let val = self.function.values.get_mut(rhs);
                        val.dbg_name = Some(val.dbg_name.unwrap_or(ident.sym));
                    }
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
                    is_global: false,
                    is_function: false,
                };
                let var_id = self.prepass.vars.intern(output_var);

                let (lhs_place, next) = self.lower_expr(binop.left, block_id, None);
                let lhs = lhs_place.read(self, next);

                self.write_variable(VarId::Id(var_id), next, lhs);

                self.new_branch(next, binop.span, lhs, true_block, end_block);
                self.seal_block(true_block);

                let (rhs_place, next) = self.lower_expr(binop.right, true_block, None);
                let rhs = rhs_place.read(self, next);
                self.write_variable(VarId::Id(var_id), next, rhs);
                self.new_jmp(next, binop.span, end_block);
                self.seal_block(end_block);

                let res = self.read_variable(VarId::Id(var_id), end_block);
                return (Place::ssa(res), end_block);
            }
            ast::BinaryOpKind::Or => {
                let or_end = self.ctx.intern_symbol("or_end");
                let or_false = self.ctx.intern_symbol("or_false");
                let end_block = self.new_block(or_end);
                let false_block = self.new_block(or_false);

                let int = self.ctx.intern_type(ast::Type::Int);
                let output_var = prepass::Var {
                    address_taken: false,
                    typ: int,
                    is_global: false,
                    is_function: false,
                };
                let var_id = self.prepass.vars.intern(output_var);

                let (lhs_place, next) = self.lower_expr(binop.left, block_id, None);
                let lhs = lhs_place.read(self, next);

                self.write_variable(VarId::Id(var_id), next, lhs);

                self.new_branch(next, binop.span, lhs, end_block, false_block);
                self.seal_block(false_block);

                let (rhs_place, next) = self.lower_expr(binop.right, false_block, None);
                let rhs = rhs_place.read(self, next);
                self.write_variable(VarId::Id(var_id), next, rhs);
                self.new_jmp(next, binop.span, end_block);
                self.seal_block(end_block);

                let res = self.read_variable(VarId::Id(var_id), end_block);
                return (Place::ssa(res), end_block);
            }
            _ => {}
        }
        let (lhs_place, block) = self.lower_expr(binop.left, block_id, None);
        let lhs = lhs_place.read(self, block);
        let (rhs_place, block) = self.lower_expr(binop.right, block, None);
        let rhs = rhs_place.read(self, block);
        let lhs_typ_id = self.get_expr_type(binop.left);
        let rhs_typ_id = self.get_expr_type(binop.right);
        let lhs_typ = *self.typectx.get_type(lhs_typ_id);
        let rhs_typ = *self.typectx.get_type(rhs_typ_id);
        match binop.kind {
            ast::BinaryOpKind::Assign | ast::BinaryOpKind::And | ast::BinaryOpKind::Or => {
                unreachable!()
            }
            ast::BinaryOpKind::Add => match (lhs_typ, rhs_typ) {
                (Type::I64, Type::I64) => {
                    let typ = self.typectx.i64_typ();
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
                    let typ = self.typectx.ptr_typ();
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
                    let typ = self.typectx.i64_typ();
                    let (val_id, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);
                    (Place::ssa(val_id), block)
                }
                (Type::Ptr, Type::I64) => {
                    let typ = self.typectx.ptr_typ();
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
                    let typ = self.typectx.i64_typ();
                    let (lhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[lhs], typ);
                    let (rhs, _, _, _) =
                        self.new_inst1(Opcode::BitCast, block, binop.span, &[rhs], typ);
                    let (raw_sub, _, _, _) =
                        self.new_inst1(Opcode::Sub, block, binop.span, &[lhs, rhs], typ);

                    let pointee_type = self.get_expr_pointee_type(binop.left);
                    let base_size = self.typectx.type_size(pointee_type);
                    let base_size = i64::try_from(base_size).expect("Internal compiler error");
                    if base_size == 1 {
                        (Place::ssa(raw_sub), block)
                    } else {
                        let (base_size, _) = self.load_const(base_size, block, binop.span);
                        let (res, _, _, _) = self.new_inst1(
                            Opcode::Div,
                            block,
                            binop.span,
                            &[raw_sub, base_size],
                            typ,
                        );
                        (Place::ssa(res), block)
                    }
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
                let typ = self.typectx.i64_typ();
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
                    let typ = self.typectx.i64_typ();
                    let (val_id, _, _, _) = self.new_inst1(
                        if binop.kind == ast::BinaryOpKind::AddAssign {
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
                    let typ = self.typectx.ptr_typ();
                    let pointee_type = self.get_expr_pointee_type(binop.left);
                    let (val_id, _, _, inst) =
                        self.new_inst1(Opcode::IndexAddr, block, binop.span, &[lhs, rhs], typ);
                    inst.extra = InstExtraData::IndexAddrData {
                        typ: pointee_type,
                        forward: binop.kind == ast::BinaryOpKind::AddAssign,
                    };
                    (Place::ssa(val_id), block)
                }
                _ => unreachable!(),
            },
        }
    }
    fn lower_prefix_op(
        &mut self,
        prefixop: ast::PrefixOp,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        match prefixop.kind {
            ast::PrefixOpKind::Increment | ast::PrefixOpKind::Decrement => {
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let expr_type = self.get_expr_type(prefixop.expr);
                let (one, _) = self.load_const(1, block, prefixop.span);
                let typ = self.typectx.get_type(expr_type);
                let val_id = match typ {
                    Type::I64 => {
                        let (val_id, _, _, _) = self.new_inst1(
                            if prefixop.kind == ast::PrefixOpKind::Increment {
                                Opcode::Add
                            } else {
                                Opcode::Sub
                            },
                            block,
                            prefixop.span,
                            &[val, one],
                            expr_type,
                        );
                        val_id
                    }
                    Type::Ptr => {
                        let pointee_type = self.get_expr_pointee_type(prefixop.expr);
                        let (val_id, _, _, inst) = self.new_inst1(
                            Opcode::IndexAddr,
                            block,
                            prefixop.span,
                            &[val, one],
                            expr_type,
                        );
                        inst.extra = InstExtraData::IndexAddrData {
                            typ: pointee_type,
                            forward: prefixop.kind == ast::PrefixOpKind::Increment,
                        };
                        val_id
                    }
                    _ => unreachable!(),
                };
                place.write(val_id, self, block);
                (Place::ssa(val_id), block)
            }
            ast::PrefixOpKind::UnaryPlus => self.lower_expr(prefixop.expr, block_id, None),
            ast::PrefixOpKind::UnaryMinus => {
                let typ = self.typectx.i64_typ();
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let (val_id, _, _, _) =
                    self.new_inst1(Opcode::Neg, block, prefixop.span, &[val], typ);
                (Place::ssa(val_id), block)
            }
            ast::PrefixOpKind::AddressOf => {
                let (place, block) = self._lower_expr(prefixop.expr, block_id, None, false);
                let val = place.get_ptr();
                (Place::ssa(val), block)
            }
            ast::PrefixOpKind::Dereference => {
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let ptr = place.read(self, block);
                let base_typ = self.get_expr_pointee_type(prefixop.expr);
                if let Some(sptr) = sptr {
                    self.new_memcpy(block, prefixop.span, sptr, ptr, base_typ);
                    (Place::ssa(sptr), block)
                } else {
                    (Place::ptr(ptr, prefixop.span, base_typ), block)
                }
            }
            ast::PrefixOpKind::Not => {
                let typ = self.typectx.i64_typ();
                let (place, block) = self.lower_expr(prefixop.expr, block_id, None);
                let val = place.read(self, block);
                let (val_id, _, _, _) =
                    self.new_inst1(Opcode::Not, block, prefixop.span, &[val], typ);
                (Place::ssa(val_id), block)
            }
            ast::PrefixOpKind::BitNot => {
                let typ = self.typectx.i64_typ();
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
        let (one, _) = self.load_const(1, block, postfixop.span);
        let typ = self.typectx.get_type(expr_type);
        let val_id = match typ {
            Type::I64 => {
                let (val_id, _, _, _) = self.new_inst1(
                    if postfixop.kind == ast::PostfixOpKind::Increment {
                        Opcode::Add
                    } else {
                        Opcode::Sub
                    },
                    block,
                    postfixop.span,
                    &[original_val, one],
                    expr_type,
                );
                val_id
            }
            Type::Ptr => {
                let pointee_type = self.get_expr_pointee_type(postfixop.expr);
                let (val_id, _, _, inst) = self.new_inst1(
                    Opcode::IndexAddr,
                    block,
                    postfixop.span,
                    &[original_val, one],
                    expr_type,
                );
                inst.extra = InstExtraData::IndexAddrData {
                    typ: pointee_type,
                    forward: postfixop.kind == ast::PostfixOpKind::Increment,
                };
                val_id
            }
            _ => unreachable!(),
        };
        place.write(val_id, self, block);
        (Place::ssa(original_val), block)
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
            is_global: false,
            is_function: false,
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
        self.seal_block(true_block);
        self.seal_block(false_block);
        let (true_val, true_next) = self.lower_expr(ternary.true_branch, true_block, sptr);
        let (false_val, false_next) = self.lower_expr(ternary.false_branch, false_block, sptr);

        let val = true_val.read(self, true_next);
        self.write_variable(VarId::Id(var_id), true_next, val);
        self.new_jmp(true_next, ternary.span, end_block);

        let val = false_val.read(self, false_next);
        self.write_variable(VarId::Id(var_id), false_next, val);
        self.new_jmp(false_next, ternary.span, end_block);

        self.seal_block(end_block);
        let val = self.read_variable(VarId::Id(var_id), end_block);
        (Place::ssa(val), end_block)
    }
    fn lower_function_call(
        &mut self,
        funccall: ast::FunctionCall,
        block_id: BlockId,
        sptr: Option<ValueId>,
        expr_id: ExprId,
    ) -> (Place, BlockId) {
        let (func, next) = self.lower_expr(funccall.func_expr, block_id, None);
        let func = func.read(self, next);
        let mut args = Vec::new();
        args.push(func);
        if let Some(sptr) = sptr {
            args.push(sptr);
        }
        let mut block = next;
        for arg in funccall.args {
            let (arg, next) = self.lower_expr(arg, block, None);
            let arg = arg.read(self, next);
            args.push(arg);
            block = next;
        }

        let (no_return, callkind) = {
            let expr = self.ctx.get_expr(funccall.func_expr);
            let id = expr.id;
            let ast_typinfo = self.type_table[&id];
            let ast_typ = self.ctx.get_type(ast_typinfo.id);
            let ast::Type::FuncPtr {
                return_type, kind, ..
            } = ast_typ
            else {
                unreachable!()
            };
            let typ = self.ctx.get_type(*return_type);
            // If void, then obv no return value
            // if struct or array, sret is used so no return value.
            let noreturn = typ.is_void() || typ.is_struct() || typ.is_array();
            let callkind = match kind {
                ast::FnPtrKind::Internal => CallKind::Internal,
                ast::FnPtrKind::Abi => CallKind::Abi,
            };
            (noreturn, callkind)
        };
        if no_return {
            let (_, inst) = self.new_inst0(Opcode::IndirectCall, block, funccall.span, &args);
            inst.extra = InstExtraData::ICallKind(callkind);
            if let Some(sptr) = sptr {
                (Place::ssa(sptr), block)
            } else {
                (Place::None, block)
            }
        } else {
            // TODO provenance for ptr return
            let typ = self.get_expr_type(expr_id);
            let (val_id, _, _, inst) =
                self.new_inst1(Opcode::IndirectCall, block, funccall.span, &args, typ);
            inst.extra = InstExtraData::ICallKind(callkind);
            (Place::ssa(val_id), block)
        }
    }
    fn lower_array_index(
        &mut self,
        arrayindex: ast::ArrayIndex,
        block_id: BlockId,
        sptr: Option<ValueId>,
    ) -> (Place, BlockId) {
        let (ptr_place, block) = self._lower_expr(arrayindex.array, block_id, None, false);
        let ptr = ptr_place.get_ptr();
        let (index_place, block) = self.lower_expr(arrayindex.index, block, None);
        let index = index_place.read(self, block);
        let pointee_typ = self.get_expr_pointee_type(arrayindex.array);
        let ptr_typ = self.typectx.ptr_typ();
        let (val_id, _, _, inst) = self.new_inst1(
            Opcode::IndexAddr,
            block,
            arrayindex.span,
            &[ptr, index],
            ptr_typ,
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
            self.new_memcpy(block, arrayindex.span, ptr, val_id, pointee_typ);
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
        let size = self.typectx.type_size(typ);
        let size: i64 = size.try_into().expect("Bigger than i64 size... why???");
        let (constant, _) = self.load_const(size, block_id, size_of_type.span);
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
            .map(|(name, expr)| (name.sym, expr, s_info.order[&name.sym]))
            .collect();
        let struct_id = self.get_structid(struct_init.name.sym);
        let ptr_typ = self.typectx.ptr_typ();
        let mut block = block_id;
        for (sym, expr, idx) in inits {
            if self.needs_sptr(expr) {
                let (ptr, _, _, inst) =
                    self.new_inst1(Opcode::FieldAddr, block, struct_init.span, &[sptr], ptr_typ);
                inst.extra = InstExtraData::StructField {
                    struct_sym: struct_init.name.sym,
                    struct_id,
                    member_sym: sym,
                    field: idx as u32,
                };
                let (_place, next) = self.lower_expr(expr, block, Some(ptr));
                block = next;
            } else {
                let (val, next) = self.lower_expr(expr, block, None);
                let val = val.read(self, next);
                let (ptr, _, _, inst) =
                    self.new_inst1(Opcode::FieldAddr, next, struct_init.span, &[sptr], ptr_typ);
                inst.extra = InstExtraData::StructField {
                    struct_sym: struct_init.name.sym,
                    struct_id,
                    member_sym: sym,
                    field: idx as u32,
                };
                self.new_store(next, struct_init.span, val, ptr);
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
        let ptr_typ = self.typectx.ptr_typ();
        for (index, expr) in array_init.elements.into_iter().enumerate() {
            if self.needs_sptr(expr) {
                let (num, _) = self.load_const(index as i64, block, array_init.span);
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
                let (_place, next) = self.lower_expr(expr, block, Some(ptr));
                block = next;
            } else {
                let (val, next) = self.lower_expr(expr, block, None);
                let val = val.read(self, next);
                let (num, _) = self.load_const(index as i64, next, array_init.span);
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

                self.new_store(next, array_init.span, val, ptr);
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
        let field_id = s_info.fields[&member_access.member_name.sym].typ;
        let field_typ = self.ast_to_ir_type(field_id);

        let ptr_typ = self.typectx.ptr_typ();

        let (place, block) = self._lower_expr(member_access.struct_expr, block_id, None, false);
        let ptr = place.get_ptr();

        let struct_id = self.get_structid(name);
        let (field_ptr, _, _, inst) = self.new_inst1(
            Opcode::FieldAddr,
            block,
            member_access.span,
            &[ptr],
            ptr_typ,
        );
        inst.extra = InstExtraData::StructField {
            struct_sym: name,
            member_sym: member_access.member_name.sym,
            struct_id,
            field: index,
        };

        if let Some(sptr) = sptr {
            // If sptr was given it means this struct field is an aggregate and is being copied somewhere,
            // So we just instantly copy the aggregate into the sptr and return it.
            self.new_memcpy(block, member_access.span, sptr, field_ptr, field_typ);
            (Place::ssa(field_ptr), block)
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
        let field_id = s_info.fields[&pointer_member_access.member_name.sym].typ;
        let field_typ = self.ast_to_ir_type(field_id);

        let (place, block) = self.lower_expr(pointer_member_access.struct_ptr_expr, block_id, None);
        let ptr = place.read(self, block);
        let ptr_typ = self.typectx.ptr_typ();

        let struct_id = self.get_structid(name);
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
            member_sym: pointer_member_access.member_name.sym,
            struct_id,
        };
        if let Some(sptr) = sptr {
            // If sptr was given it means this struct field is an aggregate and is being copied somewhere,
            // So we just instantly copy the aggregate into the sptr and return it.
            self.new_memcpy(
                block,
                pointer_member_access.span,
                sptr,
                field_ptr,
                field_typ,
            );
            (Place::ssa(field_ptr), block)
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
        let (ptr_place, next) = self.lower_expr(copy_prov.prov_ptr, block_id, None);
        let ptr = ptr_place.read(self, next);
        let (addr_place, next) = self.lower_expr(copy_prov.addr, next, None);
        let addr = addr_place.read(self, next);
        let ptr_typ = self.typectx.ptr_typ();
        let (val_id, _, _, _) = self.new_inst1(
            Opcode::CopyProvenance,
            next,
            copy_prov.span,
            &[ptr, addr],
            ptr_typ,
        );
        (Place::ssa(val_id), next)
    }
    fn lower_expose_prov(
        &mut self,
        expose_prov: ast::ExposeProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        let (ptr_place, next) = self.lower_expr(expose_prov.ptr, block_id, None);
        let ptr = ptr_place.read(self, next);
        let i64_typ = self.typectx.i64_typ();
        let (val_id, _, _, _) = self.new_inst1(
            Opcode::ExposeProvenance,
            next,
            expose_prov.span,
            &[ptr],
            i64_typ,
        );
        (Place::ssa(val_id), next)
    }
    fn lower_unexpose_prov(
        &mut self,
        unexpose_prov: ast::UnexposeProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        let (addr_place, next) = self.lower_expr(unexpose_prov.int, block_id, None);
        let addr = addr_place.read(self, next);
        let ptr_typ = self.typectx.ptr_typ();
        let (val_id, _, _, _) = self.new_inst1(
            Opcode::UnexposeProvenance,
            next,
            unexpose_prov.span,
            &[addr],
            ptr_typ,
        );
        (Place::ssa(val_id), next)
    }
    fn lower_new_prov(
        &mut self,
        new_prov: ast::NewProvenance,
        block_id: BlockId,
    ) -> (Place, BlockId) {
        let (ptr_place, next) = self.lower_expr(new_prov.ptr, block_id, None);
        let ptr = ptr_place.read(self, next);
        let ptr_typ = self.typectx.ptr_typ();
        let (val_id, inst_id, _, _) =
            self.new_inst1(Opcode::NewProvenance, next, new_prov.span, &[ptr], ptr_typ);
        let prov = Provenance::NewProv(NewProvInst(inst_id));
        let prov_id = self.function.provenances.intern_deduplicated(prov);
        self.function.value_provenances.insert(val_id, prov_id);
        (Place::ssa(val_id), next)
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
    fn new_jmp(
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
    fn new_branch(
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
        let typ = self.typectx.get_type(typ_id);
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
    fn load_const(&mut self, int: i64, block: BlockId, span: Span) -> (ValueId, &mut Value) {
        let typ = self.typectx.i64_typ();
        let (val_id, _, val, inst) = self.new_inst1(Opcode::LoadConst, block, span, &[], typ);
        inst.extra = InstExtraData::ConstInt(int);
        (val_id, val)
    }
    fn connect1(&mut self, from_id: BlockId, to_id: BlockId) {
        let from = self.function.blocks.get_mut(from_id);
        from.add_succ(to_id);
        let to = self.function.blocks.get_mut(to_id);
        to.add_pred(from_id);
    }
    fn new_block(&mut self, dbg_name: Symbol) -> BlockId {
        let block = Block::new(dbg_name);
        let id = self.function.blocks.intern(block);
        let id_usize = id.get() as usize;
        if self.current_def.len() <= id_usize {
            self.current_def.resize_with(id_usize + 1, AHashMap::new);
        }
        if self.sealed_blocks.len() <= id_usize {
            self.sealed_blocks.resize(id_usize + 1, false);
        }
        if self.incomplete_phis.len() <= id_usize {
            self.incomplete_phis
                .resize_with(id_usize + 1, AHashMap::new);
        }

        id
    }
    fn ast_to_ir_type(&mut self, typ: crate::syntax::context::TypeId) -> TypeId {
        ast_to_ir_type(typ, self)
    }
    fn get_var_type(&mut self, var: VarId) -> TypeId {
        match var {
            VarId::Mem => self.typectx.mem_typ(),
            VarId::Id(var_id) => {
                let var = self.prepass.get_var(var_id);
                self.ast_to_ir_type(var.typ)
            }
        }
    }
    fn block_sealed(&mut self, block: BlockId) -> bool {
        let block = block.get() as usize;
        self.sealed_blocks[block]
    }
    fn write_variable(&mut self, var: VarId, block: BlockId, value: ValueId) {
        let block = block.get() as usize;
        self.current_def[block].insert(var, value);
    }
    #[track_caller]
    fn read_variable(&mut self, var: VarId, block: BlockId) -> ValueId {
        if let Some(val) = self.current_def[block.get() as usize].get(&var) {
            return self.resolve_alias(*val);
        }
        self.read_variable_recursive(var, block)
    }
    #[track_caller]
    fn read_variable_recursive(&mut self, var: VarId, block_id: BlockId) -> ValueId {
        {
            let block = self.function.blocks.get(block_id);
            if block.preds.is_empty() {
                panic!("Internal compiler error");
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
        self.sealed_blocks[block] = true;
    }
    fn resolve_alias(&self, mut val: ValueId) -> ValueId {
        while let Some(alias) = self.aliases.get(&val) {
            val = *alias;
        }
        val
    }
    fn new_memcpy(
        &mut self,
        block: BlockId,
        span: Span,
        to: ValueId,
        from: ValueId,
        elem_typ: TypeId,
    ) {
        let mem_typ = self.typectx.mem_typ();
        let mem_val = self.read_variable(VarId::Mem, block);
        let (val_id, _, _, inst) =
            self.new_inst1(Opcode::Memcpy, block, span, &[to, from, mem_val], mem_typ);
        inst.extra = InstExtraData::ElementType(elem_typ);
        self.write_variable(VarId::Mem, block, val_id);
    }
    fn get_structid(&mut self, sym: Symbol) -> StructId {
        let asttyp = ast::Type::Struct { name: sym };
        let asttyp_id = self.ctx.intern_type(asttyp);
        let id = self.ast_to_ir_type(asttyp_id);
        let typ = self.typectx.get_type(id);
        let Type::Struct(id) = typ else {
            unreachable!();
        };
        *id
    }
    fn new_store(&mut self, block: BlockId, span: Span, val: ValueId, into: ValueId) {
        let mem_sym = self.ctx.intern_symbol("mem");
        let mem_typ = self.typectx.mem_typ();
        let (val_id, _, val, _) = self.new_inst1(Opcode::Store, block, span, &[val, into], mem_typ);
        val.dbg_name = Some(mem_sym);
        self.write_variable(VarId::Mem, block, val_id)
    }
    fn new_load(&mut self, block: BlockId, span: Span, from: ValueId, typ: TypeId) -> ValueId {
        let mem = self.read_variable(VarId::Mem, block);
        let (val_id, _, _, _) = self.new_inst1(Opcode::Load, block, span, &[from, mem], typ);
        val_id
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
        self.blocks.get_mut(block_id).insts.insert(0, inst_id);
        val_id
    }
    // replaces any users that use this, then removes the defining
    // phi instruction as well.
    fn replace_value(&mut self, from: ValueId, to: ValueId) {
        let val = self.values.get(from);
        let uses = val.uses.clone();
        for user in uses {
            self.replace_inst_value(user, from, to);
        }
        self.remove_phi_inst(from);
    }
    fn remove_phi_inst(&mut self, phi: ValueId) {
        let val = self.values.get(phi);
        let inst_id = val.inst;
        let inst = self.insts.get(inst_id);
        let block_id = inst.block;
        let block = self.blocks.get_mut(block_id);
        let idx = block
            .insts
            .iter()
            .position(|&i| i == inst_id)
            .expect("Should only be called for phi insts that exist");
        block.insts.remove(idx);
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
            let sid = lowerer.typectx.intern_struct(s);
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
    lowerer.typectx.intern_type(typ)
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
