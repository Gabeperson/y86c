use std::sync::Arc;

use ahash::{AHashMap, AHashSet, RandomState};
use indexmap::IndexMap;
use tinyvec::TinyVec;

use crate::analysis::symbol_table::{Layout, SymbolTable};
use crate::analysis::type_checker::ExprTypeInfo;
use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::syntax::ast::{self};
use crate::syntax::context::{Context, ExprId, TypeId};

#[derive(Clone, Debug, Default)]
pub struct EvaluatedGlobals {
    pub map: IndexMap<Symbol, Global, RandomState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstVal {
    Int(i64),
    Addr {
        sym: Symbol,
        add: i64,
    },
    Struct {
        name: Symbol,
        layout: Layout,
        fields: Arc<[StructField]>,
    },
    Array {
        elem_layout: Layout,
        elems: Arc<[ConstVal]>,
    },
    Invalid,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructField {
    pub sym: Symbol,
    pub offset: u64,
    pub elem: ConstVal,
}

#[derive(Clone, Debug)]
pub struct Global {
    pub val: ConstVal,
}

impl Global {
    fn new(val: ConstVal) -> Self {
        Self { val }
    }
}

impl ConstVal {
    pub fn write<F: FnMut(Symbol) -> i64>(&self, buf: &mut [u8], mut f: F) {
        self.write_inner(buf, &mut f);
    }
    pub fn write_inner<F: FnMut(Symbol) -> i64>(&self, buf: &mut [u8], f: &mut F) {
        match self {
            ConstVal::Int(int) => {
                let bytes = int.to_le_bytes();
                buf[..8].copy_from_slice(&bytes)
            }
            ConstVal::Addr { sym, add } => {
                let addr = f(*sym);
                let bytes = i64::to_le_bytes(addr + *add);
                buf[..8].copy_from_slice(&bytes)
            }
            ConstVal::Struct {
                name: _,
                layout: _,
                fields,
            } => {
                for field in fields.iter() {
                    let offset = field.offset as usize;
                    let buf = &mut buf[offset..];
                    field.elem.write_inner(buf, f);
                }
            }
            ConstVal::Array { elem_layout, elems } => {
                let size = elem_layout.size;
                for (index, elem) in elems.iter().enumerate() {
                    let offset = index * size;
                    let buf = &mut buf[offset..];
                    elem.write_inner(buf, f)
                }
            }
            ConstVal::Invalid => unreachable!(),
            ConstVal::None => unreachable!(),
        }
    }
    pub fn layout(&self) -> Layout {
        match self {
            ConstVal::Int(_) => Layout::new(8, 8),
            ConstVal::Addr { .. } => Layout::new(8, 8),
            ConstVal::Struct { layout, .. } => *layout,
            ConstVal::Array { elem_layout, elems } => {
                Layout::new(elem_layout.size * elems.len(), elem_layout.align)
            }
            ConstVal::Invalid => unreachable!(),
            ConstVal::None => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum GlobalEvalError {
    GlobalCyclicInit {
        chain: TinyVec<[GlobalEvalReq; 5]>,
    },
    ArrayOutOfBounds {
        array: TypeId,
        index: i64,
    },
    UninitializedFieldAccess {
        struct_sym: Symbol,
        member: Symbol,
        span: Span,
    },
    DoubleLabelMath {
        span: Span,
        sym1: Symbol,
        sym2: Symbol,
    },
    InvalidOpForLabel {
        label: Symbol,
        span: Span,
        op: ast::BinaryOpKind,
    },
}

#[derive(Debug, Copy, Default, Clone)]
pub struct GlobalEvalReq {
    pub resolving: Span,
    pub needs: Span,
}

#[derive(Debug, Clone)]
pub struct GlobalEvaluator<'a> {
    ctx: &'a Context,
    type_table: &'a AHashMap<ast::NodeId, ExprTypeInfo>,
    symbol_table: &'a SymbolTable,

    resolving: IndexMap<Symbol, GlobalEvalReq, RandomState>,
    error_globals: AHashSet<Symbol>,
    evaled: IndexMap<Symbol, Global, RandomState>,
    errors: Vec<GlobalEvalError>,
}

impl<'a> GlobalEvaluator<'a> {
    pub fn eval(
        program: &ast::Program,
        ctx: &Context,
        type_table: &'a AHashMap<ast::NodeId, ExprTypeInfo>,
        symbol_table: &'a SymbolTable,
    ) -> Result<EvaluatedGlobals, Vec<GlobalEvalError>> {
        let mut globals = AHashMap::new();
        for decl in &program.decls {
            if let ast::GlobalDeclarationKind::Variable(vardecl) = decl.kind {
                globals.insert(vardecl.name.sym, vardecl);
            }
        }
        let mut evaluator = GlobalEvaluator {
            ctx,
            type_table,
            symbol_table,
            resolving: IndexMap::with_hasher(RandomState::new()),
            error_globals: AHashSet::new(),
            evaled: IndexMap::with_hasher(RandomState::new()),
            errors: Vec::new(),
        };
        for decl in globals.values() {
            _ = evaluator.eval_var_decl(*decl, &globals);
        }
        if evaluator.errors.is_empty() {
            Ok(EvaluatedGlobals {
                map: evaluator.evaled,
            })
        } else {
            Err(evaluator.errors)
        }
    }
    fn eval_var_decl(
        &mut self,
        decl: ast::VariableDeclaration,
        globals: &AHashMap<Symbol, ast::VariableDeclaration>,
    ) -> Result<Global, ()> {
        let sym = decl.name.sym;
        if self.error_globals.contains(&sym) {
            return Err(());
        }
        if let Some(global) = self.evaled.get(&sym) {
            return Ok(global.clone());
        }
        if self.resolving.get(&sym).is_some() {
            self.insert_placeholder(sym);
            self.error_globals.insert(sym);
            self.errors.push(GlobalEvalError::GlobalCyclicInit {
                chain: self.resolving.values().copied().collect(),
            });
            return Err(());
        }

        let eval_res = decl
            .init_value
            .map(|init| self.eval_expr(init, globals))
            .transpose()?
            .unwrap_or(ConstVal::None);

        let global = Global::new(eval_res);
        self.evaled.insert(sym, global.clone());

        Ok(global)
    }

    fn eval_expr(
        &mut self,
        expr_id: ExprId,
        globals: &AHashMap<Symbol, ast::VariableDeclaration>,
    ) -> Result<ConstVal, ()> {
        let expr = self.ctx.get_expr(expr_id);
        match &expr.kind {
            ast::ExprKind::Nullptr(_nullptr) => Ok(ConstVal::Int(0)),
            ast::ExprKind::Cast(cast) => {
                let val = self.eval_expr(cast.expr, globals)?;
                std::assert_matches!(val, ConstVal::Int(_) | ConstVal::Addr { .. });
                Ok(val)
            }
            ast::ExprKind::Ident(ident) => {
                let decl = match globals.get(&ident.sym) {
                    Some(decl) => decl,
                    // This means this ident is a function, and is guaranteed
                    // to exist, since it passed type checking.
                    // As functions don't need to be "evaluated" at this time,
                    // we just return the addr of fucntion immediately.
                    None => {
                        return Ok(ConstVal::Addr {
                            sym: ident.sym,
                            add: 0,
                        });
                    }
                };
                let val = self.eval_var_decl(*decl, globals).map(|g| g.val)?;
                if val == ConstVal::Invalid {
                    Err(())
                } else {
                    Ok(val)
                }
            }
            ast::ExprKind::Int(int) => Ok(ConstVal::Int(int.lit)),
            ast::ExprKind::BinaryOp(binary_op) => self.eval_binop(*binary_op, globals),
            ast::ExprKind::PrefixOp(prefix_op) => self.eval_prefixop(*prefix_op, globals),
            ast::ExprKind::PostfixOp(_) => unreachable!("Checked in type checker"),
            ast::ExprKind::Ternary(ternary) => {
                let cond = self.eval_expr(ternary.condition, globals)?;
                let ConstVal::Int(int) = cond else {
                    unreachable!("Checked in type checker");
                };
                if int != 0 {
                    self.eval_expr(ternary.true_branch, globals)
                } else {
                    self.eval_expr(ternary.false_branch, globals)
                }
            }
            ast::ExprKind::FunctionCall(_function_call) => {
                unreachable!("Function calls not allowed, prevented in type check")
            }
            ast::ExprKind::ArrayIndex(array_index) => {
                let array = self.eval_expr(array_index.array, globals)?;
                let ConstVal::Array { elems, .. } = array else {
                    unreachable!("Checked in type checker");
                };
                let index = self.eval_expr(array_index.index, globals)?;
                let ConstVal::Int(int) = index else {
                    unreachable!("Checked in type checker");
                };
                match elems.get(int as usize) {
                    Some(val) => Ok(val.clone()),
                    None => {
                        let arr = self.ctx.get_expr(array_index.array);
                        let typ = self.type_table.get(&arr.id).expect("Should be added").id;
                        self.errors.push(GlobalEvalError::ArrayOutOfBounds {
                            array: typ,
                            index: int,
                        });
                        Err(())
                    }
                }
            }
            ast::ExprKind::SizeOfType(size_of_type) => Ok(ConstVal::Int(
                self.get_layout_of_type(size_of_type.typ.inner).size as i64,
            )),
            ast::ExprKind::StructInit(struct_init) => {
                let mut fields = Vec::with_capacity(struct_init.field_inits.len());
                let s = &self.symbol_table.structs[&struct_init.name.sym];
                for (name, expr) in struct_init.field_inits.iter().copied() {
                    let e = self.eval_expr(expr, globals)?;
                    let offset = s.fields[&name.sym].offset as u64;
                    let field = StructField {
                        sym: name.sym,
                        offset,
                        elem: e,
                    };
                    fields.push(field);
                }
                Ok(ConstVal::Struct {
                    name: struct_init.name.sym,
                    layout: s.layout,
                    fields: fields.into(),
                })
            }
            ast::ExprKind::ArrayInit(array_init) => {
                let len = array_init.elements.len();
                let mut v = Vec::with_capacity(len);
                for elem in &array_init.elements {
                    let e = self.eval_expr(*elem, globals)?;
                    v.push(e);
                }
                let e1 = self.ctx.get_expr(array_init.elements[0]);
                let e1_typ = self.type_table[&e1.id];
                let elem_layout = self.get_layout_of_type(e1_typ.id);
                Ok(ConstVal::Array {
                    elem_layout,
                    elems: v.into(),
                })
            }
            ast::ExprKind::MemberAccess(member_access) => {
                let s = self.eval_expr(member_access.struct_expr, globals)?;
                let ConstVal::Struct { name, fields, .. } = s else {
                    unreachable!("Checked by type checker");
                };
                for field in fields.as_ref() {
                    if field.sym == member_access.member_name.sym {
                        return Ok(field.elem.clone());
                    }
                }
                self.errors.push(GlobalEvalError::UninitializedFieldAccess {
                    struct_sym: name,
                    member: member_access.member_name.sym,
                    span: member_access.span,
                });
                Err(())
            }
            ast::ExprKind::PointerMemberAccess(_)
            | ast::ExprKind::CopyProvenance(_)
            | ast::ExprKind::ExposeProvenance(_)
            | ast::ExprKind::UnexposeProvenance(_)
            | ast::ExprKind::NewProvenance(_) => {
                unreachable!("Checked in type checking")
            }
            ast::ExprKind::Error => todo!(),
        }
    }

    fn eval_binop(
        &mut self,
        binop: ast::BinaryOp,
        globals: &AHashMap<Symbol, ast::VariableDeclaration>,
    ) -> Result<ConstVal, ()> {
        let lhs = self.eval_expr(binop.left, globals)?;
        let rhs = self.eval_expr(binop.right, globals)?;
        let lhs_typid = self.expr_type(binop.left);
        let rhs_typid = self.expr_type(binop.right);
        let lhs_typ = self.ctx.get_type(lhs_typid);
        let rhs_typ = self.ctx.get_type(rhs_typid);
        match binop.kind {
            ast::BinaryOpKind::Add => match (lhs_typ, rhs_typ) {
                (ast::Type::Int, ast::Type::Int) => self.add(lhs, rhs, binop.span),
                (ast::Type::Int, ast::Type::Ptr { pointee, .. }) => {
                    let size = self.get_layout_of_type(*pointee).size as i64;
                    let lhs = self.mul(lhs, size, binop.span, binop.kind)?;
                    self.add(lhs, rhs, binop.span)
                }
                (ast::Type::Ptr { pointee, .. }, ast::Type::Int) => {
                    let size = self.get_layout_of_type(*pointee).size as i64;
                    let rhs = self.mul(rhs, size, binop.span, binop.kind)?;
                    self.add(lhs, rhs, binop.span)
                }
                _ => unreachable!(),
            },
            ast::BinaryOpKind::Sub => match (lhs_typ, rhs_typ) {
                (ast::Type::Int, ast::Type::Int) => self.sub(lhs, rhs, binop.span),
                (ast::Type::Ptr { pointee, .. }, ast::Type::Int) => {
                    let size = self.get_layout_of_type(*pointee).size as i64;
                    let rhs = self.mul(rhs, size, binop.span, binop.kind)?;
                    self.sub(lhs, rhs, binop.span)
                }
                (ast::Type::Ptr { pointee: p1, .. }, ast::Type::Ptr { pointee: p2, .. }) => {
                    assert_eq!(p1, p2);
                    let size = self.get_layout_of_type(*p1).size as i64;
                    let raw = self.sub(lhs, rhs, binop.span)?;
                    self.div(raw, size, binop.span, binop.kind)
                }
                _ => unreachable!(),
            },
            ast::BinaryOpKind::Mul
            | ast::BinaryOpKind::Div
            | ast::BinaryOpKind::Eq
            | ast::BinaryOpKind::Greater
            | ast::BinaryOpKind::Less
            | ast::BinaryOpKind::GreaterOrEqual
            | ast::BinaryOpKind::LessOrEqual
            | ast::BinaryOpKind::NotEq
            | ast::BinaryOpKind::And
            | ast::BinaryOpKind::Or
            | ast::BinaryOpKind::BitAnd
            | ast::BinaryOpKind::BitOr
            | ast::BinaryOpKind::Xor
            | ast::BinaryOpKind::Mod
            | ast::BinaryOpKind::Shl
            | ast::BinaryOpKind::Shr => self.perform_simple_binop(lhs, rhs, binop.kind, binop.span),

            ast::BinaryOpKind::ShlAssign
            | ast::BinaryOpKind::ShrAssign
            | ast::BinaryOpKind::AddAssign
            | ast::BinaryOpKind::SubAssign
            | ast::BinaryOpKind::MulAssign
            | ast::BinaryOpKind::DivAssign
            | ast::BinaryOpKind::BitAndAssign
            | ast::BinaryOpKind::BitOrAssign
            | ast::BinaryOpKind::XorAssign
            | ast::BinaryOpKind::ModAssign
            | ast::BinaryOpKind::Assign => unreachable!(),
        }
    }
    fn perform_simple_binop(
        &mut self,
        lhs: ConstVal,
        rhs: ConstVal,
        binop: ast::BinaryOpKind,
        span: Span,
    ) -> Result<ConstVal, ()> {
        let lhs = match lhs {
            ConstVal::Int(lhs) => lhs,
            ConstVal::Addr { sym, .. } => {
                self.errors.push(GlobalEvalError::InvalidOpForLabel {
                    label: sym,
                    span,
                    op: binop,
                });
                return Err(());
            }
            _ => unreachable!(),
        };
        let rhs = match rhs {
            ConstVal::Int(rhs) => rhs,
            ConstVal::Addr { sym, .. } => {
                self.errors.push(GlobalEvalError::InvalidOpForLabel {
                    label: sym,
                    span,
                    op: binop,
                });
                return Err(());
            }
            _ => unreachable!(),
        };
        Ok(match binop {
            ast::BinaryOpKind::Mul => ConstVal::Int(lhs * rhs),
            ast::BinaryOpKind::Div => ConstVal::Int(lhs / rhs),
            ast::BinaryOpKind::Eq => ConstVal::Int((lhs == rhs) as i64),
            ast::BinaryOpKind::Greater => ConstVal::Int((lhs > rhs) as i64),
            ast::BinaryOpKind::Less => ConstVal::Int((lhs < rhs) as i64),
            ast::BinaryOpKind::GreaterOrEqual => ConstVal::Int((lhs >= rhs) as i64),
            ast::BinaryOpKind::LessOrEqual => ConstVal::Int((lhs <= rhs) as i64),
            ast::BinaryOpKind::NotEq => ConstVal::Int((lhs != rhs) as i64),
            ast::BinaryOpKind::And => ConstVal::Int((lhs != 0 && rhs != 0) as i64),
            ast::BinaryOpKind::Or => ConstVal::Int((lhs != 0 || rhs != 0) as i64),
            ast::BinaryOpKind::BitAnd => ConstVal::Int(lhs & rhs),
            ast::BinaryOpKind::BitOr => ConstVal::Int(lhs | rhs),
            ast::BinaryOpKind::Xor => ConstVal::Int(lhs ^ rhs),
            ast::BinaryOpKind::Mod => ConstVal::Int(lhs % rhs),
            ast::BinaryOpKind::Shl => ConstVal::Int(lhs << rhs),
            ast::BinaryOpKind::Shr => ConstVal::Int(lhs >> rhs),

            _ => unreachable!(),
        })
    }
    fn eval_prefixop(
        &mut self,
        prefixop: ast::PrefixOp,
        globals: &AHashMap<Symbol, ast::VariableDeclaration>,
    ) -> Result<ConstVal, ()> {
        if let ast::PrefixOpKind::AddressOf = prefixop.kind {
            return self.eval_expr_addrof(prefixop.expr, globals);
        }
        let expr = self.eval_expr(prefixop.expr, globals)?;
        match prefixop.kind {
            ast::PrefixOpKind::UnaryPlus => {
                std::assert_matches!(expr, ConstVal::Int(_));
                Ok(expr)
            }
            ast::PrefixOpKind::UnaryMinus => {
                let ConstVal::Int(int) = expr else {
                    unreachable!("Checked by type checker");
                };
                Ok(ConstVal::Int(-int))
            }
            ast::PrefixOpKind::Not => {
                let ConstVal::Int(int) = expr else {
                    unreachable!("Checked by type checker");
                };
                Ok(ConstVal::Int((int == 0) as i64))
            }
            ast::PrefixOpKind::BitNot => {
                let ConstVal::Int(int) = expr else {
                    unreachable!("Checked by type checker");
                };
                Ok(ConstVal::Int(!int))
            }

            ast::PrefixOpKind::AddressOf
            | ast::PrefixOpKind::Increment
            | ast::PrefixOpKind::Decrement
            | ast::PrefixOpKind::Dereference => unreachable!(),
        }
    }

    fn eval_expr_addrof(
        &mut self,
        expr_id: ExprId,
        globals: &AHashMap<Symbol, ast::VariableDeclaration>,
    ) -> Result<ConstVal, ()> {
        let expr = self.ctx.get_expr(expr_id);
        match expr.kind {
            ast::ExprKind::Ident(ident) => Ok(ConstVal::Addr {
                sym: ident.sym,
                add: 0,
            }),
            ast::ExprKind::ArrayIndex(array_index) => {
                let array = self.eval_expr_addrof(array_index.array, globals)?;
                let ConstVal::Addr { sym, add } = array else {
                    unreachable!("Checked by type checker");
                };
                let index = self.eval_expr(expr_id, globals)?;
                let ConstVal::Int(idx) = index else {
                    unreachable!("Chcked by type checker");
                };
                let typid = self.expr_type(expr_id);
                let elem_size = self.get_layout_of_type(typid).size as i64;

                Ok(ConstVal::Addr {
                    sym,
                    add: add + idx * elem_size,
                })
            }
            ast::ExprKind::MemberAccess(member_access) => {
                let s = self.eval_expr_addrof(expr_id, globals)?;
                let ConstVal::Addr { sym, add } = s else {
                    unreachable!("Checked by type checker");
                };
                let typid = self.expr_type(member_access.struct_expr);
                let typ = self.ctx.get_type(typid);
                let ast::Type::Struct { name } = typ else {
                    unreachable!("Checked by type checker");
                };
                let sinfo = &self.symbol_table.structs[name];
                let offset = sinfo.fields[&member_access.member_name.sym].offset as i64;

                Ok(ConstVal::Addr {
                    sym,
                    add: add + offset,
                })
            }
            _ => unreachable!("Stopped by type checker"),
        }
    }

    fn get_layout_of_type(&self, type_id: TypeId) -> Layout {
        let typ = self.ctx.get_type(type_id);
        match typ {
            ast::Type::Void => unreachable!(),
            ast::Type::Int => Layout::new(8, 8),
            ast::Type::Ptr { .. } => Layout::new(8, 8),
            ast::Type::Struct { name } => {
                let s = &self.symbol_table.structs[name];
                s.layout
            }
            ast::Type::Array { element_type, len } => {
                let layout = self.get_layout_of_type(*element_type);
                Layout::new(layout.size * *len as usize, layout.align)
            }
            ast::Type::FuncPtr { .. } => Layout::new(8, 8),
        }
    }

    fn expr_type(&self, expr_id: ExprId) -> TypeId {
        let expr = self.ctx.get_expr(expr_id);
        let typ = self.type_table[&expr.id];
        typ.id
    }

    fn insert_placeholder(&mut self, sym: Symbol) {
        let global = Global::new(ConstVal::Invalid);
        self.evaled.insert(sym, global);
    }

    fn mul(
        &mut self,
        lhs: ConstVal,
        rhs: i64,
        span: Span,
        binop: ast::BinaryOpKind,
    ) -> Result<ConstVal, ()> {
        if let ConstVal::Int(int) = lhs {
            return Ok(ConstVal::Int(int * rhs));
        }
        if let ConstVal::Addr { sym, .. } = lhs {
            self.errors.push(GlobalEvalError::InvalidOpForLabel {
                label: sym,
                span,
                op: binop,
            });
            return Err(());
        }
        unreachable!()
    }
    fn div(
        &mut self,
        lhs: ConstVal,
        rhs: i64,
        span: Span,
        binop: ast::BinaryOpKind,
    ) -> Result<ConstVal, ()> {
        if let ConstVal::Int(int) = lhs {
            return Ok(ConstVal::Int(int / rhs));
        }
        if let ConstVal::Addr { sym, .. } = lhs {
            self.errors.push(GlobalEvalError::InvalidOpForLabel {
                label: sym,
                span,
                op: binop,
            });
            return Err(());
        }
        unreachable!()
    }
    fn add(&mut self, lhs: ConstVal, rhs: ConstVal, span: Span) -> Result<ConstVal, ()> {
        match (lhs, rhs) {
            (ConstVal::Int(lhs), ConstVal::Int(rhs)) => Ok(ConstVal::Int(lhs + rhs)),
            (ConstVal::Addr { sym, add }, ConstVal::Int(rhs)) => Ok(ConstVal::Addr {
                sym,
                add: add + rhs,
            }),
            (ConstVal::Int(lhs), ConstVal::Addr { sym, add }) => Ok(ConstVal::Addr {
                sym,
                add: lhs + add,
            }),
            (ConstVal::Addr { sym: s1, .. }, ConstVal::Addr { sym: s2, .. }) => {
                self.errors.push(GlobalEvalError::DoubleLabelMath {
                    span,
                    sym1: s1,
                    sym2: s2,
                });
                Err(())
            }
            _ => unreachable!(),
        }
    }
    fn sub(&mut self, lhs: ConstVal, rhs: ConstVal, span: Span) -> Result<ConstVal, ()> {
        match (lhs, rhs) {
            (ConstVal::Int(lhs), ConstVal::Int(rhs)) => Ok(ConstVal::Int(lhs - rhs)),
            (ConstVal::Addr { sym, add }, ConstVal::Int(rhs)) => Ok(ConstVal::Addr {
                sym,
                add: add - rhs,
            }),
            (ConstVal::Int(lhs), ConstVal::Addr { sym, add }) => Ok(ConstVal::Addr {
                sym,
                add: lhs - add,
            }),
            (ConstVal::Addr { sym: s1, .. }, ConstVal::Addr { sym: s2, .. }) => {
                self.errors.push(GlobalEvalError::DoubleLabelMath {
                    span,
                    sym1: s1,
                    sym2: s2,
                });
                Err(())
            }
            _ => unreachable!(),
        }
    }
}
