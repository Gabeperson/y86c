use ahash::AHashMap;
use smol_str::SmolStr;

use crate::common::span::Span;
use crate::syntax::ast::visitor::*;
use crate::syntax::ast::*;

#[derive(Debug)]
pub struct ASTValidator {
    loop_depth: u32,
    no_main: bool,
    found_main: bool,
    pub errors: Vec<ASTValidationError>,
    global_var_scope: AHashMap<SmolStr, Span>,
    struct_scope: AHashMap<SmolStr, Span>,
}

impl ASTValidator {
    pub fn validate(ast: &Program<'_>, no_main: bool) -> Vec<ASTValidationError> {
        let mut validator = ASTValidator {
            loop_depth: 0,
            no_main,
            found_main: false,
            errors: Vec::new(),
            global_var_scope: AHashMap::new(),
            struct_scope: AHashMap::new(),
        };
        validator.visit_program(ast);
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
        name: SmolStr,
        param1: Span,
        param2: Span,
    },
    StructWithNoField {
        name: SmolStr,
        decl: Span,
    },
    DuplicateStructFieldDeclared {
        name: SmolStr,
        field1: Span,
        field2: Span,
    },
    DuplicateStructFieldInit {
        name: SmolStr,
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
        symbol: SmolStr,
        symbol_type: GlobalSymbolType,
        prev_def: Span,
        new_def: Span,
    },
    DuplicateStructDecl {
        name: SmolStr,
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
    fn visit_type(&mut self, typ: &Type<'_>) {
        self.visit_type_impl(typ, false, false)
    }

    fn visit_sizeof_type(&mut self, expr: &SizeOfType<'_>) {
        self.visit_type_impl(&expr.typ, false, true);
    }

    fn visit_cast(&mut self, expr: &Cast<'_>) {
        self.visit_expr(&expr.expr);
        self.visit_type_impl(&expr.to_type, false, true);
    }

    fn visit_break(&mut self, stmt: &Break) {
        if self.loop_depth == 0 {
            self.errors
                .push(ASTValidationError::BreakNotWithinLoop { span: stmt.span });
        }
    }

    fn visit_continue(&mut self, stmt: &Continue) {
        if self.loop_depth == 0 {
            self.errors
                .push(ASTValidationError::ContinueNotWithinLoop { span: stmt.span });
        }
    }

    fn visit_while_loop(&mut self, stmt: &WhileLoop<'_>) {
        self.visit_expr(&stmt.condition);
        self.loop_depth += 1;
        self.visit_stmt(&stmt.body);
        self.loop_depth -= 1;
    }

    fn visit_for_loop(&mut self, stmt: &ForLoop<'_>) {
        if let Some(init) = &stmt.init {
            self.visit_stmt(init);
        }
        if let Some(condition) = &stmt.condition {
            self.visit_expr(condition);
        }
        if let Some(post) = &stmt.post {
            self.visit_expr(post);
        }
        self.loop_depth += 1;
        self.visit_stmt(&stmt.body);
        self.loop_depth -= 1;
    }

    fn visit_struct_init(&mut self, init: &StructInit<'_>) {
        let mut map = AHashMap::new();
        for (name, expr) in &init.field_inits {
            if let Some(span) = map.get(name.ident) {
                self.errors
                    .push(ASTValidationError::DuplicateStructFieldInit {
                        name: SmolStr::new(name.ident),
                        field1: *span,
                        field2: name.span,
                    });
            } else {
                map.insert(name.ident, name.span);
            }
            self.visit_expr(expr);
        }
    }

    fn visit_struct_declaration(&mut self, decl: &StructDeclaration<'_>) {
        if let Some(span) = self.struct_scope.get(decl.name.ident) {
            self.errors.push(ASTValidationError::DuplicateStructDecl {
                name: SmolStr::new(decl.name.ident),
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.struct_scope
                .insert(SmolStr::new(decl.name.ident), decl.span);
        }
        if decl.fields.is_empty() {
            self.errors.push(ASTValidationError::StructWithNoField {
                decl: decl.span,
                name: SmolStr::new(decl.name.ident),
            })
        }
        let mut map = AHashMap::new();
        for (name, typ) in &decl.fields {
            if let Some(span) = map.get(name.ident) {
                self.errors
                    .push(ASTValidationError::DuplicateStructFieldDeclared {
                        name: SmolStr::new(name.ident),
                        field1: *span,
                        field2: name.span,
                    });
            } else {
                map.insert(name.ident, name.span);
            }
            self.visit_type(typ);
        }
    }

    fn visit_function_declaration(&mut self, decl: &FunctionDeclaration<'_>) {
        if let Some(span) = self.global_var_scope.get(decl.name.ident) {
            self.errors.push(ASTValidationError::DuplicateGlobalSymbol {
                symbol: SmolStr::new(decl.name.ident),
                symbol_type: GlobalSymbolType::Function,
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.global_var_scope
                .insert(SmolStr::new(decl.name.ident), decl.span);
        }
        let name = decl.name.ident;
        if name == "main" && !self.no_main {
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
            if let Some(span) = map.get(name.ident) {
                self.errors
                    .push(ASTValidationError::DuplicateParameterName {
                        name: SmolStr::new(name.ident),
                        param1: *span,
                        param2: name.span,
                    });
            } else {
                map.insert(name.ident, name.span);
            }
            self.visit_type_impl(typ, false, true);
        }
        if let Some(ret_type) = &decl.return_type {
            self.visit_type_impl(ret_type, false, true);
        }
        self.visit_block(&decl.body);
    }

    fn visit_global_declaration(&mut self, decl: &GlobalDeclaration<'_>) {
        match &decl.kind {
            GlobalDeclarationKind::Variable(variable_declaration) => {
                self.visit_vardecl_global(variable_declaration)
            }
            GlobalDeclarationKind::Struct(struct_declaration) => {
                self.visit_struct_declaration(struct_declaration)
            }
            GlobalDeclarationKind::Function(function_declaration) => {
                self.visit_function_declaration(function_declaration)
            }
        }
    }

    fn visit_program(&mut self, program: &Program<'_>) {
        for decl in &program.decls {
            self.visit_global_declaration(decl);
        }
        if !self.no_main && !self.found_main {
            self.errors.push(ASTValidationError::NoMain)
        }
    }
}

impl ASTValidator {
    fn visit_type_impl(&mut self, typ: &Type<'_>, behind_ptr: bool, noalias_allowed: bool) {
        match typ.inner {
            TypeKind::Void => {
                if !behind_ptr {
                    self.errors
                        .push(ASTValidationError::VoidWithoutPtr { span: typ.span });
                }
            }
            TypeKind::Ptr { pointee, noalias } => {
                if *noalias && !noalias_allowed {
                    self.errors
                        .push(ASTValidationError::InvalidNoAlias { span: typ.span });
                }
                self.visit_type_impl(pointee, true, noalias_allowed);
            }
            TypeKind::Array { element_type, .. } => {
                self.visit_type_impl(element_type, false, noalias_allowed);
            }
            TypeKind::FuncPtr {
                return_type,
                param_types,
            } => {
                if let Some(return_type) = return_type {
                    self.visit_type_impl(return_type, false, true)
                }
                for param_type in param_types {
                    self.visit_type_impl(param_type, false, true)
                }
            }
            _ => (),
        }
    }

    fn visit_vardecl_global(&mut self, decl: &VariableDeclaration<'_>) {
        if let Some(span) = self.global_var_scope.get(decl.name.ident) {
            self.errors.push(ASTValidationError::DuplicateGlobalSymbol {
                symbol: SmolStr::new(decl.name.ident),
                symbol_type: GlobalSymbolType::VariableName,
                prev_def: *span,
                new_def: decl.span,
            })
        } else {
            self.global_var_scope
                .insert(SmolStr::new(decl.name.ident), decl.span);
        }
        self.visit_type(&decl.var_type);
        if let Some(init) = &decl.init_value {
            self.visit_expr(init);
        }
    }
}

#[cfg(test)]
mod tests {
    use bumpalo::Bump;

    use super::*;
    use crate::syntax::{lexer::Lexer, parser::Parser};

    #[track_caller]
    fn validate(s: &str, no_main: bool, success: bool) {
        let lexed = Lexer::lex(s);
        assert!(!lexed.has_errors());
        let bump = Bump::new();
        let parsed = Parser::parse(&lexed.tokens, &bump);
        assert!(!parsed.has_errors());
        let validation_errors = ASTValidator::validate(&parsed.program, no_main);
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
    }
}
