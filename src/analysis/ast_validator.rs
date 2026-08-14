use ahash::AHashMap;

use crate::common::span::Span;
use crate::common::symbol::Symbol;
use crate::syntax::ast::visitor::*;
use crate::syntax::ast::*;
use crate::syntax::context::Context;

#[derive(Debug)]
pub struct ASTValidator {
    loop_depth: u32,
    no_main: bool,
    found_main: bool,
    pub errors: Vec<ASTValidationError>,
    global_var_scope: AHashMap<Symbol, Span>,
    struct_scope: AHashMap<Symbol, Span>,
}

impl ASTValidator {
    pub fn validate(ast: &Program, no_main: bool, ctx: &Context) -> Vec<ASTValidationError> {
        let mut validator = ASTValidator {
            loop_depth: 0,
            no_main,
            found_main: false,
            errors: Vec::new(),
            global_var_scope: AHashMap::new(),
            struct_scope: AHashMap::new(),
        };
        validator.visit_program(ast, ctx);
        validator.errors
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalSymbolType {
    VariableName,
    Function,
}

#[derive(Debug, Clone)]
pub enum ASTValidationError {
    MainHasParameters {
        span: Span,
    },
    MainHasReturnType {
        typ: Span,
    },
    DuplicateParameterName {
        name: Symbol,
        param1: Span,
        param2: Span,
    },
    StructWithNoField {
        name: Symbol,
        decl: Span,
    },
    DuplicateStructFieldDeclared {
        name: Symbol,
        field1: Span,
        field2: Span,
    },
    DuplicateStructFieldInit {
        name: Symbol,
        field1: Span,
        field2: Span,
    },
    BreakNotWithinLoop {
        span: Span,
    },
    ContinueNotWithinLoop {
        span: Span,
    },
    NoMain,
    DuplicateGlobalSymbol {
        symbol: Symbol,
        symbol_type: GlobalSymbolType,
        prev_def: Span,
        new_def: Span,
    },
    DuplicateStructDecl {
        name: Symbol,
        prev_def: Span,
        new_def: Span,
    },
    VoidWithoutPtr {
        span: Span,
    },
    InvalidNoAlias {
        span: Span,
    },
}

impl AstVisitor for ASTValidator {
    fn visit_type(&mut self, _typ: &Type, _ctx: &Context) {
        unreachable!()
    }

    fn visit_typenode(&mut self, typ: &TypeNode, ctx: &Context) {
        self.visit_type_impl(ctx.get_type(typ.inner), false, false, typ.span, ctx)
    }

    fn visit_sizeof_type(&mut self, expr: &SizeOfType, ctx: &Context) {
        self.visit_type_impl(
            ctx.get_type(expr.typ.inner),
            false,
            true,
            expr.typ.span,
            ctx,
        );
    }

    fn visit_cast(&mut self, expr: &Cast, ctx: &Context) {
        self.visit_expr(ctx.get_expr(expr.expr), ctx);
        self.visit_type_impl(
            ctx.get_type(expr.to_type.inner),
            false,
            true,
            expr.to_type.span,
            ctx,
        );
    }

    fn visit_break(&mut self, stmt: &Break, _ctx: &Context) {
        if self.loop_depth == 0 {
            self.errors
                .push(ASTValidationError::BreakNotWithinLoop { span: stmt.span });
        }
    }

    fn visit_continue(&mut self, stmt: &Continue, _ctx: &Context) {
        if self.loop_depth == 0 {
            self.errors
                .push(ASTValidationError::ContinueNotWithinLoop { span: stmt.span });
        }
    }
    fn visit_while_loop(&mut self, stmt: &WhileLoop, ctx: &Context) {
        self.visit_expr(ctx.get_expr(stmt.condition), ctx);
        self.loop_depth += 1;
        self.visit_stmt(ctx.get_stmt(stmt.body), ctx);
        self.loop_depth -= 1;
    }
    fn visit_for_loop(&mut self, stmt: &ForLoop, ctx: &Context) {
        if let Some(init) = stmt.init {
            self.visit_stmt(ctx.get_stmt(init), ctx);
        }
        if let Some(condition) = stmt.condition {
            self.visit_expr(ctx.get_expr(condition), ctx);
        }
        if let Some(post) = stmt.post {
            self.visit_expr(ctx.get_expr(post), ctx);
        }
        self.loop_depth += 1;
        self.visit_stmt(ctx.get_stmt(stmt.body), ctx);
        self.loop_depth -= 1;
    }

    fn visit_struct_init(&mut self, init: &StructInit, ctx: &Context) {
        let mut map = AHashMap::new();
        for (name, expr) in &init.field_inits {
            if let Some(span) = map.get(&name.sym) {
                self.errors
                    .push(ASTValidationError::DuplicateStructFieldInit {
                        name: name.sym,
                        field1: *span,
                        field2: name.span,
                    });
            } else {
                map.insert(name.sym, name.span);
            }
            self.visit_expr(ctx.get_expr(*expr), ctx);
        }
    }

    fn visit_struct_declaration(&mut self, decl: &StructDeclaration, ctx: &Context) {
        if let Some(span) = self.struct_scope.get(&decl.name.sym) {
            self.errors.push(ASTValidationError::DuplicateStructDecl {
                name: decl.name.sym,
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.struct_scope.insert(decl.name.sym, decl.span);
        }
        if decl.fields.is_empty() {
            self.errors.push(ASTValidationError::StructWithNoField {
                decl: decl.span,
                name: decl.name.sym,
            })
        }
        let mut map = AHashMap::new();
        for (name, typ) in &decl.fields {
            if let Some(span) = map.get(&name.sym) {
                self.errors
                    .push(ASTValidationError::DuplicateStructFieldDeclared {
                        name: name.sym,
                        field1: *span,
                        field2: name.span,
                    });
            } else {
                map.insert(name.sym, name.span);
            }
            self.visit_typenode(typ, ctx);
        }
    }

    fn visit_function_declaration(&mut self, decl: &FunctionDeclaration, ctx: &Context) {
        if let Some(span) = self.global_var_scope.get(&decl.name.sym) {
            self.errors.push(ASTValidationError::DuplicateGlobalSymbol {
                symbol: decl.name.sym,
                symbol_type: GlobalSymbolType::Function,
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.global_var_scope.insert(decl.name.sym, decl.span);
        }
        let name = decl.name.sym;
        if ctx.get_symbol(name) == "main" && !self.no_main {
            self.found_main = true;
            if !decl.params.is_empty() {
                self.errors
                    .push(ASTValidationError::MainHasParameters { span: decl.span });
            }
            if decl.return_type.is_some() {
                self.errors
                    .push(ASTValidationError::MainHasReturnType { typ: decl.span });
            }
        }
        let mut map = AHashMap::new();
        for (name, typ) in &decl.params {
            if let Some(span) = map.get(&name.sym) {
                self.errors
                    .push(ASTValidationError::DuplicateParameterName {
                        name: name.sym,
                        param1: *span,
                        param2: name.span,
                    });
            } else {
                map.insert(name.sym, name.span);
            }
            self.visit_type_impl(ctx.get_type(typ.inner), false, true, typ.span, ctx);
        }
        if let Some(ret_type) = &decl.return_type {
            self.visit_type_impl(ctx.get_type(ret_type.inner), true, true, ret_type.span, ctx);
        }
        self.visit_block(&decl.body, ctx);
    }
    fn visit_global_declaration(&mut self, decl: &GlobalDeclaration, ctx: &Context) {
        match &decl.kind {
            GlobalDeclarationKind::Variable(variable_declaration) => {
                self.visit_vardecl_global(variable_declaration, ctx)
            }
            GlobalDeclarationKind::Struct(struct_declaration) => {
                self.visit_struct_declaration(struct_declaration, ctx)
            }
            GlobalDeclarationKind::Function(function_declaration) => {
                self.visit_function_declaration(function_declaration, ctx)
            }
        }
    }

    fn visit_program(&mut self, program: &Program, ctx: &Context) {
        for decl in &program.decls {
            self.visit_global_declaration(decl, ctx);
        }
        if !self.no_main && !self.found_main {
            self.errors.push(ASTValidationError::NoMain)
        }
    }
}

impl ASTValidator {
    fn visit_type_impl(
        &mut self,
        typ: &Type,
        void_allowed: bool,
        noalias_allowed: bool,
        span: Span,
        ctx: &Context,
    ) {
        match typ {
            Type::Void => {
                if !void_allowed {
                    self.errors
                        .push(ASTValidationError::VoidWithoutPtr { span });
                }
            }
            Type::Ptr { pointee, noalias } => {
                if *noalias && !noalias_allowed {
                    self.errors
                        .push(ASTValidationError::InvalidNoAlias { span });
                }
                self.visit_type_impl(ctx.get_type(*pointee), true, noalias_allowed, span, ctx);
            }
            Type::Array { element_type, .. } => {
                self.visit_type_impl(
                    ctx.get_type(*element_type),
                    false,
                    noalias_allowed,
                    span,
                    ctx,
                );
            }
            Type::FuncPtr {
                return_type,
                param_types,
            } => {
                self.visit_type_impl(ctx.get_type(*return_type), true, true, span, ctx);
                for param_type in param_types {
                    self.visit_type_impl(ctx.get_type(*param_type), false, true, span, ctx)
                }
            }
            _ => (),
        }
    }

    fn visit_vardecl_global(&mut self, decl: &VariableDeclaration, ctx: &Context) {
        if let Some(span) = self.global_var_scope.get(&decl.name.sym) {
            self.errors.push(ASTValidationError::DuplicateGlobalSymbol {
                symbol: decl.name.sym,
                symbol_type: GlobalSymbolType::VariableName,
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.global_var_scope.insert(decl.name.sym, decl.span);
        }
        self.visit_typenode(&decl.var_type, ctx);
        if let Some(init) = &decl.init_value {
            self.visit_expr(ctx.get_expr(*init), ctx);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::syntax::{lexer::Lexer, parser::Parser};

    #[track_caller]
    fn validate(s: &str, no_main: bool, success: bool) {
        let mut ctx = Context::new();
        let lexed = Lexer::lex(s, &mut ctx);
        assert!(!lexed.has_errors());
        let parsed = Parser::parse_test(&lexed.tokens, &mut ctx);
        assert!(!parsed.has_errors());
        let validation_errors = ASTValidator::validate(&parsed.program, no_main, &ctx);
        assert_eq!(
            success,
            validation_errors.is_empty(),
            "{validation_errors:?}"
        );
    }
    #[track_caller]
    fn assert_success(s: &str, no_main: bool) {
        validate(s, no_main, true)
    }
    #[track_caller]
    fn assert_fail(s: &str, no_main: bool) {
        validate(s, no_main, false)
    }
    #[test]
    fn test_validation() {
        assert_success("fn main() {}", false);
        assert_success("fn main() {}", true);
        assert_success("", true);
        assert_fail("", false);
        assert_fail("fn something() {}", false);

        assert_success("fn main(a: int) -> *int {}", true);

        assert_fail("fn main(a: int) {}", false);
        assert_fail("fn main() -> int {}", false);
        assert_fail("fn main(a: int) -> *int {}", false);

        assert_success("fn foo(a: int, b: int) {}", true);
        assert_success("fn foo(a: int, b: SomeStruct) {}", true);
        assert_fail("fn foo(a: int, a: int) {}", true);
        assert_fail("fn foo(a: int, a: AStruct) {}", true);

        assert_success("struct Struct {x: something}", true);
        assert_success("struct Struct {x: something, y: something}", true);
        assert_fail("struct Struct {x: something, x: something}", true);
        assert_fail("struct Struct {x: something, x: int}", true);
        assert_fail("struct Struct {}", true);

        assert_success("fn foo() { while (1) {break;} }", true);
        assert_success("fn foo() { for (;;) {break;} }", true);
        assert_success("fn foo() { while (1) {continue;} }", true);
        assert_success("fn foo() { for (;;) {continue;} }", true);
        assert_fail("fn foo() { break; }", true);
        assert_fail("fn foo() { continue; }", true);
        assert_fail("fn foo() { { break; } }", true);
        assert_fail("fn foo() { { continue; } }", true);

        assert_success("fn foo() {} fn bar() {}", true);
        assert_success("fn foo() {} let bar: int;", true);
        assert_success("fn bar() {} let foo: int;", true);
        assert_fail("fn bar() {} let bar: int;", true);
        assert_fail("fn foo() {} let foo: int;", true);

        assert_success("struct Struct {x: int}", true);
        assert_success("struct Struct {x: int} struct OtherStruct {x: int}", true);
        assert_fail("struct Struct {x: int} struct Struct {x: int}", true);
        assert_fail("struct Struct {x: int} struct Struct {y: *int}", true);

        assert_success("let a: *void;", true);
        assert_success("let a: ***void;", true);
        assert_fail("let a: void;", true);

        assert_success("struct Struct {x: *void}", true);
        assert_fail("struct Struct {x: void}", true);

        assert_success("fn foo(x: *int) {}", true);
        assert_success("fn foo(x: noalias *int) {}", true);
        assert_success("fn foo(x: noalias **int) {}", true);
        assert_success("fn foo(x: *noalias *int) {}", true);
        assert_success("fn foo(x: *void) {}", true);
        assert_success("fn foo(x: noalias *void) {}", true);
        assert_success("fn foo(x: noalias **void) {}", true);
        assert_success("fn foo(x: *noalias *void) {}", true);

        assert_success("fn foo() -> *int {}", true);
        assert_success("fn foo() -> noalias *int {}", true);
        assert_success("fn foo() -> noalias **int {}", true);
        assert_success("fn foo() -> *noalias *int {}", true);
        assert_success("fn foo() -> *void {}", true);
        assert_success("fn foo() -> noalias *void {}", true);
        assert_success("fn foo() -> noalias **void {}", true);
        assert_success("fn foo() -> *noalias *void {}", true);

        assert_success("fn bar() { let x: fn() -> *int; }", true);
        assert_success("fn bar() { let x: fn() -> noalias *int; }", true);
        assert_success("fn bar() { let x: fn() -> noalias **int; }", true);
        assert_success("fn bar() { let x: fn() -> *noalias *int; }", true);
        assert_success("fn bar() { let x: fn() -> *void; }", true);
        assert_success("fn bar() { let x: fn() -> noalias *void; }", true);
        assert_success("fn bar() { let x: fn() -> noalias **void; }", true);
        assert_success("fn bar() { let x: fn() -> *noalias *void; }", true);

        assert_success("fn bar() { let x: int = sizeof(fn() -> *int); }", true);
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> noalias *int); }",
            true,
        );
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> noalias **int); }",
            true,
        );
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> *noalias *int); }",
            true,
        );
        assert_success("fn bar() { let x: int = sizeof(fn() -> *void); }", true);
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> noalias *void); }",
            true,
        );
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> noalias **void); }",
            true,
        );
        assert_success(
            "fn bar() { let x: int = sizeof(fn() -> *noalias *void); }",
            true,
        );

        assert_fail("let x: noalias *int;", true);
        assert_fail("fn foo() { let x: noalias *int; }", true);
        // Should fail in global var calc but should fail here too
        assert_success("let x: int = X as noalias *int;", true);
        assert_success("struct X { x: *int }", true);
        assert_fail("struct X { x: noalias *int }", true);
        assert_success("fn foo() { return 0 as noalias *int; }", true);

        assert_success("fn foo() { return SomeStruct {}; }", true);
        assert_success("fn foo() { return SomeStruct {a: x}; }", true);
        assert_success("fn foo() { return SomeStruct {a: x, b: x}; }", true);
        assert_fail("fn foo() { return SomeStruct {a: x, a: x}; }", true);

        assert_fail("fn foo() {} let foo: int;", true);
        assert_fail("let foo: int; let foo: int;", true);

        assert_success("fn foo() -> void {}", true);
        assert_success("let x: fn() -> void;", true);
        assert_success("let x: fn(int) -> void;", true);
        assert_success("let x: fn(*void) -> void;", true);

        assert_success("fn foo() -> void {let x: fn() -> void;}", true);
        assert_fail("fn foo(x: void) {}", true);
        assert_fail("fn foo() {let x: void;}", true);
    }
}
