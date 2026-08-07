use ahash::AHashSet;
use ahash::{AHashMap, HashMap, RandomState};
use indexmap::IndexMap;
use smol_str::SmolStr;
use tinyvec::TinyVec;

use crate::analysis::types::Type;
use crate::analysis::types::*;
use crate::common::span::Span;
use crate::syntax::ast::TypeNode as AstType;
use crate::syntax::ast::*;

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
    pub fields: IndexMap<SmolStr, FieldInfo, RandomState>,
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
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    pub structs: HashMap<SmolStr, StructInfo>,
    pub vars: HashMap<SmolStr, GlobalVariableEntry>,
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
    resolving: IndexMap<SmolStr, StructResolvingRequirement, RandomState>,
    error_structs: AHashSet<SmolStr>,
    symbol_table: SymbolTable,
    errors: Vec<SymbolTableBuildError>,
    struct_map: AHashMap<&'a str, &'a StructDeclaration<'a>>,
    type_arena: &'a mut TypeArena,
}

impl<'a> SymbolTableBuilder<'a> {
    pub fn build(program: &'a Program, type_arena: &'a mut TypeArena) -> SymbolTableBuildOutput {
        let mut builder = Self::new(type_arena);
        builder.build_inner(program);
        SymbolTableBuildOutput {
            symbol_table: builder.symbol_table,
            errors: builder.errors,
        }
    }
    fn new(type_arena: &'a mut TypeArena) -> Self {
        Self {
            resolving: IndexMap::with_hasher(RandomState::new()),
            error_structs: AHashSet::new(),
            symbol_table: SymbolTable::new(),
            errors: Vec::new(),
            struct_map: AHashMap::new(),
            type_arena,
        }
    }
    fn build_inner(&mut self, program: &'a Program) {
        for decl in &program.decls {
            match &decl.kind {
                GlobalDeclarationKind::Variable(decl) => {
                    let typ = self.type_arena.intern_ast_type(&decl.var_type);
                    let name = SmolStr::new(decl.name.ident);
                    let entry = GlobalVariableEntry {
                        typ,
                        is_function: false,
                    };
                    assert!(
                        self.symbol_table.vars.insert(name, entry).is_none(),
                        "Internal Compiler Error"
                    );
                }
                GlobalDeclarationKind::Struct(decl) => {
                    self.struct_map.insert(decl.name.ident, decl);
                }
                GlobalDeclarationKind::Function(decl) => {
                    let name = SmolStr::new(decl.name.ident);
                    let return_type = decl
                        .return_type
                        .as_ref()
                        .map(|t| self.type_arena.intern_ast_type(t));
                    let param_types: TinyVec<[TypeId; 5]> = decl
                        .params
                        .iter()
                        .map(|(_name, typ)| self.type_arena.intern_ast_type(typ))
                        .collect();
                    let fnptr = self.type_arena.intern_type(TypeNode::FuncPtr {
                        return_type,
                        param_types,
                    });
                    let entry = GlobalVariableEntry {
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
        let names: Vec<_> = self.struct_map.values().map(|decl| &decl.name).collect();
        for name in names {
            _ = self.resolve_struct(name);
        }
    }

    fn resolve_struct(&mut self, struct_ident: &Ident<'_>) -> Result<Layout, ()> {
        if self.error_structs.contains(struct_ident.ident) {
            return Err(());
        }
        if let Some(info) = self.symbol_table.structs.get(struct_ident.ident) {
            return Ok(info.layout);
        }
        let struct_name = SmolStr::new(struct_ident.ident);
        self.type_arena.intern_type(TypeNode::Struct {
            name: struct_name.clone(),
        });
        if self.resolving.get(struct_ident.ident).is_some() {
            self.insert_placeholder(struct_ident);
            self.error_structs.insert(struct_name);
            self.errors.push(SymbolTableBuildError::RecursiveStruct {
                chain: self.resolving.values().copied().collect(),
            });
            return Err(());
        }
        let Some(&decl) = self.struct_map.get(struct_ident.ident) else {
            self.errors.push(SymbolTableBuildError::StructDoesNotExist {
                span: struct_ident.span,
            });
            return Err(());
        };
        let mut size: usize = 0;
        let mut align = 1;
        let mut map = IndexMap::<SmolStr, FieldInfo, RandomState>::with_hasher(RandomState::new());
        for (name, typ) in &decl.fields {
            self.resolving.insert(
                struct_name.clone(),
                StructResolvingRequirement {
                    outer: struct_ident.span,
                    inner: typ.span,
                },
            );
            let layout = if let Ok(layout) = self.resolve_type(typ) {
                layout
            } else {
                Layout::new(1, 1)
            };
            self.resolving.pop();
            let id = self.type_arena.intern_ast_type(typ);
            size = size.next_multiple_of(layout.align);
            let info = FieldInfo {
                offset: size,
                typ: id,
                layout,
            };
            map.insert(SmolStr::new(name.ident), info);
            size += layout.size;
            align = usize::max(align, layout.align);
        }
        size = size.next_multiple_of(align);
        let layout = Layout::new(size, align);
        self.symbol_table.structs.insert(
            SmolStr::new(struct_ident.ident),
            StructInfo {
                fields: map,
                layout,
                span: decl.span,
            },
        );
        Ok(layout)
    }
    fn insert_placeholder(&mut self, name: &Ident<'_>) {
        self.symbol_table.structs.insert(
            SmolStr::new(name.ident),
            StructInfo {
                fields: IndexMap::with_hasher(RandomState::new()),
                layout: Layout::new(1, 1),
                span: name.span,
            },
        );
    }
    fn resolve_type(&mut self, typ: &AstType<'_>) -> Result<Layout, ()> {
        match typ.inner {
            TypeKind::Void => unreachable!("Internal Compiler Error"),
            TypeKind::Int => Ok(Layout::new(8, 8)),
            TypeKind::Ptr { .. } => Ok(Layout::new(8, 8)),
            TypeKind::Struct { name } => self.resolve_struct(name),
            TypeKind::Array { element_type, size } => {
                let layout = self.resolve_type(element_type)?;
                let size = layout.size * *size as usize;
                Ok(Layout {
                    size,
                    align: layout.align,
                })
            }
            TypeKind::FuncPtr { .. } => Ok(Layout::new(8, 8)),
            TypeKind::Error => Ok(Layout::new(1, 1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use bumpalo::Bump;

    use crate::{
        analysis::ast_validator::ASTValidator,
        syntax::{lexer::Lexer, parser::Parser},
    };

    use super::*;
    #[track_caller]
    fn build(s: &str) -> SymbolTableBuildOutput {
        let res = Lexer::lex(s);
        assert!(res.errors.is_empty());
        let bump = Bump::new();
        let res = Parser::parse(&res.tokens, &bump);
        assert!(res.errors.is_empty());
        let validation = ASTValidator::validate(&res.program, true);
        assert!(validation.is_empty());
        let mut type_arena = TypeArena::new();
        SymbolTableBuilder::build(&res.program, &mut type_arena)
    }

    #[track_caller]
    fn succeeds(s: &str) -> SymbolTable {
        let res = build(s);
        assert!(res.errors.is_empty());
        res.symbol_table
    }

    #[track_caller]
    fn fails(s: &str) -> SymbolTable {
        let res = build(s);
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
        "#;
        let table = succeeds(code);
        let outoforder = table.structs.get("OutOfOrder").unwrap();
        assert_eq!(outoforder.layout, Layout::new(8, 8));
        assert_eq!(outoforder.fields.len(), 1);
        assert_eq!(outoforder.fields[0].offset, 0);
        let eight = table.structs.get("Eight").unwrap();
        assert_eq!(eight.layout, Layout::new(8, 8));
        assert_eq!(eight.fields.len(), 1);
        assert_eq!(eight.fields[0].offset, 0);
        let sixteen = table.structs.get("Sixteen").unwrap();
        assert_eq!(sixteen.layout, Layout::new(16, 8));
        assert_eq!(sixteen.fields.len(), 2);
        assert_eq!(sixteen.fields[0].offset, 0);
        assert_eq!(sixteen.fields[1].offset, 8);
        let thirtytwo = table.structs.get("ThirtyTwo").unwrap();
        assert_eq!(thirtytwo.layout, Layout::new(32, 8));
        assert_eq!(thirtytwo.fields.len(), 4);
        assert_eq!(thirtytwo.fields[0].offset, 0);
        assert_eq!(thirtytwo.fields[1].offset, 8);
        assert_eq!(thirtytwo.fields[2].offset, 16);
        assert_eq!(thirtytwo.fields[3].offset, 24);

        let sixteen_two = table.structs.get("SixteenTwo").unwrap();
        assert_eq!(sixteen_two.layout, Layout::new(32, 8));
        assert_eq!(sixteen_two.fields.len(), 2);
        assert_eq!(sixteen_two.fields[0].offset, 0);
        assert_eq!(sixteen_two.fields[1].offset, 16);

        let twenty_four = table.structs.get("TwentyFour").unwrap();
        assert_eq!(twenty_four.layout, Layout::new(24, 8));
        assert_eq!(twenty_four.fields.len(), 2);
        assert_eq!(twenty_four.fields[0].offset, 0);
        assert_eq!(twenty_four.fields[1].offset, 8);

        let arrays = table.structs.get("Arrays").unwrap();
        assert_eq!(arrays.layout, Layout::new(264, 8));
        assert_eq!(arrays.fields.len(), 3);
        assert_eq!(arrays.fields[0].offset, 0);
        assert_eq!(arrays.fields[1].offset, 16);
        assert_eq!(arrays.fields[2].offset, 24);

        let ptrs = table.structs.get("Ptrs").unwrap();
        assert_eq!(ptrs.layout, Layout::new(32, 8));
        assert_eq!(ptrs.fields.len(), 4);
        assert_eq!(ptrs.fields[0].offset, 0);
        assert_eq!(ptrs.fields[1].offset, 8);
        assert_eq!(ptrs.fields[2].offset, 16);
        assert_eq!(ptrs.fields[3].offset, 24);

        let nestedarr = table.structs.get("NestedArray").unwrap();
        assert_eq!(nestedarr.layout, Layout::new(192, 8));
        assert_eq!(nestedarr.fields.len(), 2);
        assert_eq!(nestedarr.fields[0].offset, 0);
        assert_eq!(nestedarr.fields[1].offset, 160);
    }

    #[test]
    fn test_symbol_table_fails() {
        fails("struct A { a: X }");
        fails("struct A { a: A }");
        fails("struct A { x: X } struct X { a: A }");
        fails("struct A { b: B } struct B { c: C } struct C { d: D } struct D { a: C }");
    }
}
