use ahash::AHashSet;
use ahash::{AHashMap, HashMap, RandomState};
use indexmap::IndexMap;
use tinyvec::TinyVec;

use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::syntax::ast::Type;
use crate::syntax::ast::*;
use crate::syntax::context::{Context, TypeId};

#[derive(Debug, Clone)]
pub enum SymbolTableBuildError {
    RecursiveStruct {
        chain: TinyVec<[StructResolvingRequirement; 5]>,
    },
    StructDoesNotExist {
        span: Span,
    },
}
#[derive(Debug, Clone, Default, Copy)]
pub struct StructResolvingRequirement {
    pub outer: Span,
    pub inner: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub size: usize,
    pub align: usize,
}

impl Layout {
    pub fn new(size: usize, align: usize) -> Self {
        Self { size, align }
    }
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: IndexMap<Symbol, FieldInfo, RandomState>,
    pub order: AHashMap<Symbol, usize>,
    pub layout: Layout,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub offset: usize,
    pub typ: TypeId,
    pub layout: Layout,
}

#[derive(Debug, Clone)]
pub struct GlobalVariableEntry {
    pub typ: TypeId,
    pub is_function: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub structs: HashMap<Symbol, StructInfo>,
    pub vars: HashMap<Symbol, GlobalVariableEntry>,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            structs: Default::default(),
            vars: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SymbolTableBuildOutput {
    pub symbol_table: SymbolTable,
    pub errors: Vec<SymbolTableBuildError>,
}

#[derive(Debug)]
pub struct SymbolTableBuilder<'a> {
    resolving: IndexMap<Symbol, StructResolvingRequirement, RandomState>,
    error_structs: AHashSet<Symbol>,
    symbol_table: SymbolTable,
    errors: Vec<SymbolTableBuildError>,
    struct_map: AHashMap<Symbol, &'a StructDeclaration>,
    ctx: &'a mut Context,
}

impl<'a> SymbolTableBuilder<'a> {
    pub fn build(program: &'a Program, ctx: &'a mut Context) -> SymbolTableBuildOutput {
        let mut builder = Self::new(ctx);
        builder.build_inner(program);
        SymbolTableBuildOutput {
            symbol_table: builder.symbol_table,
            errors: builder.errors,
        }
    }
    fn new(ctx: &'a mut Context) -> Self {
        Self {
            resolving: IndexMap::with_hasher(RandomState::new()),
            error_structs: AHashSet::new(),
            symbol_table: SymbolTable::new(),
            errors: Vec::new(),
            struct_map: AHashMap::new(),
            ctx,
        }
    }
    fn build_inner(&mut self, program: &'a Program) {
        for decl in &program.decls {
            match &decl.kind {
                GlobalDeclarationKind::Variable(decl) => {
                    let typ = decl.var_type.inner;
                    let name = decl.name.sym;
                    let entry = GlobalVariableEntry {
                        span: decl.name.span,
                        typ,
                        is_function: false,
                    };
                    assert!(
                        self.symbol_table.vars.insert(name, entry).is_none(),
                        "Internal Compiler Error"
                    );
                }
                GlobalDeclarationKind::Struct(decl) => {
                    self.struct_map.insert(decl.name.sym, decl);
                }
                GlobalDeclarationKind::Function(decl) => {
                    let name = decl.name.sym;
                    let return_type = decl
                        .return_type
                        .as_ref()
                        .map(|tn| tn.inner)
                        .unwrap_or_else(|| self.ctx.intern_type(Type::Void));
                    let param_types: TinyVec<[TypeId; 5]> =
                        decl.params.iter().map(|(_name, typ)| typ.inner).collect();
                    let fnptr = self.ctx.intern_type(Type::FuncPtr {
                        return_type,
                        param_types,
                    });
                    let entry = GlobalVariableEntry {
                        span: decl.name.span,
                        typ: fnptr,
                        is_function: true,
                    };
                    assert!(
                        self.symbol_table.vars.insert(name, entry).is_none(),
                        "Internal Compiler Error"
                    )
                }
            }
        }
        let names: Vec<_> = self
            .struct_map
            .iter()
            .map(|(s, decl)| (*s, decl.span))
            .collect();
        for (sym, span) in names {
            _ = self.resolve_struct(sym, span);
        }
    }

    fn resolve_struct(&mut self, struct_ident: Symbol, span: Span) -> Result<Layout, ()> {
        if self.error_structs.contains(&struct_ident) {
            return Err(());
        }
        if let Some(info) = self.symbol_table.structs.get(&struct_ident) {
            return Ok(info.layout);
        }
        let struct_name = struct_ident;
        self.ctx.intern_type(Type::Struct { name: struct_name });
        if self.resolving.get(&struct_ident).is_some() {
            self.insert_placeholder(struct_ident, span);
            self.error_structs.insert(struct_name);
            self.errors.push(SymbolTableBuildError::RecursiveStruct {
                chain: self.resolving.values().copied().collect(),
            });
            return Err(());
        }
        let Some(&decl) = self.struct_map.get(&struct_ident) else {
            self.errors
                .push(SymbolTableBuildError::StructDoesNotExist { span });
            return Err(());
        };
        let mut size: usize = 0;
        let mut align = 1;
        let mut map = IndexMap::<Symbol, FieldInfo, RandomState>::with_hasher(RandomState::new());
        let mut order = AHashMap::new();
        for (i, (name, typ)) in decl.fields.iter().enumerate() {
            self.resolving.insert(
                struct_name,
                StructResolvingRequirement {
                    outer: span,
                    inner: typ.span,
                },
            );
            let layout = if let Ok(layout) = self.resolve_type(typ.inner, span) {
                layout
            } else {
                Layout::new(1, 1)
            };
            self.resolving.pop();
            let id = typ.inner;
            size = size.next_multiple_of(layout.align);
            let info = FieldInfo {
                offset: size,
                typ: id,
                layout,
            };
            map.insert(name.sym, info);
            size += layout.size;
            align = usize::max(align, layout.align);
            order.insert(name.sym, i);
        }
        size = size.next_multiple_of(align);
        let layout = Layout::new(size, align);
        self.symbol_table.structs.insert(
            struct_ident,
            StructInfo {
                fields: map,
                layout,
                span: decl.span,
                order,
            },
        );
        Ok(layout)
    }
    fn insert_placeholder(&mut self, name: Symbol, span: Span) {
        self.symbol_table.structs.insert(
            name,
            StructInfo {
                fields: IndexMap::with_hasher(RandomState::new()),
                layout: Layout::new(1, 1),
                span,
                order: AHashMap::new(),
            },
        );
    }
    fn resolve_type(&mut self, typ: TypeId, span: Span) -> Result<Layout, ()> {
        let typ = self.ctx.get_type(typ);
        match typ {
            // This type is put into the AST if the parser has an error parsing a type
            // We just put a placeholder basically
            Type::Void => Ok(Layout::new(1, 1)),
            Type::Int => Ok(Layout::new(8, 8)),
            Type::Ptr { .. } => Ok(Layout::new(8, 8)),
            Type::Struct { name } => self.resolve_struct(*name, span),
            Type::Array { element_type, len } => {
                let len = *len;
                let layout = self.resolve_type(*element_type, span)?;
                let size = layout.size * len as usize;
                Ok(Layout {
                    size,
                    align: layout.align,
                })
            }
            Type::FuncPtr { .. } => Ok(Layout::new(8, 8)),
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        analysis::ast_validator::AstValidator,
        syntax::{lexer::Lexer, parser::Parser},
    };

    use super::*;
    #[track_caller]
    fn build(s: &str, ctx: &mut Context) -> SymbolTableBuildOutput {
        let res = Lexer::lex(s, ctx);
        assert!(res.errors.is_empty());
        let res = Parser::parse_test(&res.tokens, ctx);
        assert!(res.errors.is_empty());
        let validation = AstValidator::validate(&res.program, true, ctx);
        assert!(validation.is_empty());
        SymbolTableBuilder::build(&res.program, ctx)
    }

    #[track_caller]
    fn succeeds(s: &str, ctx: &mut Context) -> SymbolTable {
        let res = build(s, ctx);
        assert!(res.errors.is_empty());
        res.symbol_table
    }

    #[track_caller]
    fn fails(s: &str, ctx: &mut Context) -> SymbolTable {
        let res = build(s, ctx);
        assert!(!res.errors.is_empty());
        res.symbol_table
    }

    #[test]
    fn test_symbol_table_build() {
        let code = r#"
            struct OutOfOrder {e: Eight}
            struct Eight { a: int, };
            struct Sixteen { a: int, b: *Sixteen, };
            struct ThirtyTwo { a: int, b: int, c: int, d: int, };
            struct SixteenTwo { a: Sixteen, b: Sixteen, };
            struct TwentyFour { b: Eight, c: Sixteen,}
            struct Arrays { a: [int; 2], b: [Eight; 1], c: [Sixteen; 15], };
            struct Ptrs { a: *int, b: *Eight, c: *Sixteen, d: *fn(int) -> *int, };
            struct NestedArray { a: [[int; 4]; 5], b: [[*NestedArray; 2]; 2] }
            let x: int = 5;
            let y: *int;
            let z: *SomeStruct;
            let a: fn(int) -> *int;
            let b: fn() -> void;
            fn foo(a: int) -> void {}
            fn bar(b: int) {}
            fn baz(c: int) -> *int {}
        "#;
        let mut ctx = Context::new();
        let table = succeeds(code, &mut ctx);
        let outoforder = table.structs.get(&ctx.intern_symbol("OutOfOrder")).unwrap();
        assert_eq!(outoforder.layout, Layout::new(8, 8));
        assert_eq!(outoforder.fields.len(), 1);
        assert_eq!(outoforder.fields[0].offset, 0);
        let eight = table.structs.get(&ctx.intern_symbol("Eight")).unwrap();
        assert_eq!(eight.layout, Layout::new(8, 8));
        assert_eq!(eight.fields.len(), 1);
        assert_eq!(eight.fields[0].offset, 0);
        let sixteen = table.structs.get(&ctx.intern_symbol("Sixteen")).unwrap();
        assert_eq!(sixteen.layout, Layout::new(16, 8));
        assert_eq!(sixteen.fields.len(), 2);
        assert_eq!(sixteen.fields[0].offset, 0);
        assert_eq!(sixteen.fields[1].offset, 8);
        let thirtytwo = table.structs.get(&ctx.intern_symbol("ThirtyTwo")).unwrap();
        assert_eq!(thirtytwo.layout, Layout::new(32, 8));
        assert_eq!(thirtytwo.fields.len(), 4);
        assert_eq!(thirtytwo.fields[0].offset, 0);
        assert_eq!(thirtytwo.fields[1].offset, 8);
        assert_eq!(thirtytwo.fields[2].offset, 16);
        assert_eq!(thirtytwo.fields[3].offset, 24);

        let sixteen_two = table.structs.get(&ctx.intern_symbol("SixteenTwo")).unwrap();
        assert_eq!(sixteen_two.layout, Layout::new(32, 8));
        assert_eq!(sixteen_two.fields.len(), 2);
        assert_eq!(sixteen_two.fields[0].offset, 0);
        assert_eq!(sixteen_two.fields[1].offset, 16);

        let twenty_four = table.structs.get(&ctx.intern_symbol("TwentyFour")).unwrap();
        assert_eq!(twenty_four.layout, Layout::new(24, 8));
        assert_eq!(twenty_four.fields.len(), 2);
        assert_eq!(twenty_four.fields[0].offset, 0);
        assert_eq!(twenty_four.fields[1].offset, 8);

        let arrays = table.structs.get(&ctx.intern_symbol("Arrays")).unwrap();
        assert_eq!(arrays.layout, Layout::new(264, 8));
        assert_eq!(arrays.fields.len(), 3);
        assert_eq!(arrays.fields[0].offset, 0);
        assert_eq!(arrays.fields[1].offset, 16);
        assert_eq!(arrays.fields[2].offset, 24);

        let ptrs = table.structs.get(&ctx.intern_symbol("Ptrs")).unwrap();
        assert_eq!(ptrs.layout, Layout::new(32, 8));
        assert_eq!(ptrs.fields.len(), 4);
        assert_eq!(ptrs.fields[0].offset, 0);
        assert_eq!(ptrs.fields[1].offset, 8);
        assert_eq!(ptrs.fields[2].offset, 16);
        assert_eq!(ptrs.fields[3].offset, 24);

        let nestedarr = table
            .structs
            .get(&ctx.intern_symbol("NestedArray"))
            .unwrap();
        assert_eq!(nestedarr.layout, Layout::new(192, 8));
        assert_eq!(nestedarr.fields.len(), 2);
        assert_eq!(nestedarr.fields[0].offset, 0);
        assert_eq!(nestedarr.fields[1].offset, 160);
        let int = ctx.intern_type(Type::Int);
        let int_ptr = ctx.intern_type(Type::Ptr {
            pointee: int,
            noalias: false,
        });
        let name = ctx.intern_symbol("SomeStruct");
        let pointee = ctx.intern_type(Type::Struct { name });
        let somestruct_ptr = ctx.intern_type(Type::Ptr {
            pointee,
            noalias: false,
        });
        let mut v = TinyVec::new();
        v.push(int);
        let fnptr1 = ctx.intern_type(Type::FuncPtr {
            return_type: int_ptr,
            param_types: v.clone(),
        });
        let void = ctx.intern_type(Type::Void);
        let fnptr2 = ctx.intern_type(Type::FuncPtr {
            return_type: void,
            param_types: TinyVec::new(),
        });
        let fnptr34 = ctx.intern_type(Type::FuncPtr {
            return_type: void,
            param_types: v.clone(),
        });
        let fnptr5 = ctx.intern_type(Type::FuncPtr {
            return_type: int_ptr,
            param_types: v,
        });
        let e1 = table.vars.get(&ctx.intern_symbol("x")).unwrap();
        assert!(!e1.is_function);
        assert_eq!(e1.typ, int);
        let e2 = table.vars.get(&ctx.intern_symbol("y")).unwrap();
        assert!(!e2.is_function);
        assert_eq!(e2.typ, int_ptr);
        let e3 = table.vars.get(&ctx.intern_symbol("z")).unwrap();
        assert!(!e3.is_function);
        assert_eq!(e3.typ, somestruct_ptr);
        let e4 = table.vars.get(&ctx.intern_symbol("a")).unwrap();
        assert!(!e4.is_function);
        assert_eq!(e4.typ, fnptr1);
        let e5 = table.vars.get(&ctx.intern_symbol("b")).unwrap();
        assert!(!e5.is_function);
        assert_eq!(e5.typ, fnptr2);
        let e6 = table.vars.get(&ctx.intern_symbol("foo")).unwrap();
        assert!(e6.is_function);
        assert_eq!(e6.typ, fnptr34);
        let e7 = table.vars.get(&ctx.intern_symbol("bar")).unwrap();
        assert!(e7.is_function);
        assert_eq!(e7.typ, fnptr34);
        let e8 = table.vars.get(&ctx.intern_symbol("baz")).unwrap();
        assert!(e8.is_function);
        assert_eq!(e8.typ, fnptr5);
    }

    #[test]
    fn test_symbol_table_fails() {
        let mut ctx = Context::new();
        fails("struct A { a: X }", &mut ctx);
        fails("struct A { a: A }", &mut ctx);
        fails("struct A { x: X } struct X { a: A }", &mut ctx);
        fails(
            "struct A { b: B } struct B { c: C } struct C { d: D } struct D { a: C }",
            &mut ctx,
        );
    }
}
